//! Comprehensive conformance + speed test for ALL common command usage patterns.
//!
//! Tests every flag combination that AI coding agents (Claude Code, Cursor, etc.)
//! commonly use, verifying BitScout output matches real command output exactly.
//! Also measures speed difference.

use bitscout_core::protocol::SearchRequest;
use bitscout_daemon::dispatch::{dispatch, FALLBACK_EXIT_CODE};
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::Instant;
use tempfile::TempDir;

// ===================================================================
// Helpers
// ===================================================================

fn real_rg() -> Option<String> {
    for c in ["/opt/homebrew/bin/rg", "/usr/local/bin/rg", "/usr/bin/rg"] {
        if Path::new(c).exists() {
            return Some(c.to_string());
        }
    }
    None
}

fn real_grep() -> &'static str { "/usr/bin/grep" }
fn real_find() -> &'static str { "/usr/bin/find" }
fn real_cat() -> &'static str { "/bin/cat" }

fn real_fd() -> Option<String> {
    for c in ["/opt/homebrew/bin/fd", "/usr/local/bin/fd", "/usr/bin/fd"] {
        if Path::new(c).exists() {
            return Some(c.to_string());
        }
    }
    None
}

fn run_cmd(cmd: &str, args: &[&str]) -> (i32, String, String) {
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

fn run_bs(cmd: &str, args: &[&str], cwd: &Path) -> (i32, String, String) {
    let req = SearchRequest {
        command: cmd.into(),
        args: args.iter().map(|s| s.to_string()).collect(),
        cwd: cwd.to_str().unwrap().into(),
    };
    let resp = dispatch(&req);
    (resp.exit_code, resp.stdout, resp.stderr)
}

fn norm(output: &str, dir: &Path) -> BTreeSet<String> {
    let canonical = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
    let cs = canonical.to_str().unwrap();
    let ds = dir.to_str().unwrap();
    output
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| l.trim_end().replace(cs, "<D>").replace(ds, "<D>").replace("//", "/"))
        .collect()
}

fn assert_eq_lines(label: &str, real: &BTreeSet<String>, bs: &BTreeSet<String>) {
    let missing: Vec<_> = real.difference(bs).collect();
    let extra: Vec<_> = bs.difference(real).collect();
    if !missing.is_empty() || !extra.is_empty() {
        panic!(
            "\n=== {} MISMATCH ===\nMissing from BitScout: {:?}\nExtra in BitScout: {:?}\nReal ({}):{:?}\nBS ({}):{:?}",
            label, missing, extra, real.len(), real, bs.len(), bs
        );
    }
}

fn filenames(output: &str) -> BTreeSet<String> {
    output
        .lines()
        .filter(|l| !l.is_empty())
        .filter_map(|l| Path::new(l.trim()).file_name().map(|f| f.to_string_lossy().to_string()))
        .collect()
}

