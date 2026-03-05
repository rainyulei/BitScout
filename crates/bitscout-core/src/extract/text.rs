use memmap2::Mmap;
use std::fs::File;
use std::path::Path;

pub struct MmapContent {
    mmap: Mmap,
}

impl MmapContent {
    pub fn open(path: &Path) -> Result<Self, crate::Error> {
        let file = File::open(path).map_err(|e| crate::Error::Io(crate::clean_io_error(&e)))?;
        // SAFETY: We assume no other process modifies the file while mapped.
        let mmap = unsafe { Mmap::map(&file) }.map_err(|e| crate::Error::Io(crate::clean_io_error(&e)))?;
        Ok(Self { mmap })
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.mmap
    }

    pub fn len(&self) -> usize {
        self.mmap.len()
    }

    pub fn is_empty(&self) -> bool {
        self.mmap.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_mmap_read_text_file() {
        let mut tmp = NamedTempFile::new().unwrap();
        let content = b"Hello, mmap world!";
        tmp.write_all(content).unwrap();
        tmp.flush().unwrap();

        let mmap_content = MmapContent::open(tmp.path()).unwrap();
        assert_eq!(mmap_content.as_bytes(), content);
        assert_eq!(mmap_content.len(), content.len());
        assert!(!mmap_content.is_empty());
    }

    #[test]
    fn test_mmap_search_integration() {
        let code = b"fn main() {\n    let auth = get_auth();\n    let session = start_session(auth);\n}\n";
        let mut tmp = NamedTempFile::new().unwrap();
        tmp.write_all(code).unwrap();
        tmp.flush().unwrap();

        let mmap_content = MmapContent::open(tmp.path()).unwrap();

        let matcher = crate::search::matcher::Matcher::new(&["auth", "session"]).unwrap();
        let matches = matcher.find_all(mmap_content.as_bytes());

        assert!(matches.len() >= 2, "expected at least 2 matches, got {}", matches.len());
        // Verify we found both patterns
        let pattern_indices: std::collections::HashSet<usize> =
            matches.iter().map(|m| m.pattern_index).collect();
        assert!(pattern_indices.contains(&0), "should find 'auth'");
        assert!(pattern_indices.contains(&1), "should find 'session'");
    }
}
