use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// 感知条目的来源类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PerceptionSource {
    WorkspaceMonitor,
    BatchAgent,
    PerceptionEngine,
    Environment,
    System,
}

impl PerceptionSource {
    pub fn prefix(&self) -> &'static str {
        match self {
            Self::WorkspaceMonitor => "📁 工作区",
            Self::BatchAgent => "📊 Batch",
            Self::PerceptionEngine => "⚠️ 告警",
            Self::Environment => "🔌 环境",
            Self::System => "ℹ️ 系统",
        }
    }
}

/// 单条感知条目
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerceptionEntry {
    pub id: String,
    pub source: PerceptionSource,
    pub content: String,
    pub detail_iri: Option<String>,
    pub priority: u8, // 0-9, 9=最高
    pub timestamp: DateTime<Utc>,
    pub consumed: bool,
}

impl PerceptionEntry {
    pub fn new(source: PerceptionSource, content: impl Into<String>) -> Self {
        Self {
            id: format!("pe_{}", uuid::Uuid::new_v4().hyphenated()),
            source,
            content: content.into(),
            detail_iri: None,
            priority: 5,
            timestamp: Utc::now(),
            consumed: false,
        }
    }

    pub fn with_priority(mut self, priority: u8) -> Self {
        self.priority = priority.clamp(0, 9);
        self
    }

    pub fn with_detail_iri(mut self, iri: String) -> Self {
        self.detail_iri = Some(iri);
        self
    }

    /// 渲染为单行文本，用于感知区域
    pub fn render(&self) -> String {
        let prefix = self.source.prefix();
        if let Some(ref iri) = self.detail_iri {
            format!("[{}] {} | details: {}", prefix, self.content, iri)
        } else {
            format!("[{}] {}", prefix, self.content)
        }
    }
}

/// 感知内容存储
///
/// 作用类似 SupplementaryInputStore，但面向的是系统级主动感知而非任务级补充输入。
///
/// 区别：
///   - PerceptionStore: 生产者 = 系统组件（WorkspaceMonitor/BatchAgent/PerceptionEngine）
///   - SupplementaryInputStore: 生产者 = SA（任务级补充输入）
///   - PerceptionStore 的内容在 exec() 初始组装时注入（messages 头部）
///   - SupplementaryInputStore 在 CycleStart 逐轮注入（messages 中部）
///
/// # 生命周期
/// 1. 系统组件调用 `store(task_iri, entry)` 写入感知数据
/// 2. AgentRunner 在 exec() 初始组装时调用 `take_perception_text()` 获取文本
/// 3. 文本作为 `role: "system"` 消息注入到 messages[0] 之后
/// 4. Task 完成后调用 `cleanup(task_iri)` 清理
pub struct PerceptionStore {
    /// task_iri → 感知条目列表
    pending: Arc<Mutex<HashMap<String, Vec<PerceptionEntry>>>>,
    /// 全局感知条目（不绑定特定 task），所有 task 可见
    global: Arc<Mutex<Vec<PerceptionEntry>>>,
    /// 去重缓存：source+content → 上次写入时间，用于 60 秒去重
    dedup_cache: Arc<Mutex<HashMap<(PerceptionSource, String), DateTime<Utc>>>>,
}

