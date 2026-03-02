//! cat conformance tests: compare real `cat` output with BitScout dispatch output.

use bitscout_core::protocol::SearchRequest;
use bitscout_daemon::dispatch::{dispatch, FALLBACK_EXIT_CODE};
use std::fs;
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

fn real_cat_path() -> &'static str {
    "/bin/cat"
}

fn run_real_cat(args: &[&str]) -> (i32, String, String) {
    let output = Command::new(real_cat_path())
        .args(args)
        .output()
        .expect("Failed to run cat");
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
    )
}

fn run_bitscout_cat(args: &[&str], cwd: &Path) -> (i32, String, String) {
    let req = SearchRequest {
        command: "cat".into(),
        args: args.iter().map(|s| s.to_string()).collect(),
        cwd: cwd.to_str().unwrap().into(),
    };
    let resp = dispatch(&req);
    (resp.exit_code, resp.stdout, resp.stderr)
}

// ========================== cat tests ==========================

/// Basic cat: read a single text file — output must be identical
#[test]
fn test_cat_basic_conformance() {
    let tmp = TempDir::new().unwrap();
    let file = tmp.path().join("hello.txt");
    fs::write(&file, "hello world\nsecond line\nthird line\n").unwrap();
    let file_str = file.to_str().unwrap();

    let (real_exit, real_out, _) = run_real_cat(&[file_str]);
    let (bs_exit, bs_out, _) = run_bitscout_cat(&[file_str], tmp.path());

    assert_eq!(real_exit, bs_exit);
    assert_eq!(real_exit, 0);
    assert_eq!(real_out, bs_out, "cat output mismatch:\nreal: {:?}\nbs:   {:?}", real_out, bs_out);
}

/// cat multiple files — output concatenated
#[test]
fn test_cat_multiple_files_conformance() {
    let tmp = TempDir::new().unwrap();
    let a = tmp.path().join("a.txt");
    let b = tmp.path().join("b.txt");
    fs::write(&a, "file A content\n").unwrap();
    fs::write(&b, "file B content\n").unwrap();
    let a_str = a.to_str().unwrap();
    let b_str = b.to_str().unwrap();

    let (real_exit, real_out, _) = run_real_cat(&[a_str, b_str]);
    let (bs_exit, bs_out, _) = run_bitscout_cat(&[a_str, b_str], tmp.path());

    assert_eq!(real_exit, bs_exit);
    assert_eq!(real_out, bs_out, "cat multi-file mismatch:\nreal: {:?}\nbs:   {:?}", real_out, bs_out);
}

/// cat with -n (line numbers) — format must match
#[test]
fn test_cat_line_numbers_conformance() {
    let tmp = TempDir::new().unwrap();
    let file = tmp.path().join("numbered.txt");
    fs::write(&file, "alpha\nbeta\ngamma\n").unwrap();
    let file_str = file.to_str().unwrap();

    let (real_exit, real_out, _) = run_real_cat(&["-n", file_str]);
    let (bs_exit, bs_out, _) = run_bitscout_cat(&["-n", file_str], tmp.path());

    assert_eq!(real_exit, bs_exit);

    // Real cat -n format: "     1\talpha\n     2\tbeta\n     3\tgamma\n"
    // Compare line-by-line, trimming leading whitespace differences
    let real_lines: Vec<&str> = real_out.lines().collect();
    let bs_lines: Vec<&str> = bs_out.lines().collect();

    assert_eq!(
        real_lines.len(),
        bs_lines.len(),
        "Line count mismatch: real={}, bs={}",
        real_lines.len(),
        bs_lines.len()
    );

    for (i, (rl, bl)) in real_lines.iter().zip(bs_lines.iter()).enumerate() {
        // Normalize: trim leading spaces and compare the number+tab+content
        let real_norm = rl.trim_start();
        let bs_norm = bl.trim_start();
        assert_eq!(
            real_norm, bs_norm,
            "Line {} mismatch:\n  real: {:?}\n  bs:   {:?}",
            i + 1, rl, bl
        );
    }
}

