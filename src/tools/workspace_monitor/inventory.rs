use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use tracing::{debug, instrument, warn};
use walkdir::WalkDir;

use crate::memory::l2_blackboard::Blackboard;

/// File state machine states.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FileState {
    /// File exists on disk but not yet tracked.
    Undiscovered,
    /// Discovered via full_scan().
    Discovered,
    /// File has been read and is up-to-date.
    ReadFresh,
    /// File was read but has been modified externally.
    ReadStale,
    /// File was written by the Agent but not yet re-read.
    WrittenUnread,
}

impl FileState {
    pub fn as_str(&self) -> &'static str {
        match self {
            FileState::Undiscovered => "undiscovered",
            FileState::Discovered => "discovered",
            FileState::ReadFresh => "read_fresh",
            FileState::ReadStale => "read_stale",
            FileState::WrittenUnread => "written_unread",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "undiscovered" => FileState::Undiscovered,
            "discovered" => FileState::Discovered,
            "read_fresh" => FileState::ReadFresh,
            "read_stale" => FileState::ReadStale,
            "written_unread" => FileState::WrittenUnread,
            _ => FileState::Undiscovered,
        }
    }
}

/// A single file entry in the inventory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntry {
    pub path: String,
    pub file_size: u64,
    pub file_ext: String,
    pub language: String,
    pub mtime: i64,
    pub content_hash: String,
    pub state: FileState,
    pub last_read_at: Option<i64>,
    pub last_read_version: u64,
    pub current_version: u64,
    pub read_count: u64,
}

impl FileEntry {
    /// The IRI used for this file in L2 Named Graph.
    pub fn iri(&self) -> String {
        format!("iri://workspace/file/{}", self.path)
    }

    /// The parent directory IRI.
    pub fn parent_dir_iri(&self) -> String {
        let parent = Path::new(&self.path)
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        format!("iri://workspace/dir/{}/", parent)
    }
}

/// Classification of a language from a file extension.
fn classify_language(ext: &str) -> &'static str {
    match ext {
        "rs" => "rust",
        "ts" | "tsx" => "typescript",
        "js" | "jsx" | "mjs" | "cjs" => "javascript",
        "py" => "python",
        "go" => "go",
        "java" => "java",
        "c" | "h" => "c",
        "cpp" | "hpp" | "cc" | "cxx" => "cpp",
        "rs" => "rust",
        "rb" => "ruby",
        "php" => "php",
        "swift" => "swift",
        "kt" | "kts" => "kotlin",
        "scala" => "scala",
        "toml" => "toml",
        "json" => "json",
        "yaml" | "yml" => "yaml",
        "md" | "mdx" => "markdown",
        "html" | "htm" => "html",
        "css" | "scss" | "less" => "css",
        _ => "unknown",
    }
}

/// Shared workspace RDF constant.
pub const WORKSPACE_GRAPH: &str = "iri://workspace";

/// FileInventory — thin facade over L2 Blackboard (RDF) with sled hot cache.
///
/// The authority data source is L2 (Oxigraph RDF named graph `iri://workspace`).
/// Sled serves as a hot cache for fast metadata lookups.
pub struct FileInventory {
    /// L2 Blackboard (RDF triple store) for authority data.
    blackboard: Option<Arc<Blackboard>>,
    /// Sled hot cache: path → serialized FileEntry.
    cache: Option<sled::Db>,
    /// In-memory cache for fastest access (no sled deserialization).
    mem_cache: RwLock<HashMap<String, FileEntry>>,
    /// Exclude patterns for scanning.
    exclude_patterns: Vec<String>,
}

impl FileInventory {
    /// Create a new FileInventory.
    ///
    /// * `blackboard` - Optional L2 Blackboard for RDF storage.
    /// * `sled_db` - Optional sled database for hot cache.
    /// * `exclude_patterns` - Glob patterns to exclude from scanning (e.g., "node_modules/").
    pub fn new(
        blackboard: Option<Arc<Blackboard>>,
        sled_db: Option<sled::Db>,
        exclude_patterns: Vec<String>,
    ) -> Self {
        let mut mem_cache = HashMap::new();

        // Pre-warm from sled if available
        if let Some(ref db) = sled_db {
            for result in db.iter() {
                if let Ok((key, value)) = result {
                    let path = String::from_utf8_lossy(&key).to_string();
                    if let Ok(entry) = serde_json::from_slice::<FileEntry>(&value) {
                        mem_cache.insert(path, entry);
                    }
                }
            }
        }

        Self {
            blackboard,
            cache: sled_db,
            mem_cache: RwLock::new(mem_cache),
            exclude_patterns,
        }
    }

