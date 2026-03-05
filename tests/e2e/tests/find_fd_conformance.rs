//! find/fd conformance tests: compare real `find`/`fd` output with BitScout dispatch.

use bitscout_core::dispatch::{dispatch, FALLBACK_EXIT_CODE};
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

fn real_find_path() -> Option<String> {
    for c in ["/usr/bin/find", "/opt/homebrew/bin/find"] {
        if Path::new(c).exists() {
            return Some(c.to_string());
        }
    }
    None
}

fn real_fd_path() -> Option<String> {
    for c in ["/opt/homebrew/bin/fd", "/usr/local/bin/fd", "/usr/bin/fd"] {
        if Path::new(c).exists() {
            return Some(c.to_string());
        }
    }
    // Try which
    let output = Command::new("sh")
        .args(["-c", "which -a fd 2>/dev/null || true"])
        .output()
        .ok()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        let line = line.trim();
        if !line.is_empty() && !line.contains(".bitscout") {
            if Path::new(line).exists() {
                return Some(line.to_string());
            }
        }
    }
    None
}

fn run_real_cmd(cmd: &str, args: &[&str]) -> (i32, String, String) {
    let output = Command::new(cmd)
        .args(args)
        .output()
        .expect(&format!("Failed to run {}", cmd));
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
    )
}

fn run_bitscout(cmd: &str, args: &[&str], cwd: &Path) -> (i32, String, String) {
    let cwd_str = cwd.to_str().unwrap();
    let args_owned: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    let resp = dispatch(cmd, &args_owned, cwd_str);
    (resp.exit_code, resp.stdout, resp.stderr)
}

fn normalize_find_lines(output: &str, dir: &Path) -> BTreeSet<String> {
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
            msg.push_str("Only in real:\n");
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
    let tests = dir.join("tests");
    fs::create_dir_all(&tests).unwrap();
    let docs = dir.join("docs");
    fs::create_dir_all(&docs).unwrap();

    fs::write(src.join("main.rs"), "fn main() {}\n").unwrap();
    fs::write(src.join("lib.rs"), "pub mod utils;\n").unwrap();
    fs::write(src.join("utils.rs"), "pub fn helper() {}\n").unwrap();
    fs::write(tests.join("test_main.rs"), "#[test]\nfn test() {}\n").unwrap();
    fs::write(docs.join("guide.md"), "# Guide\n").unwrap();
    fs::write(dir.join("README.md"), "# README\n").unwrap();
    fs::write(dir.join("config.json"), "{}\n").unwrap();
}

// ========================== find tests ==========================

#[test]
fn test_find_name_glob_conformance() {
    let find = match real_find_path() {
        Some(p) => p,
        None => return,
    };

    let tmp = TempDir::new().unwrap();
    create_corpus(tmp.path());
    let dir_str = tmp.path().to_str().unwrap();

    // find <dir> -name "*.rs"
    let (real_exit, real_out, _) = run_real_cmd(&find, &[dir_str, "-name", "*.rs"]);
    let (bs_exit, bs_out, _) = run_bitscout("find", &[".", "-name", "*.rs"], tmp.path());

    assert_eq!(real_exit, bs_exit);

    // Real find uses absolute paths when given absolute dir; our find uses relative.
    // Compare just filenames to verify same set of files found.
    let extract_filenames = |output: &str| -> BTreeSet<String> {
        output
            .lines()
            .filter(|l| !l.is_empty())
            .filter_map(|l| {
                Path::new(l.trim())
                    .file_name()
                    .map(|f| f.to_string_lossy().to_string())
            })
            .collect()
    };

    let real_files = extract_filenames(&real_out);
    let bs_files = extract_filenames(&bs_out);
    assert_same_lines("find -name *.rs (filenames)", &real_files, &bs_files);
}

