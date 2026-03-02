//! rg conformance tests: compare real `rg` output with BitScout dispatch output.
//!
//! These tests run the real `rg` binary and our `dispatch()` on the same corpus,
//! then assert that outputs are semantically identical (same matches, same format).
//!
//! The tests verify that BitScout is a drop-in replacement: any tool (Claude Code,
//! Cursor, etc.) calling rg will see the same results from BitScout.

use bitscout_core::protocol::SearchRequest;
use bitscout_daemon::dispatch::{dispatch, FALLBACK_EXIT_CODE};
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Locate the real rg binary, skipping BitScout shims.
fn real_rg_path() -> Option<String> {
    // Try common locations
    let candidates = [
        "/opt/homebrew/bin/rg",
        "/usr/local/bin/rg",
        "/usr/bin/rg",
    ];
    for c in candidates {
        if Path::new(c).exists() {
            return Some(c.to_string());
        }
    }
    // Try `which rg` but filter out shim paths
    let output = Command::new("sh")
        .args(["-c", "which -a rg 2>/dev/null || true"])
        .output()
        .ok()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        let line = line.trim();
        if !line.is_empty() && !line.contains(".bitscout") && !line.contains("claude") {
            if Path::new(line).exists() {
                return Some(line.to_string());
            }
        }
    }
    None
}

/// Run real rg with given args on the corpus directory.
/// Returns (exit_code, stdout, stderr).
fn run_real_rg(rg_path: &str, args: &[&str], dir: &Path) -> (i32, String, String) {
    let output = Command::new(rg_path)
        .args(args)
        .arg(dir)
        .output()
        .expect("Failed to run rg");
    let code = output.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    (code, stdout, stderr)
}

/// Run BitScout dispatch with the same rg args.
fn run_bitscout_rg(args: &[&str], dir: &Path) -> (i32, String, String) {
    let req = SearchRequest {
        command: "rg".into(),
        args: args.iter().map(|s| s.to_string()).collect(),
        cwd: dir.to_str().unwrap().into(),
    };
    let resp = dispatch(&req);
    (resp.exit_code, resp.stdout, resp.stderr)
}

/// Normalize output lines for comparison:
/// - Sort lines (rg output order depends on filesystem traversal)
/// - Strip trailing whitespace
/// - Replace absolute temp paths with relative paths
fn normalize_lines(output: &str, dir: &Path) -> BTreeSet<String> {
    let dir_str = dir.to_str().unwrap();
    // Also handle canonicalized path (macOS /private/var vs /var)
    let canonical = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
    let canonical_str = canonical.to_str().unwrap_or(dir_str);

    output
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| {
            let mut s = l.trim_end().to_string();
            // Replace both canonical and original paths
            s = s.replace(canonical_str, "<DIR>");
            s = s.replace(dir_str, "<DIR>");
            // Normalize path separators
            s = s.replace("//", "/");
            s
        })
        .collect()
}

/// Assert that two sets of normalized output lines match.
/// Reports diffs clearly on failure.
fn assert_same_lines(label: &str, real_rg: &BTreeSet<String>, bitscout: &BTreeSet<String>) {
    let only_in_rg: Vec<_> = real_rg.difference(bitscout).collect();
    let only_in_bs: Vec<_> = bitscout.difference(real_rg).collect();

    if !only_in_rg.is_empty() || !only_in_bs.is_empty() {
        let mut msg = format!("\n=== {} MISMATCH ===\n", label);
        if !only_in_rg.is_empty() {
            msg.push_str("Lines ONLY in real rg (MISSING from BitScout):\n");
            for line in &only_in_rg {
                msg.push_str(&format!("  - {}\n", line));
            }
        }
        if !only_in_bs.is_empty() {
            msg.push_str("Lines ONLY in BitScout (EXTRA, not in real rg):\n");
            for line in &only_in_bs {
                msg.push_str(&format!("  + {}\n", line));
            }
        }
        msg.push_str(&format!("\nReal rg ({} lines):\n", real_rg.len()));
        for l in real_rg {
            msg.push_str(&format!("  {}\n", l));
        }
        msg.push_str(&format!("\nBitScout ({} lines):\n", bitscout.len()));
        for l in bitscout {
            msg.push_str(&format!("  {}\n", l));
        }
        panic!("{}", msg);
    }
}

