use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tracing::debug;

use crate::memory::l0_store::{L0Store, MesiState};
use crate::CoreError;

/// L1 单轮摘要记录
///
/// L1 仅存储 LLM 响应的 `summary` 字段。
/// 完整的 `thought` + `content` 通过 `archive_full()` 归档至 L0。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct L1Turn {
    pub role: String,
    pub summary: String,
    pub timestamp: DateTime<Utc>,
    /// L0 中归档完整 thought+content 的 IRI
    pub l0_archive_iri: Option<String>,
    /// 语义向量，用于相关度计算
    #[serde(default)]
    pub embedding: Option<Vec<f32>>,
}

/// L1 会话 — 单 Agent 摘要链
///
/// 设计:
/// - 每个 LLM 响应仅存储 `summary` 字段
/// - 完整 `thought` + `content` 归档至 L0
/// - 构建上下文时仅使用摘要 (节省令牌)
/// - 完整细节可从 L0 按需重载
/// - 内置令牌预算机制, 超出时按策略自动驱逐
///
/// 多轮对话的摘要链格式:
/// ```text
/// [Session History]
/// [agent_A] Step 1 completed: found the main issue
/// [agent_A] Step 2 completed: applied the fix
/// ```
#[derive(Debug, Clone)]
pub struct L1Session {
    session_id: String,
    agent_id: String,
    agent_role: String,
    task_iri: String,
    turns: Vec<L1Turn>,
    created_at: DateTime<Utc>,
    token_budget: usize,
    current_tokens: usize,
    /// 淘汰的 IRI 弱引用列表，用于缺页中断重载
    weak_refs: Vec<String>,
    /// MESI 缓存一致性状态（L1 作为 S/I 状态持有者）
    mesi_state: MesiState,
}

impl L1Session {
    pub fn new(agent_id: &str, agent_role: &str, task_iri: &str) -> Self {
        Self::with_budget(agent_id, agent_role, task_iri, 4000)
    }

    pub fn with_budget(agent_id: &str, agent_role: &str, task_iri: &str, token_budget: usize) -> Self {
        Self {
            session_id: format!("l1_{}", uuid::Uuid::new_v4().hyphenated()),
            agent_id: agent_id.to_string(),
            agent_role: agent_role.to_string(),
            task_iri: task_iri.to_string(),
            turns: Vec::new(),
            created_at: Utc::now(),
            token_budget,
            current_tokens: 0,
            weak_refs: Vec::new(),
            mesi_state: MesiState::Shared,
        }
    }

    pub fn session_id(&self) -> &str { &self.session_id }
    pub fn agent_id(&self) -> &str { &self.agent_id }
    pub fn agent_role(&self) -> &str { &self.agent_role }
    pub fn task_iri(&self) -> &str { &self.task_iri }
    pub fn turn_count(&self) -> usize { self.turns.len() }
    pub fn created_at(&self) -> &DateTime<Utc> { &self.created_at }
    pub fn duration(&self) -> chrono::Duration { Utc::now() - self.created_at }
    pub fn token_budget(&self) -> usize { self.token_budget }

    pub fn set_token_budget(&mut self, budget: usize) {
        self.token_budget = budget;
        if self.current_tokens > self.token_budget {
            self.evict_by_policy();
        }
    }

    /// 按驱逐策略淘汰超出令牌预算的轮次
    ///
    /// 策略: 保留第一个 turn, 淘汰得分最低的 turn。
    /// 得分 = (1 / 距上次访问秒数) * 0.3 + (1 / 语义相关度) * 0.4 + token_cost * 0.3
    /// 得分越低越应被淘汰。
    pub fn evict_by_policy(&mut self) -> usize {
        if self.current_tokens <= self.token_budget || self.turns.len() <= 1 {
            return 0;
        }

        let now = Utc::now();
        let mut evicted = 0;

        while self.current_tokens > self.token_budget && self.turns.len() > 1 {
            let mut min_idx = None;
            let mut min_score = f64::MAX;
            for (i, t) in self.turns.iter().enumerate().skip(1) {
                let time_since = (now - t.timestamp).num_seconds().max(1) as f64;
                let token_cost = (t.summary.len() as f64 * 0.3) as f64;
                
                let semantic_relevance = if let Some(ref embedding) = t.embedding {
                    (embedding.iter().map(|x| x * x).sum::<f32>().sqrt() as f64).max(0.1)
                } else {
                    0.5
                };
                
                let score = (1.0 / time_since) * 0.3 + (1.0 / semantic_relevance) * 0.4 + token_cost * 0.3;
                if score < min_score {
                    min_score = score;
                    min_idx = Some(i);
                }
            }

            if let Some(idx) = min_idx {
                let removed = self.turns.remove(idx);
                self.current_tokens -= (removed.summary.len() as f64 * 0.3) as usize;
                
                if let Some(iri) = removed.l0_archive_iri {
                    self.weak_refs.push(iri);
                }
                
                evicted += 1;
            } else {
                break;
            }
        }

        evicted
    }

