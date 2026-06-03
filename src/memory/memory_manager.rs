use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use tracing::{debug, info};

use crate::memory::l0_store::L0Store;
use crate::memory::l1_session::{L1Session, SessionSummary};
use crate::memory::l2_blackboard::Blackboard;
use crate::memory::l3_projection::ProjectionEngine;
use crate::memory::scheduler::MemoryScheduler;
use crate::{CoreConfig, CoreError};

/// 协调全部四层记忆 (L0/L1/L2/L3)
///
/// 记忆生命周期:
/// L1 Session → (压缩) → L2 Blackboard → (归档) → L0 持久化
///                                                  → L3 投影 (按需)
pub struct MemoryManager {
    l0: Arc<L0Store>,
    l2: Arc<Blackboard>,
    projection: Arc<ProjectionEngine>,
    config: CoreConfig,
    sessions: HashMap<String, L1Session>,
    scheduler: Option<Arc<MemoryScheduler>>,
    l1_active_count: AtomicU64,
}

impl MemoryManager {
    pub fn new(
        l0: Arc<L0Store>,
        l2: Arc<Blackboard>,
        projection: Arc<ProjectionEngine>,
        config: CoreConfig,
    ) -> Self {
        info!("MemoryManager initialized");
        Self {
            l0,
            l2,
            projection,
            config,
            sessions: HashMap::new(),
            scheduler: None,
            l1_active_count: AtomicU64::new(0),
        }
    }

    /// 使用 MemoryScheduler 构造 MemoryManager
    ///
    /// 当 scheduler 存在时, session 变更会同步到 scheduler,
    /// 使 scheduler 能执行上下文请求、溢出处理等高层操作。
    pub fn with_scheduler(
        l0: Arc<L0Store>,
        l2: Arc<Blackboard>,
        projection: Arc<ProjectionEngine>,
        config: CoreConfig,
        scheduler: Arc<MemoryScheduler>,
    ) -> Self {
        info!("MemoryManager initialized (with scheduler)");
        Self {
            l0,
            l2,
            projection,
            config,
            sessions: HashMap::new(),
            scheduler: Some(scheduler),
            l1_active_count: AtomicU64::new(0),
        }
    }

    /// 运行时设置 scheduler（用于延迟注入场景）
    pub fn set_scheduler(&mut self, scheduler: Arc<MemoryScheduler>) {
        self.scheduler = Some(scheduler);
    }

    /// 获取 scheduler 引用
    pub fn scheduler(&self) -> Option<&Arc<MemoryScheduler>> {
        self.scheduler.as_ref()
    }

    /// 获取 L3 ProjectionEngine 引用
    pub fn projection(&self) -> &Arc<ProjectionEngine> {
        &self.projection
    }

    // ========== L1 Session 管理 ==========

    /// 创建新的 L1 session
    pub fn create_session(&mut self, agent_id: &str, agent_role: &str, task_iri: &str) -> L1Session {
        let session = match self.config.eviction_config {
            Some(cfg) => L1Session::with_config(agent_id, agent_role, task_iri, 2000, cfg),
            None => L1Session::new(agent_id, agent_role, task_iri),
        };
        self.l1_active_count.fetch_add(1, Ordering::Relaxed);
        debug!(
            session_id = %session.session_id(),
            agent_id = %agent_id,
            "L1 session created"
        );
        session
    }

    /// 将 session 注册到管理器, 返回 session_id
    ///
    /// 当 scheduler 存在时, 同时同步到 scheduler 以支持其高层操作。
    pub fn track_session(&mut self, session: L1Session) -> String {
        let id = session.session_id().to_string();
        if let Some(ref scheduler) = self.scheduler {
            scheduler.insert_session(session);
        } else {
            self.sessions.insert(id.clone(), session);
        }
        self.l1_active_count.fetch_add(1, Ordering::Relaxed);
        id
    }

    /// 按 ID 获取 session 的不可变引用
    pub fn get_session(&self, session_id: &str) -> Option<&L1Session> {
        self.sessions.get(session_id)
    }

    /// 按 ID 获取 session 的可变引用
    pub fn get_session_mut(&mut self, session_id: &str) -> Option<&mut L1Session> {
        self.sessions.get_mut(session_id)
    }