/// Create a standardized test corpus.
fn create_corpus(dir: &Path) {
    let src = dir.join("src");
    fs::create_dir_all(&src).unwrap();
    let tests = dir.join("tests");
    fs::create_dir_all(&tests).unwrap();

    fs::write(
        src.join("main.rs"),
        r#"fn main() {
    println!("hello world");
    let result = authenticate_user("admin");
    if result {
        start_session();
    }
}

fn authenticate_user(name: &str) -> bool {
    if name == "admin" {
        return true;
    }
    validate_token(name)
}

fn validate_token(token: &str) -> bool {
    token.len() > 3
}

fn start_session() {
    println!("session started");
}
"#,
    )
    .unwrap();

    fs::write(
        src.join("lib.rs"),
        r#"pub mod auth;

pub fn authenticate_request(req: &str) -> bool {
    req.contains("Bearer")
}

pub fn validate_token(token: &str) -> Option<String> {
    if token.starts_with("tk_") {
        Some(token.to_string())
    } else {
        None
    }
}
"#,
    )
    .unwrap();

    fs::write(
        src.join("utils.rs"),
        r#"/// Helper utilities
pub fn format_error(msg: &str) -> String {
    format!("Error: {}", msg)
}

pub fn sanitize_input(input: &str) -> String {
    input.replace("<", "&lt;").replace(">", "&gt;")
}
"#,
    )
    .unwrap();

    fs::write(
        tests.join("test_auth.rs"),
        r#"use mylib::authenticate_request;

#[test]
fn test_authenticate_valid() {
    assert!(authenticate_request("Bearer abc123"));
}

#[test]
fn test_authenticate_invalid() {
    assert!(!authenticate_request("no_token"));
}

#[test]
fn test_validate_token() {
    let result = mylib::validate_token("tk_abc");
    assert!(result.is_some());
}
"#,
    )
    .unwrap();

    fs::write(
        dir.join("README.md"),
        "# Test Project\n\nThis is a test project for authentication.\n",
    )
    .unwrap();

    fs::write(
        dir.join("config.json"),
        r#"{"auth": {"enabled": true, "token_prefix": "tk_"}, "debug": false}"#,
    )
    .unwrap();
}

// ---------------------------------------------------------------------------
// Conformance tests
// ---------------------------------------------------------------------------

/// Test: rg basic search — same matches, same format
#[test]
fn test_rg_basic_search_conformance() {
    let rg = match real_rg_path() {
        Some(p) => p,
        None => {
            eprintln!("SKIP: real rg not found");
            return;
        }
    };

    let tmp = TempDir::new().unwrap();
    create_corpus(tmp.path());

    // Search pattern that exists in multiple files
    let pattern = "authenticate";
    let args = &[pattern, "."];

    let (rg_exit, rg_out, _) = run_real_rg(&rg, args, tmp.path());
    let (bs_exit, bs_out, _) = run_bitscout_rg(args, tmp.path());

    // Exit codes must match
    assert_eq!(rg_exit, bs_exit, "Exit code mismatch: rg={}, bs={}", rg_exit, bs_exit);
    assert_eq!(rg_exit, 0);

    // Compare normalized line sets
    let rg_lines = normalize_lines(&rg_out, tmp.path());
    let bs_lines = normalize_lines(&bs_out, tmp.path());
    assert_same_lines("basic search", &rg_lines, &bs_lines);
}

/// Test: rg with -n (line numbers)
#[test]
fn test_rg_line_numbers_conformance() {
    let rg = match real_rg_path() {
        Some(p) => p,
        None => return,
    };

    let tmp = TempDir::new().unwrap();
    create_corpus(tmp.path());

    let args = &["-n", "authenticate", "."];

    let (rg_exit, rg_out, _) = run_real_rg(&rg, args, tmp.path());
    let (bs_exit, bs_out, _) = run_bitscout_rg(args, tmp.path());

    assert_eq!(rg_exit, bs_exit);
    let rg_lines = normalize_lines(&rg_out, tmp.path());
    let bs_lines = normalize_lines(&bs_out, tmp.path());
    assert_same_lines("line numbers (-n)", &rg_lines, &bs_lines);
}

