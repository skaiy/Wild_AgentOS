use std::path::PathBuf;
use std::sync::Arc;

use parking_lot::RwLock;
use tokio::sync::broadcast;
use tracing::{debug, info, instrument};

use crate::core::event_bus::{EventBus, EventType};
use crate::core::perception_store::{PerceptionEntry, PerceptionSource, PerceptionStore};
use crate::memory::l2_blackboard::Blackboard;
use crate::tools::hooks::{FunctionHook, HookContext, HookManager, HookPoint, HookResult};

pub mod content_store;
pub mod diff_engine;
pub mod inventory;
pub mod snapshot;
pub mod watch_engine;

pub use content_store::{ContentStore, ReadMode, ReadResult};
pub use diff_engine::DiffEngine;
pub use inventory::{FileEntry, FileInventory, FileState};
pub use snapshot::{RollbackResult, SnapshotManager, WorkspaceSnapshot};
pub use watch_engine::{WatchConfig, WatchEngine};

/// Configuration for the workspace monitor subsystem.
#[derive(Debug, Clone)]
pub struct WorkspaceMonitorConfig {
    /// Root directory of the workspace to monitor.
    pub workspace_root: PathBuf,
    /// Glob patterns to exclude from file scanning.
    pub exclude_patterns: Vec<String>,
    /// Maximum content cache size in bytes.
    pub content_store_max_bytes: usize,
    /// Maximum number of files in LRU content cache.
    pub content_cache_capacity: usize,
    /// Enable native file system watching.
    pub watch_enabled: bool,
    /// Polling interval in ms (fallback when native watching unavailable).
    pub poll_interval_ms: u64,
    /// Debounce window in ms for file events.
    pub debounce_ms: u64,
    /// Maximum debounce wait in ms.
    pub max_debounce_wait_ms: u64,
    /// Optional redb database path for persistent storage.
    pub db_path: Option<PathBuf>,
}

impl Default for WorkspaceMonitorConfig {
    fn default() -> Self {
        Self {
            workspace_root: PathBuf::from("."),
            exclude_patterns: vec![
                "node_modules/".into(),
                "target/".into(),
                ".git/".into(),
                "dist/".into(),
                "build/".into(),
                "__pycache__/".into(),
                ".venv/".into(),
                "venv/".into(),
                ".next/".into(),
                "data/".into(),
                ".gliding_horse/".into(),
            ],
            content_store_max_bytes: 64 * 1024 * 1024, // 64 MB
            content_cache_capacity: 1000,
            watch_enabled: true,
            poll_interval_ms: 5000,
            debounce_ms: 500,
            max_debounce_wait_ms: 5000,
            db_path: None,
        }
    }
}

/// The top-level workspace monitor orchestrator.
///
/// Owns all sub-components:
/// - `FileInventory`: tracks file metadata and state
/// - `ContentStore`: caches file content with versioning
/// - `SnapshotManager`: creates/restores workspace snapshots
/// - `WatchEngine`: listens for filesystem changes
pub struct WorkspaceMonitor {
    pub config: WorkspaceMonitorConfig,
    pub inventory: Arc<RwLock<FileInventory>>,
    pub content_store: Arc<ContentStore>,
    pub snapshot_manager: Arc<SnapshotManager>,
    watch_engine: Option<WatchEngine>,
    event_bus: Option<Arc<EventBus>>,
    perception_store: RwLock<Option<Arc<PerceptionStore>>>,
}