impl PerceptionStore {
    pub fn new() -> Self {
        Self {
            pending: Arc::new(Mutex::new(HashMap::new())),
            global: Arc::new(Mutex::new(Vec::new())),
            dedup_cache: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// 生产者写入: 为指定 task 添加一条感知条目
    /// 自动去重：同一 source+content 在 60 秒内不重复写入
    pub fn store(&self, task_iri: &str, entry: PerceptionEntry) {
        let dedup_key = (entry.source, entry.content.clone());
        {
            let mut dedup = self.dedup_cache.lock().expect("dedup_cache lock poisoned");
            let now = Utc::now();
            if let Some(last) = dedup.get(&dedup_key) {
                if now.signed_duration_since(*last).num_seconds() < 60 {
                    return; // 去重
                }
            }
            dedup.insert(dedup_key, now);
        }

        let mut map = self.pending.lock().expect("PerceptionStore lock poisoned");
        map.entry(task_iri.to_string()).or_default().push(entry);
    }

    /// 生产者写入: 添加全局感知条目（所有 task 可见）
    pub fn store_global(&self, entry: PerceptionEntry) {
        let dedup_key = (entry.source, entry.content.clone());
        {
            let mut dedup = self.dedup_cache.lock().expect("dedup_cache lock poisoned");
            let now = Utc::now();
            if let Some(last) = dedup.get(&dedup_key) {
                if now.signed_duration_since(*last).num_seconds() < 60 {
                    return;
                }
            }
            dedup.insert(dedup_key, now);
        }

        let mut global = self.global.lock().expect("PerceptionStore global lock poisoned");
        global.push(entry);
    }

    /// 消费者拉取: 获取所有未消费的感知条目（全局 + task 级别），合并为文本
    ///
    /// 返回格式化的感知文本，如果没有新内容则返回空字符串。
    /// 已消费的条目会被标记 consumed 但保留在列表中供审计。
    pub fn take_perception_text(&self, task_iri: &str) -> String {
        let mut entries: Vec<PerceptionEntry> = Vec::new();

        // 1. 取全局未消费条目
        {
            let mut global = self.global.lock().expect("PerceptionStore global lock poisoned");
            for entry in global.iter_mut() {
                if !entry.consumed {
                    entry.consumed = true;
                    entries.push(entry.clone());
                }
            }
        }

        // 2. 取 task 级别未消费条目
        {
            let mut map = self.pending.lock().expect("PerceptionStore lock poisoned");
            if let Some(task_entries) = map.get_mut(task_iri) {
                for entry in task_entries.iter_mut() {
                    if !entry.consumed {
                        entry.consumed = true;
                        entries.push(entry.clone());
                    }
                }
            }
        }

        if entries.is_empty() {
            return String::new();
        }

        // 按优先级排序，高优先级的在前
        entries.sort_by(|a, b| b.priority.cmp(&a.priority).then(a.timestamp.cmp(&b.timestamp)));

        // 限制最多 10 条，防膨胀
        if entries.len() > 10 {
            entries.truncate(10);
        }

        let lines: Vec<String> = entries.iter().map(|e| e.render()).collect();
        lines.join("\n")
    }

    /// 检查指定 task 是否有未消费的感知条目
    pub fn has_new(&self, task_iri: &str) -> bool {
        // 检查全局
        {
            let global = self.global.lock().expect("PerceptionStore global lock poisoned");
            if global.iter().any(|e| !e.consumed) {
                return true;
            }
        }
        // 检查 task 级别
        {
            let map = self.pending.lock().expect("PerceptionStore lock poisoned");
            if let Some(entries) = map.get(task_iri) {
                if entries.iter().any(|e| !e.consumed) {
                    return true;
                }
            }
        }
        false
    }

    /// 清理已完成 task 的数据
    pub fn cleanup(&self, task_iri: &str) {
        let mut map = self.pending.lock().expect("PerceptionStore lock poisoned");
        map.remove(task_iri);
    }

    /// 清理过期的去重缓存（超过 120 秒的条目）
    pub fn evict_dedup_cache(&self) {
        let mut dedup = self.dedup_cache.lock().expect("dedup_cache lock poisoned");
        let now = Utc::now();
        dedup.retain(|_, last| now.signed_duration_since(*last).num_seconds() < 120);
    }
}

impl Default for PerceptionStore {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for PerceptionStore {
    fn clone(&self) -> Self {
        Self {
            pending: self.pending.clone(),
            global: self.global.clone(),
            dedup_cache: self.dedup_cache.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_store_empty() {
        let store = PerceptionStore::new();
        let text = store.take_perception_text("iri://task/test1");
        assert!(text.is_empty(), "New store should have no entries");
    }

    #[test]
    fn test_store_and_take() {
        let store = PerceptionStore::new();
        store.store("iri://task/test1", PerceptionEntry::new(
            PerceptionSource::WorkspaceMonitor,
            "3 stale files detected",
        ));
        store.store("iri://task/test1", PerceptionEntry::new(
            PerceptionSource::BatchAgent,
            "Intent shift detected: db→cache",
        ));

        let text = store.take_perception_text("iri://task/test1");
        assert!(!text.is_empty(), "Should have perception text");
        assert!(text.contains("工作区"), "Should contain WorkspaceMonitor prefix");
        assert!(text.contains("Batch"), "Should contain BatchAgent prefix");
    }

    #[test]
    fn test_store_global() {
        let store = PerceptionStore::new();
        store.store_global(PerceptionEntry::new(
            PerceptionSource::System,
            "System initialized",
        ));

        // Global entries visible to any task
        let text1 = store.take_perception_text("iri://task/a");
        assert!(!text1.is_empty(), "Global entry should be visible to task_a");
        assert!(text1.contains("System"), "Should contain System prefix");

        let text2 = store.take_perception_text("iri://task/b");
        assert!(text2.is_empty(), "Global entry should be consumed after first take");
    }

    #[test]
    fn test_dedup() {
        let store = PerceptionStore::new();
        store.store("iri://task/test1", PerceptionEntry::new(
            PerceptionSource::WorkspaceMonitor,
            "dedup test",
        ));
        store.store("iri://task/test1", PerceptionEntry::new(
            PerceptionSource::WorkspaceMonitor,
            "dedup test", // 重复
        ));

        let text = store.take_perception_text("iri://task/test1");
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 1, "Duplicate entries should be dedup'd to 1 line");
    }

    #[test]
    fn test_priority_order() {
        let store = PerceptionStore::new();
        store.store("iri://task/t", PerceptionEntry::new(
            PerceptionSource::WorkspaceMonitor,
            "low priority",
        ).with_priority(1));
        store.store("iri://task/t", PerceptionEntry::new(
            PerceptionSource::PerceptionEngine,
            "high priority alert!",
        ).with_priority(9));

        let text = store.take_perception_text("iri://task/t");
        // High priority should come first
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("告警"), "High priority alert should come first");
    }

    #[test]
    fn test_max_10_entries() {
        let store = PerceptionStore::new();
        for i in 0..15 {
            store.store("iri://task/t", PerceptionEntry::new(
                PerceptionSource::System,
                format!("entry {}", i),
            ).with_priority(1));
        }

        let text = store.take_perception_text("iri://task/t");
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 10, "Should max at 10 perception entries");
    }

    #[test]
    fn test_cleanup() {
        let store = PerceptionStore::new();
        store.store("iri://task/t", PerceptionEntry::new(
            PerceptionSource::System,
            "test",
        ));
        store.cleanup("iri://task/t");

        let text = store.take_perception_text("iri://task/t");
        assert!(text.is_empty(), "Should be empty after cleanup");
    }

    #[test]
    fn test_has_new() {
        let store = PerceptionStore::new();
        assert!(!store.has_new("iri://task/t"));

        store.store("iri://task/t", PerceptionEntry::new(
            PerceptionSource::System,
            "new entry",
        ));
        assert!(store.has_new("iri://task/t"));

        store.take_perception_text("iri://task/t");
        assert!(!store.has_new("iri://task/t"));
    }

    #[test]
    fn test_perception_entry_render() {
        let entry = PerceptionEntry::new(PerceptionSource::WorkspaceMonitor, "file changed")
            .with_detail_iri("iri://detail/1".to_string());
        let text = entry.render();
        assert!(text.contains("工作区"));
        assert!(text.contains("file changed"));
        assert!(text.contains("iri://detail/1"));
    }

    #[test]
    fn test_store_different_tasks_same_source_content_dedup() {
        let store = PerceptionStore::new();
        store.store("iri://task/a", PerceptionEntry::new(PerceptionSource::System, "same content"));
        // Same source+content to different task — dedup cache IS global so this is blocked
        store.store("iri://task/b", PerceptionEntry::new(PerceptionSource::System, "same content"));

        let text_a = store.take_perception_text("iri://task/a");
        assert!(!text_a.is_empty(), "task_a should have perception");
        let text_b = store.take_perception_text("iri://task/b");
        // task_b's store was dedup'd — so has_new should still be false
        assert!(text_b.is_empty(), "task_b should be empty because store was dedup'd");
    }

    #[test]
    fn test_same_content_different_sources_allowed_across_tasks() {
        let store = PerceptionStore::new();
        // Same content but from different sources → NOT dedup'd
        store.store("iri://task/a", PerceptionEntry::new(PerceptionSource::WorkspaceMonitor, "file changed"));
        store.store("iri://task/b", PerceptionEntry::new(PerceptionSource::PerceptionEngine, "file changed"));

        let text_a = store.take_perception_text("iri://task/a");
        assert!(!text_a.is_empty(), "task_a should have entry");
        let text_b = store.take_perception_text("iri://task/b");
        assert!(!text_b.is_empty(), "task_b should have entry (different source)");
    }

    #[test]
    fn test_global_not_duplicated_by_task_store() {
        let store = PerceptionStore::new();
        store.store_global(PerceptionEntry::new(PerceptionSource::System, "global msg"));

        // global + task store with same content — should keep both
        store.store("iri://task/t", PerceptionEntry::new(PerceptionSource::WorkspaceMonitor, "task specific"));

        let text = store.take_perception_text("iri://task/t");
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2, "Global + task entries should both appear");
    }

    #[test]
    fn test_take_empty_after_consumed() {
        let store = PerceptionStore::new();
        store.store("iri://task/t", PerceptionEntry::new(PerceptionSource::System, "msg"));

        let first = store.take_perception_text("iri://task/t");
        assert!(!first.is_empty(), "First take should have content");
        assert!(!store.has_new("iri://task/t"), "has_new should be false after take");

        let second = store.take_perception_text("iri://task/t");
        assert!(second.is_empty(), "Second take should be empty");
    }

    #[test]
    fn test_has_new_global_only() {
        let store = PerceptionStore::new();
        assert!(!store.has_new("iri://task/x"), "No entries initially");
        store.store_global(PerceptionEntry::new(PerceptionSource::Environment, "env event"));
        assert!(store.has_new("iri://task/x"), "Should detect new global entry");
        store.take_perception_text("iri://task/x");
        assert!(!store.has_new("iri://task/x"), "Should be consumed after take");
    }

    #[test]
    fn test_priority_clamping() {
        let entry = PerceptionEntry::new(PerceptionSource::System, "test")
            .with_priority(99);
        assert_eq!(entry.priority, 9, "Priority should clamp to 9");

        let entry = PerceptionEntry::new(PerceptionSource::System, "test")
            .with_priority(0);
        assert_eq!(entry.priority, 0, "Priority 0 should stay 0");

        let entry = PerceptionEntry::new(PerceptionSource::System, "test")
            .with_priority(5);
        assert_eq!(entry.priority, 5, "Priority 5 should stay 5");
    }

    #[test]
    fn test_dedup_cross_task_global() {
        let store = PerceptionStore::new();
        // Same source+content to global → should dedup
        store.store_global(PerceptionEntry::new(PerceptionSource::System, "dedup me"));
        store.store_global(PerceptionEntry::new(PerceptionSource::System, "dedup me"));

        let text = store.take_perception_text("iri://task/t");
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 1, "Global dedup should work");
    }

