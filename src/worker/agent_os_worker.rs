use std::sync::Arc;
use std::time::Instant;

use tracing::{info, warn, error};

use crate::core::{SupervisorAgent, AgentRunner, CoreConfig};
use crate::memory::consistency_engine::ConsistencyEngine;
use crate::memory::memory_bus::MemoryBus;
use crate::memory::prefetch_engine::PrefetchEngine;
use crate::memory::scheduler::MemoryScheduler;
use crate::memory::{L0Store, Blackboard, MemoryManager, ProjectionEngine};
use crate::gateway::UnifiedGateway;
use crate::templates::TemplateEngine;
use crate::tools::SkillRegistry;
use crate::tools::hooks::{
    HookManager, HumanApprovalHook, HumanApprovalConfig,
    ApprovalPoint, ApprovalCondition, ChannelApprovalNotifier,
};
use crate::config::GatewaySettings;
use crate::core::EventBus;

use super::task_queue::{WorkerQueue, AgentOsTask, AgentOsResult, QueueError};

/// Agent OS Worker 配置
#[derive(Debug, Clone)]
pub struct WorkerConfig {
    /// 队列基础路径
    pub queue_base_path: String,
    /// L0 存储路径
    pub l0_path: String,
    /// 并发数
    pub concurrency: usize,
    /// LLM 网关配置
    pub gateway: Option<GatewaySettings>,
    /// Human Approval 配置
    pub approval_config: Option<HumanApprovalConfig>,
}

impl Default for WorkerConfig {
    fn default() -> Self {
        Self {
            queue_base_path: "./data/agent_os_queue".to_string(),
            l0_path: "./data/l0".to_string(),
            concurrency: 4,
            gateway: None,
            approval_config: None,
        }
    }
}

impl WorkerConfig {
    pub fn from_env() -> Self {
        let gateway = std::env::var("ONE_API_URL").ok().map(|base_url| {
            let api_key = std::env::var("ONE_API_KEY").unwrap_or_default();
            GatewaySettings {
                base_url,
                api_key,
                default_model: "deepseek-chat".to_string(),
                timeout_seconds: 300,
                max_retries: 3,
                model_mapping: Default::default(),
            }
        });
        
        let approval_config = if std::env::var("AGENT_OS_APPROVAL_ENABLED")
            .ok()
            .map(|v| v == "true" || v == "1")
            .unwrap_or(false)
        {
            Some(HumanApprovalConfig {
                enabled: true,
                approval_points: vec![ApprovalPoint {
                    hook_point: crate::tools::hooks::HookPoint::PhaseEnd,
                    condition: ApprovalCondition::OnStageComplete,
                    message_template: "阶段 {stage} 完成，请确认是否继续".to_string(),
                    timeout_seconds: std::env::var("AGENT_OS_APPROVAL_TIMEOUT")
                        .ok()
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(3600),
                    default_action: crate::tools::hooks::DefaultAction::Approve,
                    stages: Vec::new(),
                }],
                default_timeout_seconds: 3600,
                default_action: crate::tools::hooks::DefaultAction::Approve,
            })
        } else {
            None
        };
        
        Self {
            queue_base_path: std::env::var("AGENT_OS_QUEUE_PATH")
                .unwrap_or_else(|_| "./data/agent_os_queue".to_string()),
            l0_path: std::env::var("AGENT_OS_L0_PATH")
                .unwrap_or_else(|_| "./data/l0".to_string()),
            concurrency: std::env::var("AGENT_OS_CONCURRENCY")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(4),
            gateway,
            approval_config,
        }
    }
}

/// Agent OS Worker
pub struct AgentOsWorker {
    config: WorkerConfig,
    queue: WorkerQueue,
    sa: SupervisorAgent,
    approval_notifier: Option<Arc<ChannelApprovalNotifier>>,
}