    /// 压缩并关闭 session, 返回会话摘要
    pub fn close_session(&mut self, session_id: &str) -> Result<SessionSummary, CoreError> {
        let result = if let Some(ref scheduler) = self.scheduler {
            let session = scheduler.remove_session(session_id).ok_or_else(|| CoreError::Internal {
                message: format!("Session not found: {}", session_id),
            })?;
            let summary = session.summarize();
            info!(
                session_id = %session_id,
                turn_count = summary.turn_count,
                "L1 session closed (via scheduler)"
            );
            Ok(summary)
        } else {
            let session = self.sessions.remove(session_id).ok_or_else(|| CoreError::Internal {
                message: format!("Session not found: {}", session_id),
            })?;
            let summary = session.summarize();
            info!(
                session_id = %session_id,
                turn_count = summary.turn_count,
                "L1 session closed"
            );
            Ok(summary)
        };
        if result.is_ok() {
            self.l1_active_count.fetch_sub(1, Ordering::Relaxed);
        }
        result
    }

    /// 当前活跃 session 数量
    pub fn session_count(&self) -> usize {
        if let Some(ref scheduler) = self.scheduler {
            scheduler.session_count()
        } else {
            self.sessions.len()
        }
    }

    /// Lock-free 活跃 session 计数（通过原子计数器维护）
    pub fn l1_session_count(&self) -> u64 {
        self.l1_active_count.load(Ordering::Relaxed)
    }

    // ========== L2/L0 归档 ==========

    /// 将 session 摘要归档到 L2 黑板
    pub fn archive_to_l2(&self, _task_iri: &str, summary: &SessionSummary) -> Result<(), CoreError> {
        let json_ld = serde_json::json!({
            "@context": "https://pdca-agent.org/context/memory",
            "@id": format!("iri://memory/{}", uuid::Uuid::new_v4().hyphenated()),
            "@type": "SessionSummary",
            "session_id": summary.session_id,
            "agent_id": summary.agent_id,
            "agent_role": summary.agent_role,
            "task_iri": summary.task_iri,
            "turn_count": summary.turn_count,
            "summary_text": summary.summary_text,
        })
        .to_string();

        self.l2
            .write_node(&format!("iri://session/{}", summary.session_id), &json_ld, &self.config)
    }

    /// 将摘要归档到 L0 永久存储
    pub fn archive_to_l0(&self, summary: &SessionSummary) -> Result<(), CoreError> {
        let iri = format!("iri://archive/session/{}", summary.session_id);
        let content = serde_json::json!({
            "session_id": summary.session_id,
            "agent_id": summary.agent_id,
            "agent_role": summary.agent_role,
            "task_iri": summary.task_iri,
            "turn_count": summary.turn_count,
            "summary_text": summary.summary_text,
        })
        .to_string();

        self.l0.store(&iri, &content)
    }

    // ========== L3 投影 ==========

    /// 获取指定 agent 角色的投影 (同步包装, 内部异步)
    pub fn get_projection(
        &self,
        task_iri: &str,
        frame_name: &str,
    ) -> Result<Option<String>, CoreError> {
        let params = HashMap::new();
        let handle = tokio::runtime::Handle::try_current();

        match handle {
            Ok(_h) => {
                let frame = self.projection.get_frame(frame_name);
                let actual_frame = if frame.is_some() { frame_name } else { "reference_only" };
                let proj = self.projection.clone();
                let task_iri = task_iri.to_string();
                let actual_frame = actual_frame.to_string();
                
                let result = tokio::task::block_in_place(|| {
                    tokio::runtime::Handle::current().block_on(async {
                        proj.project(&task_iri, &actual_frame, params).await
                    })
                })?;
                Ok(Some(result))
            }
            Err(_) => {
                let frames: Vec<String> = self.projection.list_frames().iter().map(|f| f.name.clone()).collect();
                let result = serde_json::json!({
                    "@context": "https://pdca-agent.org/context/projection",
                    "note": "Async runtime not available, returning frame list",
                    "available_frames": frames,
                }).to_string();
                Ok(Some(result))
            }
        }
    }

    // ========== 统一存储接口 ==========

    /// 统一存储接口：根据层级存储数据
    pub fn store(&self, agent_id: &str, key: &str, value: &str, layer: &str) -> Result<String, CoreError> {
        match layer {
            "L0" | "l0" => {
                let iri = format!("iri://{}/{}", agent_id, key);
                self.l0.store(&iri, value)?;
                Ok(iri)
            }
            "L1" | "l1" => {
                Err(CoreError::Internal {
                    message: "L1 layer does not support direct key-value storage; use session APIs instead".to_string(),
                })
            }
            "L2" | "l2" => {
                let iri = format!("iri://{}/{}", agent_id, key);
                self.l2.write_node(&iri, value, &self.config)?;
                Ok(iri)
            }
            _ => Err(CoreError::Internal {
                message: format!("不支持的存储层: {}", layer),
            }),
        }
    }

