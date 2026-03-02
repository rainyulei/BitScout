use std::fs;
use std::path::{Path, PathBuf};

/// Content-Addressable Storage for extracted text.
/// Files are stored as `<cache_dir>/<key>` where key is typically a SHA256 hash.
pub struct ContentCache {
    dir: PathBuf,
}

impl ContentCache {
    pub fn new(cache_dir: &Path) -> Self {
        Self {
            dir: cache_dir.to_path_buf(),
        }
    }

    pub fn get(&self, key: &str) -> Option<String> {
        let path = self.dir.join(key);
        fs::read_to_string(&path).ok()
    }

    pub fn put(&self, key: &str, content: &str) -> Result<(), crate::Error> {
        fs::create_dir_all(&self.dir)
            .map_err(|e| crate::Error::Io(format!("cache dir create failed: {e}")))?;
        let path = self.dir.join(key);
        fs::write(&path, content)
            .map_err(|e| crate::Error::Io(format!("cache write failed: {e}")))?;
        Ok(())
    }
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
}