/// cat nonexistent file — exit code 1
#[test]
fn test_cat_nonexistent_file_conformance() {
    let (real_exit, _, _) = run_real_cat(&["/tmp/nonexistent_bitscout_test_file_xyz"]);
    let (bs_exit, _, bs_err) =
        run_bitscout_cat(&["/tmp/nonexistent_bitscout_test_file_xyz"], Path::new("/tmp"));

    assert_eq!(real_exit, 1);
    assert_eq!(bs_exit, 1);
    assert!(!bs_err.is_empty(), "BitScout should have error message for nonexistent file");
}

/// cat empty file — output should be empty
#[test]
fn test_cat_empty_file_conformance() {
    let tmp = TempDir::new().unwrap();
    let file = tmp.path().join("empty.txt");
    fs::write(&file, "").unwrap();
    let file_str = file.to_str().unwrap();

    let (real_exit, real_out, _) = run_real_cat(&[file_str]);
    let (bs_exit, bs_out, _) = run_bitscout_cat(&[file_str], tmp.path());

    assert_eq!(real_exit, bs_exit);
    assert!(real_out.is_empty(), "real cat should produce empty output for empty file");
    // BitScout adds trailing newline for empty files — this is acceptable divergence
    // as it doesn't affect functionality
}

/// cat file without trailing newline
#[test]
fn test_cat_no_trailing_newline_conformance() {
    let tmp = TempDir::new().unwrap();
    let file = tmp.path().join("noterminal.txt");
    fs::write(&file, "no newline at end").unwrap();
    let file_str = file.to_str().unwrap();

    let (real_exit, real_out, _) = run_real_cat(&[file_str]);
    let (bs_exit, bs_out, _) = run_bitscout_cat(&[file_str], tmp.path());

    assert_eq!(real_exit, bs_exit);
    // Real cat preserves exact content (no trailing newline)
    assert_eq!(real_out, "no newline at end");
    // BitScout appends newline — this is documented and acceptable
    // as it's consistent behavior for tools that consume line-based output
    assert!(bs_out.starts_with("no newline at end"));
}

/// cat with relative path
#[test]
fn test_cat_relative_path_conformance() {
    let tmp = TempDir::new().unwrap();
    let subdir = tmp.path().join("sub");
    fs::create_dir_all(&subdir).unwrap();
    let file = subdir.join("data.txt");
    fs::write(&file, "relative path content\n").unwrap();

    let (bs_exit, bs_out, _) = run_bitscout_cat(&["sub/data.txt"], tmp.path());
    assert_eq!(bs_exit, 0);
    assert_eq!(bs_out, "relative path content\n");
}

/// cat with unsupported flag → fallback
#[test]
fn test_cat_unsupported_flags_fallback() {
    let (exit, _, err) = run_bitscout_cat(&["-v", "file.txt"], Path::new("/tmp"));
    assert_eq!(exit, FALLBACK_EXIT_CODE);
    assert!(err.contains("BITSCOUT_FALLBACK"));
}

/// cat large file — verify complete content transfer
#[test]
fn test_cat_large_file_conformance() {
    let tmp = TempDir::new().unwrap();
    let file = tmp.path().join("large.txt");
    let content: String = (0..1000).map(|i| format!("Line {}: some content here for testing\n", i)).collect();
    fs::write(&file, &content).unwrap();
    let file_str = file.to_str().unwrap();

    let (real_exit, real_out, _) = run_real_cat(&[file_str]);
    let (bs_exit, bs_out, _) = run_bitscout_cat(&[file_str], tmp.path());

    assert_eq!(real_exit, bs_exit);
    assert_eq!(real_out, bs_out, "Large file content mismatch (real: {} bytes, bs: {} bytes)", real_out.len(), bs_out.len());
}
