use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};

/// 补充输入条目
#[derive(Debug, Clone)]
pub struct SupplementEntry {
    pub content: String,
    pub embedding: Option<Vec<f32>>,
    pub relevance_score: f64,
    pub timestamp: DateTime<Utc>,
    pub consumed: bool,
}

/// 补充输入共享存储
///
/// SA (生产者) 在分类处理补充输入后存入，同时写入 L1Session。
/// AgentRunner (消费者) 在每个 CycleStart 检查并注入到 messages[]。
///
/// 设计目标:
/// - 解耦 SA 的事件处理与 AgentRunner 的执行循环
/// - 确保补充输入不会丢失（即使 AgentRunner 当前不在 CycleStart 点）
/// - 支持批量消费（多个补充输入在一次 CycleStart 全部注入）
pub struct SupplementaryInputStore {
    /// task_iri → 补充输入列表
    pending: Arc<Mutex<HashMap<String, Vec<SupplementEntry>>>>,
}

impl SupplementaryInputStore {
    pub fn new() -> Self {
        Self {
            pending: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// SA 调用: 存入一条补充输入
    pub fn store(
        &self,
        task_iri: &str,
        content: &str,
        embedding: Option<Vec<f32>>,
        relevance_score: f64,
    ) {
        let entry = SupplementEntry {
            content: content.to_string(),
            embedding,
            relevance_score,
            timestamp: Utc::now(),
            consumed: false,
        };
        let mut map = self.pending.lock().expect("SupplementaryInputStore lock poisoned");
        map.entry(task_iri.to_string()).or_default().push(entry);
        tracing::info!(
            task_iri = %task_iri,
            content = %content.chars().take(80).collect::<String>(),
            score = relevance_score,
            "补充输入已存入 SupplementaryInputStore"
        );
    }

    /// AgentRunner 调用: 拉取当前 task 所有未消费的补充输入
    ///
    /// 原子操作: 取出的条目会被标记 consumed，但保留在列表中供审计。
    /// 返回 Vec 而非 iterator，确保锁持有时间最小化。
    pub fn take_pending(&self, task_iri: &str) -> Vec<SupplementEntry> {
        let mut map = self.pending.lock().expect("SupplementaryInputStore lock poisoned");
        let entries = map.entry(task_iri.to_string()).or_default();
        let pending: Vec<_> = entries.iter_mut()
            .filter(|e| !e.consumed)
            .map(|e| {
                e.consumed = true;
                e.clone()
            })
            .collect();
        pending
    }

    /// 检查是否有未消费的补充输入
    pub fn has_pending(&self, task_iri: &str) -> bool {
        let map = self.pending.lock().expect("SupplementaryInputStore lock poisoned");
        map.get(task_iri)
            .map(|entries| entries.iter().any(|e| !e.consumed))
            .unwrap_or(false)
    }

    /// 获取指定 task 的未消费条目数
    pub fn pending_count(&self, task_iri: &str) -> usize {
        let map = self.pending.lock().expect("SupplementaryInputStore lock poisoned");
        map.get(task_iri)
            .map(|entries| entries.iter().filter(|e| !e.consumed).count())
            .unwrap_or(0)
    }

    /// 清理已完成 task 的数据
    pub fn cleanup(&self, task_iri: &str) {
        let mut map = self.pending.lock().expect("SupplementaryInputStore lock poisoned");
        map.remove(task_iri);
    }
}

impl Default for SupplementaryInputStore {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for SupplementaryInputStore {
    fn clone(&self) -> Self {
        Self {
            pending: self.pending.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_store_and_take_pending() {
        let store = SupplementaryInputStore::new();
        store.store("iri://task/test1", "补充信息1", None, 0.85);
        store.store("iri://task/test1", "补充信息2", Some(vec![1.0, 2.0]), 0.45);

        let pending = store.take_pending("iri://task/test1");
        assert_eq!(pending.len(), 2);
        assert_eq!(pending[0].content, "补充信息1");
        assert_eq!(pending[1].content, "补充信息2");
        assert!(pending[0].consumed);
    }

    #[test]
    fn test_take_pending_only_once() {
        let store = SupplementaryInputStore::new();
        store.store("iri://task/test2", "single", None, 0.9);

        let first = store.take_pending("iri://task/test2");
        assert_eq!(first.len(), 1);

        let second = store.take_pending("iri://task/test2");
        assert_eq!(second.len(), 0, "已消费的不应再次返回");
    }

    #[test]
    fn test_has_pending() {
        let store = SupplementaryInputStore::new();
        assert!(!store.has_pending("iri://task/empty"));

        store.store("iri://task/t", "x", None, 0.5);
        assert!(store.has_pending("iri://task/t"));

        store.take_pending("iri://task/t");
        assert!(!store.has_pending("iri://task/t"));
    }

    #[test]
    fn test_cleanup() {
        let store = SupplementaryInputStore::new();
        store.store("iri://task/t", "x", None, 0.5);
        assert!(store.has_pending("iri://task/t"));

        store.cleanup("iri://task/t");
        assert!(!store.has_pending("iri://task/t"));
    }

    #[test]
    fn test_pending_count() {
        let store = SupplementaryInputStore::new();
        assert_eq!(store.pending_count("iri://task/t"), 0);

        store.store("iri://task/t", "a", None, 0.5);
        store.store("iri://task/t", "b", None, 0.6);
        assert_eq!(store.pending_count("iri://task/t"), 2);

        store.take_pending("iri://task/t");
        assert_eq!(store.pending_count("iri://task/t"), 0);
    }
}