    /// Perform a full directory scan of `root`, discovering all files.
    ///
    /// Returns the number of discovered files.
    #[instrument(skip(self))]
    pub fn full_scan(&self, root: &str) -> usize {
        let mut count = 0;

        for entry in WalkDir::new(root)
            .into_iter()
            .filter_entry(|e| !self.is_excluded(e.path()))
        {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };

            if !entry.file_type().is_file() {
                continue;
            }

            let path = entry.path().to_string_lossy().to_string();
            // Only add if not already tracked
            if self.get_entry(&path).is_none() {
                self.add_entry(&path);
                count += 1;
            }
        }

        debug!(root = %root, discovered = count, "FileInventory: full scan completed");
        count
    }

    /// Add or update a single file entry by scanning the file on disk.
    pub fn add_or_update(&self, path: &str) -> Option<FileEntry> {
        let path_obj = Path::new(path);
        if !path_obj.is_file() {
            // File doesn't exist — treat as removal
            self.remove_internal(path);
            return None;
        }

        let metadata = match std::fs::metadata(path) {
            Ok(m) => m,
            Err(_) => return None,
        };

        let file_size = metadata.len();
        let mtime = metadata
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);

        let content = std::fs::read_to_string(path).unwrap_or_default();
        let content_hash = hash_content(&content);

        let ext = path_obj
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_string();

        let language = classify_language(&ext).to_string();
        let state = FileState::Discovered;
        let version = 0;

        let entry = FileEntry {
            path: path.to_string(),
            file_size,
            file_ext: ext,
            language,
            mtime,
            content_hash,
            state,
            last_read_at: None,
            last_read_version: 0,
            current_version: version,
            read_count: 0,
        };

        self.store_entry(&entry);
        self.sync_to_l2(&entry);

        debug!(path = %path, "FileInventory: file added/updated");
        Some(entry)
    }

    /// Mark a file as stale (externally modified).
    pub fn mark_stale(&self, path: &str) {
        let mut mem = self.mem_cache.write();
        if let Some(entry) = mem.get_mut(path) {
            entry.state = FileState::ReadStale;
            // Update content hash & mtime from disk
            if let Ok(content) = std::fs::read_to_string(path) {
                entry.content_hash = hash_content(&content);
            }
            if let Ok(meta) = std::fs::metadata(path) {
                if let Ok(t) = meta.modified() {
                    if let Ok(d) = t.duration_since(std::time::UNIX_EPOCH) {
                        entry.mtime = d.as_millis() as i64;
                    }
                }
                entry.file_size = meta.len();
            }
            entry.current_version += 1;
            let cloned = entry.clone();
            drop(mem);
            self.persist_to_cache(&cloned);
            self.sync_to_l2(&cloned);
            debug!(path = %path, version = cloned.current_version, "FileInventory: marked stale");
        }
    }

    /// Mark a file as read (fresh).
    pub fn mark_read(&self, path: &str, version: u64) {
        let mut mem = self.mem_cache.write();
        if let Some(entry) = mem.get_mut(path) {
            entry.state = FileState::ReadFresh;
            entry.last_read_at = Some(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as i64,
            );
            entry.last_read_version = version;
            entry.read_count += 1;
            let cloned = entry.clone();
            drop(mem);
            self.persist_to_cache(&cloned);
            self.sync_to_l2(&cloned);
        }
    }

    /// Mark a file as written (but not re-read) by the agent.
    pub fn mark_written(&self, path: &str) {
        let mut mem = self.mem_cache.write();
        if let Some(entry) = mem.get_mut(path) {
            entry.state = FileState::WrittenUnread;
            entry.current_version += 1;
            let cloned = entry.clone();
            drop(mem);
            self.persist_to_cache(&cloned);
            self.sync_to_l2(&cloned);
            debug!(path = %path, "FileInventory: marked written_unread");
        } else {
            // New file written by agent
            drop(mem);
            self.add_or_update(path);
        }
    }

    /// Remove a file from the inventory (e.g., on deletion).
    pub fn remove(&self, path: &str) -> bool {
        self.remove_internal(path);
        self.remove_from_l2(path);
        debug!(path = %path, "FileInventory: file removed");
        true
    }

    /// Get a file entry by path.
    pub fn get_entry(&self, path: &str) -> Option<FileEntry> {
        let mem = self.mem_cache.read();
        mem.get(path).cloned()
    }

    /// List all files matching a state filter.
    pub fn list_by_state(&self, state: FileState) -> Vec<FileEntry> {
        let mem = self.mem_cache.read();
        mem.values()
            .filter(|e| e.state == state)
            .cloned()
            .collect()
    }

    /// List all tracked files.
    pub fn list_all(&self) -> Vec<FileEntry> {
        let mem = self.mem_cache.read();
        mem.values().cloned().collect()
    }

    /// List files under a directory prefix.
    pub fn list_dir(&self, dir_prefix: &str) -> Vec<FileEntry> {
        let prefix = if dir_prefix.ends_with('/') {
            dir_prefix.to_string()
        } else {
            format!("{}/", dir_prefix)
        };
        let mem = self.mem_cache.read();
        mem.values()
            .filter(|e| e.path.starts_with(&prefix) || e.path.starts_with(dir_prefix))
            .cloned()
            .collect()
    }

    /// Count files by state.
    pub fn state_counts(&self) -> HashMap<String, usize> {
        let mut counts = HashMap::new();
        let mem = self.mem_cache.read();
        for entry in mem.values() {
            *counts.entry(entry.state.as_str().to_string()).or_insert(0) += 1;
        }
        counts
    }

    /// Total number of tracked files.
    pub fn total_count(&self) -> usize {
        self.mem_cache.read().len()
    }

    /// Get stale files (for prompting re-read).
    pub fn stale_files(&self) -> Vec<FileEntry> {
        self.list_by_state(FileState::ReadStale)
    }

    /// Get files with state ReadStale or WrittenUnread.
    pub fn unread_files(&self) -> Vec<FileEntry> {
        let mut result = self.list_by_state(FileState::ReadStale);
        result.extend(self.list_by_state(FileState::WrittenUnread));
        result
    }

    // ── Private helpers ──

    fn is_excluded(&self, path: &std::path::Path) -> bool {
        let path_str = path.to_string_lossy();
        // Normalize path separators
        let normalized = path_str.replace('\\', "/");
        for pattern in &self.exclude_patterns {
            let pat = pattern.replace('\\', "/");
            if normalized.contains(&pat) || normalized.ends_with(&pat) {
                return true;
            }
        }
        false
    }

    fn add_entry(&self, path: &str) -> Option<FileEntry> {
        let path_obj = Path::new(path);
        let content = std::fs::read_to_string(path).unwrap_or_default();
        let content_hash = hash_content(&content);
        let metadata = std::fs::metadata(path).ok()?;

        let mtime = metadata
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);

        let ext = path_obj
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_string();

        let language = classify_language(&ext).to_string();

        let entry = FileEntry {
            path: path.to_string(),
            file_size: metadata.len(),
            file_ext: ext,
            language,
            mtime,
            content_hash,
            state: FileState::Discovered,
            last_read_at: None,
            last_read_version: 0,
            current_version: 0,
            read_count: 0,
        };

        self.store_entry(&entry);
        self.sync_to_l2(&entry);
        Some(entry)
    }

    fn store_entry(&self, entry: &FileEntry) {
        // In-memory cache
        {
            let mut mem = self.mem_cache.write();
            mem.insert(entry.path.clone(), entry.clone());
        }

        // Sled persistence
        if let Some(ref db) = self.cache {
            if let Ok(encoded) = serde_json::to_vec(entry) {
                if let Err(e) = db.insert(entry.path.as_bytes(), encoded) {
                    warn!(path = %entry.path, error = %e, "FileInventory: sled insert failed");
                }
            }
        }
    }

    fn remove_internal(&self, path: &str) {
        {
            let mut mem = self.mem_cache.write();
            mem.remove(path);
        }
        if let Some(ref db) = self.cache {
            let _ = db.remove(path.as_bytes());
        }
    }

    fn persist_to_cache(&self, entry: &FileEntry) {
        if let Some(ref db) = self.cache {
            if let Ok(encoded) = serde_json::to_vec(entry) {
                if let Err(e) = db.insert(entry.path.as_bytes(), encoded) {
                    warn!(path = %entry.path, error = %e, "FileInventory: sled persist failed");
                }
            }
        }
    }

    fn sync_to_l2(&self, entry: &FileEntry) {
        let blackboard = match self.blackboard.as_ref() {
            Some(b) => b,
            None => return,
        };

        let iri = entry.iri();
        let parent_dir_iri = entry.parent_dir_iri();

        let json_ld = serde_json::json!({
            "@id": &iri,
            "@type": ["ws:File"],
            "ws:filePath": entry.path,
            "ws:fileSize": entry.file_size,
            "ws:fileExt": entry.file_ext,
            "ws:language": entry.language,
            "ws:mtime": entry.mtime,
            "ws:contentHash": entry.content_hash,
            "ws:state": entry.state.as_str(),
            "ws:lastReadAt": entry.last_read_at.unwrap_or(0),
            "ws:lastReadVersion": entry.last_read_version,
            "ws:currentVersion": entry.current_version,
            "ws:readCount": entry.read_count,
            "ws:parentDir": parent_dir_iri,
        });

        let config = crate::CoreConfig {
            max_node_size: 65536,
            ..crate::CoreConfig::default()
        };

        if let Err(e) = blackboard.write_node_to_graph(&iri, &json_ld.to_string(), WORKSPACE_GRAPH, &config) {
            warn!(path = %entry.path, error = %e, "FileInventory: L2 sync failed");
        }
    }

    fn remove_from_l2(&self, path: &str) {
        let blackboard = match self.blackboard.as_ref() {
            Some(b) => b,
            None => return,
        };

        let iri = format!("iri://workspace/file/{}", path);
        let _ = blackboard.delete_node(&iri);
    }
}

