//! grep conformance tests: compare real `grep` output with BitScout dispatch output.

use bitscout_core::dispatch::{dispatch, FALLBACK_EXIT_CODE};
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;


fn real_grep_path() -> Option<String> {
    for c in ["/usr/bin/grep", "/opt/homebrew/bin/grep", "/usr/local/bin/grep"] {
        if Path::new(c).exists() {
            return Some(c.to_string());
        }
    }
    None
}

fn run_real_grep(grep_path: &str, args: &[&str]) -> (i32, String, String) {
    let output = Command::new(grep_path)
        .args(args)
        .output()
        .expect("Failed to run grep");
    let code = output.status.code().unwrap_or(-1);
    (
        code,
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
    )
}

fn run_bitscout_grep(args: &[&str], cwd: &Path) -> (i32, String, String) {
    let cwd_str = cwd.to_str().unwrap();
    let args_owned: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    let resp = dispatch("grep", &args_owned, cwd_str);
    (resp.exit_code, resp.stdout, resp.stderr)
}

fn normalize_lines(output: &str, dir: &Path) -> BTreeSet<String> {
    let canonical = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
    let canonical_str = canonical.to_str().unwrap();
    let dir_str = dir.to_str().unwrap();
    output
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| {
            l.trim_end()
                .replace(canonical_str, "<DIR>")
                .replace(dir_str, "<DIR>")
                .replace("//", "/")
        })
        .collect()
}

fn assert_same_lines(label: &str, real: &BTreeSet<String>, bs: &BTreeSet<String>) {
    let only_real: Vec<_> = real.difference(bs).collect();
    let only_bs: Vec<_> = bs.difference(real).collect();
    if !only_real.is_empty() || !only_bs.is_empty() {
        let mut msg = format!("\n=== {} MISMATCH ===\n", label);
        if !only_real.is_empty() {
            msg.push_str("Only in real grep:\n");
            for l in &only_real {
                msg.push_str(&format!("  - {}\n", l));
            }
        }
        if !only_bs.is_empty() {
            msg.push_str("Only in BitScout:\n");
            for l in &only_bs {
                msg.push_str(&format!("  + {}\n", l));
            }
        }
        panic!("{}", msg);
    }
}

fn create_corpus(dir: &Path) {
    let src = dir.join("src");
    fs::create_dir_all(&src).unwrap();

    fs::write(
        src.join("main.rs"),
        "fn main() {\n    authenticate_user(\"admin\");\n    start_session();\n}\n\nfn authenticate_user(name: &str) -> bool {\n    name == \"admin\"\n}\n\nfn start_session() {}\n",
    ).unwrap();

    fs::write(
        src.join("lib.rs"),
        "pub fn authenticate_request(req: &str) -> bool {\n    req.contains(\"Bearer\")\n}\n",
    ).unwrap();

    fs::write(
        dir.join("README.md"),
        "# Test Project\n\nAuthenticate users via token.\n",
    ).unwrap();
}

#[test]
fn test_grep_basic_recursive_conformance() {
    let grep = match real_grep_path() {
        Some(p) => p,
        None => return,
    };

    let tmp = TempDir::new().unwrap();
    create_corpus(tmp.path());
    let dir_str = tmp.path().to_str().unwrap();

    let real_args = &["-r", "authenticate", dir_str];
    let bs_args = &["-r", "authenticate", "."];

    let (rg_exit, rg_out, _) = run_real_grep(&grep, real_args);
    let (bs_exit, bs_out, _) = run_bitscout_grep(bs_args, tmp.path());

    assert_eq!(rg_exit, bs_exit);
    let rg_lines = normalize_lines(&rg_out, tmp.path());
    let bs_lines = normalize_lines(&bs_out, tmp.path());
    assert_same_lines("grep -r basic", &rg_lines, &bs_lines);
}

#[test]
fn test_grep_with_line_numbers_conformance() {
    let grep = match real_grep_path() {
        Some(p) => p,
        None => return,
    };

    let tmp = TempDir::new().unwrap();
    create_corpus(tmp.path());
    let dir_str = tmp.path().to_str().unwrap();

    let real_args = &["-rn", "authenticate", dir_str];
    let bs_args = &["-rn", "authenticate", "."];

    let (rg_exit, rg_out, _) = run_real_grep(&grep, real_args);
    let (bs_exit, bs_out, _) = run_bitscout_grep(bs_args, tmp.path());

    assert_eq!(rg_exit, bs_exit);
    let rg_lines = normalize_lines(&rg_out, tmp.path());
    let bs_lines = normalize_lines(&bs_out, tmp.path());
    assert_same_lines("grep -rn", &rg_lines, &bs_lines);
}