impl WorkspaceMonitor {
    /// Initialize the workspace monitor with the given config.
    ///
    /// Sets up:
    /// 1. redb database (if path configured)
    /// 2. ContentStore with version storage
    /// 3. FileInventory with L2 Blackboard sync
    /// 4. SnapshotManager for rollback support
    /// 5. WatchEngine for file system events
    #[instrument(skip(config, blackboard, event_bus))]
    pub fn initialize(
        config: WorkspaceMonitorConfig,
        blackboard: Option<Arc<Blackboard>>,
        event_bus: Option<Arc<EventBus>>,
    ) -> Result<Self, String> {
        let root = config.workspace_root.to_string_lossy().to_string();

        // Initialize redb database
        let (meta_db, content_db) = Self::open_databases(&config)?;
        let meta_db = meta_db;
        let content_db = content_db;

        // ContentStore
        let content_store = Arc::new(ContentStore::new(
            config.content_cache_capacity,
            config.content_store_max_bytes,
            content_db,
        ));

        // FileInventory
        let inventory = Arc::new(RwLock::new(FileInventory::new(
            blackboard.clone(),
            meta_db,
            config.exclude_patterns.clone(),
        )));

        // SnapshotManager (in-memory redb)
        let snap_db = Arc::new(
            redb::Builder::new()
                .create_with_backend(redb::backends::InMemoryBackend::new())
                .map_err(|e| format!("Failed to open snapshot redb: {}", e))?,
        );
        let snapshot_manager = Arc::new(SnapshotManager::new(
            snap_db,
            content_store.clone(),
            inventory.clone(),
        ));

        let event_bus_for_struct = event_bus.clone();

        // WatchEngine — 原生文件监听（inotify 线程，无需 tokio）
        let watch_engine = if let Some(eb) = event_bus.clone() {
            let mut watch_config = WatchConfig {
                debounce_ms: config.debounce_ms,
                max_debounce_wait_ms: config.max_debounce_wait_ms,
                poll_interval_ms: config.poll_interval_ms,
                watch_enabled: config.watch_enabled,
                exclude_patterns: config.exclude_patterns.clone(),
                use_gitignore: true,
            };
            if watch_config.use_gitignore {
                watch_config.load_gitignore(&config.workspace_root);
            }
            match WatchEngine::start(&root, watch_config, eb, Some(inventory.clone())) {
                Ok(engine) => {
                    info!("WatchEngine started for {}", root);
                    Some(engine)
                }
                Err(e) => {
                    tracing::warn!("WatchEngine failed to start: {}", e);
                    None
                }
            }
        } else {
            None
        };

        // 构建自身
        let ws = Self {
            config,
            inventory,
            content_store,
            snapshot_manager,
            watch_engine,
            event_bus: event_bus_for_struct,
            perception_store: RwLock::new(None),
        };

        // 异步消费者需要 tokio runtime，若当前无 runtime 则跳过
        // （glidingcode 场景：后续由 start_async_components 在 async 上下文中调用）
        if tokio::runtime::Handle::try_current().is_ok() {
            ws.register_event_consumers();
        } else {
            tracing::info!("No tokio runtime in init context, event consumers deferred");
        }

        // Perform initial scan
        {
            let inv = ws.inventory.read();
            let discovered = inv.full_scan(&root);
            debug!(discovered = discovered, "Initial workspace scan completed");
        }

        info!("WorkspaceMonitor initialized for root={}", root);

        Ok(ws)
    }

    /// Read a file through ContentStore with cache/diff support.
    pub fn read_file(&self, path: &str, mode: ReadMode) -> std::io::Result<ReadResult> {
        let result = self.content_store.read_file(path, mode)?;

        // Update FileInventory state
        let inv = self.inventory.read();
        if result.changed {
            inv.add_or_update(path);
        }
        inv.mark_read(path, result.version);

        Ok(result)
    }

    /// Mark a file as externally read without disk I/O.
    /// Used when file content was provided via read_full_result micro-tool,
    /// so subsequent file_read calls recognize it as already-read.
    pub fn mark_file_read_external(&self, path: &str) {
        let inv = self.inventory.read();
        inv.mark_external_read(path);
    }

    /// Mark a file as written by the agent.
    pub fn mark_file_written(&self, path: &str) {
        let inv = self.inventory.read();
        inv.mark_written(path);
        self.content_store.invalidate(path);
    }

    /// Re-scan the entire workspace root, discovering new files and tracking state changes.
    /// Returns the number of newly discovered files.
    pub fn rescan(&self) -> usize {
        let root = self.config.workspace_root.to_string_lossy().to_string();
        self.inventory.read().full_scan(&root)
    }

    /// Get the snapshot manager reference.
    pub fn snapshots(&self) -> &Arc<SnapshotManager> {
        &self.snapshot_manager
    }

    /// Get the content store reference.
    pub fn content(&self) -> &Arc<ContentStore> {
        &self.content_store
    }