    #[test]
    fn test_different_sources_no_dedup() {
        let store = PerceptionStore::new();
        store.store("iri://task/t", PerceptionEntry::new(PerceptionSource::WorkspaceMonitor, "same text"));
        store.store("iri://task/t", PerceptionEntry::new(PerceptionSource::BatchAgent, "same text"));

        let text = store.take_perception_text("iri://task/t");
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2, "Different sources with same content should NOT dedup");
    }

    #[test]
    fn test_dedup_cache_prevents_immediate_repeat() {
        let store = PerceptionStore::new();
        store.store("iri://task/t", PerceptionEntry::new(PerceptionSource::System, "repeat"));
        store.store("iri://task/t", PerceptionEntry::new(PerceptionSource::System, "repeat"));

        let text = store.take_perception_text("iri://task/t");
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 1, "Immediate repeat should be dedup'd");
    }

    #[test]
    fn test_cleanup_removes_only_target_task() {
        let store = PerceptionStore::new();
        store.store("iri://task/a", PerceptionEntry::new(PerceptionSource::System, "a_data"));
        store.store("iri://task/b", PerceptionEntry::new(PerceptionSource::System, "b_data"));

        store.cleanup("iri://task/a");

        // task_a should be gone
        let text_a = store.take_perception_text("iri://task/a");
        assert!(text_a.is_empty(), "Cleaned task should have no entries");

        // task_b should still have its entry (not consumed)
        assert!(store.has_new("iri://task/b"), "Task b should still have entries");
        let text_b = store.take_perception_text("iri://task/b");
        assert!(!text_b.is_empty(), "Task b should have perception");
    }

    #[test]
    fn test_evict_dedup_cache_removes_old() {
        let store = PerceptionStore::new();
        store.store("iri://task/t", PerceptionEntry::new(PerceptionSource::System, "old"));

        // Manually set the dedup entry to be old (>120s)
        {
            let mut dedup = store.dedup_cache.lock().unwrap();
            dedup.insert(
                (PerceptionSource::System, "old".to_string()),
                Utc::now() - chrono::Duration::seconds(130),
            );
        }

        store.evict_dedup_cache();
        {
            let dedup = store.dedup_cache.lock().unwrap();
            assert!(dedup.is_empty(), "Evict should remove expired entries");
        }

        // After eviction the dedup cache is clean, but the original entry is still in pending.
        // Re-storing the same content adds a second entry (dedup check passes since cache was cleared).
        // Then take returns both entries.
        store.store("iri://task/t", PerceptionEntry::new(PerceptionSource::System, "old"));
        let text = store.take_perception_text("iri://task/t");
        let lines: Vec<&str> = text.lines().collect();
        // Two entries: original (step 1) + re-stored (after eviction)
        assert_eq!(lines.len(), 2, "After eviction+restore, both entries should be present");
    }

    #[test]
    fn test_perception_entry_default_id_is_unique() {
        let e1 = PerceptionEntry::new(PerceptionSource::System, "a");
        let e2 = PerceptionEntry::new(PerceptionSource::System, "a");
        assert_ne!(e1.id, e2.id, "Each entry should have a unique id");
    }

    #[test]
    fn test_entry_render_no_detail_iri() {
        let entry = PerceptionEntry::new(PerceptionSource::BatchAgent, "simple message");
        let text = entry.render();
        assert!(text.contains("Batch"), "Should contain Batch prefix");
        assert!(text.contains("simple message"), "Should contain message");
        assert!(!text.contains("details:"), "Should NOT contain details: without detail_iri");
    }

    #[test]
    fn test_store_with_high_priority_comes_first() {
        let store = PerceptionStore::new();
        // 3 entries with different priorities
        store.store_global(PerceptionEntry::new(PerceptionSource::System, "low").with_priority(1));
        store.store_global(PerceptionEntry::new(PerceptionSource::PerceptionEngine, "critical!").with_priority(9));
        store.store_global(PerceptionEntry::new(PerceptionSource::WorkspaceMonitor, "mid").with_priority(5));

        let text = store.take_perception_text("iri://task/t");
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 3, "All 3 entries should appear");
        // First line should be the high-priority one (告警 prefix)
        assert!(lines[0].contains("告警"), "Highest priority should come first: got {}", lines[0]);
        // Last line should be the low priority
        assert!(lines[2].contains("工作区") || lines[2].contains("系统"), "Lower priority should be last");
    }

    #[test]
    fn test_take_perception_respects_10_limit_across_global_and_task() {
        let store = PerceptionStore::new();
        for i in 0..8 {
            store.store_global(PerceptionEntry::new(PerceptionSource::System, format!("global_{}", i)));
        }
        for i in 0..8 {
            store.store("iri://task/t", PerceptionEntry::new(PerceptionSource::WorkspaceMonitor, format!("task_{}", i)));
        }
        let text = store.take_perception_text("iri://task/t");
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 10, "Should cap at 10 even with 16 total entries");
    }
}