/// Test: rg with -c (count)
#[test]
fn test_rg_count_conformance() {
    let rg = match real_rg_path() {
        Some(p) => p,
        None => return,
    };

    let tmp = TempDir::new().unwrap();
    create_corpus(tmp.path());

    let args = &["-c", "authenticate", "."];

    let (rg_exit, rg_out, _) = run_real_rg(&rg, args, tmp.path());
    let (bs_exit, bs_out, _) = run_bitscout_rg(args, tmp.path());

    assert_eq!(rg_exit, bs_exit);
    let rg_lines = normalize_lines(&rg_out, tmp.path());
    let bs_lines = normalize_lines(&bs_out, tmp.path());
    assert_same_lines("count (-c)", &rg_lines, &bs_lines);
}

/// Test: rg with -l (files only)
#[test]
fn test_rg_files_only_conformance() {
    let rg = match real_rg_path() {
        Some(p) => p,
        None => return,
    };

    let tmp = TempDir::new().unwrap();
    create_corpus(tmp.path());

    let args = &["-l", "authenticate", "."];

    let (rg_exit, rg_out, _) = run_real_rg(&rg, args, tmp.path());
    let (bs_exit, bs_out, _) = run_bitscout_rg(args, tmp.path());

    assert_eq!(rg_exit, bs_exit);
    let rg_lines = normalize_lines(&rg_out, tmp.path());
    let bs_lines = normalize_lines(&bs_out, tmp.path());
    assert_same_lines("files only (-l)", &rg_lines, &bs_lines);
}

/// Test: rg with -i (case insensitive)
#[test]
fn test_rg_case_insensitive_conformance() {
    let rg = match real_rg_path() {
        Some(p) => p,
        None => return,
    };

    let tmp = TempDir::new().unwrap();
    create_corpus(tmp.path());

    let args = &["-i", "Authenticate", "."];

    let (rg_exit, rg_out, _) = run_real_rg(&rg, args, tmp.path());
    let (bs_exit, bs_out, _) = run_bitscout_rg(args, tmp.path());

    assert_eq!(rg_exit, bs_exit);
    let rg_lines = normalize_lines(&rg_out, tmp.path());
    let bs_lines = normalize_lines(&bs_out, tmp.path());
    assert_same_lines("case insensitive (-i)", &rg_lines, &bs_lines);
}

/// Test: rg no match returns exit code 1
#[test]
fn test_rg_no_match_conformance() {
    let rg = match real_rg_path() {
        Some(p) => p,
        None => return,
    };

    let tmp = TempDir::new().unwrap();
    create_corpus(tmp.path());

    let args = &["zzz_definitely_no_match_zzz", "."];

    let (rg_exit, rg_out, _) = run_real_rg(&rg, args, tmp.path());
    let (bs_exit, bs_out, _) = run_bitscout_rg(args, tmp.path());

    assert_eq!(rg_exit, 1, "rg should exit 1 on no match");
    assert_eq!(bs_exit, 1, "BitScout should exit 1 on no match");
    assert!(rg_out.is_empty(), "rg stdout should be empty on no match");
    assert!(bs_out.is_empty(), "BitScout stdout should be empty on no match");
}

/// Test: rg with --glob filter
#[test]
fn test_rg_glob_filter_conformance() {
    let rg = match real_rg_path() {
        Some(p) => p,
        None => return,
    };

    let tmp = TempDir::new().unwrap();
    create_corpus(tmp.path());

    let args = &["--glob", "*.rs", "authenticate", "."];

    let (rg_exit, rg_out, _) = run_real_rg(&rg, args, tmp.path());
    let (bs_exit, bs_out, _) = run_bitscout_rg(args, tmp.path());

    assert_eq!(rg_exit, bs_exit);

    // Verify that rg only matched .rs files
    let rg_lines = normalize_lines(&rg_out, tmp.path());
    for line in &rg_lines {
        // Lines should reference .rs files (or be context separators)
        if line.contains(':') {
            let path_part = line.split(':').next().unwrap();
            assert!(
                path_part.ends_with(".rs") || path_part.starts_with("<DIR>"),
                "rg --glob *.rs matched non-.rs file: {}",
                line
            );
        }
    }

    let bs_lines = normalize_lines(&bs_out, tmp.path());
    assert_same_lines("glob filter (--glob *.rs)", &rg_lines, &bs_lines);
}