    /// 尝试从 L0 重载指定 IRI 的内容到 L1 会话
    pub fn try_reload_from_l0(&mut self, l0_store: &L0Store, iri: &str) -> bool {
        if let Ok(Some(entry)) = l0_store.retrieve(iri) {
            let summary = if entry.content.len() > 200 {
                entry.content.chars().take(200).collect()
            } else {
                entry.content.clone()
            };
            self.add_summary("system", &format!("[重载] {}", summary), Some(iri.to_string()));
            true
        } else {
            false
        }
    }

    /// 存储 LLM `summary` 字段到 L1。
    /// thought+content 应通过 archive_full() 单独归档至 L0。
    /// 添加后自动检查令牌预算, 超出则触发驱逐。
    pub fn add_summary(&mut self, role: &str, summary: &str, l0_archive_iri: Option<String>) -> &L1Turn {
        let turn = L1Turn {
            role: role.to_string(),
            summary: summary.to_string(),
            timestamp: Utc::now(),
            l0_archive_iri,
            embedding: None,
        };
        let token_cost = (summary.len() as f64 * 0.3) as usize;
        self.current_tokens += token_cost;
        self.turns.push(turn);

        if self.current_tokens > self.token_budget {
            self.evict_by_policy();
        }

        self.turns.last().unwrap()
    }

    /// 归档完整 thought+content 到 L0 并返回归档 IRI。
    /// 在 assistant turn 添加之后调用。
    pub fn archive_full_to_l0(
        &self,
        l0_store: &L0Store,
        role: &str,
        thought: &str,
        content_json: &str,
    ) -> Result<String, CoreError> {
        let iri = format!(
            "iri://archive/{}/{}/{}",
            self.task_iri.strip_prefix("iri://").unwrap_or(&self.task_iri),
            role,
            uuid::Uuid::new_v4().hyphenated()
        );
        let payload = serde_json::json!({
            "@id": &iri,
            "@type": "LLMResponse",
            "role": role,
            "agent_id": self.agent_id,
            "session_id": self.session_id,
            "thought": thought,
            "content": serde_json::from_str::<serde_json::Value>(content_json).ok(),
            "timestamp": Utc::now().to_rfc3339(),
        });
        l0_store.store(&iri, &payload.to_string())?;
        debug!(iri = %iri, "Archived full LLM response to L0");
        Ok(iri)
    }

    /// 获取摘要链用于 LLM 上下文构建。
    /// 返回前确保令牌预算满足。
    pub fn get_summary_chain(&mut self) -> Vec<serde_json::Value> {
        if self.turns.is_empty() {
            return Vec::new();
        }

        if self.current_tokens > self.token_budget {
            self.evict_by_policy();
        }

        let summaries: Vec<String> = self
            .turns
            .iter()
            .map(|t| format!("[{}] {}", t.role, t.summary))
            .collect();
        vec![serde_json::json!({
            "role": "system",
            "content": format!(
                "[Previous context from {} ({})]\n{}",
                self.agent_id,
                self.agent_role,
                summaries.join("\n")
            )
        })]
    }

    /// 构建紧凑摘要字符串, 用于 Agent 间交接 (L1→下一个 L1)
    pub fn handoff_summary(&self) -> String {
        if self.turns.is_empty() {
            return format!(
                "Agent {} ({}) ran with {} turns.",
                self.agent_id, self.agent_role, self.turns.len()
            );
        }
        let summaries: Vec<String> = self
            .turns
            .iter()
            .map(|t| format!("[{}] {}", t.role, t.summary))
            .collect();
        format!(
            "From {} ({}):\n{}",
            self.agent_id,
            self.agent_role,
            summaries.join("\n")
        )
    }

    /// 当前会话的估算令牌消耗
    pub fn estimated_tokens(&self) -> u32 {
        self.current_tokens as u32
    }

    /// 汇总会话状态
    pub fn summarize(&self) -> SessionSummary {
        SessionSummary {
            session_id: self.session_id.clone(),
            agent_id: self.agent_id.clone(),
            agent_role: self.agent_role.clone(),
            task_iri: self.task_iri.clone(),
            turn_count: self.turns.len(),
            created_at: self.created_at,
            summary_text: self.handoff_summary(),
        }
    }