    /// Subscribe to EventBus for workspace file events and update inventory.
    pub fn register_event_consumers(&self) {
        let event_bus = match &self.event_bus {
            Some(eb) => eb.clone(),
            None => {
                tracing::warn!("EventBus not available, event consumers not registered");
                return;
            }
        };

        let inventory = self.inventory.clone();
        let perception = self.perception_store.read().clone();
        // Subscribe before spawning to ensure no events are missed between
        // spawn and subscribe.
        let mut receiver = event_bus.subscribe();
        tokio::spawn(async move {
            loop {
                match receiver.recv().await {
                    Ok(event) => {
                        match EventType::from_str(&event.event_type) {
                            EventType::WorkspaceFileCreated => {
                                inventory.read().add_or_update(&event.payload);
                                // 通知感知区域
                                if let Some(ref p) = perception {
                                    let entry = PerceptionEntry::new(
                                        PerceptionSource::WorkspaceMonitor,
                                        format!("新文件创建: {}", event.payload),
                                    ).with_priority(6);
                                    p.store_global(entry);
                                }
                            }
                            EventType::WorkspaceFileModified => {
                                inventory.read().mark_stale(&event.payload);
                                // 通知感知区域
                                if let Some(ref p) = perception {
                                    let entry = PerceptionEntry::new(
                                        PerceptionSource::WorkspaceMonitor,
                                        format!("文件外部变更: {}", event.payload),
                                    ).with_priority(6);
                                    p.store_global(entry);
                                }
                            }
                            EventType::WorkspaceFileRemoved => {
                                inventory.read().remove(&event.payload);
                                // 通知感知区域
                                if let Some(ref p) = perception {
                                    let entry = PerceptionEntry::new(
                                        PerceptionSource::WorkspaceMonitor,
                                        format!("文件已删除: {}", event.payload),
                                    ).with_priority(5);
                                    p.store_global(entry);
                                }
                            }
                            _ => {}
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!(
                            "WorkspaceMonitor event consumer lagged by {} events",
                            n
                        );
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        tracing::error!("WorkspaceMonitor event bus connection closed");
                        break;
                    }
                }
            }
        });
    }

    /// Complete async-dependent initialization.
    ///
    /// Must be called from within a tokio runtime (e.g., during `process_task`).
    /// Registers event consumers that listen for WorkspaceFile* events via EventBus.
    ///
    /// Safe to call multiple times — event consumer registration is idempotent
    /// (each call spawns a redundant listener, but the extra listener will simply
    /// process events that have already been handled by the first).
    pub fn start_async_components(&self) {
        if tokio::runtime::Handle::try_current().is_err() {
            tracing::error!("start_async_components must be called from within a tokio runtime");
            return;
        }
        self.register_event_consumers();
    }

    /// Register hooks for file read/write tools to check inventory state.
    pub fn register_hooks(&self, hook_manager: &HookManager) {
        let inv_for_read = self.inventory.clone();

        let read_hook = FunctionHook::new(
            "workspace_monitor_file_read",
            vec![HookPoint::SkillBefore],
            100,
            move |ctx: &mut HookContext| {
                let path = match ctx.data.get("path").and_then(|v| v.as_str()) {
                    Some(p) => p.to_string(),
                    None => return HookResult::Continue,
                };
                let inv = inv_for_read.read();
                if let Some(entry) = inv.get_entry(&path) {
                    match entry.state {
                        FileState::ReadStale => {
                            let warning = format!(
                                "[workspace_monitor] File '{}' is stale (last read version {}), consider re-reading before writing",
                                path, entry.last_read_version
                            );
                            ctx.data.insert(
                                "stale_warning".to_string(),
                                serde_json::Value::String(warning.clone()),
                            );
                            // 同时写入 metadata 以便 ToolGuard pre-injection 将其注入 system prompt
                            let injections = ctx.metadata
                                .entry("guard_pre_injections".to_string())
                                .or_insert_with(|| serde_json::Value::Array(Vec::new()));
                            if let Some(arr) = injections.as_array_mut() {
                                arr.push(serde_json::Value::String(warning));
                            }
                        }
                        FileState::ReadFresh if entry.last_read_version == entry.current_version => {
                            // File unchanged since last read — inject hint to skip full re-read
                            ctx.data.insert(
                                "file_unchanged".to_string(),
                                serde_json::Value::Bool(true),
                            );
                            let hint = format!(
                                "[workspace_monitor] File '{}' unchanged since last read (v{}). Use mode:diff for incremental changes or mode:force_refresh to re-read.",
                                path, entry.current_version
                            );
                            ctx.data.insert(
                                "file_unchanged_hint".to_string(),
                                serde_json::Value::String(hint.clone()),
                            );
                            // 同时写入 metadata 以便 ToolGuard pre-injection
                            let injections = ctx.metadata
                                .entry("guard_pre_injections".to_string())
                                .or_insert_with(|| serde_json::Value::Array(Vec::new()));
                            if let Some(arr) = injections.as_array_mut() {
                                arr.push(serde_json::Value::String(hint));
                            }
                        }
                        _ => {}
                    }
                }
                HookResult::Continue
            },
        );
        hook_manager.register(Box::new(read_hook));

        let inv_for_write = self.inventory.clone();

        let write_before_hook = FunctionHook::new(
            "workspace_monitor_file_write_before",
            vec![HookPoint::SkillBefore],
            100,
            move |ctx: &mut HookContext| {
                let path = match ctx.data.get("path").and_then(|v| v.as_str()) {
                    Some(p) => p.to_string(),
                    None => return HookResult::Continue,
                };
                let inv = inv_for_write.read();
                if let Some(entry) = inv.get_entry(&path) {
                    if entry.state == FileState::ReadStale {
                        ctx.data.insert(
                            "stale_warning".to_string(),
                            serde_json::Value::String(format!(
                                "File '{}' is stale, writing may overwrite external changes",
                                path
                            )),
                        );
                    }
                }
                inv.add_or_update(&path);
                HookResult::Continue
            },
        );
        hook_manager.register(Box::new(write_before_hook));

        let inv_for_mark = self.inventory.clone();

        let write_after_hook = FunctionHook::new(
            "workspace_monitor_file_write_after",
            vec![HookPoint::SkillAfter],
            100,
            move |ctx: &mut HookContext| {
                let path = match ctx.data.get("path").and_then(|v| v.as_str()) {
                    Some(p) => p.to_string(),
                    None => return HookResult::Continue,
                };
                let inv = inv_for_mark.read();
                inv.mark_written(&path);
                HookResult::Continue
            },
        );
        hook_manager.register(Box::new(write_after_hook));
    }

    /// 设置主动感知存储，使 WorkspaceMonitor 能向 Agent 注入文件状态感知数据
    pub fn with_perception_store(self, store: Arc<PerceptionStore>) -> Self {
        *self.perception_store.write() = Some(store);
        self
    }

    /// 在 WorkspaceMonitor 已构造后设置感知存储（用于 Arc<WorkspaceMonitor> 场景）
    pub fn set_perception_store(&self, store: Arc<PerceptionStore>) {
        *self.perception_store.write() = Some(store);
    }

    /// 生成工作区文件状态摘要文本，用于注入感知区域
    pub fn generate_perception_text(&self) -> Option<String> {
        let inv = self.inventory.read();
        let all = inv.list_all();
        if all.is_empty() {
            return None;
        }

        let total = all.len();
        let stale: Vec<_> = all.iter().filter(|e| e.state == crate::tools::workspace_monitor::FileState::ReadStale).collect();
        let written_unread: Vec<_> = all.iter().filter(|e| e.state == crate::tools::workspace_monitor::FileState::WrittenUnread).collect();
        let discovered: Vec<_> = all.iter().filter(|e| e.state == crate::tools::workspace_monitor::FileState::Discovered).collect();

        let mut parts = Vec::new();
        parts.push(format!("共 {} 个文件", total));

        if !stale.is_empty() {
            let names: Vec<&str> = stale.iter().take(5).map(|e| e.path.as_str()).collect();
            parts.push(format!(
                "{} 个有外部变更{}",
                stale.len(),
                if names.is_empty() { String::new() } else {
                    format!(": {}", names.join(", "))
                }
            ));
        }

        if !written_unread.is_empty() {
            let names: Vec<&str> = written_unread.iter().take(5).map(|e| e.path.as_str()).collect();
            parts.push(format!(
                "{} 个已写入未重新读取{}",
                written_unread.len(),
                if names.is_empty() { String::new() } else {
                    format!(": {}", names.join(", "))
                }
            ));
        }

        if !discovered.is_empty() {
            parts.push(format!("{} 个新发现未读取", discovered.len()));
        }

        Some(format!("{} | {}", total, parts.join(" | ")))
    }

    /// 向 PerceptionStore 写入当前文件状态的感知摘要
    pub fn inject_file_perception(&self) {
        let ps = self.perception_store.read();
        if let Some(ref store) = *ps {
            if let Some(text) = self.generate_perception_text() {
                let entry = PerceptionEntry::new(PerceptionSource::WorkspaceMonitor, text);
                store.store_global(entry);
            }
        }
    }

    // ── Private ──

    fn open_databases(
        config: &WorkspaceMonitorConfig,
    ) -> Result<(Option<redb::Database>, Option<redb::Database>), String> {
        match &config.db_path {
            Some(path) => {
                std::fs::create_dir_all(path)
                    .map_err(|e| format!("Failed to create database directory: {}", e))?;

                let meta_path = path.join("metadata");
                let content_path = path.join("content");

                let meta_db = redb::Database::create(&meta_path)
                    .map_err(|e| format!("Failed to open metadata redb: {}", e))?;

                let content_db = redb::Database::create(&content_path)
                    .map_err(|e| format!("Failed to open content redb: {}", e))?;

                Ok((Some(meta_db), Some(content_db)))
            }
            None => Ok((None, None)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::event_bus::{EventBus, EventType};
    use crate::tools::hooks::{HookContext, HookManager, HookPoint, HookResult};
    use serde_json::Value;
    use std::sync::Arc;

    fn temp_ws_monitor() -> (WorkspaceMonitor, tempfile::TempDir) {
        let dir = tempfile::TempDir::new().unwrap();
        let config = WorkspaceMonitorConfig {
            workspace_root: dir.path().to_path_buf(),
            watch_enabled: false,
            db_path: None,
            ..WorkspaceMonitorConfig::default()
        };
        let ws = WorkspaceMonitor::initialize(config, None, None).unwrap();
        (ws, dir)
    }

    #[test]
    fn test_register_hooks_read_stale_warning() {
        let (ws, dir) = temp_ws_monitor();
        let file_path = dir.path().join("test.rs");
        std::fs::write(&file_path, "fn main() {}").unwrap();

        {
            let inv = ws.inventory.read();
            inv.add_or_update(&file_path.to_string_lossy()).unwrap();
            inv.mark_stale(&file_path.to_string_lossy());
        }

        let hm = HookManager::new();
        ws.register_hooks(&hm);

        let mut ctx = HookContext::new(HookPoint::SkillBefore, "agent_1", "DA");
        ctx.data.insert("path".to_string(), Value::String(file_path.to_string_lossy().to_string()));

        let result = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(hm.execute(HookPoint::SkillBefore, &mut ctx));
        assert_eq!(result, HookResult::Continue);

        let warning = ctx.data.get("stale_warning").and_then(|v| v.as_str()).unwrap_or("");
        assert!(warning.contains("stale"), "Expected stale warning, got: {}", warning);
    }

    #[test]
    fn test_register_hooks_write_marks_written() {
        let (ws, dir) = temp_ws_monitor();
        let file_path = dir.path().join("test.rs");
        std::fs::write(&file_path, "fn main() {}").unwrap();

        {
            let inv = ws.inventory.read();
            inv.add_or_update(&file_path.to_string_lossy()).unwrap();
        }

        let hm = HookManager::new();
        ws.register_hooks(&hm);

        let mut ctx = HookContext::new(HookPoint::SkillAfter, "agent_1", "DA");
        ctx.data.insert("path".to_string(), Value::String(file_path.to_string_lossy().to_string()));

        let _ = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(hm.execute(HookPoint::SkillAfter, &mut ctx));

        let inv = ws.inventory.read();
        let entry = inv.get_entry(&file_path.to_string_lossy()).unwrap();
        assert_eq!(entry.state, FileState::WrittenUnread);
    }

    #[tokio::test]
    async fn test_event_consumer_file_created() {
        let bus = Arc::new(EventBus::new(100));
        let dir = tempfile::TempDir::new().unwrap();

        let config = WorkspaceMonitorConfig {
            workspace_root: dir.path().to_path_buf(),
            watch_enabled: false,
            db_path: None,
            ..WorkspaceMonitorConfig::default()
        };

        let ws = WorkspaceMonitor::initialize(config, None, Some(bus.clone())).unwrap();
        ws.register_event_consumers();

        let test_file = dir.path().join("created.rs");
        std::fs::write(&test_file, "fn test() {}").unwrap();

        bus.emit(
            "iri://test_task",
            EventType::WorkspaceFileCreated.as_str(),
            "iri://test_agent",
            &test_file.to_string_lossy(),
        )
        .await;

        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        let inv = ws.inventory.read();
        let entry = inv.get_entry(&test_file.to_string_lossy());
        assert!(entry.is_some(), "File should be in inventory after Create event");
    }

    #[tokio::test]
    async fn test_event_consumer_file_removed() {
        let bus = Arc::new(EventBus::new(100));
        let dir = tempfile::TempDir::new().unwrap();

        let config = WorkspaceMonitorConfig {
            workspace_root: dir.path().to_path_buf(),
            watch_enabled: false,
            db_path: None,
            ..WorkspaceMonitorConfig::default()
        };

        let ws = WorkspaceMonitor::initialize(config, None, Some(bus.clone())).unwrap();
        ws.register_event_consumers();

        let test_file = dir.path().join("toremove.rs");
        std::fs::write(&test_file, "fn x() {}").unwrap();

        {
            let inv = ws.inventory.read();
            inv.add_or_update(&test_file.to_string_lossy()).unwrap();
        }
        assert!(ws.inventory.read().get_entry(&test_file.to_string_lossy()).is_some());

        std::fs::remove_file(&test_file).unwrap();

        bus.emit(
            "iri://test_task",
            EventType::WorkspaceFileRemoved.as_str(),
            "iri://test_agent",
            &test_file.to_string_lossy(),
        )
        .await;

        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        let inv = ws.inventory.read();
        assert!(inv.get_entry(&test_file.to_string_lossy()).is_none(), "File should be removed from inventory");
    }

    #[test]
    fn test_hooks_no_path_noop() {
        let (ws, _dir) = temp_ws_monitor();
        let hm = HookManager::new();
        ws.register_hooks(&hm);

        let mut ctx = HookContext::new(HookPoint::SkillBefore, "agent_1", "DA");
        let result = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(hm.execute(HookPoint::SkillBefore, &mut ctx));
        assert_eq!(result, HookResult::Continue);
    }

    #[test]
    fn test_mark_file_written_updates_inventory() {
        let (ws, dir) = temp_ws_monitor();
        let file_path = dir.path().join("write.rs");
        std::fs::write(&file_path, "initial").unwrap();

        {
            let inv = ws.inventory.read();
            inv.add_or_update(&file_path.to_string_lossy()).unwrap();
        }

        ws.mark_file_written(&file_path.to_string_lossy());

        let inv = ws.inventory.read();
        let entry = inv.get_entry(&file_path.to_string_lossy()).unwrap();
        assert_eq!(entry.state, FileState::WrittenUnread);
    }

    #[tokio::test]
    async fn test_full_event_consumer_hooks_lifecycle() {
        let bus = Arc::new(EventBus::new(100));
        let dir = tempfile::TempDir::new().unwrap();
        let hm = HookManager::new();

        let config = WorkspaceMonitorConfig {
            workspace_root: dir.path().to_path_buf(),
            watch_enabled: false,
            db_path: None,
            ..WorkspaceMonitorConfig::default()
        };

        let ws = WorkspaceMonitor::initialize(config, None, Some(bus.clone())).unwrap();
        ws.register_event_consumers();
        ws.register_hooks(&hm);

        let test_file = dir.path().join("lifecycle.rs");
        let file_path_str = test_file.to_string_lossy().to_string();
        std::fs::write(&test_file, "fn start() {}").unwrap();

        // Step 1: Emit create event → consumer adds to inventory
        bus.emit(
            "iri://test_task",
            EventType::WorkspaceFileCreated.as_str(),
            "iri://test_agent",
            &file_path_str,
        )
        .await;
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        assert!(
            ws.inventory.read().get_entry(&file_path_str).is_some(),
            "File should exist after create event"
        );

        // Step 2: Mark stale externally, emit modified → consumer marks stale
        std::fs::write(&test_file, "fn updated() {}").unwrap();
        bus.emit(
            "iri://test_task",
            EventType::WorkspaceFileModified.as_str(),
            "iri://test_agent",
            &file_path_str,
        )
        .await;
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        let entry = ws.inventory.read().get_entry(&file_path_str).unwrap();
        assert_eq!(entry.state, FileState::ReadStale, "File should be stale after modify event");

        // Step 3: Hook SkillBefore read detects stale state
        let mut ctx = HookContext::new(HookPoint::SkillBefore, "agent_1", "DA");
        ctx.data.insert("path".to_string(), Value::String(file_path_str.clone()));
        let result = hm.execute(HookPoint::SkillBefore, &mut ctx).await;
        assert_eq!(result, HookResult::Continue);
        let warning = ctx.data.get("stale_warning").and_then(|v| v.as_str()).unwrap_or("");
        assert!(warning.contains("stale"), "Expected stale warning in lifecycle: {}", warning);

        // Step 4: File write → SkillAfter hook marks WrittenUnread
        let mut write_ctx = HookContext::new(HookPoint::SkillAfter, "agent_1", "DA");
        write_ctx.data.insert("path".to_string(), Value::String(file_path_str.clone()));
        let _ = hm.execute(HookPoint::SkillAfter, &mut write_ctx).await;
        let entry = ws.inventory.read().get_entry(&file_path_str).unwrap();
        assert_eq!(entry.state, FileState::WrittenUnread, "File should be WrittenUnread after write hook");

        // Step 5: Remove file + emit remove → consumer removes from inventory
        std::fs::remove_file(&test_file).unwrap();
        bus.emit(
            "iri://test_task",
            EventType::WorkspaceFileRemoved.as_str(),
            "iri://test_agent",
            &file_path_str,
        )
        .await;
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        assert!(
            ws.inventory.read().get_entry(&file_path_str).is_none(),
            "File should be removed from inventory after remove event"
        );
    }

    #[test]
    fn test_hook_unchanged_file_detection() {
        let (ws, dir) = temp_ws_monitor();
        let file_path = dir.path().join("unchanged.rs");
        std::fs::write(&file_path, "fn main() {}").unwrap();

        {
            let inv = ws.inventory.read();
            inv.add_or_update(&file_path.to_string_lossy()).unwrap();
            inv.mark_read(&file_path.to_string_lossy(), 0);
        }

        let hm = HookManager::new();
        ws.register_hooks(&hm);

        // File is ReadFresh with matching versions — hook should inject file_unchanged
        let mut ctx = HookContext::new(HookPoint::SkillBefore, "agent_1", "DA");
        ctx.data.insert("path".to_string(), Value::String(file_path.to_string_lossy().to_string()));

        let result = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(hm.execute(HookPoint::SkillBefore, &mut ctx));
        assert_eq!(result, HookResult::Continue);

        assert_eq!(
            ctx.data.get("file_unchanged").and_then(|v| v.as_bool()),
            Some(true),
            "Expected file_unchanged flag for ReadFresh file with matching version"
        );
        let hint = ctx.data.get("file_unchanged_hint").and_then(|v| v.as_str()).unwrap_or("");
        assert!(hint.contains("unchanged"), "Expected unchanged hint: {}", hint);
    }

    #[test]
    fn test_hook_stale_warning_for_stale_file() {
        let (ws, dir) = temp_ws_monitor();
        let file_path = dir.path().join("stale.rs");
        std::fs::write(&file_path, "fn stale() {}").unwrap();

        {
            let inv = ws.inventory.read();
            inv.add_or_update(&file_path.to_string_lossy()).unwrap();
            inv.mark_read(&file_path.to_string_lossy(), 0);
            inv.mark_stale(&file_path.to_string_lossy());
        }

        let hm = HookManager::new();
        ws.register_hooks(&hm);

        let mut ctx = HookContext::new(HookPoint::SkillBefore, "agent_1", "DA");
        ctx.data.insert("path".to_string(), Value::String(file_path.to_string_lossy().to_string()));

        let _ = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(hm.execute(HookPoint::SkillBefore, &mut ctx));

        assert!(
            ctx.data.get("file_unchanged").is_none(),
            "Stale file should NOT have file_unchanged flag"
        );
        assert!(
            ctx.data.get("stale_warning").is_some(),
            "Stale file SHOULD have stale_warning"
        );
    }

    // ── PerceptionStore integration ──

    #[tokio::test]
    async fn test_event_consumer_injects_perception_store_on_create() {
        let bus = Arc::new(EventBus::new(100));
        let dir = tempfile::TempDir::new().unwrap();
        let ps = Arc::new(PerceptionStore::new());

        let config = WorkspaceMonitorConfig {
            workspace_root: dir.path().to_path_buf(),
            watch_enabled: false,
            db_path: None,
            ..WorkspaceMonitorConfig::default()
        };

        let ws = WorkspaceMonitor::initialize(config, None, Some(bus.clone()))
            .unwrap()
            .with_perception_store(ps.clone());
        ws.register_event_consumers();

        let test_file = dir.path().join("percept_create.rs");
        std::fs::write(&test_file, "fn test() {}").unwrap();

        bus.emit(
            "iri://test_task",
            EventType::WorkspaceFileCreated.as_str(),
            "iri://test_agent",
            &test_file.to_string_lossy(),
        )
        .await;

        tokio::time::sleep(std::time::Duration::from_millis(300)).await;

        assert!(ps.has_new("iri://test_task"), "PerceptionStore should have new entry after create event");
        let text = ps.take_perception_text("iri://test_task");
        assert!(text.contains("新文件"), "Perception text should mention file creation: {}", text);
    }

    #[tokio::test]
    async fn test_event_consumer_injects_perception_store_on_modify() {
        let bus = Arc::new(EventBus::new(100));
        let dir = tempfile::TempDir::new().unwrap();
        let ps = Arc::new(PerceptionStore::new());

        let config = WorkspaceMonitorConfig {
            workspace_root: dir.path().to_path_buf(),
            watch_enabled: false,
            db_path: None,
            ..WorkspaceMonitorConfig::default()
        };

        let ws = WorkspaceMonitor::initialize(config, None, Some(bus.clone()))
            .unwrap()
            .with_perception_store(ps.clone());
        ws.register_event_consumers();

        let test_file = dir.path().join("percept_modify.rs");
        std::fs::write(&test_file, "fn old() {}").unwrap();
        std::fs::write(&test_file, "fn new() {}").unwrap();

        bus.emit(
            "iri://test_task",
            EventType::WorkspaceFileModified.as_str(),
            "iri://test_agent",
            &test_file.to_string_lossy(),
        )
        .await;

        tokio::time::sleep(std::time::Duration::from_millis(300)).await;

        assert!(ps.has_new("iri://test_task"), "PerceptionStore should have new entry after modify event");
        let text = ps.take_perception_text("iri://test_task");
        assert!(text.contains("外部变更"), "Perception text should mention external change: {}", text);
    }

    #[tokio::test]
    async fn test_event_consumer_injects_perception_store_on_remove() {
        let bus = Arc::new(EventBus::new(100));
        let dir = tempfile::TempDir::new().unwrap();
        let ps = Arc::new(PerceptionStore::new());

        let config = WorkspaceMonitorConfig {
            workspace_root: dir.path().to_path_buf(),
            watch_enabled: false,
            db_path: None,
            ..WorkspaceMonitorConfig::default()
        };

        let ws = WorkspaceMonitor::initialize(config, None, Some(bus.clone()))
            .unwrap()
            .with_perception_store(ps.clone());
        ws.register_event_consumers();

        let test_file = dir.path().join("percept_remove.rs");
        std::fs::write(&test_file, "fn gone() {}").unwrap();

        bus.emit(
            "iri://test_task",
            EventType::WorkspaceFileRemoved.as_str(),
            "iri://test_agent",
            &test_file.to_string_lossy(),
        )
        .await;

        tokio::time::sleep(std::time::Duration::from_millis(300)).await;

        assert!(ps.has_new("iri://test_task"), "PerceptionStore should have new entry after remove event");
        let text = ps.take_perception_text("iri://test_task");
        assert!(text.contains("已删除"), "Perception text should mention file removal: {}", text);
    }

    #[test]
    fn test_inject_file_perception_with_stale_files() {
        let (_ws, dir) = temp_ws_monitor();
        let ps = Arc::new(PerceptionStore::new());

        // Can't use with_perception_store after initialize since it consumes self
        // We need to build one directly with perception_store set
        let config = WorkspaceMonitorConfig {
            workspace_root: dir.path().to_path_buf(),
            watch_enabled: false,
            db_path: None,
            ..WorkspaceMonitorConfig::default()
        };

        let ws = WorkspaceMonitor::initialize(config, None, None)
            .unwrap()
            .with_perception_store(ps.clone());

        // Add some files to inventory
        let file1 = dir.path().join("stale1.rs");
        std::fs::write(&file1, "fn a() {}").unwrap();
        let file2 = dir.path().join("stale2.rs");
        std::fs::write(&file2, "fn b() {}").unwrap();

        {
            let inv = ws.inventory.read();
            inv.add_or_update(&file1.to_string_lossy()).unwrap();
            inv.add_or_update(&file2.to_string_lossy()).unwrap();
            inv.mark_read(&file1.to_string_lossy(), 1);
            inv.mark_read(&file2.to_string_lossy(), 1);
            inv.mark_stale(&file1.to_string_lossy());
            inv.mark_stale(&file2.to_string_lossy());
        }

        ws.inject_file_perception();

        let text = ps.take_perception_text("iri://task/t");
        assert!(!text.is_empty(), "Should have perception text after inject");
        assert!(text.contains("stale"), "Should mention stale files: {}", text);
    }

    #[test]
    fn test_generate_perception_text_empty_inventory() {
        let (ws, _dir) = temp_ws_monitor();
        let text = ws.generate_perception_text();
        // Fresh inventory with initial scan may have files
        // If empty, it returns None; otherwise it should have text
        let inv = ws.inventory.read();
        if inv.list_all().is_empty() {
            assert!(text.is_none(), "Empty inventory should return None");
        } else {
            assert!(text.is_some(), "Non-empty inventory should return Some");
        }
    }

    #[test]
    fn test_generate_perception_text_with_state_counts() {
        let (ws, dir) = temp_ws_monitor();

        let f1 = dir.path().join("active.rs");
        std::fs::write(&f1, "fn active() {}").unwrap();
        let f2 = dir.path().join("stale.rs");
        std::fs::write(&f2, "fn stale() {}").unwrap();

        {
            let inv = ws.inventory.read();
            inv.add_or_update(&f1.to_string_lossy()).unwrap();
            inv.add_or_update(&f2.to_string_lossy()).unwrap();
            inv.mark_read(&f1.to_string_lossy(), 1);
            inv.mark_read(&f2.to_string_lossy(), 1);
            inv.mark_stale(&f2.to_string_lossy());
        }

        let text = ws.generate_perception_text();
        assert!(text.is_some(), "Should generate perception text");
        let t = text.unwrap();
        assert!(t.contains("共"), "Should mention total file count: {}", t);
    }

    #[test]
    fn test_inject_file_perception_no_perception_store_noop() {
        let (ws, _dir) = temp_ws_monitor();
        // Should not panic when perception_store is None
        ws.inject_file_perception();
    }

    #[test]
    fn test_with_perception_store_chain() {
        let dir = tempfile::TempDir::new().unwrap();
        let config = WorkspaceMonitorConfig {
            workspace_root: dir.path().to_path_buf(),
            watch_enabled: false,
            db_path: None,
            ..WorkspaceMonitorConfig::default()
        };
        let ps = Arc::new(PerceptionStore::new());
        let ws = WorkspaceMonitor::initialize(config, None, None)
            .unwrap()
            .with_perception_store(ps.clone());

        // Verify it's configured
        ws.inject_file_perception();
        // Should not panic, meaning perception_store is Some
        let _ = ws.generate_perception_text();
    }

    /// Full integration: event consumer → perception store → take → verify
    #[tokio::test]
    async fn test_event_to_perception_full_flow() {
        let bus = Arc::new(EventBus::new(100));
        let dir = tempfile::TempDir::new().unwrap();
        let ps = Arc::new(PerceptionStore::new());

        let config = WorkspaceMonitorConfig {
            workspace_root: dir.path().to_path_buf(),
            watch_enabled: false,
            db_path: None,
            ..WorkspaceMonitorConfig::default()
        };

        let ws = WorkspaceMonitor::initialize(config, None, Some(bus.clone()))
            .unwrap()
            .with_perception_store(ps.clone());
        ws.register_event_consumers();

        let test_file = dir.path().join("full_flow.rs");
        std::fs::write(&test_file, "fn test() {}").unwrap();

        // Emit create event
        bus.emit(
            "iri://task_full",
            EventType::WorkspaceFileCreated.as_str(),
            "iri://agent",
            &test_file.to_string_lossy(),
        )
        .await;
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        // take perception text
        let text1 = ps.take_perception_text("iri://task_full");
        assert!(!text1.is_empty(), "Perception should be available after create event");
        assert!(text1.contains("新文件"), "Should mention new file: {}", text1);

        // Second take should be empty (consumed)
        let text2 = ps.take_perception_text("iri://task_full");
        assert!(text2.is_empty(), "Second take should be empty after consumption");
    }
}