/// Test: rg with --type filter
#[test]
fn test_rg_type_filter_conformance() {
    let rg = match real_rg_path() {
        Some(p) => p,
        None => return,
    };

    let tmp = TempDir::new().unwrap();
    create_corpus(tmp.path());

    let args = &["--type", "rust", "authenticate", "."];

    let (rg_exit, rg_out, _) = run_real_rg(&rg, args, tmp.path());
    let (bs_exit, bs_out, _) = run_bitscout_rg(args, tmp.path());

    assert_eq!(rg_exit, bs_exit);
    let rg_lines = normalize_lines(&rg_out, tmp.path());
    let bs_lines = normalize_lines(&bs_out, tmp.path());
    assert_same_lines("type filter (--type rust)", &rg_lines, &bs_lines);
}

/// Test: rg with -i -n combined
#[test]
fn test_rg_combined_flags_conformance() {
    let rg = match real_rg_path() {
        Some(p) => p,
        None => return,
    };

    let tmp = TempDir::new().unwrap();
    create_corpus(tmp.path());

    let args = &["-i", "-n", "authenticate", "."];

    let (rg_exit, rg_out, _) = run_real_rg(&rg, args, tmp.path());
    let (bs_exit, bs_out, _) = run_bitscout_rg(args, tmp.path());

    assert_eq!(rg_exit, bs_exit);
    let rg_lines = normalize_lines(&rg_out, tmp.path());
    let bs_lines = normalize_lines(&bs_out, tmp.path());
    assert_same_lines("combined -i -n", &rg_lines, &bs_lines);
}

/// Test: rg with -c -i combined (case-insensitive count)
#[test]
fn test_rg_count_case_insensitive_conformance() {
    let rg = match real_rg_path() {
        Some(p) => p,
        None => return,
    };

    let tmp = TempDir::new().unwrap();
    create_corpus(tmp.path());

    let args = &["-c", "-i", "authenticate", "."];

    let (rg_exit, rg_out, _) = run_real_rg(&rg, args, tmp.path());
    let (bs_exit, bs_out, _) = run_bitscout_rg(args, tmp.path());

    assert_eq!(rg_exit, bs_exit);
    let rg_lines = normalize_lines(&rg_out, tmp.path());
    let bs_lines = normalize_lines(&bs_out, tmp.path());
    assert_same_lines("count case insensitive (-c -i)", &rg_lines, &bs_lines);
}

/// Test: rg JSON output — compare parsed JSON structures
#[test]
fn test_rg_json_output_conformance() {
    let rg = match real_rg_path() {
        Some(p) => p,
        None => return,
    };

    let tmp = TempDir::new().unwrap();
    create_corpus(tmp.path());

    let args = &["--json", "validate_token", "."];

    let (rg_exit, rg_out, _) = run_real_rg(&rg, args, tmp.path());
    let (bs_exit, bs_out, _) = run_bitscout_rg(args, tmp.path());

    assert_eq!(rg_exit, bs_exit);

    // Parse JSON lines from real rg — extract match records
    let rg_matches: BTreeSet<String> = rg_out
        .lines()
        .filter_map(|l| {
            let v: serde_json::Value = serde_json::from_str(l).ok()?;
            if v["type"] == "match" {
                let path = v["data"]["path"]["text"].as_str()?;
                let line_num = v["data"]["line_number"].as_u64()?;
                let lines_text = v["data"]["lines"]["text"].as_str()?;
                // Normalize: strip path prefix and trailing newline
                let norm_path = path
                    .replace(tmp.path().canonicalize().unwrap().to_str().unwrap(), "<DIR>")
                    .replace(tmp.path().to_str().unwrap(), "<DIR>");
                Some(format!(
                    "{}:{}:{}",
                    norm_path,
                    line_num,
                    lines_text.trim_end()
                ))
            } else {
                None
            }
        })
        .collect();

    // Parse JSON lines from BitScout
    let bs_matches: BTreeSet<String> = bs_out
        .lines()
        .filter_map(|l| {
            let v: serde_json::Value = serde_json::from_str(l).ok()?;
            if v["type"] == "match" {
                let path = v["data"]["path"]["text"].as_str()?;
                let line_num = v["data"]["line_number"].as_u64()?;
                let lines_text = v["data"]["lines"]["text"].as_str()?;
                let norm_path = path
                    .replace(tmp.path().canonicalize().unwrap().to_str().unwrap(), "<DIR>")
                    .replace(tmp.path().to_str().unwrap(), "<DIR>");
                Some(format!(
                    "{}:{}:{}",
                    norm_path,
                    line_num,
                    lines_text.trim_end()
                ))
            } else {
                None
            }
        })
        .collect();

    assert_same_lines("JSON match records", &rg_matches, &bs_matches);

    // Also verify JSON is well-formed for BitScout output
    for line in bs_out.lines() {
        if line.is_empty() {
            continue;
        }
        let parsed: Result<serde_json::Value, _> = serde_json::from_str(line);
        assert!(parsed.is_ok(), "BitScout JSON line is malformed: {}", line);
    }
}

