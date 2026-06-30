use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tracing::debug;

use crate::memory::l0_store::{L0Store, MesiState};
use crate::CoreError;

/// 余弦相似度计算
///
/// 计算两个等长 f32 向量之间的余弦相似度。
/// 范围: [-1.0, 1.0], 1.0 = 完全相同方向, 0.0 = 正交, -1.0 = 完全相反。
/// 用于 L1 淘汰策略中 turn 与当前查询的语义相关性评估。
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f64 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }
    (dot / (norm_a * norm_b)).clamp(-1.0, 1.0) as f64
}

/// L1 淘汰策略权重配置
///
/// 控制 `evict_by_policy()` 中三项评估指标的权重。
/// 不同 Agent 角色使用不同配置以优化其上下文中保留的内容。
///
/// 公式: `score = recency_weight * (1/time_since) + relevance_weight * (1/semantic_relevance) + cost_weight * token_cost`
///
/// 其中 `semantic_relevance = beta * query_sim + (1-beta) * task_relevance`
///
/// 增强: 新增硬阈值过滤 (relevance_threshold + safe_window_seconds)，
/// 低相关度且超安全窗口的条目直接被淘汰，不参与分数排序。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EvictionConfig {
    pub recency_weight: f64,
    pub relevance_weight: f64,
    pub cost_weight: f64,
    /// 低相关度硬阈值: relevance_score < 此值且超过安全窗口 → 直接淘汰
    pub relevance_threshold: f64,
    /// 安全窗口秒数: 即使低相关度也保留的最小时间
    pub safe_window_seconds: i64,
    /// beta 融合权重: β * query_sim + (1-β) * task_relevance
    pub beta: f64,
}

impl EvictionConfig {
    /// 默认配置 — 适用于 Supervisor (SA)，全面视野
    pub const fn default_sa() -> Self {
        Self { recency_weight: 0.30, relevance_weight: 0.40, cost_weight: 0.30, relevance_threshold: 0.3, safe_window_seconds: 300, beta: 0.7 }
    }

    /// Plan (PA) — 优先保留与计划结构相关的历史
    pub const fn plan() -> Self {
        Self { recency_weight: 0.20, relevance_weight: 0.60, cost_weight: 0.20, relevance_threshold: 0.3, safe_window_seconds: 300, beta: 0.7 }
    }

    /// Do (DA) — 优先保留近期技术细节，平衡 token 成本
    pub const fn do_agent() -> Self {
        Self { recency_weight: 0.35, relevance_weight: 0.30, cost_weight: 0.35, relevance_threshold: 0.3, safe_window_seconds: 300, beta: 0.7 }
    }

    /// Check (CA) — 优先保留审计标准与验证相关
    pub const fn check() -> Self {
        Self { recency_weight: 0.15, relevance_weight: 0.65, cost_weight: 0.20, relevance_threshold: 0.3, safe_window_seconds: 300, beta: 0.7 }
    }

    /// Act (AA) — 均衡配置，略偏向决策上下文
    pub const fn act() -> Self {
        Self { recency_weight: 0.25, relevance_weight: 0.45, cost_weight: 0.30, relevance_threshold: 0.3, safe_window_seconds: 300, beta: 0.7 }
    }

    pub fn for_role(role: &str) -> Self {
        match role {
            "Plan" | "PA" => Self::plan(),
            "Do" | "DA" | "Executor" => Self::do_agent(),
            "Check" | "CA" | "Reviewer" => Self::check(),
            "Act" | "AA" | "Decision" => Self::act(),
            _ => Self::default_sa(),
        }
    }
}

impl Default for EvictionConfig {
    fn default() -> Self {
        Self::default_sa()
    }
}

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
    /// 任务关联度系数 [0,1]，用于增强 eviction 策略
    #[serde(default)]
    pub relevance_score: Option<f64>,
    /// 最后访问时间（用于安全窗口计算）
    #[serde(default)]
    pub last_access: Option<DateTime<Utc>>,
    /// 补充输入标识：true = 用户中间补充，不受硬阈值淘汰
    #[serde(default)]
    pub is_supplement: bool,
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
    eviction_config: EvictionConfig,
    /// 任务级语义向量（从 5W2H.what+why 或 objective 生成）
    /// 用于 evict_with_query 的 fallback query_embedding
    task_embedding: Option<Vec<f32>>,
    /// 用户态会话隔离：发起任务的用户标识（多租户血缘）
    user_id: Option<String>,
    /// 用户态会话隔离：租户标识（多租户血缘）
    tenant_id: Option<String>,
}