#[test]
fn test_find_type_f_conformance() {
    let find = match real_find_path() {
        Some(p) => p,
        None => return,
    };

    let tmp = TempDir::new().unwrap();
    create_corpus(tmp.path());
    let dir_str = tmp.path().to_str().unwrap();

    // find <dir> -type f — all regular files
    let (real_exit, real_out, _) = run_real_cmd(&find, &[dir_str, "-type", "f"]);
    let (bs_exit, bs_out, _) = run_bitscout("find", &[".", "-type", "f"], tmp.path());

    assert_eq!(real_exit, bs_exit);

    let extract_filenames = |output: &str| -> BTreeSet<String> {
        output
            .lines()
            .filter(|l| !l.is_empty())
            .filter_map(|l| {
                Path::new(l.trim())
                    .file_name()
                    .map(|f| f.to_string_lossy().to_string())
            })
            .collect()
    };

    let real_files = extract_filenames(&real_out);
    let bs_files = extract_filenames(&bs_out);
    assert_same_lines("find -type f (filenames)", &real_files, &bs_files);
}

#[test]
fn test_find_type_d_conformance() {
    let find = match real_find_path() {
        Some(p) => p,
        None => return,
    };

    let tmp = TempDir::new().unwrap();
    create_corpus(tmp.path());
    let dir_str = tmp.path().to_str().unwrap();

    // find <dir> -type d
    let (real_exit, real_out, _) = run_real_cmd(&find, &[dir_str, "-type", "d"]);
    let (bs_exit, bs_out, _) = run_bitscout("find", &[".", "-type", "d"], tmp.path());

    assert_eq!(real_exit, bs_exit);

    let extract_names = |output: &str| -> BTreeSet<String> {
        output
            .lines()
            .filter(|l| !l.is_empty())
            .filter_map(|l| {
                let p = Path::new(l.trim());
                p.file_name().map(|f| f.to_string_lossy().to_string())
            })
            .collect()
    };

    let real_dirs = extract_names(&real_out);
    let bs_dirs = extract_names(&bs_out);
    // BitScout may not include the root dir itself; filter it out from real find
    let real_filtered: BTreeSet<String> = real_dirs
        .into_iter()
        .filter(|n| {
            // Real find includes the root dir; skip temp dir name
            let root_name = tmp
                .path()
                .file_name()
                .unwrap()
                .to_string_lossy()
                .to_string();
            n != &root_name
        })
        .collect();
    assert_same_lines("find -type d (dir names)", &real_filtered, &bs_dirs);
}

#[test]
fn test_find_combined_name_type_conformance() {
    let find = match real_find_path() {
        Some(p) => p,
        None => return,
    };

    let tmp = TempDir::new().unwrap();
    create_corpus(tmp.path());
    let dir_str = tmp.path().to_str().unwrap();

    let (real_exit, real_out, _) = run_real_cmd(&find, &[dir_str, "-name", "*.rs", "-type", "f"]);
    let (bs_exit, bs_out, _) =
        run_bitscout("find", &[".", "-name", "*.rs", "-type", "f"], tmp.path());

    assert_eq!(real_exit, bs_exit);

    let extract_filenames = |output: &str| -> BTreeSet<String> {
        output
            .lines()
            .filter(|l| !l.is_empty())
            .filter_map(|l| {
                Path::new(l.trim())
                    .file_name()
                    .map(|f| f.to_string_lossy().to_string())
            })
            .collect()
    };

    let real_files = extract_filenames(&real_out);
    let bs_files = extract_filenames(&bs_out);
    assert_same_lines("find -name *.rs -type f", &real_files, &bs_files);
}

#[test]
fn test_find_unsupported_flags_fallback() {
    let (exit, _, err) = run_bitscout(
        "find",
        &[".", "-exec", "echo", "{}", ";"],
        Path::new("/tmp"),
    );
    assert_eq!(exit, FALLBACK_EXIT_CODE);
    assert!(err.contains("BITSCOUT_FALLBACK"));
}

// ========================== fd tests ==========================