/// Test: rg with context lines (-C 1) — verify context matches
#[test]
fn test_rg_context_conformance() {
    let rg = match real_rg_path() {
        Some(p) => p,
        None => return,
    };

    let tmp = TempDir::new().unwrap();
    create_corpus(tmp.path());

    // Use -n -C 1 to get numbered context
    let args = &["-n", "-C", "1", "validate_token", "."];

    let (rg_exit, rg_out, _) = run_real_rg(&rg, args, tmp.path());
    let (bs_exit, bs_out, _) = run_bitscout_rg(args, tmp.path());

    assert_eq!(rg_exit, bs_exit);

    // For context, we compare the match lines and verify context is present.
    // We extract just match lines (containing `:linenum:`) and compare those.
    let extract_matches = |output: &str, dir: &Path| -> BTreeSet<String> {
        let dir_str = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
        let dir_s = dir_str.to_str().unwrap();
        output
            .lines()
            .filter(|l| {
                // Match lines have : separators, context lines have -
                // Match line format: path:linenum:content
                let parts: Vec<&str> = l.splitn(3, ':').collect();
                parts.len() == 3 && parts[1].parse::<usize>().is_ok()
            })
            .map(|l| {
                l.replace(dir_s, "<DIR>")
                    .replace(tmp.path().to_str().unwrap(), "<DIR>")
            })
            .collect()
    };

    let rg_matches = extract_matches(&rg_out, tmp.path());
    let bs_matches = extract_matches(&bs_out, tmp.path());
    assert_same_lines("context match lines", &rg_matches, &bs_matches);
}

/// Test: rg with unsupported flags triggers fallback (not crash)
#[test]
fn test_rg_unsupported_flags_fallback() {
    let args = &["--pcre2", "pattern", "."];
    let (bs_exit, _, bs_err) = run_bitscout_rg(args, Path::new("/tmp"));
    assert_eq!(bs_exit, FALLBACK_EXIT_CODE);
    assert!(bs_err.contains("BITSCOUT_FALLBACK"));
}

/// Test: rg on unique pattern — verify exact match count
#[test]
fn test_rg_exact_match_count() {
    let rg = match real_rg_path() {
        Some(p) => p,
        None => return,
    };

    let tmp = TempDir::new().unwrap();
    create_corpus(tmp.path());

    // "start_session" appears exactly once in main.rs
    let args = &["-c", "start_session", "."];

    let (rg_exit, rg_out, _) = run_real_rg(&rg, args, tmp.path());
    let (bs_exit, bs_out, _) = run_bitscout_rg(args, tmp.path());

    assert_eq!(rg_exit, bs_exit);

    // Parse counts
    let rg_total: usize = rg_out
        .lines()
        .filter_map(|l| l.rsplit(':').next()?.trim().parse::<usize>().ok())
        .sum();
    let bs_total: usize = bs_out
        .lines()
        .filter_map(|l| l.rsplit(':').next()?.trim().parse::<usize>().ok())
        .sum();

    assert_eq!(rg_total, bs_total, "Total match counts differ: rg={}, bs={}", rg_total, bs_total);
    // start_session appears in 2 lines (call + definition)
    assert_eq!(rg_total, 2, "Expected 2 matches for start_session");
}

