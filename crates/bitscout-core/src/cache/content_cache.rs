use std::fs;
use std::path::{Path, PathBuf};

use filetime::FileTime;

/// Default max cache size: 256 MB.
const DEFAULT_MAX_BYTES: u64 = 256 * 1024 * 1024;

/// Content-Addressable Storage for extracted text with LRU eviction.
/// Files are stored as `<cache_dir>/<key>` where key is typically a SHA256 hash.
pub struct ContentCache {
    dir: PathBuf,
    max_bytes: u64,
}

impl ContentCache {
    pub fn new(cache_dir: &Path) -> Self {
        Self {
            dir: cache_dir.to_path_buf(),
            max_bytes: DEFAULT_MAX_BYTES,
        }
    }

    pub fn new_with_limit(cache_dir: &Path, max_bytes: u64) -> Self {
        Self {
            dir: cache_dir.to_path_buf(),
            max_bytes,
        }
    }

    /// Get cached content by key. Touches mtime to mark as recently used.
    pub fn get(&self, key: &str) -> Option<String> {
        let path = self.dir.join(key);
        let content = fs::read_to_string(&path).ok()?;
        // Touch mtime for LRU tracking
        let now = FileTime::now();
        let _ = filetime::set_file_mtime(&path, now);
        Some(content)
    }

    /// Store content and evict oldest entries if over capacity.
    pub fn put(&self, key: &str, content: &str) -> Result<(), crate::Error> {
        fs::create_dir_all(&self.dir)
            .map_err(|e| crate::Error::Io(format!("cache dir create failed: {e}")))?;
        let path = self.dir.join(key);
        fs::write(&path, content)
            .map_err(|e| crate::Error::Io(format!("cache write failed: {e}")))?;
        self.evict_if_needed();
        Ok(())
    }

    /// Evict oldest entries (by mtime) until total size is within limit.
    fn evict_if_needed(&self) {
        let entries = match self.collect_entries() {
            Ok(e) => e,
            Err(_) => return,
        };

        let total: u64 = entries.iter().map(|e| e.size).sum();
        if total <= self.max_bytes {
            return;
        }

        // Sort by mtime ascending (oldest first)
        let mut sorted = entries;
        sorted.sort_by_key(|e| e.mtime);

        let mut current = total;
        for entry in &sorted {
            if current <= self.max_bytes {
                break;
            }
            if fs::remove_file(&entry.path).is_ok() {
                current = current.saturating_sub(entry.size);
            }
        }
    }

    fn collect_entries(&self) -> Result<Vec<CacheEntry>, std::io::Error> {
        let mut entries = Vec::new();
        for entry in fs::read_dir(&self.dir)? {
            let entry = entry?;
            let meta = entry.metadata()?;
            if meta.is_file() {
                entries.push(CacheEntry {
                    path: entry.path(),
                    size: meta.len(),
                    mtime: FileTime::from_last_modification_time(&meta),
                });
            }
        }
        Ok(entries)
    }
}

struct CacheEntry {
    path: PathBuf,
    size: u64,
    mtime: FileTime,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_cache_miss_then_hit() {
        let tmp = TempDir::new().unwrap();
        let cache = ContentCache::new(tmp.path());

        let key = "abc123def456";
        assert!(cache.get(key).is_none());

        cache.put(key, "extracted text content").unwrap();
        let cached = cache.get(key).unwrap();
        assert_eq!(cached, "extracted text content");
    }

    #[test]
    fn test_cache_overwrite() {
        let tmp = TempDir::new().unwrap();
        let cache = ContentCache::new(tmp.path());

        let key = "same_key";
        cache.put(key, "version 1").unwrap();
        cache.put(key, "version 2").unwrap();

        let cached = cache.get(key).unwrap();
        assert_eq!(cached, "version 2");
    }

    #[test]
    fn test_cache_different_keys() {
        let tmp = TempDir::new().unwrap();
        let cache = ContentCache::new(tmp.path());

        cache.put("key_a", "content A").unwrap();
        cache.put("key_b", "content B").unwrap();

        assert_eq!(cache.get("key_a").unwrap(), "content A");
        assert_eq!(cache.get("key_b").unwrap(), "content B");
    }

    #[test]
    fn test_lru_eviction() {
        let tmp = TempDir::new().unwrap();
        // Limit to 30 bytes — each entry ~10 bytes
        let cache = ContentCache::new_with_limit(tmp.path(), 30);

        cache.put("k1", "aaaaaaaaaa").unwrap(); // 10 bytes
        // Set k1 mtime to the past so it's oldest
        let past = FileTime::from_unix_time(1000, 0);
        filetime::set_file_mtime(tmp.path().join("k1"), past).unwrap();

        cache.put("k2", "bbbbbbbbbb").unwrap(); // 10 bytes
        cache.put("k3", "cccccccccc").unwrap(); // 10 bytes — total 30, at limit

        // This put pushes over 30 bytes, should evict k1 (oldest mtime)
        cache.put("k4", "dddddddddd").unwrap();

        assert!(cache.get("k1").is_none(), "k1 should have been evicted");
        assert!(cache.get("k4").is_some(), "k4 should exist");
    }

    #[test]
    fn test_get_touches_mtime() {
        let tmp = TempDir::new().unwrap();
        let cache = ContentCache::new(tmp.path());

        cache.put("key", "content").unwrap();

        // Set mtime to the past
        let past = FileTime::from_unix_time(1000, 0);
        filetime::set_file_mtime(tmp.path().join("key"), past).unwrap();

        // get() should touch mtime
        cache.get("key").unwrap();

        let meta = fs::metadata(tmp.path().join("key")).unwrap();
        let mtime = FileTime::from_last_modification_time(&meta);
        assert!(mtime > past, "mtime should have been updated by get()");
    }
}