#[test]
fn test_grep_count_conformance() {
    let grep = match real_grep_path() {
        Some(p) => p,
        None => return,
    };

    let tmp = TempDir::new().unwrap();
    create_corpus(tmp.path());
    let dir_str = tmp.path().to_str().unwrap();

    let real_args = &["-rc", "authenticate", dir_str];
    let bs_args = &["-rc", "authenticate", "."];

    let (rg_exit, rg_out, _) = run_real_grep(&grep, real_args);
    let (bs_exit, bs_out, _) = run_bitscout_grep(bs_args, tmp.path());

    assert_eq!(rg_exit, bs_exit);
    // grep -rc shows all files including 0-count ones; BitScout only shows matches.
    // Compare only non-zero count lines.
    let filter_nonzero = |output: &str, dir: &Path| -> BTreeSet<String> {
        normalize_lines(output, dir)
            .into_iter()
            .filter(|l| !l.ends_with(":0"))
            .collect()
    };
    let rg_lines = filter_nonzero(&rg_out, tmp.path());
    let bs_lines = filter_nonzero(&bs_out, tmp.path());
    assert_same_lines("grep -rc (non-zero)", &rg_lines, &bs_lines);
}

#[test]
fn test_grep_files_only_conformance() {
    let grep = match real_grep_path() {
        Some(p) => p,
        None => return,
    };

    let tmp = TempDir::new().unwrap();
    create_corpus(tmp.path());
    let dir_str = tmp.path().to_str().unwrap();

    let real_args = &["-rl", "authenticate", dir_str];
    let bs_args = &["-rl", "authenticate", "."];

    let (rg_exit, rg_out, _) = run_real_grep(&grep, real_args);
    let (bs_exit, bs_out, _) = run_bitscout_grep(bs_args, tmp.path());

    assert_eq!(rg_exit, bs_exit);
    let rg_lines = normalize_lines(&rg_out, tmp.path());
    let bs_lines = normalize_lines(&bs_out, tmp.path());
    assert_same_lines("grep -rl", &rg_lines, &bs_lines);
}

#[test]
fn test_grep_case_insensitive_conformance() {
    let grep = match real_grep_path() {
        Some(p) => p,
        None => return,
    };

    let tmp = TempDir::new().unwrap();
    create_corpus(tmp.path());
    let dir_str = tmp.path().to_str().unwrap();

    let real_args = &["-ri", "AUTHENTICATE", dir_str];
    let bs_args = &["-ri", "AUTHENTICATE", "."];

    let (rg_exit, rg_out, _) = run_real_grep(&grep, real_args);
    let (bs_exit, bs_out, _) = run_bitscout_grep(bs_args, tmp.path());

    assert_eq!(rg_exit, bs_exit);
    let rg_lines = normalize_lines(&rg_out, tmp.path());
    let bs_lines = normalize_lines(&bs_out, tmp.path());
    assert_same_lines("grep -ri", &rg_lines, &bs_lines);
}

#[test]
fn test_grep_no_match_conformance() {
    let grep = match real_grep_path() {
        Some(p) => p,
        None => return,
    };

    let tmp = TempDir::new().unwrap();
    create_corpus(tmp.path());
    let dir_str = tmp.path().to_str().unwrap();

    let real_args = &["-r", "zzz_no_match_zzz", dir_str];
    let bs_args = &["-r", "zzz_no_match_zzz", "."];

    let (rg_exit, _, _) = run_real_grep(&grep, real_args);
    let (bs_exit, bs_out, _) = run_bitscout_grep(bs_args, tmp.path());

    assert_eq!(rg_exit, 1);
    assert_eq!(bs_exit, 1);
    assert!(bs_out.is_empty());
}

#[test]
fn test_grep_include_glob_conformance() {
    let grep = match real_grep_path() {
        Some(p) => p,
        None => return,
    };

    let tmp = TempDir::new().unwrap();
    create_corpus(tmp.path());
    let dir_str = tmp.path().to_str().unwrap();

    let real_args = &["-r", "--include=*.rs", "authenticate", dir_str];
    let bs_args = &["-r", "--include=*.rs", "authenticate", "."];

    let (rg_exit, rg_out, _) = run_real_grep(&grep, real_args);
    let (bs_exit, bs_out, _) = run_bitscout_grep(bs_args, tmp.path());

    assert_eq!(rg_exit, bs_exit);
    let rg_lines = normalize_lines(&rg_out, tmp.path());
    let bs_lines = normalize_lines(&bs_out, tmp.path());
    assert_same_lines("grep --include=*.rs", &rg_lines, &bs_lines);
}

#[test]
fn test_grep_unsupported_flags_fallback() {
    let (bs_exit, _, bs_err) = run_bitscout_grep(&["-P", "pattern", "."], Path::new("/tmp"));
    assert_eq!(bs_exit, FALLBACK_EXIT_CODE);
    assert!(bs_err.contains("BITSCOUT_FALLBACK"));
}