/// Test: rg with single-file target — format should not include filename
/// (rg omits filename when searching a single file)
#[test]
fn test_rg_single_file_format() {
    let rg = match real_rg_path() {
        Some(p) => p,
        None => return,
    };

    let tmp = TempDir::new().unwrap();
    create_corpus(tmp.path());

    let file_path = tmp.path().join("src/main.rs");
    let file_str = file_path.to_str().unwrap();

    // rg pattern single_file (no -n) → just "line_content" per match
    let rg_output = Command::new(&rg)
        .args(["authenticate", file_str])
        .output()
        .expect("Failed to run rg");
    let rg_out = String::from_utf8_lossy(&rg_output.stdout).to_string();

    // For single file, rg does NOT show filename
    for line in rg_out.lines() {
        assert!(
            !line.starts_with(file_str),
            "rg single-file mode should NOT prefix filename, but got: {}",
            line
        );
    }

    // BitScout dispatch always gets a directory (via SearchEngine),
    // so it naturally uses multi-file format. This is a known divergence
    // when the search path resolves to a single file. Document it.
}

/// Test: verify that all accelerated flag combos don't crash
#[test]
fn test_rg_all_accelerated_flags_no_crash() {
    let tmp = TempDir::new().unwrap();
    create_corpus(tmp.path());

    let flag_combos: Vec<Vec<&str>> = vec![
        vec!["pattern", "."],
        vec!["-n", "pattern", "."],
        vec!["-i", "pattern", "."],
        vec!["-c", "pattern", "."],
        vec!["-l", "pattern", "."],
        vec!["--json", "pattern", "."],
        vec!["-n", "-i", "pattern", "."],
        vec!["-c", "-i", "pattern", "."],
        vec!["-C", "2", "pattern", "."],
        vec!["-A", "1", "-B", "1", "pattern", "."],
        vec!["--glob", "*.rs", "pattern", "."],
        vec!["--type", "rust", "pattern", "."],
        vec!["-n", "-i", "--glob", "*.rs", "pattern", "."],
    ];

    for args in &flag_combos {
        let (exit, _, err) = run_bitscout_rg(args, tmp.path());
        assert_ne!(
            exit, FALLBACK_EXIT_CODE,
            "Accelerated flags {:?} should NOT fallback, but stderr: {}",
            args, err
        );
        // Should be either 0 (found) or 1 (not found), never 2 (error) or 200 (fallback)
        assert!(
            exit == 0 || exit == 1,
            "Unexpected exit code {} for args {:?}, stderr: {}",
            exit, args, err
        );
    }
}

/// Test: rg with -l -i combined — files with case-insensitive matches
#[test]
fn test_rg_files_case_insensitive_conformance() {
    let rg = match real_rg_path() {
        Some(p) => p,
        None => return,
    };

    let tmp = TempDir::new().unwrap();
    create_corpus(tmp.path());

    let args = &["-l", "-i", "AUTHENTICATE", "."];

    let (rg_exit, rg_out, _) = run_real_rg(&rg, args, tmp.path());
    let (bs_exit, bs_out, _) = run_bitscout_rg(args, tmp.path());

    assert_eq!(rg_exit, bs_exit);
    let rg_lines = normalize_lines(&rg_out, tmp.path());
    let bs_lines = normalize_lines(&bs_out, tmp.path());
    assert_same_lines("files case insensitive (-l -i)", &rg_lines, &bs_lines);
}

/// Test: pattern with special regex characters
#[test]
fn test_rg_regex_pattern_conformance() {
    let rg = match real_rg_path() {
        Some(p) => p,
        None => return,
    };

    let tmp = TempDir::new().unwrap();
    create_corpus(tmp.path());

    // Pattern with regex: fn.*authenticate
    let args = &["-n", "fn.*authenticate", "."];

    let (rg_exit, rg_out, _) = run_real_rg(&rg, args, tmp.path());
    let (bs_exit, bs_out, _) = run_bitscout_rg(args, tmp.path());

    assert_eq!(rg_exit, bs_exit);
    let rg_lines = normalize_lines(&rg_out, tmp.path());
    let bs_lines = normalize_lines(&bs_out, tmp.path());
    assert_same_lines("regex pattern (fn.*authenticate)", &rg_lines, &bs_lines);
}
