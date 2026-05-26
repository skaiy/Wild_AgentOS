use std::collections::HashMap;
use std::time::Duration;
use std::io;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// 任务队列错误
#[derive(Debug, Error)]
pub enum QueueError {
    #[error("IO 错误: {0}")]
    Io(#[from] io::Error),
    
    #[error("序列化错误: {0}")]
    Serialize(#[from] serde_json::Error),
    
    #[error("队列错误: {0}")]
    Queue(String),
    
    #[error("超时")]
    Timeout,
    
    #[error("队列已关闭")]
    Closed,
}

/// 任务上下文数据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskContextData {
    /// 阶段 ID
    pub stage_id: String,
    /// 阶段类型
    pub stage_type: String,
    /// 项目 ID
    pub project_id: String,
    /// 项目目录
    pub project_dir: String,
    /// 用户需求
    pub user_requirement: String,
    /// 前一阶段输出
    pub prev_outputs: HashMap<String, serde_json::Value>,
    /// LLM 配置
    pub llm_config: LlmConfig,
}

/// LLM 配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConfig {
    pub api_key: String,
    pub base_url: String,
    pub model: String,
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            base_url: "https://api.deepseek.com".to_string(),
            model: "deepseek-chat".to_string(),
        }
    }
}

/// Agent OS 任务
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentOsTask {
    /// 任务 ID
    pub task_id: String,
    /// 任务 IRI
    pub task_iri: String,
    /// 提示词
    pub prompt: String,
    /// 任务上下文
    pub context: TaskContextData,
    /// 创建时间戳
    pub created_at: u64,
}

impl AgentOsTask {
    pub fn new(task_iri: String, prompt: String, context: TaskContextData) -> Self {
        Self {
            task_id: uuid::Uuid::new_v4().to_string(),
            task_iri,
            prompt,
            context,
            created_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        }
    }
}

/// Agent OS 执行结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentOsResult {
    /// 对应的任务 ID
    pub task_id: String,
    /// 执行状态
    pub status: String,
    /// 摘要
    pub summary: String,
    /// 输出数据
    pub output: Option<serde_json::Value>,
    /// JSON-LD 输出
    pub jsonld_output: Option<serde_json::Value>,
    /// 工件列表
    pub artifacts: Vec<String>,
    /// 错误列表
    pub errors: Vec<String>,
    /// 执行时间（毫秒）
    pub duration_ms: u64,
    /// 工具调用次数
    pub tool_call_count: u32,
    /// 轮次计数
    pub turn_count: u32,
}

impl AgentOsResult {
    pub fn success(task_id: String, summary: String) -> Self {
        Self {
            task_id,
            status: "success".to_string(),
            summary,
            output: None,
            jsonld_output: None,
            artifacts: Vec::new(),
            errors: Vec::new(),
            duration_ms: 0,
            tool_call_count: 0,
            turn_count: 0,
        }
    }

    pub fn failure(task_id: String, error: String) -> Self {
        Self {
            task_id,
            status: "failed".to_string(),
            summary: error.clone(),
            output: None,
            jsonld_output: None,
            artifacts: Vec::new(),
            errors: vec![error],
            duration_ms: 0,
            tool_call_count: 0,
            turn_count: 0,
        }
    }
}

impl From<crate::core::agent_runner::TaskResult> for AgentOsResult {
    fn from(result: crate::core::agent_runner::TaskResult) -> Self {
        Self {
            task_id: result.task_iri.split('/').last().unwrap_or(&result.task_iri).to_string(),
            status: result.status,
            summary: result.summary,
            output: result.output,
            jsonld_output: result.jsonld_output,
            artifacts: result.artifacts.into_iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect(),
            errors: result.errors,
            duration_ms: 0,
            tool_call_count: result.tool_call_count,
            turn_count: result.turn_count,
        }
    }
}

// ============================================================
// yaque 队列实现
// ============================================================

use yaque::queue::{Sender, Receiver};

/// Activity 端任务队列
pub struct TaskQueue {
    task_sender: Sender,
    result_receiver: Receiver,
    base_path: String,
    pending_results: HashMap<String, AgentOsResult>,
}

impl TaskQueue {
    /// 创建新的任务队列（完整版，同时持有 sender 和 receiver）
    pub fn new(base_path: &str) -> Result<Self, QueueError> {
        let task_path = format!("{}/tasks", base_path);
        let result_path = format!("{}/results", base_path);
        
        let task_sender = Sender::open(&task_path)?;
        let result_receiver = Receiver::open(&result_path)?;
        
        Ok(Self {
            task_sender,
            result_receiver,
            base_path: base_path.to_string(),
            pending_results: HashMap::new(),
        })
    }
    