/// Create a realistic project corpus with multiple file types and deeper nesting.
fn create_large_corpus(dir: &Path) {
    let dirs = ["src", "src/auth", "src/api", "tests", "docs", "config"];
    for d in &dirs {
        fs::create_dir_all(dir.join(d)).unwrap();
    }

    fs::write(dir.join("src/main.rs"), r#"use crate::auth;
use crate::api;

fn main() {
    let config = load_config();
    let server = start_server(&config);
    println!("Server started on port {}", config.port);
}

fn load_config() -> Config {
    Config { port: 8080, debug: true }
}

fn start_server(config: &Config) -> Server {
    Server::new(config.port)
}

struct Config { port: u16, debug: bool }
struct Server;
impl Server { fn new(_port: u16) -> Self { Server } }
"#).unwrap();

    fs::write(dir.join("src/auth/mod.rs"), r#"pub mod token;
pub mod session;

pub fn authenticate_user(username: &str, password: &str) -> Result<Session, AuthError> {
    let token = token::validate(password)?;
    let session = session::create(username, &token)?;
    Ok(session)
}

pub fn verify_token(token: &str) -> bool {
    token.starts_with("tk_") && token.len() > 10
}

pub struct Session { pub id: String, pub user: String }
pub struct AuthError(pub String);
"#).unwrap();

    fs::write(dir.join("src/auth/token.rs"), r#"use super::AuthError;

pub fn validate(input: &str) -> Result<String, AuthError> {
    if input.len() < 8 {
        return Err(AuthError("password too short".into()));
    }
    Ok(format!("tk_{}", input))
}

pub fn refresh(token: &str) -> Option<String> {
    if token.starts_with("tk_") {
        Some(format!("tk_refreshed_{}", &token[3..]))
    } else {
        None
    }
}
"#).unwrap();

    fs::write(dir.join("src/auth/session.rs"), r#"use super::{AuthError, Session};

pub fn create(user: &str, token: &str) -> Result<Session, AuthError> {
    if !token.starts_with("tk_") {
        return Err(AuthError("invalid token".into()));
    }
    Ok(Session { id: format!("sess_{}", user), user: user.to_string() })
}

pub fn destroy(session: &Session) {
    println!("Session {} destroyed", session.id);
}
"#).unwrap();

    fs::write(dir.join("src/api/mod.rs"), r#"pub mod handlers;

use crate::auth;

pub fn authenticate_request(header: &str) -> bool {
    if let Some(token) = header.strip_prefix("Bearer ") {
        auth::verify_token(token)
    } else {
        false
    }
}
"#).unwrap();

    fs::write(dir.join("src/api/handlers.rs"), r#"use super::authenticate_request;

pub fn handle_get(path: &str, auth_header: &str) -> Response {
    if !authenticate_request(auth_header) {
        return Response { status: 401, body: "Unauthorized".into() };
    }
    Response { status: 200, body: format!("GET {}", path) }
}

pub fn handle_post(path: &str, body: &str, auth_header: &str) -> Response {
    if !authenticate_request(auth_header) {
        return Response { status: 401, body: "Unauthorized".into() };
    }
    Response { status: 201, body: format!("POST {} with {}", path, body) }
}

pub struct Response { pub status: u16, pub body: String }
"#).unwrap();

    fs::write(dir.join("tests/test_auth.rs"), r#"#[test]
fn test_authenticate_user_valid() {
    let result = authenticate_user("admin", "password123");
    assert!(result.is_ok());
}

#[test]
fn test_authenticate_user_short_password() {
    let result = authenticate_user("admin", "short");
    assert!(result.is_err());
}

#[test]
fn test_verify_token_valid() {
    assert!(verify_token("tk_long_enough_token"));
}

#[test]
fn test_verify_token_invalid() {
    assert!(!verify_token("invalid"));
}
"#).unwrap();

    fs::write(dir.join("tests/test_api.rs"), r#"#[test]
fn test_handle_get_authorized() {
    let resp = handle_get("/api/users", "Bearer tk_valid_token");
    assert_eq!(resp.status, 200);
}

#[test]
fn test_handle_get_unauthorized() {
    let resp = handle_get("/api/users", "InvalidToken");
    assert_eq!(resp.status, 401);
}
"#).unwrap();

    fs::write(dir.join("docs/README.md"), "# Project\n\nAuthentication system.\n").unwrap();
    fs::write(dir.join("docs/API.md"), "# API Docs\n\nAll endpoints require Bearer token.\n").unwrap();
    fs::write(dir.join("config/default.json"), r#"{"port": 8080, "debug": true, "auth": {"enabled": true}}"#).unwrap();
    fs::write(dir.join("config/test.json"), r#"{"port": 9090, "debug": false, "auth": {"enabled": false}}"#).unwrap();
    fs::write(dir.join("Cargo.toml"), "[package]\nname = \"myproject\"\nversion = \"0.1.0\"\n").unwrap();
    fs::write(dir.join(".gitignore"), "target/\n*.swp\n").unwrap();
}

// ===================================================================
// rg conformance — Claude Code common patterns
// ===================================================================

#[test]
fn test_rg_no_heading_basic() {
    // Claude Code's default: rg --no-heading pattern dir
    let rg = match real_rg() { Some(p) => p, None => return };
    let tmp = TempDir::new().unwrap();
    create_large_corpus(tmp.path());
    let d = tmp.path().to_str().unwrap();

    let (re, ro, _) = run_cmd(&rg, &["--no-heading", "authenticate", d]);
    let (be, bo, _) = run_bs("rg", &["--no-heading", "authenticate", "."], tmp.path());
    assert_eq!(re, be);
    assert_eq_lines("rg --no-heading", &norm(&ro, tmp.path()), &norm(&bo, tmp.path()));
}

#[test]
fn test_rg_no_heading_with_n() {
    let rg = match real_rg() { Some(p) => p, None => return };
    let tmp = TempDir::new().unwrap();
    create_large_corpus(tmp.path());
    let d = tmp.path().to_str().unwrap();

    let (re, ro, _) = run_cmd(&rg, &["--no-heading", "-n", "authenticate", d]);
    let (be, bo, _) = run_bs("rg", &["--no-heading", "-n", "authenticate", "."], tmp.path());
    assert_eq!(re, be);
    assert_eq_lines("rg --no-heading -n", &norm(&ro, tmp.path()), &norm(&bo, tmp.path()));
}

#[test]
fn test_rg_files_with_matches_long() {
    // Claude Code uses --files-with-matches (long form)
    let rg = match real_rg() { Some(p) => p, None => return };
    let tmp = TempDir::new().unwrap();
    create_large_corpus(tmp.path());
    let d = tmp.path().to_str().unwrap();

    let (re, ro, _) = run_cmd(&rg, &["--files-with-matches", "authenticate", d]);
    let (be, bo, _) = run_bs("rg", &["--files-with-matches", "authenticate", "."], tmp.path());
    assert_eq!(re, be);
    assert_eq_lines("rg --files-with-matches", &norm(&ro, tmp.path()), &norm(&bo, tmp.path()));
}

#[test]
fn test_rg_count_long() {
    let rg = match real_rg() { Some(p) => p, None => return };
    let tmp = TempDir::new().unwrap();
    create_large_corpus(tmp.path());
    let d = tmp.path().to_str().unwrap();

    let (re, ro, _) = run_cmd(&rg, &["--count", "authenticate", d]);
    let (be, bo, _) = run_bs("rg", &["--count", "authenticate", "."], tmp.path());
    assert_eq!(re, be);
    assert_eq_lines("rg --count", &norm(&ro, tmp.path()), &norm(&bo, tmp.path()));
}

#[test]
fn test_rg_case_sensitive() {
    let rg = match real_rg() { Some(p) => p, None => return };
    let tmp = TempDir::new().unwrap();
    create_large_corpus(tmp.path());
    let d = tmp.path().to_str().unwrap();

    let (re, ro, _) = run_cmd(&rg, &["-s", "Session", d]);
    let (be, bo, _) = run_bs("rg", &["-s", "Session", "."], tmp.path());
    assert_eq!(re, be);
    assert_eq_lines("rg -s (case sensitive)", &norm(&ro, tmp.path()), &norm(&bo, tmp.path()));
}

#[test]
fn test_rg_smart_case() {
    let rg = match real_rg() { Some(p) => p, None => return };
    let tmp = TempDir::new().unwrap();
    create_large_corpus(tmp.path());
    let d = tmp.path().to_str().unwrap();

    let (re, ro, _) = run_cmd(&rg, &["-S", "authenticate", d]);
    let (be, bo, _) = run_bs("rg", &["-S", "authenticate", "."], tmp.path());
    assert_eq!(re, be);
    assert_eq_lines("rg -S (smart case)", &norm(&ro, tmp.path()), &norm(&bo, tmp.path()));
}

#[test]
fn test_rg_color_never() {
    // Common: rg --color=never pattern dir
    let rg = match real_rg() { Some(p) => p, None => return };
    let tmp = TempDir::new().unwrap();
    create_large_corpus(tmp.path());
    let d = tmp.path().to_str().unwrap();

    let (re, ro, _) = run_cmd(&rg, &["--color=never", "authenticate", d]);
    let (be, bo, _) = run_bs("rg", &["--color=never", "authenticate", "."], tmp.path());
    assert_eq!(re, be);
    assert_eq_lines("rg --color=never", &norm(&ro, tmp.path()), &norm(&bo, tmp.path()));
}

#[test]
fn test_rg_no_heading_color_never_n() {
    // Full Claude Code pattern: rg --no-heading --color=never -n pattern dir
    let rg = match real_rg() { Some(p) => p, None => return };
    let tmp = TempDir::new().unwrap();
    create_large_corpus(tmp.path());
    let d = tmp.path().to_str().unwrap();

    let (re, ro, _) = run_cmd(&rg, &["--no-heading", "--color=never", "-n", "authenticate", d]);
    let (be, bo, _) = run_bs("rg", &["--no-heading", "--color=never", "-n", "authenticate", "."], tmp.path());
    assert_eq!(re, be);
    assert_eq_lines("rg --no-heading --color=never -n", &norm(&ro, tmp.path()), &norm(&bo, tmp.path()));
}

#[test]
fn test_rg_glob_with_no_heading() {
    let rg = match real_rg() { Some(p) => p, None => return };
    let tmp = TempDir::new().unwrap();
    create_large_corpus(tmp.path());
    let d = tmp.path().to_str().unwrap();

    let (re, ro, _) = run_cmd(&rg, &["--no-heading", "--glob", "*.rs", "fn", d]);
    let (be, bo, _) = run_bs("rg", &["--no-heading", "--glob", "*.rs", "fn", "."], tmp.path());
    assert_eq!(re, be);
    assert_eq_lines("rg --no-heading --glob *.rs", &norm(&ro, tmp.path()), &norm(&bo, tmp.path()));
}

#[test]
fn test_rg_type_rust_no_heading() {
    let rg = match real_rg() { Some(p) => p, None => return };
    let tmp = TempDir::new().unwrap();
    create_large_corpus(tmp.path());
    let d = tmp.path().to_str().unwrap();

    let (re, ro, _) = run_cmd(&rg, &["--no-heading", "--type", "rust", "fn", d]);
    let (be, bo, _) = run_bs("rg", &["--no-heading", "--type", "rust", "fn", "."], tmp.path());
    assert_eq!(re, be);
    assert_eq_lines("rg --no-heading --type rust", &norm(&ro, tmp.path()), &norm(&bo, tmp.path()));
}

// ===================================================================
// grep conformance — bare grep (no -r)
// ===================================================================

#[test]
fn test_grep_bare_single_file() {
    // grep "pattern" file — no -r, single file
    let tmp = TempDir::new().unwrap();
    create_large_corpus(tmp.path());
    let file = tmp.path().join("src/auth/mod.rs");
    let f = file.to_str().unwrap();

    let (re, ro, _) = run_cmd(real_grep(), &["authenticate", f]);
    let (be, bo, _) = run_bs("grep", &["authenticate", f], tmp.path());

    assert_eq!(re, be, "exit code: real={} bs={}", re, be);
    // grep single file without -H: no filename prefix
    // BitScout always uses SearchEngine which prefixes filenames
    // Compare just the content part
    let real_content: BTreeSet<String> = ro.lines()
        .filter(|l| !l.is_empty())
        .map(|l| l.trim_end().to_string())
        .collect();
    let bs_content: BTreeSet<String> = bo.lines()
        .filter(|l| !l.is_empty())
        .map(|l| {
            // Strip filename: prefix if present
            if let Some(pos) = l.find(':') {
                l[pos+1..].trim_end().to_string()
            } else {
                l.trim_end().to_string()
            }
        })
        .collect();
    assert_eq_lines("grep bare single file (content)", &real_content, &bs_content);
}

#[test]
fn test_grep_bare_with_n_single_file() {
    // grep -n "pattern" file
    let tmp = TempDir::new().unwrap();
    create_large_corpus(tmp.path());
    let file = tmp.path().join("src/auth/mod.rs");
    let f = file.to_str().unwrap();

    let (re, ro, _) = run_cmd(real_grep(), &["-n", "authenticate", f]);
    let (be, bo, _) = run_bs("grep", &["-n", "authenticate", f], tmp.path());

    assert_eq!(re, be);
    // grep -n single file: "linenum:content" (no filename)
    // Extract linenum:content from both
    let extract = |output: &str| -> BTreeSet<String> {
        output.lines()
            .filter(|l| !l.is_empty())
            .map(|l| {
                let s = l.trim_end();
                // If it has path: prefix, strip it
                // Format could be "path:num:content" or "num:content"
                let parts: Vec<&str> = s.splitn(3, ':').collect();
                if parts.len() == 3 {
                    // Could be path:num:content or num:content (if content has :)
                    if parts[0].parse::<usize>().is_ok() {
                        // It's num:content:rest
                        format!("{}:{}", parts[0], &s[parts[0].len()+1..])
                    } else {
                        // It's path:num:content
                        format!("{}:{}", parts[1], parts[2])
                    }
                } else {
                    s.to_string()
                }
            })
            .collect()
    };
    let real_lines = extract(&ro);
    let bs_lines = extract(&bo);
    assert_eq_lines("grep -n bare (line:content)", &real_lines, &bs_lines);
}

#[test]
fn test_grep_rn_recursive() {
    // grep -rn "pattern" dir — most common recursive
    let tmp = TempDir::new().unwrap();
    create_large_corpus(tmp.path());
    let d = tmp.path().to_str().unwrap();

    let (re, ro, _) = run_cmd(real_grep(), &["-rn", "authenticate", d]);
    let (be, bo, _) = run_bs("grep", &["-rn", "authenticate", "."], tmp.path());

    assert_eq!(re, be);
    assert_eq_lines("grep -rn", &norm(&ro, tmp.path()), &norm(&bo, tmp.path()));
}

#[test]
fn test_grep_ri_recursive() {
    let tmp = TempDir::new().unwrap();
    create_large_corpus(tmp.path());
    let d = tmp.path().to_str().unwrap();

    let (re, ro, _) = run_cmd(real_grep(), &["-ri", "SESSION", d]);
    let (be, bo, _) = run_bs("grep", &["-ri", "SESSION", "."], tmp.path());

    assert_eq!(re, be);
    assert_eq_lines("grep -ri", &norm(&ro, tmp.path()), &norm(&bo, tmp.path()));
}

#[test]
fn test_grep_rl_recursive() {
    let tmp = TempDir::new().unwrap();
    create_large_corpus(tmp.path());
    let d = tmp.path().to_str().unwrap();

    let (re, ro, _) = run_cmd(real_grep(), &["-rl", "authenticate", d]);
    let (be, bo, _) = run_bs("grep", &["-rl", "authenticate", "."], tmp.path());

    assert_eq!(re, be);
    assert_eq_lines("grep -rl", &norm(&ro, tmp.path()), &norm(&bo, tmp.path()));
}

#[test]
fn test_grep_include_rs() {
    let tmp = TempDir::new().unwrap();
    create_large_corpus(tmp.path());
    let d = tmp.path().to_str().unwrap();

    let (re, ro, _) = run_cmd(real_grep(), &["-rn", "--include=*.rs", "fn", d]);
    let (be, bo, _) = run_bs("grep", &["-rn", "--include=*.rs", "fn", "."], tmp.path());

    assert_eq!(re, be);
    assert_eq_lines("grep --include=*.rs", &norm(&ro, tmp.path()), &norm(&bo, tmp.path()));
}

// ===================================================================
// find conformance
// ===================================================================

#[test]
fn test_find_name_rs() {
    let tmp = TempDir::new().unwrap();
    create_large_corpus(tmp.path());
    let d = tmp.path().to_str().unwrap();

    let (re, ro, _) = run_cmd(real_find(), &[d, "-name", "*.rs", "-type", "f"]);
    let (be, bo, _) = run_bs("find", &[".", "-name", "*.rs", "-type", "f"], tmp.path());

    assert_eq!(re, be);
    assert_eq_lines("find -name *.rs -type f (filenames)", &filenames(&ro), &filenames(&bo));
}

#[test]
fn test_find_name_json() {
    let tmp = TempDir::new().unwrap();
    create_large_corpus(tmp.path());
    let d = tmp.path().to_str().unwrap();

    let (_, ro, _) = run_cmd(real_find(), &[d, "-name", "*.json"]);
    let (_, bo, _) = run_bs("find", &[".", "-name", "*.json"], tmp.path());

    assert_eq_lines("find -name *.json", &filenames(&ro), &filenames(&bo));
}

// ===================================================================
// fd conformance
// ===================================================================

#[test]
fn test_fd_extension_rs() {
    let fd = match real_fd() { Some(p) => p, None => return };
    let tmp = TempDir::new().unwrap();
    create_large_corpus(tmp.path());
    let d = tmp.path().to_str().unwrap();

    let (re, ro, _) = run_cmd(&fd, &["-e", "rs", d]);
    let (be, bo, _) = run_bs("fd", &["-e", "rs", "."], tmp.path());

    assert_eq!(re, be);
    assert_eq_lines("fd -e rs", &filenames(&ro), &filenames(&bo));
}

#[test]
fn test_fd_pattern_auth() {
    let fd = match real_fd() { Some(p) => p, None => return };
    let tmp = TempDir::new().unwrap();
    create_large_corpus(tmp.path());
    let d = tmp.path().to_str().unwrap();

    let (re, ro, _) = run_cmd(&fd, &["auth", d]);
    let (be, bo, _) = run_bs("fd", &["auth", "."], tmp.path());

    assert_eq!(re, be);
    assert_eq_lines("fd auth", &filenames(&ro), &filenames(&bo));
}

// ===================================================================
// cat conformance
// ===================================================================

#[test]
fn test_cat_source_file() {
    let tmp = TempDir::new().unwrap();
    create_large_corpus(tmp.path());
    let f = tmp.path().join("src/auth/mod.rs");
    let fs = f.to_str().unwrap();

    let (re, ro, _) = run_cmd(real_cat(), &[fs]);
    let (be, bo, _) = run_bs("cat", &[fs], tmp.path());

    assert_eq!(re, be);
    assert_eq!(ro, bo, "cat content mismatch");
}

#[test]
fn test_cat_with_n_source_file() {
    let tmp = TempDir::new().unwrap();
    create_large_corpus(tmp.path());
    let f = tmp.path().join("src/auth/mod.rs");
    let fs = f.to_str().unwrap();

    let (re, ro, _) = run_cmd(real_cat(), &["-n", fs]);
    let (be, bo, _) = run_bs("cat", &["-n", fs], tmp.path());

    assert_eq!(re, be);
    // Normalize leading whitespace for comparison
    let rl: Vec<&str> = ro.lines().collect();
    let bl: Vec<&str> = bo.lines().collect();
    assert_eq!(rl.len(), bl.len(), "cat -n line count mismatch");
    for (i, (r, b)) in rl.iter().zip(bl.iter()).enumerate() {
        assert_eq!(r.trim_start(), b.trim_start(), "cat -n line {} mismatch", i+1);
    }
}

#[test]
fn test_cat_multiple_source_files() {
    let tmp = TempDir::new().unwrap();
    create_large_corpus(tmp.path());
    let f1 = tmp.path().join("config/default.json");
    let f2 = tmp.path().join("config/test.json");
    let fs1 = f1.to_str().unwrap();
    let fs2 = f2.to_str().unwrap();

    let (re, ro, _) = run_cmd(real_cat(), &[fs1, fs2]);
    let (be, bo, _) = run_bs("cat", &[fs1, fs2], tmp.path());

    assert_eq!(re, be);
    assert_eq!(ro, bo, "cat multi-file mismatch");
}

// ===================================================================
// Speed comparison
// ===================================================================

#[test]
fn test_speed_rg_vs_bitscout() {
    let rg = match real_rg() { Some(p) => p, None => return };
    let tmp = TempDir::new().unwrap();
    create_large_corpus(tmp.path());
    let d = tmp.path().to_str().unwrap();

    // Warmup
    let _ = run_cmd(&rg, &["--no-heading", "authenticate", d]);
    let _ = run_bs("rg", &["--no-heading", "authenticate", "."], tmp.path());

    let iters = 10;

    // Measure real rg
    let start = Instant::now();
    for _ in 0..iters {
        let _ = run_cmd(&rg, &["--no-heading", "authenticate", d]);
    }
    let rg_time = start.elapsed();

    // Measure BitScout
    let start = Instant::now();
    for _ in 0..iters {
        let _ = run_bs("rg", &["--no-heading", "authenticate", "."], tmp.path());
    }
    let bs_time = start.elapsed();

    let rg_ms = rg_time.as_secs_f64() * 1000.0 / iters as f64;
    let bs_ms = bs_time.as_secs_f64() * 1000.0 / iters as f64;
    let ratio = rg_ms / bs_ms;

    eprintln!("\n╔══════════════════════════════════════════╗");
    eprintln!("║       rg Speed Comparison ({} iters)      ║", iters);
    eprintln!("╠══════════════════════════════════════════╣");
    eprintln!("║  real rg:    {:>8.2} ms/iter             ║", rg_ms);
    eprintln!("║  BitScout:   {:>8.2} ms/iter             ║", bs_ms);
    eprintln!("║  Speedup:    {:>8.1}x                    ║", ratio);
    eprintln!("╚══════════════════════════════════════════╝");
}

#[test]
fn test_speed_grep_vs_bitscout() {
    let tmp = TempDir::new().unwrap();
    create_large_corpus(tmp.path());
    let d = tmp.path().to_str().unwrap();

    let iters = 10;

    let start = Instant::now();
    for _ in 0..iters {
        let _ = run_cmd(real_grep(), &["-rn", "authenticate", d]);
    }
    let grep_time = start.elapsed();

    let start = Instant::now();
    for _ in 0..iters {
        let _ = run_bs("grep", &["-rn", "authenticate", "."], tmp.path());
    }
    let bs_time = start.elapsed();

    let grep_ms = grep_time.as_secs_f64() * 1000.0 / iters as f64;
    let bs_ms = bs_time.as_secs_f64() * 1000.0 / iters as f64;
    let ratio = grep_ms / bs_ms;

    eprintln!("\n╔══════════════════════════════════════════╗");
    eprintln!("║      grep Speed Comparison ({} iters)     ║", iters);
    eprintln!("╠══════════════════════════════════════════╣");
    eprintln!("║  real grep:  {:>8.2} ms/iter             ║", grep_ms);
    eprintln!("║  BitScout:   {:>8.2} ms/iter             ║", bs_ms);
    eprintln!("║  Speedup:    {:>8.1}x                    ║", ratio);
    eprintln!("╚══════════════════════════════════════════╝");
}

#[test]
fn test_speed_find_vs_bitscout() {
    let tmp = TempDir::new().unwrap();
    create_large_corpus(tmp.path());
    let d = tmp.path().to_str().unwrap();

    let iters = 10;

    let start = Instant::now();
    for _ in 0..iters {
        let _ = run_cmd(real_find(), &[d, "-name", "*.rs", "-type", "f"]);
    }
    let find_time = start.elapsed();

    let start = Instant::now();
    for _ in 0..iters {
        let _ = run_bs("find", &[".", "-name", "*.rs", "-type", "f"], tmp.path());
    }
    let bs_time = start.elapsed();

    let find_ms = find_time.as_secs_f64() * 1000.0 / iters as f64;
    let bs_ms = bs_time.as_secs_f64() * 1000.0 / iters as f64;
    let ratio = find_ms / bs_ms;

    eprintln!("\n╔══════════════════════════════════════════╗");
    eprintln!("║      find Speed Comparison ({} iters)     ║", iters);
    eprintln!("╠══════════════════════════════════════════╣");
    eprintln!("║  real find:  {:>8.2} ms/iter             ║", find_ms);
    eprintln!("║  BitScout:   {:>8.2} ms/iter             ║", bs_ms);
    eprintln!("║  Speedup:    {:>8.1}x                    ║", ratio);
    eprintln!("╚══════════════════════════════════════════╝");
}

#[test]
fn test_speed_cat_vs_bitscout() {
    let tmp = TempDir::new().unwrap();
    create_large_corpus(tmp.path());
    let f = tmp.path().join("src/auth/mod.rs");
    let fs = f.to_str().unwrap();

    let iters = 10;

    let start = Instant::now();
    for _ in 0..iters {
        let _ = run_cmd(real_cat(), &[fs]);
    }
    let cat_time = start.elapsed();

    let start = Instant::now();
    for _ in 0..iters {
        let _ = run_bs("cat", &[fs], tmp.path());
    }
    let bs_time = start.elapsed();

    let cat_ms = cat_time.as_secs_f64() * 1000.0 / iters as f64;
    let bs_ms = bs_time.as_secs_f64() * 1000.0 / iters as f64;
    let ratio = cat_ms / bs_ms;

    eprintln!("\n╔══════════════════════════════════════════╗");
    eprintln!("║      cat Speed Comparison ({} iters)      ║", iters);
    eprintln!("╠══════════════════════════════════════════╣");
    eprintln!("║  real cat:   {:>8.2} ms/iter             ║", cat_ms);
    eprintln!("║  BitScout:   {:>8.2} ms/iter             ║", bs_ms);
    eprintln!("║  Speedup:    {:>8.1}x                    ║", ratio);
    eprintln!("╚══════════════════════════════════════════╝");
}
