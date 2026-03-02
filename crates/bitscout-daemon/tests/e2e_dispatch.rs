//! End-to-end integration tests for the dispatch pipeline.
//!
//! These tests exercise the full: SearchRequest -> dispatch() -> SearchResponse
//! pipeline for all 5 commands: rg, grep, find, fd, cat.
//!
//! Each test creates a temporary directory with known files and verifies that
//! dispatch produces the correct exit code, stdout, and stderr.

use bitscout_core::protocol::SearchRequest;
use bitscout_daemon::dispatch::{dispatch, FALLBACK_EXIT_CODE};
use std::fs;
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Test fixture helpers
// ---------------------------------------------------------------------------

/// Create a standard test directory tree:
///
/// tmpdir/
///   src/
///     main.rs       -> "fn main() {\n    println!(\"hello\");\n}\n"
///     lib.rs        -> "pub mod utils;\npub fn add(a: i32, b: i32) -> i32 { a + b }\n"
///     utils.rs      -> "pub fn helper() -> bool { true }\n"
///   tests/
///     test_add.rs   -> "use mylib::add;\n#[test]\nfn test_add() { assert_eq!(add(1,2), 3); }\n"
///   docs/
///     guide.md      -> "# User Guide\n\nWelcome to the project.\n"
///   config.json     -> "{\"debug\": true, \"port\": 8080}\n"
///   README.md       -> "# My Project\n\nA sample project for testing.\n"
///   Cargo.toml      -> "[package]\nname = \"myproject\"\nversion = \"0.1.0\"\n"
fn create_test_tree(tmp: &TempDir) {
    fs::create_dir_all(tmp.path().join("src")).unwrap();
    fs::create_dir_all(tmp.path().join("tests")).unwrap();
    fs::create_dir_all(tmp.path().join("docs")).unwrap();

    fs::write(
        tmp.path().join("src/main.rs"),
        "fn main() {\n    println!(\"hello\");\n}\n",
    )
    .unwrap();
    fs::write(
        tmp.path().join("src/lib.rs"),
        "pub mod utils;\npub fn add(a: i32, b: i32) -> i32 { a + b }\n",
    )
    .unwrap();
    fs::write(
        tmp.path().join("src/utils.rs"),
        "pub fn helper() -> bool { true }\n",
    )
    .unwrap();
    fs::write(
        tmp.path().join("tests/test_add.rs"),
        "use mylib::add;\n#[test]\nfn test_add() { assert_eq!(add(1,2), 3); }\n",
    )
    .unwrap();
    fs::write(
        tmp.path().join("docs/guide.md"),
        "# User Guide\n\nWelcome to the project.\n",
    )
    .unwrap();
    fs::write(
        tmp.path().join("config.json"),
        "{\"debug\": true, \"port\": 8080}\n",
    )
    .unwrap();
    fs::write(
        tmp.path().join("README.md"),
        "# My Project\n\nA sample project for testing.\n",
    )
    .unwrap();
    fs::write(
        tmp.path().join("Cargo.toml"),
        "[package]\nname = \"myproject\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
}

fn cwd(tmp: &TempDir) -> String {
    tmp.path().to_str().unwrap().to_string()
}

fn args(strs: &[&str]) -> Vec<String> {
    strs.iter().map(|s| s.to_string()).collect()
}

// ===========================================================================
// 1. rg command tests
// ===========================================================================

mod rg {
    use super::*;

    #[test]
    fn basic_search_returns_matching_lines() {
        let tmp = TempDir::new().unwrap();
        create_test_tree(&tmp);

        let req = SearchRequest {
            command: "rg".into(),
            args: args(&["hello", "."]),
            cwd: cwd(&tmp),
        };
        let resp = dispatch(&req);
        assert_eq!(resp.exit_code, 0);
        assert!(resp.stdout.contains("hello"), "stdout: {}", resp.stdout);
        // Should include the file path
        assert!(resp.stdout.contains("main.rs"), "stdout: {}", resp.stdout);
    }

    #[test]
    fn with_line_numbers() {
        let tmp = TempDir::new().unwrap();
        create_test_tree(&tmp);

        let req = SearchRequest {
            command: "rg".into(),
            args: args(&["-n", "hello", "."]),
            cwd: cwd(&tmp),
        };
        let resp = dispatch(&req);
        assert_eq!(resp.exit_code, 0);
        // Format: path:line_number:content
        assert!(
            resp.stdout.contains(":2:"),
            "expected line number 2 in output: {}",
            resp.stdout
        );
    }

    #[test]
    fn count_format() {
        let tmp = TempDir::new().unwrap();
        create_test_tree(&tmp);

        let req = SearchRequest {
            command: "rg".into(),
            args: args(&["-c", "fn", "."]),
            cwd: cwd(&tmp),
        };
        let resp = dispatch(&req);
        assert_eq!(resp.exit_code, 0);
        // Format: path:count
        // main.rs has 1 "fn", lib.rs has 1 "fn", utils.rs has 1 "fn"
        for line in resp.stdout.trim().lines() {
            assert!(
                line.contains(':'),
                "count line missing colon: {}",
                line
            );
            let count_str = line.rsplit(':').next().unwrap();
            let count: usize = count_str.parse().expect("count should be numeric");
            assert!(count > 0, "count should be positive: {}", line);
        }
    }

    #[test]
    fn files_only_format() {
        let tmp = TempDir::new().unwrap();
        create_test_tree(&tmp);

        let req = SearchRequest {
            command: "rg".into(),
            args: args(&["-l", "fn", "."]),
            cwd: cwd(&tmp),
        };
        let resp = dispatch(&req);
        assert_eq!(resp.exit_code, 0);
        // Each line should be a file path, no colons (no line content)
        let lines: Vec<&str> = resp.stdout.trim().lines().collect();
        assert!(lines.len() >= 3, "expected at least 3 files with 'fn': {:?}", lines);
        for line in &lines {
            // files_only lines should not have the content-colon format
            // They should just be paths
            assert!(
                line.contains(".rs"),
                "expected .rs file path: {}",
                line
            );
        }
    }

    #[test]
    fn json_output() {
        let tmp = TempDir::new().unwrap();
        create_test_tree(&tmp);

        let req = SearchRequest {
            command: "rg".into(),
            args: args(&["--json", "hello", "."]),
            cwd: cwd(&tmp),
        };
        let resp = dispatch(&req);
        assert_eq!(resp.exit_code, 0);
        // Each line should be valid JSON with "type": "match"
        for line in resp.stdout.trim().lines() {
            let v: serde_json::Value =
                serde_json::from_str(line).expect("each line should be valid JSON");
            assert_eq!(v["type"], "match", "JSON line: {}", line);
            assert!(v["data"]["path"]["text"].is_string());
            assert!(v["data"]["line_number"].is_number());
        }
    }

    #[test]
    fn unsupported_flags_return_fallback() {
        let req = SearchRequest {
            command: "rg".into(),
            args: args(&["--pcre2", "pattern", "."]),
            cwd: "/tmp".into(),
        };
        let resp = dispatch(&req);
        assert_eq!(resp.exit_code, FALLBACK_EXIT_CODE);
        assert!(resp.stderr.contains("BITSCOUT_FALLBACK"));
    }

    #[test]
    fn no_match_returns_exit_1() {
        let tmp = TempDir::new().unwrap();
        create_test_tree(&tmp);

        let req = SearchRequest {
            command: "rg".into(),
            args: args(&["zzz_nonexistent_pattern_xyz", "."]),
            cwd: cwd(&tmp),
        };
        let resp = dispatch(&req);
        assert_eq!(resp.exit_code, 1);
        assert!(resp.stdout.is_empty());
    }

    #[test]
    fn case_insensitive_search() {
        let tmp = TempDir::new().unwrap();
        create_test_tree(&tmp);

        let req = SearchRequest {
            command: "rg".into(),
            args: args(&["-i", "HELLO", "."]),
            cwd: cwd(&tmp),
        };
        let resp = dispatch(&req);
        assert_eq!(resp.exit_code, 0);
        assert!(resp.stdout.contains("hello"), "stdout: {}", resp.stdout);
    }
}

// ===========================================================================
// 2. grep command tests
// ===========================================================================

mod grep {
    use super::*;

    #[test]
    fn basic_recursive_search() {
        let tmp = TempDir::new().unwrap();
        create_test_tree(&tmp);

        let req = SearchRequest {
            command: "grep".into(),
            args: args(&["-r", "hello", "."]),
            cwd: cwd(&tmp),
        };
        let resp = dispatch(&req);
        assert_eq!(resp.exit_code, 0);
        assert!(resp.stdout.contains("hello"), "stdout: {}", resp.stdout);
        // Default shows filename
        assert!(resp.stdout.contains("main.rs"), "stdout: {}", resp.stdout);
    }

    #[test]
    fn with_line_numbers() {
        let tmp = TempDir::new().unwrap();
        create_test_tree(&tmp);

        let req = SearchRequest {
            command: "grep".into(),
            args: args(&["-rn", "hello", "."]),
            cwd: cwd(&tmp),
        };
        let resp = dispatch(&req);
        assert_eq!(resp.exit_code, 0);
        // Format: path:line_number:content
        assert!(
            resp.stdout.contains(":2:"),
            "expected line number in output: {}",
            resp.stdout
        );
    }

    #[test]
    fn count_format() {
        let tmp = TempDir::new().unwrap();
        create_test_tree(&tmp);

        let req = SearchRequest {
            command: "grep".into(),
            args: args(&["-rc", "fn", "."]),
            cwd: cwd(&tmp),
        };
        let resp = dispatch(&req);
        assert_eq!(resp.exit_code, 0);
        // Format: path:count
        for line in resp.stdout.trim().lines() {
            assert!(line.contains(':'), "count line should have colon: {}", line);
        }
    }

    #[test]
    fn files_only() {
        let tmp = TempDir::new().unwrap();
        create_test_tree(&tmp);

        let req = SearchRequest {
            command: "grep".into(),
            args: args(&["-rl", "fn", "."]),
            cwd: cwd(&tmp),
        };
        let resp = dispatch(&req);
        assert_eq!(resp.exit_code, 0);
        let lines: Vec<&str> = resp.stdout.trim().lines().collect();
        assert!(lines.len() >= 3, "expected at least 3 files: {:?}", lines);
    }

    #[test]
    fn case_insensitive() {
        let tmp = TempDir::new().unwrap();
        create_test_tree(&tmp);

        let req = SearchRequest {
            command: "grep".into(),
            args: args(&["-ri", "HELLO", "."]),
            cwd: cwd(&tmp),
        };
        let resp = dispatch(&req);
        assert_eq!(resp.exit_code, 0);
        assert!(resp.stdout.contains("hello"), "stdout: {}", resp.stdout);
    }

    #[test]
    fn no_match_returns_exit_1() {
        let tmp = TempDir::new().unwrap();
        create_test_tree(&tmp);

        let req = SearchRequest {
            command: "grep".into(),
            args: args(&["-r", "zzz_nonexistent_xyz", "."]),
            cwd: cwd(&tmp),
        };
        let resp = dispatch(&req);
        assert_eq!(resp.exit_code, 1);
        assert!(resp.stdout.is_empty());
    }

    #[test]
    fn unsupported_flag_returns_fallback() {
        let req = SearchRequest {
            command: "grep".into(),
            args: args(&["-P", "pattern", "."]),
            cwd: "/tmp".into(),
        };
        let resp = dispatch(&req);
        assert_eq!(resp.exit_code, FALLBACK_EXIT_CODE);
        assert!(resp.stderr.contains("BITSCOUT_FALLBACK"));
    }

    #[test]
    fn include_glob_filters_by_extension() {
        let tmp = TempDir::new().unwrap();
        create_test_tree(&tmp);

        let req = SearchRequest {
            command: "grep".into(),
            args: args(&["-r", "--include=*.rs", "fn", "."]),
            cwd: cwd(&tmp),
        };
        let resp = dispatch(&req);
        assert_eq!(resp.exit_code, 0);
        assert!(resp.stdout.contains(".rs"), "stdout: {}", resp.stdout);
        assert!(!resp.stdout.contains(".md"), "should not match .md files: {}", resp.stdout);
        assert!(!resp.stdout.contains(".json"), "should not match .json files: {}", resp.stdout);
    }

    #[test]
    fn suppress_filename_with_h() {
        let tmp = TempDir::new().unwrap();
        create_test_tree(&tmp);

        let req = SearchRequest {
            command: "grep".into(),
            args: args(&["-rh", "hello", "."]),
            cwd: cwd(&tmp),
        };
        let resp = dispatch(&req);
        assert_eq!(resp.exit_code, 0);
        // Output should not contain file paths
        assert!(!resp.stdout.contains("main.rs"), "stdout: {}", resp.stdout);
        // Should just be the matching content
        assert!(resp.stdout.contains("hello"), "stdout: {}", resp.stdout);
    }

    #[test]
    fn combined_short_flags() {
        let tmp = TempDir::new().unwrap();
        create_test_tree(&tmp);

        let req = SearchRequest {
            command: "grep".into(),
            args: args(&["-rin", "HELLO", "."]),
            cwd: cwd(&tmp),
        };
        let resp = dispatch(&req);
        assert_eq!(resp.exit_code, 0);
        // case insensitive + line numbers
        assert!(resp.stdout.contains("hello"), "stdout: {}", resp.stdout);
        // Should contain line number
        assert!(
            resp.stdout.contains(":2:"),
            "expected line number: {}",
            resp.stdout
        );
    }
}

// ===========================================================================
// 3. find command tests
// ===========================================================================

mod find {
    use super::*;

    #[test]
    fn basic_lists_all_entries() {
        let tmp = TempDir::new().unwrap();
        create_test_tree(&tmp);

        let req = SearchRequest {
            command: "find".into(),
            args: args(&["."]),
            cwd: cwd(&tmp),
        };
        let resp = dispatch(&req);
        assert_eq!(resp.exit_code, 0);
        assert!(resp.stdout.contains("main.rs"), "stdout: {}", resp.stdout);
        assert!(resp.stdout.contains("README.md"), "stdout: {}", resp.stdout);
        assert!(resp.stdout.contains("src"), "stdout: {}", resp.stdout);
        assert!(resp.stdout.contains("tests"), "stdout: {}", resp.stdout);
        assert!(resp.stdout.contains("docs"), "stdout: {}", resp.stdout);
    }

    #[test]
    fn name_glob_filters_by_pattern() {
        let tmp = TempDir::new().unwrap();
        create_test_tree(&tmp);

        let req = SearchRequest {
            command: "find".into(),
            args: args(&[".", "-name", "*.rs"]),
            cwd: cwd(&tmp),
        };
        let resp = dispatch(&req);
        assert_eq!(resp.exit_code, 0);
        assert!(resp.stdout.contains("main.rs"), "stdout: {}", resp.stdout);
        assert!(resp.stdout.contains("lib.rs"), "stdout: {}", resp.stdout);
        assert!(resp.stdout.contains("utils.rs"), "stdout: {}", resp.stdout);
        assert!(resp.stdout.contains("test_add.rs"), "stdout: {}", resp.stdout);
        // Should not contain non-.rs files
        assert!(!resp.stdout.contains("README.md"), "stdout: {}", resp.stdout);
        assert!(!resp.stdout.contains("config.json"), "stdout: {}", resp.stdout);
    }

    #[test]
    fn type_f_returns_only_files() {
        let tmp = TempDir::new().unwrap();
        create_test_tree(&tmp);

        let req = SearchRequest {
            command: "find".into(),
            args: args(&[".", "-type", "f"]),
            cwd: cwd(&tmp),
        };
        let resp = dispatch(&req);
        assert_eq!(resp.exit_code, 0);
        assert!(resp.stdout.contains("main.rs"), "stdout: {}", resp.stdout);
        // Directories should not appear as standalone entries
        let lines: Vec<&str> = resp.stdout.trim().lines().collect();
        for line in &lines {
            // A pure directory entry would not have a file extension
            // (directories like "src", "tests", "docs" would not have dots)
            assert!(
                !line.ends_with("/src")
                    && !line.ends_with("/tests")
                    && !line.ends_with("/docs")
                    && *line != "./src"
                    && *line != "./tests"
                    && *line != "./docs",
                "found directory in -type f output: {}",
                line
            );
        }
    }

    #[test]
    fn type_d_returns_only_dirs() {
        let tmp = TempDir::new().unwrap();
        create_test_tree(&tmp);

        let req = SearchRequest {
            command: "find".into(),
            args: args(&[".", "-type", "d"]),
            cwd: cwd(&tmp),
        };
        let resp = dispatch(&req);
        assert_eq!(resp.exit_code, 0);
        assert!(resp.stdout.contains("src"), "stdout: {}", resp.stdout);
        assert!(resp.stdout.contains("tests"), "stdout: {}", resp.stdout);
        assert!(resp.stdout.contains("docs"), "stdout: {}", resp.stdout);
        // Should not contain files
        assert!(!resp.stdout.contains("main.rs"), "stdout: {}", resp.stdout);
        assert!(!resp.stdout.contains("README.md"), "stdout: {}", resp.stdout);
    }

    #[test]
    fn combined_name_and_type() {
        let tmp = TempDir::new().unwrap();
        create_test_tree(&tmp);

        let req = SearchRequest {
            command: "find".into(),
            args: args(&[".", "-name", "*.md", "-type", "f"]),
            cwd: cwd(&tmp),
        };
        let resp = dispatch(&req);
        assert_eq!(resp.exit_code, 0);
        assert!(resp.stdout.contains("README.md"), "stdout: {}", resp.stdout);
        assert!(resp.stdout.contains("guide.md"), "stdout: {}", resp.stdout);
        assert!(!resp.stdout.contains("main.rs"), "stdout: {}", resp.stdout);
    }

    #[test]
    fn iname_case_insensitive_glob() {
        let tmp = TempDir::new().unwrap();
        create_test_tree(&tmp);

        let req = SearchRequest {
            command: "find".into(),
            args: args(&[".", "-iname", "readme*"]),
            cwd: cwd(&tmp),
        };
        let resp = dispatch(&req);
        assert_eq!(resp.exit_code, 0);
        assert!(resp.stdout.contains("README.md"), "stdout: {}", resp.stdout);
    }

    #[test]
    fn nonexistent_dir_returns_error() {
        let tmp = TempDir::new().unwrap();

        let req = SearchRequest {
            command: "find".into(),
            args: args(&["nonexistent_dir_xyz"]),
            cwd: cwd(&tmp),
        };
        let resp = dispatch(&req);
        assert_eq!(resp.exit_code, 1);
        assert!(!resp.stderr.is_empty(), "stderr should contain error");
    }

    #[test]
    fn unsupported_flag_returns_fallback() {
        let req = SearchRequest {
            command: "find".into(),
            args: args(&[".", "-maxdepth", "2"]),
            cwd: "/tmp".into(),
        };
        let resp = dispatch(&req);
        assert_eq!(resp.exit_code, FALLBACK_EXIT_CODE);
        assert!(resp.stderr.contains("BITSCOUT_FALLBACK"));
    }

    #[test]
    fn output_paths_are_relative_to_search_dir() {
        let tmp = TempDir::new().unwrap();
        create_test_tree(&tmp);

        let req = SearchRequest {
            command: "find".into(),
            args: args(&[".", "-name", "main.rs"]),
            cwd: cwd(&tmp),
        };
        let resp = dispatch(&req);
        assert_eq!(resp.exit_code, 0);
        let line = resp.stdout.trim();
        // Should be relative, like ./src/main.rs
        assert!(
            line.starts_with("./") || line.starts_with("src/") || line == "src/main.rs",
            "expected relative path, got: {}",
            line
        );
    }
}

// ===========================================================================
// 4. fd command tests
// ===========================================================================

mod fd {
    use super::*;

    #[test]
    fn basic_pattern_finds_matching_files() {
        let tmp = TempDir::new().unwrap();
        create_test_tree(&tmp);

        let req = SearchRequest {
            command: "fd".into(),
            args: args(&["main"]),
            cwd: cwd(&tmp),
        };
        let resp = dispatch(&req);
        assert_eq!(resp.exit_code, 0);
        assert!(resp.stdout.contains("main.rs"), "stdout: {}", resp.stdout);
        // Should not contain files that don't match
        assert!(!resp.stdout.contains("lib.rs"), "stdout: {}", resp.stdout);
    }

    #[test]
    fn extension_filter() {
        let tmp = TempDir::new().unwrap();
        create_test_tree(&tmp);

        let req = SearchRequest {
            command: "fd".into(),
            args: args(&["-e", "rs"]),
            cwd: cwd(&tmp),
        };
        let resp = dispatch(&req);
        assert_eq!(resp.exit_code, 0);
        assert!(resp.stdout.contains("main.rs"), "stdout: {}", resp.stdout);
        assert!(resp.stdout.contains("lib.rs"), "stdout: {}", resp.stdout);
        assert!(!resp.stdout.contains("README.md"), "stdout: {}", resp.stdout);
        assert!(!resp.stdout.contains("config.json"), "stdout: {}", resp.stdout);
    }

    #[test]
    fn type_f_files_only() {
        let tmp = TempDir::new().unwrap();
        create_test_tree(&tmp);

        let req = SearchRequest {
            command: "fd".into(),
            args: args(&["-t", "f"]),
            cwd: cwd(&tmp),
        };
        let resp = dispatch(&req);
        assert_eq!(resp.exit_code, 0);
        assert!(resp.stdout.contains("main.rs"), "stdout: {}", resp.stdout);
        // Directories should not appear
        let lines: Vec<&str> = resp.stdout.trim().lines().collect();
        for line in &lines {
            assert!(
                line.contains('.'),
                "unexpected directory in -t f output: {}",
                line
            );
        }
    }

    #[test]
    fn type_d_dirs_only() {
        let tmp = TempDir::new().unwrap();
        create_test_tree(&tmp);

        let req = SearchRequest {
            command: "fd".into(),
            args: args(&["-t", "d"]),
            cwd: cwd(&tmp),
        };
        let resp = dispatch(&req);
        assert_eq!(resp.exit_code, 0);
        assert!(resp.stdout.contains("src"), "stdout: {}", resp.stdout);
        assert!(resp.stdout.contains("tests"), "stdout: {}", resp.stdout);
        assert!(!resp.stdout.contains("main.rs"), "stdout: {}", resp.stdout);
    }

    #[test]
    fn combined_extension_and_pattern() {
        let tmp = TempDir::new().unwrap();
        create_test_tree(&tmp);

        let req = SearchRequest {
            command: "fd".into(),
            args: args(&["-e", "rs", "test"]),
            cwd: cwd(&tmp),
        };
        let resp = dispatch(&req);
        assert_eq!(resp.exit_code, 0);
        assert!(resp.stdout.contains("test_add.rs"), "stdout: {}", resp.stdout);
        assert!(!resp.stdout.contains("main.rs"), "stdout: {}", resp.stdout);
    }

    #[test]
    fn ignore_case() {
        let tmp = TempDir::new().unwrap();
        create_test_tree(&tmp);

        let req = SearchRequest {
            command: "fd".into(),
            args: args(&["-i", "readme"]),
            cwd: cwd(&tmp),
        };
        let resp = dispatch(&req);
        assert_eq!(resp.exit_code, 0);
        assert!(resp.stdout.contains("README.md"), "stdout: {}", resp.stdout);
    }

    #[test]
    fn no_match_returns_exit_1() {
        let tmp = TempDir::new().unwrap();
        create_test_tree(&tmp);

        let req = SearchRequest {
            command: "fd".into(),
            args: args(&["zzz_nonexistent_pattern_xyz"]),
            cwd: cwd(&tmp),
        };
        let resp = dispatch(&req);
        assert_eq!(resp.exit_code, 1);
        assert!(resp.stdout.is_empty());
    }

    #[test]
    fn unsupported_flag_returns_fallback() {
        let req = SearchRequest {
            command: "fd".into(),
            args: args(&["--hidden", "pattern"]),
            cwd: "/tmp".into(),
        };
        let resp = dispatch(&req);
        assert_eq!(resp.exit_code, FALLBACK_EXIT_CODE);
        assert!(resp.stderr.contains("BITSCOUT_FALLBACK"));
    }

    #[test]
    fn output_paths_are_relative() {
        let tmp = TempDir::new().unwrap();
        create_test_tree(&tmp);

        let req = SearchRequest {
            command: "fd".into(),
            args: args(&["main"]),
            cwd: cwd(&tmp),
        };
        let resp = dispatch(&req);
        assert_eq!(resp.exit_code, 0);
        for line in resp.stdout.trim().lines() {
            assert!(
                !line.starts_with('/'),
                "expected relative path, got: {}",
                line
            );
        }
    }
}

// ===========================================================================
// 5. cat command tests
// ===========================================================================

mod cat {
    use super::*;

    #[test]
    fn basic_reads_file_content() {
        let tmp = TempDir::new().unwrap();
        create_test_tree(&tmp);

        let req = SearchRequest {
            command: "cat".into(),
            args: args(&["README.md"]),
            cwd: cwd(&tmp),
        };
        let resp = dispatch(&req);
        assert_eq!(resp.exit_code, 0);
        assert!(resp.stdout.contains("# My Project"), "stdout: {}", resp.stdout);
        assert!(resp.stdout.contains("sample project"), "stdout: {}", resp.stdout);
        assert!(resp.stderr.is_empty());
    }

    #[test]
    fn with_line_numbers() {
        let tmp = TempDir::new().unwrap();
        create_test_tree(&tmp);

        let req = SearchRequest {
            command: "cat".into(),
            args: args(&["-n", "README.md"]),
            cwd: cwd(&tmp),
        };
        let resp = dispatch(&req);
        assert_eq!(resp.exit_code, 0);
        // Should contain line numbers and tab separator
        assert!(resp.stdout.contains("1\t"), "stdout: {}", resp.stdout);
        assert!(resp.stdout.contains("# My Project"), "stdout: {}", resp.stdout);
    }

    #[test]
    fn multiple_files_concatenates() {
        let tmp = TempDir::new().unwrap();
        create_test_tree(&tmp);

        let req = SearchRequest {
            command: "cat".into(),
            args: args(&["README.md", "config.json"]),
            cwd: cwd(&tmp),
        };
        let resp = dispatch(&req);
        assert_eq!(resp.exit_code, 0);
        assert!(resp.stdout.contains("# My Project"), "stdout: {}", resp.stdout);
        assert!(resp.stdout.contains("\"debug\""), "stdout: {}", resp.stdout);
    }

    #[test]
    fn nonexistent_file_returns_exit_1() {
        let tmp = TempDir::new().unwrap();

        let req = SearchRequest {
            command: "cat".into(),
            args: args(&["nonexistent_file.txt"]),
            cwd: cwd(&tmp),
        };
        let resp = dispatch(&req);
        assert_eq!(resp.exit_code, 1);
        assert!(
            resp.stderr.contains("nonexistent_file.txt"),
            "stderr: {}",
            resp.stderr
        );
    }

    #[test]
    fn no_files_returns_fallback() {
        let req = SearchRequest {
            command: "cat".into(),
            args: vec![],
            cwd: "/tmp".into(),
        };
        let resp = dispatch(&req);
        assert_eq!(resp.exit_code, FALLBACK_EXIT_CODE);
        assert!(resp.stderr.contains("no files specified"));
    }

    #[test]
    fn unsupported_flag_returns_fallback() {
        let req = SearchRequest {
            command: "cat".into(),
            args: args(&["-v", "file.txt"]),
            cwd: "/tmp".into(),
        };
        let resp = dispatch(&req);
        assert_eq!(resp.exit_code, FALLBACK_EXIT_CODE);
        assert!(resp.stderr.contains("unsupported cat flag"));
    }

    #[test]
    fn absolute_path_works() {
        let tmp = TempDir::new().unwrap();
        let file_path = tmp.path().join("absolute_test.txt");
        fs::write(&file_path, "absolute content\n").unwrap();

        let req = SearchRequest {
            command: "cat".into(),
            args: vec![file_path.to_str().unwrap().into()],
            cwd: "/tmp".into(),
        };
        let resp = dispatch(&req);
        assert_eq!(resp.exit_code, 0);
        assert_eq!(resp.stdout, "absolute content\n");
    }

    #[test]
    fn preserves_no_trailing_newline() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("no_newline.txt"), "no trailing newline").unwrap();

        let req = SearchRequest {
            command: "cat".into(),
            args: args(&["no_newline.txt"]),
            cwd: cwd(&tmp),
        };
        let resp = dispatch(&req);
        assert_eq!(resp.exit_code, 0);
        assert_eq!(resp.stdout, "no trailing newline");
    }

    #[test]
    fn reads_nested_file_with_relative_path() {
        let tmp = TempDir::new().unwrap();
        create_test_tree(&tmp);

        let req = SearchRequest {
            command: "cat".into(),
            args: args(&["src/main.rs"]),
            cwd: cwd(&tmp),
        };
        let resp = dispatch(&req);
        assert_eq!(resp.exit_code, 0);
        assert!(resp.stdout.contains("fn main()"), "stdout: {}", resp.stdout);
    }
}

