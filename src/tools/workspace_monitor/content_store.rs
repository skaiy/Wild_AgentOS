use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use lru::LruCache;
use parking_lot::{Mutex, RwLock};
use sha2::{Digest, Sha256};
use tracing::{debug, warn};

use crate::tools::workspace_monitor::DiffEngine;

/// Read mode for file reading operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadMode {
    /// Full read (first time or cache missed).
    Full,
    /// Diff mode: return unified diff if file changed.
    Diff,
    /// Force re-read ignoring all caches.
    ForceRefresh,
}

/// Result of a file read operation.
#[derive(Debug, Clone)]
pub struct ReadResult {
    pub path: String,
    pub lines: Vec<String>,
    pub total_lines: usize,
    pub changed: bool,
    pub changed_ranges: Option<Vec<(usize, usize)>>,
    pub unified_diff: Option<String>,
    pub from_cache: bool,
    pub version: u64,
}

/// Cached content entry in the LRU.
#[derive(Debug, Clone)]
struct CachedContent {
    lines: Vec<String>,
    hash: String,
    mtime: i64,
    version: u64,
}

/// Content cache with LRU eviction, SHA-256 change detection, and sled version store.
pub struct ContentStore {
    /// In-memory LRU: file path → cached content (mtime-keyed).
    lines_cache: Mutex<LruCache<String, CachedContent>>,
    /// Path → current version number.
    version_index: RwLock<HashMap<String, u64>>,
    /// Sled database for storing historical versions (path → versioned content).
    version_store: Option<sled::Db>,
    /// Maximum cache size in bytes (approximate).
    max_cache_bytes: usize,
}

impl ContentStore {
    /// Create a new ContentStore.
    ///
    /// * `cache_capacity` - Number of files to hold in LRU cache.
    /// * `max_cache_bytes` - Approximate maximum cache memory usage.
    /// * `sled_db` - Optional sled database for historical version storage.
    pub fn new(
        cache_capacity: usize,
        max_cache_bytes: usize,
        sled_db: Option<sled::Db>,
    ) -> Self {
        Self {
            lines_cache: Mutex::new(LruCache::new(
                std::num::NonZeroUsize::new(cache_capacity.max(1)).unwrap(),
            )),
            version_index: RwLock::new(HashMap::new()),
            version_store: sled_db,
            max_cache_bytes,
        }
    }

    /// Read a file from disk, applying caching and optional diff.
    ///
    /// Returns a `ReadResult` with content, version info, and optional diff.
    pub fn read_file(&self, path: &str, mode: ReadMode) -> std::io::Result<ReadResult> {
        let disk_content = std::fs::read_to_string(path)?;
        let disk_lines: Vec<String> = disk_content.lines().map(|l| l.to_string()).collect();
        let total_lines = disk_lines.len();
        let disk_mtime = std::fs::metadata(path)
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        let disk_hash = hash_content(&disk_content);

        let mut version_index = self.version_index.write();
        let current_version = version_index.get(path).copied().unwrap_or(0);

        // Force refresh mode: always read fresh
        if mode == ReadMode::ForceRefresh {
            let new_version = current_version + 1;
            version_index.insert(path.to_string(), new_version);
            self.store_version_in_sled(path, &disk_content, new_version);
            drop(version_index);
            self.invalidate_cache(path);

            return Ok(ReadResult {
                path: path.to_string(),
                lines: disk_lines,
                total_lines,
                changed: true,
                changed_ranges: None,
                unified_diff: None,
                from_cache: false,
                version: new_version,
            });
        }

        let cached = {
            let mut cache = self.lines_cache.lock();
            cache.get(path).cloned()
        };

        if let Some(cached) = cached {
            if disk_mtime == cached.mtime && !mode.is_diff() {
                return Ok(ReadResult {
                    path: path.to_string(),
                    lines: cached.lines,
                    total_lines,
                    changed: false,
                    changed_ranges: None,
                    unified_diff: None,
                    from_cache: true,
                    version: cached.version,
                });
            }

            if disk_hash == cached.hash {
                let mut cache = self.lines_cache.lock();
                cache.put(path.to_string(), CachedContent {
                    lines: cached.lines.clone(),
                    hash: cached.hash.clone(),
                    mtime: disk_mtime,
                    version: cached.version,
                });
                return Ok(ReadResult {
                    path: path.to_string(),
                    lines: cached.lines,
                    total_lines,
                    changed: false,
                    changed_ranges: None,
                    unified_diff: None,
                    from_cache: true,
                    version: cached.version,
                });
            }

            let new_version = current_version + 1;
            version_index.insert(path.to_string(), new_version);
            self.store_version_in_sled(path, &disk_content, new_version);
            drop(version_index);

            let (changed_ranges, unified_diff) = if mode == ReadMode::Diff {
                let ranges = DiffEngine::changed_ranges(&cached.lines, &disk_lines);
                let diff = DiffEngine::unified_diff(
                    &cached.lines,
                    &disk_lines,
                    path,
                    cached.version,
                    new_version,
                );
                (Some(ranges), Some(diff))
            } else {
                (None, None)
            };

            let mut cache = self.lines_cache.lock();
            cache.put(path.to_string(), CachedContent {
                lines: disk_lines.clone(),
                hash: disk_hash,
                mtime: disk_mtime,
                version: new_version,
            });

            return Ok(ReadResult {
                path: path.to_string(),
                lines: disk_lines,
                total_lines,
                changed: true,
                changed_ranges,
                unified_diff,
                from_cache: false,
                version: new_version,
            });
        }

        let new_version = current_version + 1;
        version_index.insert(path.to_string(), new_version);
        self.store_version_in_sled(path, &disk_content, new_version);
        drop(version_index);

        let mut cache = self.lines_cache.lock();
        cache.put(path.to_string(), CachedContent {
            lines: disk_lines.clone(),
            hash: disk_hash,
            mtime: disk_mtime,
            version: new_version,
        });

        Ok(ReadResult {
            path: path.to_string(),
            lines: disk_lines,
            total_lines,
            changed: true,
            changed_ranges: None,
            unified_diff: None,
            from_cache: false,
            version: new_version,
        })
    }

