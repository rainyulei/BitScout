use crate::extract::text::MmapContent;
use crate::fs::tree::FileTree;
use crate::search::matcher::{MatchOptions, Matcher};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct SearchResult {
    pub path: PathBuf,
    pub line_number: usize,
    pub line_content: String,
    pub context_before: Vec<String>,
    pub context_after: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct SearchOptions {
    pub case_insensitive: bool,
    pub context_lines: usize,
    pub max_results: usize,
}

impl Default for SearchOptions {
    fn default() -> Self {
        Self {
            case_insensitive: false,
            context_lines: 0,
            max_results: 1000,
        }
    }
}

pub struct SearchEngine {
    tree: FileTree,
}

impl SearchEngine {
    pub fn new(root: &Path) -> Result<Self, crate::Error> {
        let tree = FileTree::scan(root)?;
        Ok(Self { tree })
    }

    pub fn search(
        &self,
        pattern: &str,
        opts: &SearchOptions,
    ) -> Result<Vec<SearchResult>, crate::Error> {
        let matcher = Matcher::with_options(
            &[pattern],
            MatchOptions {
                case_insensitive: opts.case_insensitive,
            },
        )?;

        let mut results = Vec::new();

        for entry in self.tree.files() {
            if results.len() >= opts.max_results {
                break;
            }

            let content = match MmapContent::open(&entry.path) {
                Ok(c) => c,
                Err(_) => continue,
            };

            let bytes = content.as_bytes();
            if bytes.is_empty() || !matcher.is_match(bytes) {
                continue;
            }

            // Split into lines and search each
            let text = match std::str::from_utf8(bytes) {
                Ok(t) => t,
                Err(_) => continue, // skip binary files
            };

            let lines: Vec<&str> = text.lines().collect();

            for (idx, line) in lines.iter().enumerate() {
                if results.len() >= opts.max_results {
                    break;
                }

                if matcher.is_match(line.as_bytes()) {
                    let line_number = idx + 1; // 1-based

                    let context_before: Vec<String> = if opts.context_lines > 0 {
                        let start = idx.saturating_sub(opts.context_lines);
                        lines[start..idx]
                            .iter()
                            .map(|l| l.to_string())
                            .collect()
                    } else {
                        Vec::new()
                    };

                    let context_after: Vec<String> = if opts.context_lines > 0 {
                        let end = (idx + 1 + opts.context_lines).min(lines.len());
                        lines[(idx + 1)..end]
                            .iter()
                            .map(|l| l.to_string())
                            .collect()
                    } else {
                        Vec::new()
                    };

                    results.push(SearchResult {
                        path: entry.path.clone(),
                        line_number,
                        line_content: line.to_string(),
                        context_before,
                        context_after,
                    });
                }
            }
        }

        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_search_across_files() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();

        fs::write(
            root.join("auth.rs"),
            "fn verify_token(tok: &str) -> bool {\n    tok.len() > 0\n}\n",
        )
        .unwrap();
        fs::write(
            root.join("handler.rs"),
            "fn handle_request() {\n    let ok = verify_token(\"abc\");\n}\n",
        )
        .unwrap();
        fs::write(
            root.join("utils.rs"),
            "fn helper() {\n    println!(\"no match here\");\n}\n",
        )
        .unwrap();

        let engine = SearchEngine::new(root).unwrap();
        let opts = SearchOptions::default();
        let results = engine.search("verify_token", &opts).unwrap();

        // Should find in auth.rs and handler.rs, not in utils.rs
        let paths: std::collections::HashSet<PathBuf> =
            results.iter().map(|r| r.path.clone()).collect();
        assert_eq!(paths.len(), 2, "expected matches in 2 files, got {:?}", paths);

        let file_names: Vec<String> = paths
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert!(file_names.contains(&"auth.rs".to_string()));
        assert!(file_names.contains(&"handler.rs".to_string()));
    }

    #[test]
    fn test_search_with_context_lines() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();

        fs::write(
            root.join("code.rs"),
            "line_one\nline_two\nverify_token\nline_four\nline_five\n",
        )
        .unwrap();

        let engine = SearchEngine::new(root).unwrap();
        let opts = SearchOptions {
            context_lines: 1,
            ..SearchOptions::default()
        };
        let results = engine.search("verify_token", &opts).unwrap();

        assert_eq!(results.len(), 1);
        let r = &results[0];
        assert_eq!(r.line_content, "verify_token");
        assert_eq!(r.context_before, vec!["line_two"]);
        assert_eq!(r.context_after, vec!["line_four"]);
    }

    #[test]
    fn test_search_returns_line_numbers() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();

        fs::write(
            root.join("sample.rs"),
            "alpha\nbeta\ngamma\ndelta\nepsilon\n",
        )
        .unwrap();

        let engine = SearchEngine::new(root).unwrap();
        let opts = SearchOptions::default();

        let results = engine.search("gamma", &opts).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].line_number, 3); // 1-based: line 3

        let results = engine.search("alpha", &opts).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].line_number, 1);

        let results = engine.search("epsilon", &opts).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].line_number, 5);
    }
}