#[test]
fn test_fd_basic_conformance() {
    let fd = match real_fd_path() {
        Some(p) => p,
        None => {
            eprintln!("SKIP: fd not found");
            return;
        }
    };

    let tmp = TempDir::new().unwrap();
    create_corpus(tmp.path());
    let dir_str = tmp.path().to_str().unwrap();

    // fd "main" <dir>
    let (real_exit, real_out, _) = run_real_cmd(&fd, &["main", dir_str]);
    let (bs_exit, bs_out, _) = run_bitscout("fd", &["main", "."], tmp.path());

    assert_eq!(real_exit, bs_exit);

    let extract_filenames = |output: &str| -> BTreeSet<String> {
        output
            .lines()
            .filter(|l| !l.is_empty())
            .filter_map(|l| {
                Path::new(l.trim())
                    .file_name()
                    .map(|f| f.to_string_lossy().to_string())
            })
            .collect()
    };

    let real_files = extract_filenames(&real_out);
    let bs_files = extract_filenames(&bs_out);
    assert_same_lines("fd basic", &real_files, &bs_files);
}

#[test]
fn test_fd_extension_conformance() {
    let fd = match real_fd_path() {
        Some(p) => p,
        None => return,
    };

    let tmp = TempDir::new().unwrap();
    create_corpus(tmp.path());
    let dir_str = tmp.path().to_str().unwrap();

    // fd -e rs <dir>
    let (real_exit, real_out, _) = run_real_cmd(&fd, &["-e", "rs", dir_str]);
    let (bs_exit, bs_out, _) = run_bitscout("fd", &["-e", "rs", "."], tmp.path());

    assert_eq!(real_exit, bs_exit);

    let extract_filenames = |output: &str| -> BTreeSet<String> {
        output
            .lines()
            .filter(|l| !l.is_empty())
            .filter_map(|l| {
                Path::new(l.trim())
                    .file_name()
                    .map(|f| f.to_string_lossy().to_string())
            })
            .collect()
    };

    let real_files = extract_filenames(&real_out);
    let bs_files = extract_filenames(&bs_out);
    assert_same_lines("fd -e rs", &real_files, &bs_files);
}

#[test]
fn test_fd_type_file_conformance() {
    let fd = match real_fd_path() {
        Some(p) => p,
        None => return,
    };

    let tmp = TempDir::new().unwrap();
    create_corpus(tmp.path());
    let dir_str = tmp.path().to_str().unwrap();

    // fd -t f <dir>
    let (real_exit, real_out, _) = run_real_cmd(&fd, &["-t", "f", dir_str]);
    let (bs_exit, bs_out, _) = run_bitscout("fd", &["-t", "f", "."], tmp.path());

    assert_eq!(real_exit, bs_exit);

    let extract_filenames = |output: &str| -> BTreeSet<String> {
        output
            .lines()
            .filter(|l| !l.is_empty())
            .filter_map(|l| {
                Path::new(l.trim())
                    .file_name()
                    .map(|f| f.to_string_lossy().to_string())
            })
            .collect()
    };

    let real_files = extract_filenames(&real_out);
    let bs_files = extract_filenames(&bs_out);
    assert_same_lines("fd -t f (filenames)", &real_files, &bs_files);
}

#[test]
fn test_fd_no_match_conformance() {
    let fd = match real_fd_path() {
        Some(p) => p,
        None => return,
    };

    let tmp = TempDir::new().unwrap();
    create_corpus(tmp.path());
    let dir_str = tmp.path().to_str().unwrap();

    let (real_exit, real_out, _) = run_real_cmd(&fd, &["zzz_no_match_zzz", dir_str]);
    let (bs_exit, bs_out, _) = run_bitscout("fd", &["zzz_no_match_zzz", "."], tmp.path());

    assert_eq!(real_exit, 1);
    assert_eq!(bs_exit, 1);
    assert!(real_out.trim().is_empty());
    assert!(bs_out.trim().is_empty());
}

#[test]
fn test_fd_unsupported_flags_fallback() {
    let (exit, _, err) = run_bitscout("fd", &["--exec", "echo", "pattern"], Path::new("/tmp"));
    assert_eq!(exit, FALLBACK_EXIT_CODE);
    assert!(err.contains("BITSCOUT_FALLBACK"));
}