    pub fn clear(&mut self) {
        self.turns.clear();
        self.weak_refs.clear();
        self.current_tokens = 0;
    }

    /// 获取弱引用列表
    pub fn get_weak_refs(&self) -> &[String] {
        &self.weak_refs
    }

    /// 从弱引用列表重新加载到 L1
    pub fn reload_from_weak_refs(&mut self, l0_store: &L0Store) -> usize {
        let mut reloaded = 0;
        let refs_to_reload: Vec<String> = self.weak_refs.drain(..).collect();

        for iri in refs_to_reload {
            if self.try_reload_from_l0(l0_store, &iri) {
                reloaded += 1;
            }
        }

        reloaded
    }

    /// 设置 turn 的 embedding（用于语义相关度计算）
    pub fn set_turn_embedding(&mut self, turn_idx: usize, embedding: Vec<f32>) {
        if let Some(turn) = self.turns.get_mut(turn_idx) {
            turn.embedding = Some(embedding);
        }
    }

    /// 获取 MESI 状态
    pub fn mesi_state(&self) -> MesiState {
        self.mesi_state
    }

    /// 设置 MESI 状态
    pub fn set_mesi_state(&mut self, state: MesiState) {
        self.mesi_state = state;
    }

    /// 使缓存失效（将状态设为 Invalid）
    pub fn invalidate(&mut self) {
        self.mesi_state = MesiState::Invalid;
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSummary {
    pub session_id: String,
    pub agent_id: String,
    pub agent_role: String,
    pub task_iri: String,
    pub turn_count: usize,
    pub created_at: DateTime<Utc>,
    pub summary_text: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_summary_only_session() {
        let mut session = L1Session::new("agent_1", "DA", "iri://task/abc");
        session.add_summary("assistant", "Found the root cause in config.rs", None);
        session.add_summary("assistant", "Applied the fix and verified", None);
        assert_eq!(session.turn_count(), 2);

        let chain = session.get_summary_chain();
        assert_eq!(chain.len(), 1);
        let content = chain[0]["content"].as_str().unwrap();
        assert!(content.contains("Found the root cause"));
        assert!(content.contains("Applied the fix"));
    }

    #[test]
    fn test_handoff_is_compact() {
        let mut session = L1Session::new("agent_1", "DA", "iri://task/abc");
        session.add_summary("assistant", "Completed analysis", None);
        let handoff = session.handoff_summary();
        assert_eq!(handoff.lines().count(), 2);
        assert!(handoff.contains("agent_1"));
        assert!(handoff.contains("Completed analysis"));
    }

    #[test]
    fn test_default_token_budget() {
        let session = L1Session::new("agent_1", "DA", "iri://task/abc");
        assert_eq!(session.token_budget(), 4000);
        assert_eq!(session.estimated_tokens(), 0);
    }

    #[test]
    fn test_with_budget() {
        let session = L1Session::with_budget("agent_1", "DA", "iri://task/abc", 1000);
        assert_eq!(session.token_budget(), 1000);
    }

    #[test]
    fn test_token_tracking() {
        let mut session = L1Session::new("agent_1", "DA", "iri://task/abc");
        session.add_summary("assistant", "Hello world", None);
        assert!(session.estimated_tokens() > 0);
    }

    #[test]
    fn test_eviction_on_budget_exceeded() {
        let mut session = L1Session::with_budget("agent_1", "DA", "iri://task/abc", 10);
        session.add_summary("assistant", "First turn that stays", None);
        session.add_summary("assistant", "Second turn with content", None);
        session.add_summary("assistant", "Third turn more content here", None);
        assert!(session.current_tokens <= session.token_budget || session.turns.len() <= 1);
    }

    #[test]
    fn test_set_token_budget_triggers_eviction() {
        let mut session = L1Session::with_budget("agent_1", "DA", "iri://task/abc", 10000);
        session.add_summary("assistant", "First turn content here", None);
        session.add_summary("assistant", "Second turn content here", None);
        session.add_summary("assistant", "Third turn content here", None);
        session.set_token_budget(10);
        assert!(session.current_tokens <= session.token_budget || session.turns.len() <= 1);
    }

    #[test]
    fn test_clear_resets_tokens() {
        let mut session = L1Session::new("agent_1", "DA", "iri://task/abc");
        session.add_summary("assistant", "Some content", None);
        assert!(session.estimated_tokens() > 0);
        session.clear();
        assert_eq!(session.estimated_tokens(), 0);
    }
}
