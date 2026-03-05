use bitscout_core::search::matcher::{MatchOptions, Matcher};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MemoryEntry {
    pub key: String,
    pub content: String,
    pub created_at: u64,
}

pub struct MemoryStore {
    dir: PathBuf,
}

impl MemoryStore {
    pub fn new(dir: &Path) -> Result<Self, crate::Error> {
        fs::create_dir_all(dir).map_err(|e| crate::Error::Io(e.to_string()))?;
        Ok(Self {
            dir: dir.to_path_buf(),
        })
    }

    pub fn save(&self, key: &str, content: &str) -> Result<(), crate::Error> {
        let entry = MemoryEntry {
            key: key.to_string(),
            content: content.to_string(),
            created_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        };
        let json = serde_json::to_string_pretty(&entry)
            .map_err(|e| crate::Error::Io(e.to_string()))?;
        fs::write(self.entry_path(key), json).map_err(|e| crate::Error::Io(e.to_string()))?;
        Ok(())
    }

    pub fn get(&self, key: &str) -> Option<MemoryEntry> {
        let path = self.entry_path(key);
        let data = fs::read_to_string(path).ok()?;
        serde_json::from_str(&data).ok()
    }

    pub fn remove(&self, key: &str) -> Result<(), crate::Error> {
        let path = self.entry_path(key);
        if path.exists() {
            fs::remove_file(path).map_err(|e| crate::Error::Io(e.to_string()))?;
        }
        Ok(())
    }

    pub fn list(&self) -> Vec<String> {
        let entries = match fs::read_dir(&self.dir) {
            Ok(entries) => entries,
            Err(_) => return Vec::new(),
        };
        entries
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                name.strip_suffix(".json").map(|s| s.to_string())
            })
            .collect()
    }

    pub fn search(&self, query: &str) -> Vec<MemoryEntry> {
        let matcher = match Matcher::with_options(
            &[query],
            MatchOptions {
                case_insensitive: true,
                ..Default::default()
            },
        ) {
            Ok(m) => m,
            Err(_) => return Vec::new(),
        };

        self.list()
            .into_iter()
            .filter_map(|key| self.get(&key))
            .filter(|entry| {
                matcher.is_match(entry.key.as_bytes())
                    || matcher.is_match(entry.content.as_bytes())
            })
            .collect()
    }

    pub fn clear(&self) -> Result<(), crate::Error> {
        for key in self.list() {
            self.remove(&key)?;
        }
        Ok(())
    }

    fn entry_path(&self, key: &str) -> PathBuf {
        self.dir.join(format!("{key}.json"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_save_and_retrieve() {
        let tmp = TempDir::new().unwrap();
        let store = MemoryStore::new(tmp.path()).unwrap();
        store.save("greeting", "hello world").unwrap();
        let entry = store.get("greeting").expect("entry should exist");
        assert_eq!(entry.key, "greeting");
        assert_eq!(entry.content, "hello world");
    }

    #[test]
    fn test_remove() {
        let tmp = TempDir::new().unwrap();
        let store = MemoryStore::new(tmp.path()).unwrap();
        store.save("temp", "temporary data").unwrap();
        assert!(store.get("temp").is_some());
        store.remove("temp").unwrap();
        assert!(store.get("temp").is_none());
    }

    #[test]
    fn test_list() {
        let tmp = TempDir::new().unwrap();
        let store = MemoryStore::new(tmp.path()).unwrap();
        store.save("alpha", "first").unwrap();
        store.save("beta", "second").unwrap();
        let mut keys = store.list();
        keys.sort();
        assert_eq!(keys.len(), 2);
        assert_eq!(keys, vec!["alpha", "beta"]);
    }

    #[test]
    fn test_search() {
        let tmp = TempDir::new().unwrap();
        let store = MemoryStore::new(tmp.path()).unwrap();
        store.save("lint-config", "linting rules for Rust").unwrap();
        store.save("lint-ci", "CI linting pipeline setup").unwrap();
        store.save("deploy", "deployment instructions").unwrap();
        let results = store.search("linting");
        assert_eq!(results.len(), 2);
    }
}
