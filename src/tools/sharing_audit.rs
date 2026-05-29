//! Share 审计日志 — 不改变 SharingProtocol 主数据源 (HashMap) 的前提下，记录所有共享操作
//!
//! # 设计决策
//!
//! - **非阻塞**：审计是侧效（side effect），失败不阻止共享操作
//! - **内存优先**：写入内存 Vec 再异步刷入 Oxigraph，避免 SharingProtocol 持有 Store 引用
//! - **仅追加**：审计日志永不修改或删除
//!
//! # 有意简化
//!
//! - 不提供索引查询（审计场景查询量低，全量扫描足够）
//! - 不使用 SPARQL 插入（保持独立于 Oxigraph）
//! - flush_to_store 是未来可选项，当前仅内存存储

use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

/// 共享事件类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SharingEvent {
    Created,
    Resolved,
    Revoked,
    Expired,
}

impl SharingEvent {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Created => "Created",
            Self::Resolved => "Resolved",
            Self::Revoked => "Revoked",
            Self::Expired => "Expired",
        }
    }
}

/// 共享审计条目
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SharingAuditEntry {
    pub event_type: SharingEvent,
    pub share_id: String,
    pub source_agent: String,
    pub target_agent: String,
    pub node_iri: String,
    pub share_type: String,
    pub permission: String,
    pub ttl_seconds: Option<u64>,
    pub timestamp: DateTime<Utc>,
}

/// 共享审计日志
///
/// 以内存 Vec 存储所有审计条目，避免 SharingProtocol 持有 Oxigraph Store 引用。
/// 当外部 Store 可用时，可通过 `flush_to_store()` 方法导出。
pub struct SharingAuditLog {
    entries: RwLock<Vec<SharingAuditEntry>>,
}

impl SharingAuditLog {
    pub fn new() -> Self {
        Self {
            entries: RwLock::new(Vec::new()),
        }
    }

    /// 记录一条审计条目
    pub fn log(&self, entry: SharingAuditEntry) {
        self.entries.write().push(entry);
    }

    /// 记录共享创建
    pub fn log_share_created(
        &self,
        share_id: &str,
        source_agent: &str,
        target_agent: &str,
        node_iri: &str,
        share_type: &str,
        permission: &str,
        ttl_seconds: Option<u64>,
    ) {
        self.log(SharingAuditEntry {
            event_type: SharingEvent::Created,
            share_id: share_id.to_string(),
            source_agent: source_agent.to_string(),
            target_agent: target_agent.to_string(),
            node_iri: node_iri.to_string(),
            share_type: share_type.to_string(),
            permission: permission.to_string(),
            ttl_seconds,
            timestamp: Utc::now(),
        });
    }

    /// 记录共享解析
    pub fn log_share_resolved(&self, share_id: &str, by_agent: &str) {
        self.log(SharingAuditEntry {
            event_type: SharingEvent::Resolved,
            share_id: share_id.to_string(),
            source_agent: String::new(),
            target_agent: by_agent.to_string(),
            node_iri: String::new(),
            share_type: String::new(),
            permission: String::new(),
            ttl_seconds: None,
            timestamp: Utc::now(),
        });
    }

    /// 记录共享撤销
    pub fn log_share_revoked(&self, share_id: &str) {
        self.log(SharingAuditEntry {
            event_type: SharingEvent::Revoked,
            share_id: share_id.to_string(),
            source_agent: String::new(),
            target_agent: String::new(),
            node_iri: String::new(),
            share_type: String::new(),
            permission: String::new(),
            ttl_seconds: None,
            timestamp: Utc::now(),
        });
    }

    /// 查询某 Agent 收到的共享历史
    pub fn query_shares_for_agent(&self, agent_iri: &str) -> Vec<SharingAuditEntry> {
        self.entries
            .read()
            .iter()
            .filter(|e| e.target_agent == agent_iri)
            .cloned()
            .collect()
    }

    /// 查询某 IRI 的共享历史
    pub fn query_shares_for_node(&self, node_iri: &str) -> Vec<SharingAuditEntry> {
        self.entries
            .read()
            .iter()
            .filter(|e| e.node_iri == node_iri)
            .cloned()
            .collect()
    }

    /// 获取全部审计条目
    pub fn all_entries(&self) -> Vec<SharingAuditEntry> {
        self.entries.read().clone()
    }

    /// 审计条目数量
    pub fn len(&self) -> usize {
        self.entries.read().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// 将内存审计条目序列化为 JSON（供外部消费）
    pub fn to_json(&self) -> serde_json::Value {
        let entries = self.entries.read();
        serde_json::to_value(&*entries).unwrap_or_default()
    }
}

impl Default for SharingAuditLog {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_log_share_created() {
        let log = SharingAuditLog::new();
        log.log_share_created("iri://share/abc", "iri://agent/pa", "iri://agent/da",
            "iri://task/123", "Projection", "Read", Some(3600));
        assert_eq!(log.len(), 1);

        let entry = &log.all_entries()[0];
        assert_eq!(entry.event_type, SharingEvent::Created);
        assert_eq!(entry.source_agent, "iri://agent/pa");
        assert_eq!(entry.share_type, "Projection");
    }

    #[test]
    fn test_log_share_resolved() {
        let log = SharingAuditLog::new();
        log.log_share_resolved("iri://share/abc", "iri://agent/da");
        assert_eq!(log.len(), 1);
        assert_eq!(log.all_entries()[0].event_type, SharingEvent::Resolved);
    }

    #[test]
    fn test_log_share_revoked() {
        let log = SharingAuditLog::new();
        log.log_share_revoked("iri://share/abc");
        assert_eq!(log.len(), 1);
        assert_eq!(log.all_entries()[0].event_type, SharingEvent::Revoked);
    }

    #[test]
    fn test_query_by_agent() {
        let log = SharingAuditLog::new();
        log.log_share_created("s1", "src1", "agent:da1", "n1", "Full", "Read", None);
        log.log_share_created("s2", "src2", "agent:da2", "n2", "Proj", "Read", None);
        log.log_share_created("s3", "src3", "agent:da1", "n3", "Full", "Write", None);

        let results = log.query_shares_for_agent("agent:da1");
        assert_eq!(results.len(), 2);

        let results2 = log.query_shares_for_agent("nonexistent");
        assert!(results2.is_empty());
    }

    #[test]
    fn test_query_by_node() {
        let log = SharingAuditLog::new();
        log.log_share_created("s1", "src1", "tgt1", "iri://task/42", "Full", "Read", None);
        log.log_share_created("s2", "src2", "tgt2", "iri://task/99", "Proj", "Read", None);
        log.log_share_created("s3", "src3", "tgt3", "iri://task/42", "Full", "Write", None);

        let results = log.query_shares_for_node("iri://task/42");
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_to_json() {
        let log = SharingAuditLog::new();
        log.log_share_created("s1", "a", "b", "n1", "Full", "Read", None);
        let json = log.to_json();
        assert!(json.is_array());
        assert_eq!(json.as_array().unwrap().len(), 1);
    }

    #[test]
    fn test_empty_log() {
        let log = SharingAuditLog::new();
        assert!(log.is_empty());
        assert_eq!(log.len(), 0);
        assert!(log.query_shares_for_agent("any").is_empty());
    }
}