    /// 创建客户端队列（只发送任务，接收结果）
    pub fn new_client(base_path: &str) -> Result<Self, QueueError> {
        let task_path = format!("{}/tasks", base_path);
        let result_path = format!("{}/results", base_path);
        
        let task_sender = Sender::open(&task_path)?;
        let result_receiver = Receiver::open(&result_path)?;
        
        Ok(Self {
            task_sender,
            result_receiver,
            base_path: base_path.to_string(),
            pending_results: HashMap::new(),
        })
    }
    
    /// 发送任务
    pub async fn send_task(&mut self, task: &AgentOsTask) -> Result<(), QueueError> {
        let data = serde_json::to_vec(task)?;
        tracing::info!(task_id = %task.task_id, task_iri = %task.task_iri, data_len = data.len(), "发送任务到队列");
        self.task_sender.send(data).await?;
        tracing::info!(task_id = %task.task_id, "任务已成功发送");
        Ok(())
    }
    
    /// 接收指定任务的结果（带超时，按 task_id 匹配）
    pub async fn recv_result_for_task(&mut self, task_id: &str, timeout: Duration) -> Result<Option<AgentOsResult>, QueueError> {
        tracing::info!(expected_task_id = %task_id, "开始等待结果");
        
        if let Some(result) = self.pending_results.remove(task_id) {
            tracing::info!(task_id = %task_id, "从缓存中找到匹配结果");
            return Ok(Some(result));
        }
        
        let deadline = tokio::time::Instant::now() + timeout;
        
        loop {
            let remaining = deadline.duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                tracing::warn!(expected_task_id = %task_id, "等待结果超时");
                return Ok(None);
            }
            
            match tokio::time::timeout(remaining, self.result_receiver.recv()).await {
                Ok(guard_result) => {
                    let guard = guard_result.map_err(|e| QueueError::Queue(e.to_string()))?;
                    let result: AgentOsResult = serde_json::from_slice(&*guard)?;
                    guard.commit();
                    
                    tracing::info!(
                        expected_task_id = %task_id,
                        got_task_id = %result.task_id,
                        status = %result.status,
                        "收到结果"
                    );
                    
                    if result.task_id == task_id {
                        tracing::info!(task_id = %task_id, "结果匹配，返回");
                        return Ok(Some(result));
                    } else {
                        tracing::warn!(expected_task_id = %task_id, got_task_id = %result.task_id, "结果不匹配，缓存");
                        self.pending_results.insert(result.task_id.clone(), result);
                    }
                }
                Err(_) => {
                    tracing::warn!(expected_task_id = %task_id, "接收结果超时");
                    return Ok(None);
                }
            }
        }
    }
    
    /// 接收结果（带超时，不匹配 task_id）
    pub async fn recv_result_timeout(&mut self, timeout: Duration) -> Result<Option<AgentOsResult>, QueueError> {
        let deadline = tokio::time::Instant::now() + timeout;
        
        let guard = tokio::time::timeout_at(deadline, self.result_receiver.recv())
            .await
            .map_err(|_| QueueError::Timeout)?
            .map_err(|e| QueueError::Queue(e.to_string()))?;
        
        let result: AgentOsResult = serde_json::from_slice(&*guard)?;
        guard.commit();
        tracing::debug!(task_id = %result.task_id, status = %result.status, "收到结果");
        Ok(Some(result))
    }
    
    /// 获取队列基础路径
    pub fn base_path(&self) -> &str {
        &self.base_path
    }
}

/// Worker 端任务队列
pub struct WorkerQueue {
    task_receiver: Receiver,
    result_sender: Sender,
    base_path: String,
}

impl WorkerQueue {
    /// 创建新的 Worker 队列
    pub fn new(base_path: &str) -> Result<Self, QueueError> {
        let task_path = format!("{}/tasks", base_path);
        let result_path = format!("{}/results", base_path);
        
        let task_receiver = Receiver::open(&task_path)?;
        let result_sender = Sender::open(&result_path)?;
        
        Ok(Self {
            task_receiver,
            result_sender,
            base_path: base_path.to_string(),
        })
    }
    
    /// 接收任务
    pub async fn recv_task(&mut self) -> Result<AgentOsTask, QueueError> {
        let guard = self.task_receiver.recv().await
            .map_err(|e| QueueError::Queue(e.to_string()))?;
        let task: AgentOsTask = serde_json::from_slice(&*guard)?;
        guard.commit();
        tracing::debug!(task_id = %task.task_id, "收到任务");
        Ok(task)
    }
    
