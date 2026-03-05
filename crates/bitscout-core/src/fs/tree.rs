use ignore::WalkBuilder;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Debug, Clone)]
pub struct FileEntry {
    pub name: String,
    pub path: PathBuf,
    pub size: u64,
    pub mtime: SystemTime,
    pub is_dir: bool,
}

#[derive(Clone)]
pub struct FileTree {
    root: PathBuf,
    entries: Vec<FileEntry>,
}

impl FileTree {
    /// Scan a directory tree, respecting `.gitignore` rules.
    pub fn scan(root: &Path) -> Result<Self, crate::Error> {
        let root = root
            .canonicalize()
            .map_err(|e| crate::Error::Io(format!("cannot canonicalize root: {e}")))?;

        let mut entries = Vec::new();

        for result in WalkBuilder::new(&root)
            .hidden(false)
            .require_git(false)
            .build()
        {
            let dir_entry = result.map_err(|e| crate::Error::Io(e.to_string()))?;
            let path = dir_entry.path().to_path_buf();

            // Skip the root directory itself
            if path == root {
                continue;
            }

            let metadata = dir_entry
                .metadata()
                .map_err(|e| crate::Error::Io(e.to_string()))?;

            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();

            entries.push(FileEntry {
                name,
                path,
                size: metadata.len(),
                mtime: metadata
                    .modified()
                    .unwrap_or(SystemTime::UNIX_EPOCH),
                is_dir: metadata.is_dir(),
            });
        }

        Ok(Self { root, entries })
    }

    /// Return only file entries (not directories).
    pub fn files(&self) -> Vec<&FileEntry> {
        self.entries.iter().filter(|e| !e.is_dir).collect()
    }

    /// Number of file entries (not directories).
    pub fn file_count(&self) -> usize {
        self.entries.iter().filter(|e| !e.is_dir).count()
    }

    /// Return file entries whose path matches the given glob pattern.
    pub fn glob(&self, pattern: &str) -> Vec<&FileEntry> {
        let pat = match glob::Pattern::new(pattern) {
            Ok(p) => p,
            Err(_) => return Vec::new(),
        };
        self.entries
            .iter()
            .filter(|e| !e.is_dir && pat.matches(&e.name))
            .collect()
    }

    /// The root directory of the tree.
    pub fn root(&self) -> &Path {
        &self.root
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_scan_directory_finds_files() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();

        fs::write(root.join("a.txt"), "hello").unwrap();
        fs::write(root.join("b.rs"), "fn main() {}").unwrap();

        fs::create_dir_all(root.join("sub")).unwrap();
        fs::write(root.join("sub/c.txt"), "nested").unwrap();

        let tree = FileTree::scan(root).unwrap();
        assert_eq!(tree.file_count(), 3);

        let names: Vec<&str> = tree.files().iter().map(|f| f.name.as_str()).collect();
        assert!(names.contains(&"a.txt"));
        assert!(names.contains(&"b.rs"));
        assert!(names.contains(&"c.txt"));
    }

    #[test]
    fn test_scan_respects_gitignore() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();

        fs::write(root.join(".gitignore"), "*.log\ntarget/\n").unwrap();
        fs::write(root.join("main.rs"), "fn main() {}").unwrap();
        fs::write(root.join("debug.log"), "log data").unwrap();
        fs::create_dir_all(root.join("target")).unwrap();
        fs::write(root.join("target/output.bin"), "binary").unwrap();

        let tree = FileTree::scan(root).unwrap();

        let names: Vec<&str> = tree.files().iter().map(|f| f.name.as_str()).collect();
        assert!(names.contains(&"main.rs"));
        assert!(names.contains(&".gitignore"));
        assert!(!names.contains(&"debug.log"));
        assert!(!names.contains(&"output.bin"));
    }

    #[test]
    fn test_glob_match() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();

        fs::write(root.join("lib.rs"), "// lib").unwrap();
        fs::write(root.join("main.rs"), "fn main() {}").unwrap();
        fs::write(root.join("readme.md"), "# readme").unwrap();
        fs::write(root.join("data.json"), "{}").unwrap();

        let tree = FileTree::scan(root).unwrap();

        let rs_files = tree.glob("*.rs");
        assert_eq!(rs_files.len(), 2);
        let names: Vec<&str> = rs_files.iter().map(|f| f.name.as_str()).collect();
        assert!(names.contains(&"lib.rs"));
        assert!(names.contains(&"main.rs"));
        assert!(!names.contains(&"readme.md"));
    }
}
