use crate::cache::content_cache::ContentCache;
use crate::extract::pipeline::{extract_text, extract_text_cached};
use crate::fs::tree::FileTree;
use crate::search::bm25::{Bm25Mode, Bm25Scorer};
use crate::search::matcher::{MatchOptions, Matcher};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct SearchResult {
    pub path: PathBuf,
    pub line_number: usize,
    pub line_content: String,
    pub context_before: Vec<String>,
    pub context_after: Vec<String>,
    pub bm25_score: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct SearchOptions {
    pub case_insensitive: bool,
    pub context_lines: usize,
    pub max_results: usize,
    pub use_regex: bool,
    pub bm25: Bm25Mode,
    /// If set, only search files under this directory prefix.
    pub search_root: Option<PathBuf>,
    /// Use LSA semantic scoring.
    pub semantic: bool,
}

impl Default for SearchOptions {
    fn default() -> Self {
        Self {
            case_insensitive: false,
            context_lines: 0,
            max_results: 1000,
            use_regex: false,
            bm25: Bm25Mode::Off,
            search_root: None,
            semantic: false,
        }
    }
}

pub struct SearchEngine {
    tree: FileTree,
    cache: Option<ContentCache>,
}

impl SearchEngine {
    pub fn new(root: &Path) -> Result<Self, crate::Error> {
        let tree = FileTree::scan(root)?;
        Ok(Self { tree, cache: None })
    }

    /// Create a SearchEngine from an existing FileTree, skipping the scan.
    pub fn from_tree(tree: FileTree) -> Self {
        Self { tree, cache: None }
    }

    /// Attach a ContentCache for caching extracted text from binary formats.
    pub fn with_cache(mut self, cache: ContentCache) -> Self {
        self.cache = Some(cache);
        self
    }

    /// Extract text, using cache if available.
    fn extract(&self, path: &Path) -> Result<String, crate::Error> {
        match &self.cache {
            Some(c) => extract_text_cached(path, c),
            None => extract_text(path),
        }
    }

    pub fn search(
        &self,
        pattern: &str,
        opts: &SearchOptions,
    ) -> Result<Vec<SearchResult>, crate::Error> {
        if opts.semantic {
            return self.search_semantic(pattern, opts);
        }
        self.search_literal(pattern, opts)
    }

    /// Literal/regex search with optional BM25 scoring.
    fn search_literal(
        &self,
        pattern: &str,
        opts: &SearchOptions,
    ) -> Result<Vec<SearchResult>, crate::Error> {
        let matcher = Matcher::with_options(
            &[pattern],
            MatchOptions {
                case_insensitive: opts.case_insensitive,
                use_regex: opts.use_regex,
            },
        )?;

        // Prepare BM25 scorer if needed
        let scorer = if opts.bm25 != Bm25Mode::Off {
            let files = self.tree.files();
            let file_count = files.len();
            let avg_doc_len = if file_count > 0 {
                files.iter().map(|f| f.size as f64).sum::<f64>() / file_count as f64
            } else {
                1.0
            };
            Some(Bm25Scorer::new(file_count, avg_doc_len))
        } else {
            None
        };

        let mut results = Vec::new();
        let mut file_tf_map: Vec<(PathBuf, usize, usize)> = Vec::new();

        let canonical_search_root = opts.search_root.as_ref().and_then(|p| p.canonicalize().ok());

        for entry in self.tree.files() {
            if let Some(ref root) = canonical_search_root {
                if !entry.path.starts_with(root) {
                    continue;
                }
            }
            if results.len() >= opts.max_results {
                break;
            }

            let text = match self.extract(&entry.path) {
                Ok(t) => t,
                Err(_) => continue,
            };

            if text.is_empty() || !matcher.is_match(text.as_bytes()) {
                continue;
            }

            let bm25_score = if let Some(ref scorer) = scorer {
                let tf = matcher.find_all(text.as_bytes()).len();
                let doc_len = text.len();
                if opts.bm25 == Bm25Mode::Full {
                    file_tf_map.push((entry.path.clone(), tf, doc_len));
                }
                Some(scorer.tf_score(tf, doc_len))
            } else {
                None
            };

            let lines: Vec<&str> = text.lines().collect();

            for (idx, line) in lines.iter().enumerate() {
                if results.len() >= opts.max_results {
                    break;
                }

                if matcher.is_match(line.as_bytes()) {
                    let line_number = idx + 1;

                    let context_before: Vec<String> = if opts.context_lines > 0 {
                        let start = idx.saturating_sub(opts.context_lines);
                        lines[start..idx].iter().map(|l| l.to_string()).collect()
                    } else {
                        Vec::new()
                    };

                    let context_after: Vec<String> = if opts.context_lines > 0 {
                        let end = (idx + 1 + opts.context_lines).min(lines.len());
                        lines[(idx + 1)..end].iter().map(|l| l.to_string()).collect()
                    } else {
                        Vec::new()
                    };

                    results.push(SearchResult {
                        path: entry.path.clone(),
                        line_number,
                        line_content: line.to_string(),
                        context_before,
                        context_after,
                        bm25_score,
                    });
                }
            }
        }

        // Full mode: reassign scores with IDF
        if opts.bm25 == Bm25Mode::Full {
            if let Some(ref scorer) = scorer {
                let df = file_tf_map.len();
                for r in &mut results {
                    if let Some((_, tf, doc_len)) = file_tf_map.iter().find(|(p, _, _)| *p == r.path)
                    {
                        r.bm25_score = Some(scorer.score(*tf, *doc_len, df));
                    }
                }
            }
        }

        Ok(results)
    }

    /// Semantic search using LSA (Latent Semantic Analysis).
    ///
    /// LSA captures project-local co-occurrence patterns via truncated SVD of TF-IDF.
    fn search_semantic(
        &self,
        pattern: &str,
        opts: &SearchOptions,
    ) -> Result<Vec<SearchResult>, crate::Error> {
        use crate::search::lsa::LsaScorer;

        let canonical_search_root = opts.search_root.as_ref().and_then(|p| p.canonicalize().ok());

        // Collect all documents for indexing
        let mut docs: Vec<(std::path::PathBuf, String)> = Vec::new();

        for entry in self.tree.files() {
            if let Some(ref root) = canonical_search_root {
                if !entry.path.starts_with(root) {
                    continue;
                }
            }

            let text = match self.extract(&entry.path) {
                Ok(t) => t,
                Err(_) => continue,
            };

            if text.is_empty() {
                continue;
            }

            docs.push((entry.path.clone(), text));
        }

        if docs.is_empty() {
            return Ok(Vec::new());
        }

        // Build LSA index
        let k = 128.min(docs.len());
        let lsa_scorer = LsaScorer::build(&docs, k);
        let lsa_query_vec = lsa_scorer.project_query(pattern);

        // Rank documents by LSA cosine similarity
        let rankings = lsa_scorer.rank_documents(&lsa_query_vec);

        // Take top 20 files
        let max_files = 20;
        let top_rankings: Vec<_> = rankings.into_iter().take(max_files).collect();

        // Per-line scoring within top files
        let mut results: Vec<SearchResult> = Vec::new();

        for &(file_score, doc_idx) in &top_rankings {
            if file_score < 1e-8 {
                continue;
            }

            let path = &docs[doc_idx].0;
            let text = &docs[doc_idx].1;
            let lines: Vec<&str> = text.lines().collect();

            let mut line_scores: Vec<(usize, f32, &str)> = Vec::new();
            for (idx, line) in lines.iter().enumerate() {
                let trimmed = line.trim();
                if trimmed.len() < 10 {
                    continue;
                }

                let line_vec = lsa_scorer.project_query(trimmed);
                let score = if line_vec.is_empty() || lsa_query_vec.is_empty() {
                    0.0
                } else {
                    super::lsa::cosine_similarity_pub(&lsa_query_vec, &line_vec)
                };

                line_scores.push((idx, score, line));
            }

            // Sort lines by score desc, take top 5
            line_scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

            let mut top_lines: Vec<_> = line_scores.into_iter().take(5).collect();
            top_lines.sort_by_key(|(idx, _, _)| *idx);

            for (idx, _line_score, line) in &top_lines {
                let context_before: Vec<String> = if opts.context_lines > 0 {
                    let start = idx.saturating_sub(opts.context_lines);
                    lines[start..*idx].iter().map(|l| l.to_string()).collect()
                } else {
                    Vec::new()
                };
                let context_after: Vec<String> = if opts.context_lines > 0 {
                    let end = (idx + 1 + opts.context_lines).min(lines.len());
                    lines[(idx + 1)..end].iter().map(|l| l.to_string()).collect()
                } else {
                    Vec::new()
                };

                results.push(SearchResult {
                    path: path.clone(),
                    line_number: idx + 1,
                    line_content: line.to_string(),
                    context_before,
                    context_after,
                    bm25_score: Some(file_score as f64),
                });
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

        let paths: std::collections::HashSet<PathBuf> =
            results.iter().map(|r| r.path.clone()).collect();
        assert_eq!(paths.len(), 2);
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
        assert_eq!(results[0].line_number, 3);
    }

    #[test]
    fn test_search_inside_gzip_file() {
        use flate2::write::GzEncoder;
        use flate2::Compression;
        use std::io::Write;

        let tmp = TempDir::new().unwrap();
        let root = tmp.path();

        fs::write(root.join("plain.rs"), "fn visible() {}\n").unwrap();

        let code = b"fn hidden_in_gz() { let token = verify(); }\n";
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(code).unwrap();
        let compressed = encoder.finish().unwrap();
        fs::write(root.join("code.rs.gz"), &compressed).unwrap();

        let engine = SearchEngine::new(root).unwrap();
        let opts = SearchOptions::default();

        let results = engine.search("hidden_in_gz", &opts).unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].path.to_string_lossy().contains("code.rs.gz"));
    }

    #[test]
    fn test_search_without_bm25_has_no_scores() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("a.rs"), "hello world\n").unwrap();

        let engine = SearchEngine::new(tmp.path()).unwrap();
        let results = engine.search("hello", &SearchOptions::default()).unwrap();
        assert!(results.iter().all(|r| r.bm25_score.is_none()));
    }

    #[test]
    fn test_search_with_bm25_scores() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();

        fs::write(root.join("auth.rs"), "token token token\n").unwrap();
        fs::write(
            root.join("handler.rs"),
            "one token here and some padding text to make it longer\n",
        )
        .unwrap();

        let engine = SearchEngine::new(root).unwrap();
        let opts = SearchOptions {
            bm25: Bm25Mode::Tf,
            ..SearchOptions::default()
        };
        let results = engine.search("token", &opts).unwrap();

        assert!(results.iter().all(|r| r.bm25_score.is_some()));

        let auth_score = results
            .iter()
            .find(|r| r.path.to_string_lossy().contains("auth.rs"))
            .unwrap()
            .bm25_score
            .unwrap();
        let handler_score = results
            .iter()
            .find(|r| r.path.to_string_lossy().contains("handler.rs"))
            .unwrap()
            .bm25_score
            .unwrap();
        assert!(auth_score > handler_score);
    }

    #[test]
    fn test_search_bm25_full_has_idf() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();

        for i in 0..10 {
            let content = if i == 0 {
                "rare_token common_word common_word\n"
            } else {
                "common_word common_word common_word\n"
            };
            fs::write(root.join(format!("f{}.rs", i)), content).unwrap();
        }

        let engine = SearchEngine::new(root).unwrap();

        let tf_results = engine
            .search(
                "rare_token",
                &SearchOptions {
                    bm25: Bm25Mode::Tf,
                    ..SearchOptions::default()
                },
            )
            .unwrap();

        let full_results = engine
            .search(
                "rare_token",
                &SearchOptions {
                    bm25: Bm25Mode::Full,
                    ..SearchOptions::default()
                },
            )
            .unwrap();

        assert_eq!(tf_results.len(), full_results.len());

        let tf_score = tf_results[0].bm25_score.unwrap();
        let full_score = full_results[0].bm25_score.unwrap();
        assert!(full_score > tf_score);
    }

    #[test]
    fn test_from_tree_matches_new() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();

        fs::write(root.join("a.rs"), "fn hello() {}\n").unwrap();
        fs::write(root.join("b.rs"), "fn world() {}\n").unwrap();

        let engine_new = SearchEngine::new(root).unwrap();
        let tree = crate::fs::tree::FileTree::scan(root).unwrap();
        let engine_from = SearchEngine::from_tree(tree);

        let opts = SearchOptions::default();
        let r1 = engine_new.search("hello", &opts).unwrap();
        let r2 = engine_from.search("hello", &opts).unwrap();

        assert_eq!(r1.len(), r2.len());
        assert_eq!(r1[0].line_content, r2[0].line_content);
    }

    #[test]
    fn test_search_root_filters_subdirectory() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();

        fs::create_dir_all(root.join("src")).unwrap();
        fs::create_dir_all(root.join("tests")).unwrap();
        fs::write(root.join("src/main.rs"), "fn target() {}\n").unwrap();
        fs::write(root.join("tests/test.rs"), "fn target() {}\n").unwrap();

        let engine = SearchEngine::new(root).unwrap();

        let all = engine
            .search("target", &SearchOptions::default())
            .unwrap();
        assert_eq!(all.len(), 2);

        let src_only = engine
            .search(
                "target",
                &SearchOptions {
                    search_root: Some(root.join("src")),
                    ..SearchOptions::default()
                },
            )
            .unwrap();
        assert_eq!(src_only.len(), 1);
        assert!(src_only[0].path.to_string_lossy().contains("src"));
    }

    #[test]
    fn test_semantic_search_returns_scored_results() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();

        // Use distinct vocabularies so LSA IDF gives non-zero signal
        fs::write(
            root.join("auth.rs"),
            "fn authenticate_user(credentials: &str) -> bool {\n    validate_token(credentials)\n}\n",
        )
        .unwrap();
        fs::write(
            root.join("db.rs"),
            "fn migrate_database_schema() {\n    run_migration_step();\n}\n",
        )
        .unwrap();
        fs::write(
            root.join("handler.rs"),
            "fn handle_request() {\n    parse_json_body();\n}\n",
        )
        .unwrap();

        let engine = SearchEngine::new(root).unwrap();
        let results = engine
            .search(
                "authenticate credentials token",
                &SearchOptions {
                    semantic: true,
                    ..SearchOptions::default()
                },
            )
            .unwrap();

        assert!(!results.is_empty());
        // All results should have scores
        assert!(results.iter().all(|r| r.bm25_score.is_some()));
    }
}