    /// 发送结果
    pub async fn send_result(&mut self, result: &AgentOsResult) -> Result<(), QueueError> {
        let data = serde_json::to_vec(result)?;
        tracing::info!(task_id = %result.task_id, status = %result.status, data_len = data.len(), "发送结果到队列");
        self.result_sender.send(data).await?;
        tracing::info!(task_id = %result.task_id, "结果已成功发送");
        Ok(())
    }
    
    /// 获取队列基础路径
    pub fn base_path(&self) -> &str {
        &self.base_path
    }
}

// ============================================================
// Unix Domain Socket 实现（备选方案）
// ============================================================

#[cfg(unix)]
pub mod uds {
    use super::*;
    use tokio::net::{UnixStream, UnixListener};
    use tokio_util::codec::{Framed, LengthDelimitedCodec};
    use futures::{SinkExt, StreamExt};

    /// UDS 任务客户端
    pub struct UdsTaskClient {
        stream: Framed<UnixStream, LengthDelimitedCodec>,
    }

    impl UdsTaskClient {
        pub async fn connect(path: &str) -> Result<Self, QueueError> {
            let stream = UnixStream::connect(path).await?;
            Ok(Self {
                stream: Framed::new(stream, LengthDelimitedCodec::new()),
            })
        }

        pub async fn send(&mut self, task: &AgentOsTask) -> Result<(), QueueError> {
            let data = serde_json::to_vec(task)?;
            self.stream.send(data.into()).await
                .map_err(|e| QueueError::Queue(e.to_string()))?;
            Ok(())
        }

        pub async fn recv(&mut self) -> Result<AgentOsResult, QueueError> {
            let data = self.stream.next().await
                .ok_or(QueueError::Closed)?
                .map_err(|e| QueueError::Queue(e.to_string()))?;
            let result = serde_json::from_slice(&data)?;
            Ok(result)
        }

        pub async fn recv_timeout(&mut self, timeout: Duration) -> Result<Option<AgentOsResult>, QueueError> {
            match tokio::time::timeout(timeout, self.recv()).await {
                Ok(result) => result.map(Some),
                Err(_) => Ok(None),
            }
        }
    }

    /// UDS 任务服务器
    pub struct UdsTaskServer {
        listener: UnixListener,
    }

    impl UdsTaskServer {
        pub async fn bind(path: &str) -> Result<Self, QueueError> {
            if std::path::Path::new(path).exists() {
                std::fs::remove_file(path)?;
            }
            let listener = UnixListener::bind(path)?;
            Ok(Self { listener })
        }

        pub async fn accept(&self) -> Result<UdsTaskConnection, QueueError> {
            let (stream, _) = self.listener.accept().await?;
            Ok(UdsTaskConnection {
                stream: Framed::new(stream, LengthDelimitedCodec::new()),
            })
        }
    }

    /// UDS 连接
    pub struct UdsTaskConnection {
        stream: Framed<UnixStream, LengthDelimitedCodec>,
    }

    impl UdsTaskConnection {
        pub async fn recv_task(&mut self) -> Result<AgentOsTask, QueueError> {
            let data = self.stream.next().await
                .ok_or(QueueError::Closed)?
                .map_err(|e| QueueError::Queue(e.to_string()))?;
            let task = serde_json::from_slice(&data)?;
            Ok(task)
        }

        pub async fn send_result(&mut self, result: &AgentOsResult) -> Result<(), QueueError> {
            let data = serde_json::to_vec(result)?;
            self.stream.send(data.into()).await
                .map_err(|e| QueueError::Queue(e.to_string()))?;
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_task_queue_basic() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path().to_str().unwrap();
        
        let mut task_queue = TaskQueue::new(base_path).unwrap();
        let mut worker_queue = WorkerQueue::new(base_path).unwrap();
        
        let task = AgentOsTask::new(
            "iri://task/test".to_string(),
            "测试任务".to_string(),
            TaskContextData {
                stage_id: "test".to_string(),
                stage_type: "requirement".to_string(),
                project_id: "proj_1".to_string(),
                project_dir: "/tmp".to_string(),
                user_requirement: "测试".to_string(),
                prev_outputs: HashMap::new(),
                llm_config: LlmConfig::default(),
            },
        );
        
        task_queue.send_task(&task).await.unwrap();
        
        let received = worker_queue.recv_task().await.unwrap();
        assert_eq!(received.task_iri, "iri://task/test");
        
        let result = AgentOsResult::success(task.task_id.clone(), "完成".to_string());
        worker_queue.send_result(&result).await.unwrap();
        
        let received_result = task_queue.recv_result_timeout(Duration::from_secs(5)).await.unwrap().unwrap();
        assert_eq!(received_result.status, "success");
    }
}