    /// 统一检索接口：从指定层检索数据
    pub fn retrieve(&self, key: &str, layers: &[&str]) -> Result<Option<String>, CoreError> {
        for layer in layers {
            match *layer {
                "L0" | "l0" => {
                    if let Some(entry) = self.l0.retrieve(key)? {
                        return Ok(Some(entry.content));
                    }
                }
                "L2" | "l2" => {
                    if let Some(node) = self.l2.read_node(key)? {
                        return Ok(Some(node.json_ld.clone()));
                    }
                }
                _ => {}
            }
        }
        Ok(None)
    }

    /// 归档 L1 会话到 L0
    ///
    /// 如果 scheduler 存在，通过 scheduler 的 `on_session_close` 执行归档，
    /// 确保一致性引擎的失效传播和投影缓存清理。
    pub fn archive_session(&self, session_id: &str) -> Result<(), CoreError> {
        if let Some(ref scheduler) = self.scheduler {
            let session = scheduler.remove_session(session_id).ok_or_else(|| CoreError::Internal {
                message: format!("Session not found: {}", session_id),
            })?;
            let summary = session.summarize();
            self.archive_to_l0(&summary)?;
            self.archive_to_l2(&summary.task_iri, &summary)?;
            Ok(())
        } else {
            let session = self.sessions.get(session_id)
                .ok_or_else(|| CoreError::Internal {
                    message: format!("Session not found: {}", session_id),
                })?;
            let summary = session.summarize();
            self.archive_to_l0(&summary)?;
            self.archive_to_l2(&summary.task_iri, &summary)?;
            Ok(())
        }
    }

    /// 完成并归档一个外部持有的 L1Session（绕过 track_session/close_session 流程）
    ///
    /// 适用于 AgentRunner 等直接持有 session 所有权的调用方。
    /// 自动完成: track → close → archive_to_l2 → archive_to_l0
    pub fn finalize_session(&mut self, session: L1Session, task_iri: &str) -> Result<(), CoreError> {
        let session_id = session.session_id().to_string();
        self.track_session(session);
        let summary = self.close_session(&session_id)?;
        self.archive_to_l2(task_iri, &summary)?;
        self.archive_to_l0(&summary)?;
        info!(
            session_id = %session_id,
            task_iri = %task_iri,
            "Session finalized and archived"
        );
        Ok(())
    }

    /// 同步跨层数据
    pub fn sync_layers(&self, iri: &str) -> Result<(), CoreError> {
        if let Some(entry) = self.l0.retrieve(iri)? {
            self.l2.write_node(iri, &entry.content, &self.config)?;
        }
        Ok(())
    }

    // ========== 记忆统计 ==========

    /// 获取记忆系统统计信息
    pub fn stats(&self) -> serde_json::Value {
        serde_json::json!({
            "l0_entries": self.l0.count(),
            "l2_nodes": self.l2.node_count(),
            "l2_bytes": self.l2.total_bytes(),
            "active_sessions": self.session_count(),
        })
    }

    // ========== Agent 态势感知委派 ==========

    /// 注册 Agent 到作战地图
    pub fn register_agent(&self, agent_id: &str, role: &str, task_iri: &str) {
        self.l2.register_agent(agent_id, role, task_iri);
    }

    /// 更新 Agent 心跳
    pub fn update_agent_heartbeat(&self, agent_id: &str) {
        self.l2.update_agent_heartbeat(agent_id);
    }

    /// 更新 Agent 状态
    pub fn update_agent_status(&self, agent_id: &str, status: crate::memory::AgentActivity, operation: Option<&str>) {
        self.l2.update_agent_status(agent_id, status, operation);
    }

    /// 获取 Agent 状态
    pub fn get_agent_status(&self, agent_id: &str) -> Option<crate::memory::AgentStatus> {
        self.l2.get_agent_status(agent_id)
    }

    /// 列出活跃 Agent
    pub fn list_active_agents(&self) -> Vec<crate::memory::AgentStatus> {
        self.l2.list_active_agents()
    }

    /// 注销 Agent
    pub fn unregister_agent(&self, agent_id: &str) {
        self.l2.unregister_agent(agent_id);
    }

    /// 检测心跳超时的 Agent
    pub fn detect_stale_agents(&self, max_idle_seconds: i64) -> Vec<String> {
        self.l2.detect_stale_agents(max_idle_seconds)
    }

    /// 获取 Blackboard 引用
    pub fn blackboard(&self) -> &Arc<Blackboard> {
        &self.l2
    }
}