    /// Invalidate a specific file from the cache.
    pub fn invalidate(&self, path: &str) {
        let mut cache = self.lines_cache.lock();
        cache.pop(path);
        debug!(path = %path, "ContentStore: cache invalidated");
    }

    /// Try to get cached content for a file (without disk read).
    pub fn try_get_cached(&self, path: &str) -> Option<Vec<String>> {
        let mut cache = self.lines_cache.lock();
        cache.get(path).map(|c| c.lines.clone())
    }

    /// Get the current version number for a file.
    pub fn get_version(&self, path: &str) -> u64 {
        self.version_index.read().get(path).copied().unwrap_or(0)
    }

    /// Get the content hash for a file if cached.
    pub fn get_hash(&self, path: &str) -> Option<String> {
        let mut cache = self.lines_cache.lock();
        cache.get(path).map(|c| c.hash.clone())
    }

    /// Invalidate all cached content.
    pub fn clear(&self) {
        let mut cache = self.lines_cache.lock();
        cache.clear();
        let mut vi = self.version_index.write();
        vi.clear();
        debug!("ContentStore: all caches cleared");
    }

    /// Retrieve a specific version of file content from sled.
    pub fn get_version_content(&self, path: &str, version: u64) -> Option<String> {
        let db = self.version_store.as_ref()?;
        let key = format!("version:{}:v{}", path, version);
        db.get(key.as_bytes())
            .ok()?
            .map(|ivec| String::from_utf8_lossy(&ivec).to_string())
    }

    // ── Private helpers ──

    fn invalidate_cache(&self, path: &str) {
        let mut cache = self.lines_cache.lock();
        cache.pop(path);
    }

    fn store_version_in_sled(&self, path: &str, content: &str, version: u64) {
        if let Some(ref db) = self.version_store {
            let key = format!("version:{}:v{}", path, version);
            if let Err(e) = db.insert(key.as_bytes(), content.as_bytes()) {
                warn!(
                    path = %path, version = version, error = %e,
                    "ContentStore: failed to store version in sled"
                );
            }
        }
    }
}

/// Compute SHA-256 hash of content.
fn hash_content(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    let result = hasher.finalize();
    format!("sha256:{}", hex::encode(result))
}

impl ReadMode {
    fn is_diff(&self) -> bool {
        matches!(self, ReadMode::Diff)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn create_test_file(dir: &TempDir, name: &str, content: &str) -> String {
        let path = dir.path().join(name);
        std::fs::write(&path, content).unwrap();
        path.to_string_lossy().to_string()
    }

    #[test]
    fn test_basic_read() {
        let dir = TempDir::new().unwrap();
        let path = create_test_file(&dir, "test.txt", "hello\nworld\n");

        let store = ContentStore::new(100, 65536, None);
        let result = store.read_file(&path, ReadMode::Full).unwrap();

        assert_eq!(result.total_lines, 2);
        assert!(!result.from_cache);
        assert_eq!(result.version, 1);
    }

    #[test]
    fn test_cache_hit() {
        let dir = TempDir::new().unwrap();
        let path = create_test_file(&dir, "test.txt", "hello\nworld\n");

        let store = ContentStore::new(100, 65536, None);
        let r1 = store.read_file(&path, ReadMode::Full).unwrap();
        assert!(!r1.from_cache);

        let r2 = store.read_file(&path, ReadMode::Full).unwrap();
        assert!(r2.from_cache);
        assert_eq!(r2.version, r1.version);
    }

    #[test]
    fn test_change_detection() {
        let dir = TempDir::new().unwrap();
        let path = create_test_file(&dir, "test.txt", "hello\nworld\n");

        let store = ContentStore::new(100, 65536, None);
        let r1 = store.read_file(&path, ReadMode::Full).unwrap();
        assert_eq!(r1.version, 1);

        // Modify the file
        create_test_file(&dir, "test.txt", "hello\nmodified\nworld\n");

        let r2 = store.read_file(&path, ReadMode::Diff).unwrap();
        assert_eq!(r2.version, 2);
        assert!(r2.changed);
        assert!(r2.changed_ranges.is_some());
        assert!(r2.unified_diff.is_some());
        assert!(!r2.from_cache);
    }

    #[test]
    fn test_invalidate() {
        let dir = TempDir::new().unwrap();
        let path = create_test_file(&dir, "test.txt", "content");

        let store = ContentStore::new(100, 65536, None);
        let r1 = store.read_file(&path, ReadMode::Full).unwrap();
        assert!(!r1.from_cache);

        store.invalidate(&path);
        assert!(store.try_get_cached(&path).is_none());
    }

    #[test]
    fn test_hash_content() {
        let h1 = hash_content("hello");
        let h2 = hash_content("hello");
        let h3 = hash_content("world");

        assert_eq!(h1, h2);
        assert_ne!(h1, h3);
        assert!(h1.starts_with("sha256:"));
    }

    #[test]
    fn test_force_refresh() {
        let dir = TempDir::new().unwrap();
        let path = create_test_file(&dir, "test.txt", "content");

        let store = ContentStore::new(100, 65536, None);
        let r1 = store.read_file(&path, ReadMode::Full).unwrap();
        let r2 = store.read_file(&path, ReadMode::ForceRefresh).unwrap();

        assert!(r2.changed);
        assert!(r2.version > r1.version);
        assert!(!r2.from_cache);
    }
}