impl AgentOsWorker {
    /// 创建新的 Worker
    pub fn new(config: WorkerConfig) -> Result<Self, QueueError> {
        let queue = WorkerQueue::new(&config.queue_base_path)?;
        
        let l0 = Arc::new(L0Store::new(&config.l0_path)
            .map_err(|e| QueueError::Queue(format!("创建 L0 失败: {}", e)))?);
        
        let blackboard = Arc::new(Blackboard::new()
            .map_err(|e| QueueError::Queue(format!("创建 Blackboard 失败: {}", e)))?);
        
        let gateway_settings = config.gateway.clone().unwrap_or_else(|| {
            GatewaySettings {
                base_url: std::env::var("DEEPSEEK_API_URL")
                    .or_else(|_| std::env::var("ONE_API_URL"))
                    .unwrap_or_else(|_| "https://api.deepseek.com".to_string()),
                api_key: std::env::var("DEEPSEEK_API_KEY")
                    .or_else(|_| std::env::var("ONE_API_KEY"))
                    .unwrap_or_default(),
                default_model: "deepseek-chat".to_string(),
                timeout_seconds: 300,
                max_retries: 3,
                model_mapping: Default::default(),
            }
        });
        
        let gateway = Arc::new(UnifiedGateway::new(&gateway_settings)
            .map_err(|e| QueueError::Queue(format!("创建 Gateway 失败: {}", e)))?);
        
        let templates_dir = std::env::temp_dir();
        let templates_engine = Arc::new(TemplateEngine::new(&templates_dir)
            .map_err(|e| QueueError::Queue(format!("创建模板引擎失败: {}", e)))?);
        
        let skills = Arc::new(SkillRegistry::new());
        
        let projection_engine = Arc::new(ProjectionEngine::new(blackboard.clone(), 500));

        let memory_bus = Arc::new(MemoryBus::new(Arc::new(EventBus::new(100))));
        let consistency = Arc::new(ConsistencyEngine::new(
            memory_bus.clone(), l0.clone(), blackboard.clone(), projection_engine.clone(),
        ));
        let scheduler = Arc::new(MemoryScheduler::new(
            l0.clone(), blackboard.clone(), projection_engine.clone(), consistency.clone(), memory_bus.clone(),
        ));
        let prefetch = Arc::new(PrefetchEngine::new(
            memory_bus.clone(), blackboard.clone(), projection_engine.clone(),
        ));

        let memory_manager = Arc::new(tokio::sync::Mutex::new(
            MemoryManager::with_scheduler(
                l0.clone(),
                blackboard.clone(),
                projection_engine,
                CoreConfig::default(),
                scheduler.clone(),
            )
        ));
        
        let mut hook_manager = HookManager::new();
        let mut approval_notifier = None;
        
        if let Some(ref approval_cfg) = config.approval_config {
            if approval_cfg.enabled {
                let (hook, notifier) = HumanApprovalHook::with_channel_notifier(approval_cfg.clone());
                hook_manager.register(hook);
                approval_notifier = Some(notifier);
                info!("HumanApprovalHook 已注册");
            }
        }
        
        let runner = Arc::new(AgentRunner::new(
            gateway,
            skills.clone(),
            blackboard.clone(),
            l0,
            memory_manager,
            templates_engine.clone(),
            crate::config::AgentSettings::default(),
        ).with_hook_manager(hook_manager));
        
        let sa = SupervisorAgent::new(
            runner,
            templates_engine,
            skills,
            Arc::new(EventBus::new(100)),
            20,
        ).with_memory(Some(blackboard), Some(prefetch), Some(scheduler));
        
        Ok(Self { config, queue, sa, approval_notifier })
    }
    
    /// 获取确认通知器（用于外部提交确认）
    pub fn approval_notifier(&self) -> Option<&Arc<ChannelApprovalNotifier>> {
        self.approval_notifier.as_ref()
    }
    
    /// 运行 Worker 主循环
    pub async fn run(&mut self) -> Result<(), QueueError> {
        info!(
            queue_path = %self.config.queue_base_path,
            concurrency = self.config.concurrency,
            approval_enabled = self.approval_notifier.is_some(),
            "Agent OS Worker 启动"
        );
        
        loop {
            match self.queue.recv_task().await {
                Ok(task) => {
                    info!(task_id = %task.task_id, task_iri = %task.task_iri, "收到任务");
                    
                    let result = self.execute_task(task).await;
                    
                    if let Err(e) = self.queue.send_result(&result).await {
                        error!(error = %e, "发送结果失败");
                    }
                }
                Err(e) => {
                    error!(error = %e, "接收任务失败");
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                }
            }
        }
    }
    
    /// 执行单个任务
    async fn execute_task(&mut self, task: AgentOsTask) -> AgentOsResult {
        let start = Instant::now();
        let original_task_id = task.task_id.clone();
        
        info!(task_id = %original_task_id, "开始执行任务");
        
        match self.sa.process_task(&task.prompt, &task.task_iri).await {
            Ok(task_result) => {
                let duration_ms = start.elapsed().as_millis() as u64;
                info!(
                    task_id = %original_task_id,
                    status = %task_result.status,
                    duration_ms = duration_ms,
                    "任务执行完成"
                );
                
                let mut result = AgentOsResult::from(task_result);
                result.task_id = original_task_id;
                result.duration_ms = duration_ms;
                result
            }
            Err(e) => {
                let duration_ms = start.elapsed().as_millis() as u64;
                error!(task_id = %original_task_id, error = %e, duration_ms = duration_ms, "任务执行失败");
                
                AgentOsResult {
                    task_id: original_task_id,
                    status: "failed".to_string(),
                    summary: format!("任务执行失败: {}", e),
                    output: None,
                    jsonld_output: None,
                    artifacts: Vec::new(),
                    errors: vec![e.to_string()],
                    duration_ms,
                    tool_call_count: 0,
                    turn_count: 0,
                }
            }
        }
    }
}

/// 启动 Worker 的辅助函数
pub async fn run_worker(config: WorkerConfig) -> Result<(), Box<dyn std::error::Error>> {
    let mut worker = AgentOsWorker::new(config)?;
    worker.run().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_worker_creation() {
        let temp_dir = TempDir::new().unwrap();
        
        let config = WorkerConfig {
            queue_base_path: temp_dir.path().join("queue").to_str().unwrap().to_string(),
            l0_path: temp_dir.path().join("l0").to_str().unwrap().to_string(),
            ..Default::default()
        };
        
        let worker = AgentOsWorker::new(config);
        assert!(worker.is_ok());
    }
    
    #[test]
    fn test_worker_with_approval() {
        let temp_dir = TempDir::new().unwrap();
        
        let config = WorkerConfig {
            queue_base_path: temp_dir.path().join("queue").to_str().unwrap().to_string(),
            l0_path: temp_dir.path().join("l0").to_str().unwrap().to_string(),
            approval_config: Some(HumanApprovalConfig {
                enabled: true,
                approval_points: vec![ApprovalPoint::default()],
                default_timeout_seconds: 3600,
                default_action: crate::tools::hooks::DefaultAction::Approve,
            }),
            ..Default::default()
        };
        
        let worker = AgentOsWorker::new(config);
        assert!(worker.is_ok());
        assert!(worker.unwrap().approval_notifier.is_some());
    }
}
