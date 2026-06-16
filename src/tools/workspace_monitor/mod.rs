use std::path::PathBuf;
use std::sync::Arc;

use parking_lot::RwLock;
use tokio::sync::broadcast;
use tracing::{debug, info, instrument};

use crate::core::event_bus::{EventBus, EventType};
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
    /// Optional sled database path for persistent storage.
    pub sled_path: Option<PathBuf>,
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
            sled_path: None,
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
}

impl WorkspaceMonitor {
    /// Initialize the workspace monitor with the given config.
    ///
    /// Sets up:
    /// 1. Sled database (if path configured)
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

        // Initialize sled database
        let (meta_db, content_db) = Self::open_sled_databases(&config)?;
        let meta_db = meta_db.map(Arc::new);
        let content_db = content_db.map(Arc::new);

        // ContentStore
        let content_store = Arc::new(ContentStore::new(
            config.content_cache_capacity,
            config.content_store_max_bytes,
            content_db.clone().map(|db| (*db).clone()),
        ));

        // FileInventory
        let inventory = Arc::new(RwLock::new(FileInventory::new(
            blackboard.clone(),
            meta_db.clone().map(|db| (*db).clone()),
            config.exclude_patterns.clone(),
        )));

        // SnapshotManager
        let snap_db = Arc::new(
            sled::Config::new()
                .temporary(true)
                .open()
                .map_err(|e| format!("Failed to open snapshot sled DB: {}", e))?,
        );
        let snapshot_manager = Arc::new(SnapshotManager::new(
            snap_db,
            content_store.clone(),
            inventory.clone(),
        ));

        let event_bus_for_struct = event_bus.clone();

        // WatchEngine
        let watch_engine = if let Some(eb) = event_bus {
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
            match WatchEngine::start(&root, watch_config, eb) {
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

        // Perform initial scan
        {
            let inv = inventory.read();
            let discovered = inv.full_scan(&root);
            debug!(discovered = discovered, "Initial workspace scan completed");
        }

        info!("WorkspaceMonitor initialized for root={}", root);

        Ok(Self {
            config,
            inventory,
            content_store,
            snapshot_manager,
            watch_engine,
            event_bus: event_bus_for_struct,
        })
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

    /// Mark a file as written by the agent.
    pub fn mark_file_written(&self, path: &str) {
        let inv = self.inventory.read();
        inv.mark_written(path);
        self.content_store.invalidate(path);
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
                            }
                            EventType::WorkspaceFileModified => {
                                inventory.read().mark_stale(&event.payload);
                            }
                            EventType::WorkspaceFileRemoved => {
                                inventory.read().remove(&event.payload);
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
                    if entry.state == FileState::ReadStale {
                        ctx.data.insert(
                            "stale_warning".to_string(),
                            serde_json::Value::String(format!(
                                "File '{}' is stale (last read version {}), consider re-reading",
                                path, entry.last_read_version
                            )),
                        );
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

    // ── Private ──

    fn open_sled_databases(
        config: &WorkspaceMonitorConfig,
    ) -> Result<(Option<sled::Db>, Option<sled::Db>), String> {
        match &config.sled_path {
            Some(path) => {
                std::fs::create_dir_all(path)
                    .map_err(|e| format!("Failed to create sled directory: {}", e))?;

                let meta_path = path.join("metadata");
                let content_path = path.join("content");

                let meta_db = sled::Config::new()
                    .path(&meta_path)
                    .cache_capacity(64 * 1024 * 1024)
                    .open()
                    .map_err(|e| format!("Failed to open metadata sled DB: {}", e))?;

                let content_db = sled::Config::new()
                    .path(&content_path)
                    .cache_capacity(128 * 1024 * 1024)
                    .open()
                    .map_err(|e| format!("Failed to open content sled DB: {}", e))?;

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
            sled_path: None,
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
            sled_path: None,
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
            sled_path: None,
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
            sled_path: None,
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
}