fn hash_content(content: &str) -> String {
    use sha2::Digest;
    let mut hasher = sha2::Sha256::new();
    hasher.update(content.as_bytes());
    let result = hasher.finalize();
    format!("sha256:{}", hex::encode(result))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn create_workspace(dir: &TempDir, files: &[(&str, &str)]) {
        for (name, content) in files {
            let path = dir.path().join(name);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(&path, content).unwrap();
        }
    }

    #[test]
    fn test_full_scan() {
        let dir = TempDir::new().unwrap();
        create_workspace(&dir, &[
            ("src/main.rs", "fn main() {}"),
            ("src/lib.rs", "pub fn hello() {}"),
            ("README.md", "# Hello"),
        ]);

        let inventory = FileInventory::new(None, None, vec![]);
        let count = inventory.full_scan(&dir.path().to_string_lossy());
        assert_eq!(count, 3);
    }

    #[test]
    fn test_exclude_pattern() {
        let dir = TempDir::new().unwrap();
        create_workspace(&dir, &[
            ("src/main.rs", "fn main() {}"),
            ("node_modules/pkg/index.js", "module.exports = {};"),
            ("target/debug/app", "binary"),
        ]);

        let inventory = FileInventory::new(
            None, None,
            vec!["node_modules/".into(), "target/".into()],
        );
        let count = inventory.full_scan(&dir.path().to_string_lossy());
        assert_eq!(count, 1);
    }

    #[test]
    fn test_add_and_get() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.rs");
        std::fs::write(&path, "fn test() {}").unwrap();

        let inventory = FileInventory::new(None, None, vec![]);
        let entry = inventory.add_or_update(&path.to_string_lossy()).unwrap();

        assert_eq!(entry.file_ext, "rs");
        assert_eq!(entry.language, "rust");
        assert_eq!(entry.state, FileState::Discovered);

        let fetched = inventory.get_entry(&path.to_string_lossy()).unwrap();
        assert_eq!(fetched.path, entry.path);
    }

    #[test]
    fn test_mark_stale_and_read() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.rs");
        std::fs::write(&path, "v1").unwrap();

        let inventory = FileInventory::new(None, None, vec![]);
        inventory.add_or_update(&path.to_string_lossy());

        inventory.mark_read(&path.to_string_lossy(), 0);
        assert_eq!(inventory.get_entry(&path.to_string_lossy()).unwrap().state, FileState::ReadFresh);

        inventory.mark_stale(&path.to_string_lossy());
        assert_eq!(inventory.get_entry(&path.to_string_lossy()).unwrap().state, FileState::ReadStale);
    }

    #[test]
    fn test_list_by_state() {
        let dir = TempDir::new().unwrap();
        let p1 = dir.path().join("a.rs");
        let p2 = dir.path().join("b.rs");
        std::fs::write(&p1, "a").unwrap();
        std::fs::write(&p2, "b").unwrap();

        let inventory = FileInventory::new(None, None, vec![]);
        inventory.add_or_update(&p1.to_string_lossy());
        inventory.add_or_update(&p2.to_string_lossy());
        inventory.mark_read(&p1.to_string_lossy(), 0);

        assert_eq!(inventory.list_by_state(FileState::ReadFresh).len(), 1);
        assert_eq!(inventory.list_by_state(FileState::Discovered).len(), 1);
    }

    #[test]
    fn test_state_counts() {
        let dir = TempDir::new().unwrap();
        let p = dir.path().join("f.rs");
        std::fs::write(&p, "fn main() {}").unwrap();

        let inventory = FileInventory::new(None, None, vec![]);
        inventory.add_or_update(&p.to_string_lossy());
        inventory.mark_read(&p.to_string_lossy(), 0);

        let counts = inventory.state_counts();
        assert_eq!(*counts.get("read_fresh").unwrap_or(&0), 1);
    }
}