impl L1Session {
    pub fn new(agent_id: &str, agent_role: &str, task_iri: &str) -> Self {
        Self::with_budget(agent_id, agent_role, task_iri, 4000)
    }

    pub fn with_budget(agent_id: &str, agent_role: &str, task_iri: &str, token_budget: usize) -> Self {
        let eviction_config = EvictionConfig::for_role(agent_role);
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
            eviction_config,
            task_embedding: None,
            user_id: None,
            tenant_id: None,
        }
    }

    pub fn with_config(agent_id: &str, agent_role: &str, task_iri: &str, token_budget: usize, eviction_config: EvictionConfig) -> Self {
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
            eviction_config,
            task_embedding: None,
            user_id: None,
            tenant_id: None,
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
    pub fn user_id(&self) -> Option<&str> { self.user_id.as_deref() }
    pub fn tenant_id(&self) -> Option<&str> { self.tenant_id.as_deref() }

    /// 设置用户态会话隔离身份（多租户血缘）。
    /// 在 create_session 之后由调用方从 TaskContext 透传。
    pub fn set_identity(&mut self, user_id: Option<String>, tenant_id: Option<String>) {
        self.user_id = user_id;
        self.tenant_id = tenant_id;
    }

    /// 链式设置身份，返回自身。
    pub fn with_identity(mut self, user_id: Option<String>, tenant_id: Option<String>) -> Self {
        self.set_identity(user_id, tenant_id);
        self
    }

    pub fn set_token_budget(&mut self, budget: usize) {
        self.token_budget = budget;
        if self.current_tokens > self.token_budget {
            self.evict_by_policy();
        }
    }

    /// 设置任务级 embedding，用于 evict_with_query 的语义回退
    pub fn set_task_embedding(&mut self, embedding: Vec<f32>) {
        self.task_embedding = Some(embedding);
    }

    pub fn get_task_embedding(&self) -> Option<&[f32]> {
        self.task_embedding.as_deref()
    }

    /// 按驱逐策略淘汰超出令牌预算的轮次
    ///
    /// 策略: 保留第一个 turn, 淘汰得分最低的 turn。
    /// 得分 = recency_weight * (1 / 距上次访问秒数) + relevance_weight * (1 / 语义相关度) + cost_weight * token_cost
    /// 得分越低越应被淘汰。
    pub fn evict_by_policy(&mut self) -> usize {
        self.evict_with_query(None)
    }

    /// 使用可选的 query_embedding 进行语义相关度评估的淘汰
    ///
    /// 策略 (两阶段):
    /// 1. 硬阈值阶段: relevance < threshold 且超过安全窗口 → 直接淘汰（跳过 is_supplement 条目）
    /// 2. 评分阶段: 按 recency/relevance/cost 加权评分淘汰
    ///
    /// semantic_relevance = beta * cosine_sim(query, turn_embedding) + (1-beta) * turn.relevance_score
    pub fn evict_with_query(&mut self, query_embedding: Option<&[f32]>) -> usize {
        if self.current_tokens <= self.token_budget || self.turns.len() <= 1 {
            return 0;
        }

        let now = Utc::now();
        let mut evicted = 0;
        let cfg = &self.eviction_config;

        // 使用传入的 query_embedding，回退到 self.task_embedding
        let query = query_embedding.or(self.task_embedding.as_deref());

        // Phase 1: 硬阈值淘汰 — 低相关 + 超安全窗口 → 直接淘汰
        // is_supplement 条目跳过此阶段，仅参与评分阶段
        if cfg.relevance_threshold > 0.0 {
            let mut i = 1;
            while i < self.turns.len() && self.current_tokens > self.token_budget && self.turns.len() > 1 {
                let t = &self.turns[i];
                if !t.is_supplement {
                    let time_since = (now - t.timestamp).num_seconds();
                    let relevance = t.relevance_score.unwrap_or(0.5);
                    if relevance < cfg.relevance_threshold && time_since > cfg.safe_window_seconds {
                        let removed = self.turns.remove(i);
                        self.current_tokens -= (removed.summary.len() as f64 * 0.3) as usize;
                        if let Some(iri) = removed.l0_archive_iri {
                            self.weak_refs.push(iri);
                        }
                        evicted += 1;
                        continue; // i 不自增，因为 remove 后后续元素前移
                    }
                }
                i += 1;
            }
        }

        // Phase 2: 评分淘汰 — 按 β 融合分数淘汰最低分
        while self.current_tokens > self.token_budget && self.turns.len() > 1 {
            let mut min_idx = None;
            let mut min_score = f64::MAX;
            for (i, t) in self.turns.iter().enumerate().skip(1) {
                let time_since = (now - t.timestamp).num_seconds().max(1) as f64;
                let token_cost = (t.summary.len() as f64 * 0.3) as f64;

                let query_sim = match (query, t.embedding.as_ref()) {
                    (Some(q), Some(e)) if q.len() == e.len() && !q.is_empty() => {
                        cosine_similarity(q, e).abs().max(0.001)
                    }
                    _ => 0.5,
                };
                // β 融合: 当前查询相关度 × β + 任务关联度 × (1-β)
                let task_relevance = t.relevance_score.unwrap_or(query_sim);
                let semantic_relevance = (cfg.beta * query_sim + (1.0 - cfg.beta) * task_relevance).max(0.001);

                let score = (1.0 / time_since) * cfg.recency_weight
                    + (1.0 / semantic_relevance) * cfg.relevance_weight
                    + token_cost * cfg.cost_weight;
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

    /// 存储补充输入到 L1（由 AgentRunner 在 CycleStart 注入时调用）
    ///
    /// 与 add_summary 不同:
    /// - is_supplement = true（不受淘汰硬阈值影响）
    /// - 保留 embedding 和 relevance_score 供 eviction 策略使用
    pub fn add_supplement(
        &mut self,
        role: &str,
        summary: &str,
        embedding: Option<Vec<f32>>,
        relevance_score: Option<f64>,
    ) -> &mut L1Turn {
        let turn = L1Turn {
            role: role.to_string(),
            summary: summary.to_string(),
            timestamp: Utc::now(),
            l0_archive_iri: None,
            embedding,
            relevance_score,
            last_access: Some(Utc::now()),
            is_supplement: true,
        };
        let token_cost = (summary.len() as f64 * 0.3) as usize;
        self.current_tokens += token_cost;
        self.turns.push(turn);

        if self.current_tokens > self.token_budget {
            self.evict_with_query(None);
        }

        self.turns.last_mut().unwrap()
    }

    /// 存储 LLM `summary` 字段到 L1。
    /// thought+content 应通过 archive_full() 单独归档至 L0。
    /// 添加后自动检查令牌预算, 超出则触发驱逐。
    pub fn add_summary(&mut self, role: &str, summary: &str, l0_archive_iri: Option<String>) -> &mut L1Turn {
        let turn = L1Turn {
            role: role.to_string(),
            summary: summary.to_string(),
            timestamp: Utc::now(),
            l0_archive_iri,
            embedding: None,
            relevance_score: None,
            last_access: Some(Utc::now()),
            is_supplement: false,
        };
        let token_cost = (summary.len() as f64 * 0.3) as usize;
        self.current_tokens += token_cost;
        self.turns.push(turn);

        if self.current_tokens > self.token_budget {
            self.evict_by_policy();
        }

        self.turns.last_mut().expect("turn was just pushed above")
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
            "user_id": self.user_id,
            "tenant_id": self.tenant_id,
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

        let threshold = self.eviction_config.relevance_threshold;

        // 按相关度分流：高相关 + supplement 放 main，低相关放 reference
        let main: Vec<String> = self
            .turns
            .iter()
            .filter(|t| t.is_supplement || t.relevance_score.unwrap_or(0.5) >= threshold)
            .map(|t| format!("[{}] {}", t.role, t.summary))
            .collect();

        let mut content = format!(
            "[Previous context from {} ({})]\n{}",
            self.agent_id,
            self.agent_role,
            main.join("\n")
        );

        // 低相关度轮次附加为参考段（仅当有意义且有 low_rel 条目时）
        let low: Vec<String> = self
            .turns
            .iter()
            .filter(|t| !t.is_supplement && t.relevance_score.unwrap_or(0.5) < threshold)
            .map(|t| {
                let truncated: String = t.summary.chars().take(80).collect();
                let score = t.relevance_score.unwrap_or(0.0);
                format!("[{}] {} (相关度: {:.2})", t.role, truncated, score)
            })
            .collect();

        if !low.is_empty() {
            content.push_str("\n\n[历史参考 - 低相关度]\n");
            content.push_str(&low.join("\n"));
        }

        vec![serde_json::json!({
            "role": "system",
            "content": content
        })]
    }

    /// 获取含 IRI 的摘要链，用于消息截断时构建结构化引用摘要。
    /// 每轮摘要截断到 summary_length 字符，附带了 L0 归档 IRI。
    pub fn get_summary_chain_with_iris(&self, max_turns: usize, summary_length: usize) -> Vec<String> {
        self.turns
            .iter()
            .rev()
            .take(max_turns)
            .map(|t| {
                let truncated: String = t.summary.chars().take(summary_length).collect();
                match t.l0_archive_iri {
                    Some(ref iri) => format!("[{}] {} | {}", t.role, truncated, iri),
                    None => format!("[{}] {}", t.role, truncated),
                }
            })
            .collect()
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
            user_id: self.user_id.clone(),
            tenant_id: self.tenant_id.clone(),
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

    pub fn eviction_config(&self) -> &EvictionConfig {
        &self.eviction_config
    }

    pub fn set_eviction_config(&mut self, config: EvictionConfig) {
        self.eviction_config = config;
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
    /// 用户态会话隔离：用户标识（多租户血缘）
    #[serde(default)]
    pub user_id: Option<String>,
    /// 用户态会话隔离：租户标识（多租户血缘）
    #[serde(default)]
    pub tenant_id: Option<String>,
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

    // ========== 余弦相似度测试 ==========

    #[test]
    fn test_cosine_similarity_identical_vectors() {
        let v = vec![1.0, 2.0, 3.0];
        let sim = cosine_similarity(&v, &v);
        assert!((sim - 1.0).abs() < 1e-6, "identical vectors should have similarity 1.0, got {}", sim);
    }

    #[test]
    fn test_cosine_similarity_orthogonal_vectors() {
        let a = vec![1.0, 0.0];
        let b = vec![0.0, 1.0];
        let sim = cosine_similarity(&a, &b);
        assert!((sim - 0.0).abs() < 1e-6, "orthogonal vectors should have similarity 0.0, got {}", sim);
    }

    #[test]
    fn test_cosine_similarity_opposite_vectors() {
        let a = vec![1.0, 2.0];
        let b = vec![-1.0, -2.0];
        let sim = cosine_similarity(&a, &b);
        assert!((sim - (-1.0)).abs() < 1e-6, "opposite vectors should have similarity -1.0, got {}", sim);
    }

    #[test]
    fn test_cosine_similarity_empty_returns_zero() {
        assert_eq!(cosine_similarity(&[], &[]), 0.0);
        assert_eq!(cosine_similarity(&[1.0], &[]), 0.0);
    }

    #[test]
    fn test_cosine_similarity_zero_vector() {
        let a = vec![0.0, 0.0];
        let b = vec![1.0, 2.0];
        let sim = cosine_similarity(&a, &b);
        assert!((sim - 0.0).abs() < 1e-6, "zero vector should give 0.0, got {}", sim);
    }

    // ========== 淘汰权重配置测试 ==========

    #[test]
    fn test_eviction_config_default() {
        let cfg = EvictionConfig::default();
        assert!((cfg.recency_weight - 0.30).abs() < 1e-6);
        assert!((cfg.relevance_weight - 0.40).abs() < 1e-6);
        assert!((cfg.cost_weight - 0.30).abs() < 1e-6);
    }

    #[test]
    fn test_eviction_config_for_role() {
        let sa = EvictionConfig::for_role("Supervisor");
        assert!((sa.recency_weight - 0.30).abs() < 1e-6);

        let pa = EvictionConfig::for_role("PA");
        assert!(pa.relevance_weight > pa.recency_weight, "PA should prioritize relevance over recency");
        assert!((pa.relevance_weight - 0.60).abs() < 1e-6);

        let da = EvictionConfig::for_role("DA");
        assert!(da.recency_weight >= da.cost_weight.min(da.relevance_weight), "DA should balance recency and cost");

        let ca = EvictionConfig::for_role("CA");
        assert!(ca.relevance_weight > 0.5, "CA should heavily prioritize relevance");
    }

    #[test]
    fn test_eviction_config_with_config() {
        let custom = EvictionConfig { recency_weight: 0.5, relevance_weight: 0.3, cost_weight: 0.2, ..Default::default() };
        let session = L1Session::with_config("agent_1", "DA", "iri://task/abc", 1000, custom);
        assert!((session.eviction_config().recency_weight - 0.5).abs() < 1e-6);
    }

    #[test]
    fn test_evict_with_query_embedding() {
        let mut session = L1Session::with_budget("agent_1", "DA", "iri://task/abc", 10);
        session.add_summary("assistant", "short", None);

        let q_emb = vec![1.0, 0.0, 0.0];
        let match_emb = vec![0.99, 0.01, 0.01];
        let diff_emb = vec![0.0, 1.0, 0.0];

        session.add_summary("assistant", "matching content", Some("iri://match".to_string()));
        if let Some(t) = session.turns.last_mut() {
            t.embedding = Some(match_emb.clone());
        }

        session.add_summary("assistant", "different content", Some("iri://diff".to_string()));
        if let Some(t) = session.turns.last_mut() {
            t.embedding = Some(diff_emb.clone());
        }

        let _evicted = session.evict_with_query(Some(&q_emb));
        assert!(session.current_tokens <= session.token_budget || session.turns.len() <= 1);
        let remaining: Vec<&str> = session.turns.iter().map(|t| t.summary.as_str()).collect();
        let still_has_matching = remaining.iter().any(|s| *s == "matching content");
        assert!(still_has_matching, "matching content should be retained");
    }

    // ========== 补充输入 (Supplement) 测试 ==========

    #[test]
    fn test_add_supplement_preserves_fields() {
        let mut session = L1Session::with_budget("agent_1", "DA", "iri://task/abc", 10000);
        let emb = Some(vec![0.5, 0.5]);
        session.add_supplement("user", "supplement note", emb.clone(), Some(0.85));

        assert_eq!(session.turns.len(), 1);
        let t = &session.turns[0];
        assert!(t.is_supplement, "add_supplement should set is_supplement = true");
        assert_eq!(t.role, "user");
        assert_eq!(t.summary, "supplement note");
        assert_eq!(t.embedding, emb);
        assert!((t.relevance_score.unwrap() - 0.85).abs() < 1e-6);
    }

    #[test]
    fn test_supplement_protected_from_hard_threshold_eviction() {
        let mut session = L1Session::with_budget("agent_1", "DA", "iri://task/abc", 10000);
        // 加一个摘要做第一个 turn，add_supplement 做后续 turn
        session.add_summary("assistant", "the first real assistant turn", None);

        // 加补充输入，设置低相关度 + 旧时间戳（模拟会触发硬阈值淘汰的场景）
        session.add_supplement("user", "old supplement", None, Some(0.1));
        // 强制让这个 turn 的时间戳更老
        let old_time = chrono::Utc::now() - chrono::Duration::seconds(600);
        if let Some(t) = session.turns.last_mut() {
            t.timestamp = old_time;
        }

        // 增加 budget 压力，触发 eviction
        session.token_budget = 100;

        // 硬阈值淘汰不会移除 is_supplement 条目
        let _evicted = session.evict_with_query(None);
        // 补充输入不应该被硬阈值淘汰
        let has_supplement = session.turns.iter().any(|t| t.is_supplement);
        assert!(has_supplement, "supplement should be protected from hard threshold eviction");
    }

    #[test]
    fn test_beta_fusion_influences_eviction() {
        let mut session = L1Session::with_config(
            "agent_1", "DA", "iri://task/abc", 10000,
            EvictionConfig { recency_weight: 0.0, relevance_weight: 1.0, cost_weight: 0.0, relevance_threshold: 0.0, safe_window_seconds: 0, beta: 0.5 }
        );
        // 保留第一个 turn（always kept），后面加几个 padding 制造预算压力
        session.add_summary("assistant", "first long padding text to generate token cost xxxxxx", None);
        session.add_summary("assistant", "second long padding text to generate more cost yyyyyy", None);

        // 两个 turn: 相同 query_sim 但不同 task_relevance
        let emb = Some(vec![1.0, 0.0]);
        session.add_summary("assistant", "high_rel_turn", None);
        if let Some(t) = session.turns.last_mut() {
            t.embedding = emb.clone();
            t.relevance_score = Some(0.9);
        }
        session.add_summary("assistant", "low_rel_turn", None);
        if let Some(t) = session.turns.last_mut() {
            t.embedding = emb.clone();
            t.relevance_score = Some(0.1);
        }

        // 直接收紧预算以触发 evict（仅少 1 token，确保只淘汰 1 个 turn）
        session.token_budget = session.current_tokens - 1;
        let q_emb = vec![1.0, 0.0];
        let evicted = session.evict_with_query(Some(&q_emb));
        assert!(evicted > 0, "eviction should occur when tokens exceed budget");

        // β=0.5: high_rel semantic = 0.5*1.0+0.5*0.9=0.95, low_rel = 0.5*1.0+0.5*0.1=0.55
        // score = (1/semantic)*1.0, so high_rel ≈ 1.05, low_rel ≈ 1.82
        // min score wins eviction → high_rel evicted
        let has_low = session.turns.iter().any(|t| t.summary == "low_rel_turn");
        assert!(has_low, "low relevance turn (higher score) should survive eviction");
        let has_high = session.turns.iter().any(|t| t.summary == "high_rel_turn");
        assert!(!has_high, "high relevance turn (lower score) should be evicted first");
    }

    // ========== 用户态会话隔离（多租户血缘）测试 ==========

    #[test]
    fn test_identity_defaults_to_none() {
        let session = L1Session::new("agent_1", "DA", "iri://task/abc");
        assert_eq!(session.user_id(), None);
        assert_eq!(session.tenant_id(), None);
    }

    #[test]
    fn test_set_identity() {
        let mut session = L1Session::new("agent_1", "DA", "iri://task/abc");
        session.set_identity(Some("user_42".to_string()), Some("tenant_dianlv".to_string()));
        assert_eq!(session.user_id(), Some("user_42"));
        assert_eq!(session.tenant_id(), Some("tenant_dianlv"));
    }

    #[test]
    fn test_with_identity_chaining() {
        let session = L1Session::new("agent_1", "DA", "iri://task/abc")
            .with_identity(Some("user_42".to_string()), None);
        assert_eq!(session.user_id(), Some("user_42"));
        assert_eq!(session.tenant_id(), None);
    }

    #[test]
    fn test_summarize_propagates_identity() {
        let mut session = L1Session::new("agent_1", "DA", "iri://task/abc");
        session.set_identity(Some("user_42".to_string()), Some("tenant_dianlv".to_string()));
        session.add_summary("assistant", "done", None);
        let summary = session.summarize();
        assert_eq!(summary.user_id.as_deref(), Some("user_42"));
        assert_eq!(summary.tenant_id.as_deref(), Some("tenant_dianlv"));
    }
}
