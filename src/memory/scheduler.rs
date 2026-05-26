use std::collections::HashMap;
use std::sync::Arc;

use tracing::debug;

use crate::core::agent_instance::AgentRole;
use crate::memory::consistency_engine::ConsistencyEngine;
use crate::memory::l0_store::L0Store;
use crate::memory::l1_session::L1Session;
use crate::memory::l2_blackboard::Blackboard;
use crate::memory::l3_projection::ProjectionEngine;
use crate::memory::memory_bus::MemoryBus;
use crate::CoreError;

pub struct MemoryScheduler {
    l0_store: Arc<L0Store>,
    blackboard: Arc<Blackboard>,
    projection: Arc<ProjectionEngine>,
    consistency: Arc<ConsistencyEngine>,
    memory_bus: Arc<MemoryBus>,
    sessions: parking_lot::RwLock<HashMap<String, L1Session>>,
}

impl MemoryScheduler {
    pub fn new(
        l0_store: Arc<L0Store>,
        blackboard: Arc<Blackboard>,
        projection: Arc<ProjectionEngine>,
        consistency: Arc<ConsistencyEngine>,
        memory_bus: Arc<MemoryBus>,
    ) -> Self {
        Self {
            l0_store,
            blackboard,
            projection,
            consistency,
            memory_bus,
            sessions: parking_lot::RwLock::new(HashMap::new()),
        }
    }

    pub async fn on_context_request(
        &self,
        agent_role: AgentRole,
        task_iri: &str,
    ) -> Result<String, CoreError> {
        let frame_name = match agent_role {
            AgentRole::Plan => "pa_init",
            AgentRole::Do => "da_input",
            AgentRole::Check => "ca_review",
            AgentRole::Act => "aa_decision",
        };

        let params = HashMap::new();
        let projection_result = self.projection.project(task_iri, frame_name, params).await;

        match projection_result {
            Ok(result) => {
                if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&result) {
                    if let Some(artifacts) = parsed.get("artifacts").and_then(|a| a.as_array()) {
                        if !artifacts.is_empty() {
                            return Ok(result);
                        }
                    }
                } else {
                    return Ok(result);
                }
            }
            Err(_) => {}
        }

        let nodes = self.blackboard.query_nodes(task_iri)?;
        if !nodes.is_empty() {
            let contents: Vec<String> = nodes.iter().map(|n| n.json_ld.clone()).collect();
            return Ok(contents.join("\n"));
        }

        let results = self.l0_store.search(task_iri, 10)?;
        if !results.is_empty() {
            let contents: Vec<String> = results.iter().map(|r| r.content.clone()).collect();
            return Ok(contents.join("\n"));
        }

        Ok(String::new())
    }

    pub fn on_l1_overflow(&self, session_id: &str) -> Result<usize, CoreError> {
        let mut sessions = self.sessions.write();
        let session = sessions.get_mut(session_id).ok_or_else(|| CoreError::Internal {
            message: format!("Session not found: {}", session_id),
        })?;
        Ok(session.evict_by_policy())
    }

    pub async fn on_task_complete(&self, task_iri: &str) -> Result<(), CoreError> {
        self.blackboard.flush_dirty_nodes(&self.l0_store)?;
        self.blackboard.release_subtree(task_iri)?;
        self.memory_bus.publish("TASK_COMPLETED", task_iri, "{}").await;
        Ok(())
    }

    pub fn on_session_close(&self, session_id: &str) -> Result<(), CoreError> {
        let session = {
            let mut sessions = self.sessions.write();
            sessions.remove(session_id).ok_or_else(|| CoreError::Internal {
                message: format!("Session not found: {}", session_id),
            })?
        };

        let summary = session.summarize();
        let config = crate::CoreConfig::default();

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

        self.blackboard.write_node(
            &format!("iri://session/{}", summary.session_id),
            &json_ld,
            &config,
        )?;

        let l0_iri = format!("iri://archive/session/{}", summary.session_id);
        let content = serde_json::json!({
            "session_id": summary.session_id,
            "agent_id": summary.agent_id,
            "agent_role": summary.agent_role,
            "task_iri": summary.task_iri,
            "turn_count": summary.turn_count,
            "summary_text": summary.summary_text,
        })
        .to_string();
        self.l0_store.store(&l0_iri, &content)?;

        debug!(session_id = %session_id, "Session closed and archived to L2+L0");
        Ok(())
    }

    pub fn create_session(
        &self,
        agent_id: &str,
        agent_role: &str,
        task_iri: &str,
        token_budget: usize,
    ) -> String {
        let session = L1Session::with_budget(agent_id, agent_role, task_iri, token_budget);
        let session_id = session.session_id().to_string();
        self.sessions.write().insert(session_id.clone(), session);
        session_id
    }

    pub fn get_session(&self, session_id: &str) -> Option<L1Session> {
        self.sessions.read().get(session_id).cloned()
    }

    pub fn add_summary_to_session(
        &self,
        session_id: &str,
        role: &str,
        summary: &str,
        l0_archive_iri: Option<String>,
    ) {
        let mut sessions = self.sessions.write();
        if let Some(session) = sessions.get_mut(session_id) {
            session.add_summary(role, summary, l0_archive_iri);
        }
    }

    pub fn archive_to_l0(
        &self,
        session_id: &str,
        role: &str,
        thought: &str,
        content: &str,
    ) -> Result<String, CoreError> {
        let sessions = self.sessions.read();
        let session = sessions.get(session_id).ok_or_else(|| CoreError::Internal {
            message: format!("Session not found: {}", session_id),
        })?;
        session.archive_full_to_l0(&self.l0_store, role, thought, content)
    }

    /// 插入已存在的 session（由 MemoryManager 调用，用于同步）
    pub fn insert_session(&self, session: L1Session) {
        let id = session.session_id().to_string();
        self.sessions.write().insert(id, session);
    }

    /// 移除并返回指定 session（由 MemoryManager 调用，用于同步关闭）
    pub fn remove_session(&self, session_id: &str) -> Option<L1Session> {
        self.sessions.write().remove(session_id)
    }

    /// 返回当前 session 数量
    pub fn session_count(&self) -> usize {
        self.sessions.read().len()
    }
}