// ===========================================================================
// Cross-command / dispatch-level tests
// ===========================================================================

mod dispatch_meta {
    use super::*;

    #[test]
    fn unknown_command_returns_fallback() {
        let req = SearchRequest {
            command: "unknown_cmd".into(),
            args: vec![],
            cwd: "/tmp".into(),
        };
        let resp = dispatch(&req);
        assert_eq!(resp.exit_code, FALLBACK_EXIT_CODE);
        assert!(resp.stderr.contains("BITSCOUT_FALLBACK"));
        assert!(resp.stderr.contains("unknown command"));
    }

    #[test]
    fn all_commands_handle_empty_results_gracefully() {
        let tmp = TempDir::new().unwrap();
        // Empty directory, no files to search
        fs::create_dir_all(tmp.path().join("empty")).unwrap();

        // rg with no matches
        let resp = dispatch(&SearchRequest {
            command: "rg".into(),
            args: args(&["pattern", "empty"]),
            cwd: cwd(&tmp),
        });
        assert!(resp.exit_code == 1 || resp.exit_code == 0);

        // grep with no matches
        let resp = dispatch(&SearchRequest {
            command: "grep".into(),
            args: args(&["-r", "pattern", "empty"]),
            cwd: cwd(&tmp),
        });
        assert!(resp.exit_code == 1 || resp.exit_code == 0);

        // find on empty dir returns just the dir itself (or empty)
        let resp = dispatch(&SearchRequest {
            command: "find".into(),
            args: args(&["empty"]),
            cwd: cwd(&tmp),
        });
        assert_eq!(resp.exit_code, 0);
    }

    #[test]
    fn response_fields_are_well_formed() {
        // Verify that successful responses have empty stderr
        // and failed responses have non-empty stderr
        let tmp = TempDir::new().unwrap();
        create_test_tree(&tmp);

        let resp = dispatch(&SearchRequest {
            command: "rg".into(),
            args: args(&["fn", "."]),
            cwd: cwd(&tmp),
        });
        assert_eq!(resp.exit_code, 0);
        assert!(!resp.stdout.is_empty());
        assert!(resp.stderr.is_empty());

        // Error case
        let resp = dispatch(&SearchRequest {
            command: "cat".into(),
            args: args(&["does_not_exist.txt"]),
            cwd: cwd(&tmp),
        });
        assert_ne!(resp.exit_code, 0);
        assert!(!resp.stderr.is_empty());
    }
}
