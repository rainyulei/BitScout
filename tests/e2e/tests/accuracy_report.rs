//! Accuracy report: BitScout (literal-only) vs BitScout (regex) vs real rg/grep.
//!
//! This test produces a table showing match counts for each pattern,
//! demonstrating the accuracy improvement from adding regex support.
//!
//! Run: cargo test -p bitscout-e2e --test accuracy_report -- --nocapture

use bitscout_core::dispatch::dispatch;
use bitscout_core::search::engine::{SearchEngine, SearchOptions};

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::process::Command;
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

fn real_grep() -> &'static str {
    "/usr/bin/grep"
}

fn run_cmd(cmd: &str, args: &[&str]) -> (i32, String) {
    let output = Command::new(cmd).args(args).output().unwrap();
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).to_string(),
    )
}

fn run_bs(cmd: &str, args: &[&str], cwd: &Path) -> (i32, String) {
    let cwd_str = cwd.to_str().unwrap();
    let args_owned: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    let resp = dispatch(cmd, &args_owned, cwd_str);
    (resp.exit_code, resp.stdout)
}

fn count_lines(s: &str) -> usize {
    s.lines().filter(|l| !l.is_empty()).count()
}

fn norm_lines(output: &str, dir: &Path) -> BTreeSet<String> {
    let canonical = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
    let cs = canonical.to_str().unwrap();
    let ds = dir.to_str().unwrap();
    output
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| {
            l.trim_end()
                .replace(cs, "<D>")
                .replace(ds, "<D>")
                .replace("//", "/")
        })
        .collect()
}

/// Simulate old literal-only matching (Aho-Corasick, no regex).
/// Returns the number of lines that would have matched with literal matching.
fn literal_match_count(pattern: &str, dir: &Path) -> usize {
    let engine = match SearchEngine::new(dir) {
        Ok(e) => e,
        Err(_) => return 0,
    };
    let opts = SearchOptions {
        case_insensitive: false,
        context_lines: 0,
        max_results: 100_000,
        use_regex: false,
        ..Default::default()
    };
    match engine.search(pattern, &opts) {
        Ok(r) => r.len(),
        Err(_) => 0,
    }
}

/// New regex matching count.
fn regex_match_count(pattern: &str, dir: &Path) -> usize {
    let engine = match SearchEngine::new(dir) {
        Ok(e) => e,
        Err(_) => return 0,
    };
    let opts = SearchOptions {
        case_insensitive: false,
        context_lines: 0,
        max_results: 100_000,
        use_regex: true,
        ..Default::default()
    };
    match engine.search(pattern, &opts) {
        Ok(r) => r.len(),
        Err(_) => 0,
    }
}

/// Create a realistic corpus for testing.
fn create_corpus(dir: &Path) {
    let dirs = [
        "src",
        "src/auth",
        "src/api",
        "src/utils",
        "tests",
        "docs",
        "config",
    ];
    for d in &dirs {
        fs::create_dir_all(dir.join(d)).unwrap();
    }

    fs::write(
        dir.join("src/main.rs"),
        r#"use crate::auth;
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
"#,
    )
    .unwrap();

    fs::write(
        dir.join("src/auth/mod.rs"),
        r#"pub mod token;
pub mod session;

/// Authenticate a user with username and password.
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
"#,
    )
    .unwrap();

    fs::write(
        dir.join("src/auth/token.rs"),
        r#"use super::AuthError;

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
"#,
    )
    .unwrap();

    fs::write(
        dir.join("src/auth/session.rs"),
        r#"use super::{AuthError, Session};

pub fn create(user: &str, token: &str) -> Result<Session, AuthError> {
    if !token.starts_with("tk_") {
        return Err(AuthError("invalid token".into()));
    }
    Ok(Session { id: format!("sess_{}", user), user: user.to_string() })
}

pub fn destroy(session: &Session) {
    println!("Session {} destroyed", session.id);
}
"#,
    )
    .unwrap();

    fs::write(
        dir.join("src/api/mod.rs"),
        r#"pub mod handlers;

use crate::auth;

pub fn authenticate_request(header: &str) -> bool {
    if let Some(token) = header.strip_prefix("Bearer ") {
        auth::verify_token(token)
    } else {
        false
    }
}
"#,
    )
    .unwrap();

    fs::write(
        dir.join("src/api/handlers.rs"),
        r#"use super::authenticate_request;

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
"#,
    )
    .unwrap();

    fs::write(
        dir.join("src/utils/helpers.rs"),
        r#"/// TODO: refactor this into smaller functions
pub fn parse_int(s: &str) -> Option<i64> {
    s.parse().ok()
}

/// FIXME: error handling is wrong here
pub fn safe_divide(a: f64, b: f64) -> f64 {
    if b == 0.0 { 0.0 } else { a / b }
}

// NOTE: this is fine for now
pub fn format_number(n: i64) -> String {
    format!("{}", n)
}

pub fn clamp_value(val: i32, min: i32, max: i32) -> i32 {
    if val < min { min } else if val > max { max } else { val }
}
"#,
    )
    .unwrap();

    fs::write(
        dir.join("tests/test_auth.rs"),
        r#"#[test]
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
"#,
    )
    .unwrap();

    fs::write(
        dir.join("tests/test_api.rs"),
        r#"#[test]
fn test_handle_get_authorized() {
    let resp = handle_get("/api/users", "Bearer tk_valid_token");
    assert_eq!(resp.status, 200);
}

#[test]
fn test_handle_get_unauthorized() {
    let resp = handle_get("/api/users", "InvalidToken");
    assert_eq!(resp.status, 401);
}
"#,
    )
    .unwrap();

    fs::write(
        dir.join("docs/README.md"),
        "# Project\n\nAuthentication system with 2048-bit keys.\nSupports ports 8080, 9090, and 443.\n",
    )
    .unwrap();
    fs::write(
        dir.join("config/default.json"),
        r#"{"port": 8080, "debug": true, "auth": {"enabled": true, "timeout": 3600}}"#,
    )
    .unwrap();
    fs::write(
        dir.join("config/test.json"),
        r#"{"port": 9090, "debug": false, "auth": {"enabled": false, "timeout": 300}}"#,
    )
    .unwrap();
}

// ===================================================================
// The accuracy report test
// ===================================================================

#[test]
fn test_accuracy_report() {
    let rg = match real_rg() {
        Some(p) => p,
        None => {
            eprintln!("SKIP: rg not found");
            return;
        }
    };

    let tmp = TempDir::new().unwrap();
    create_corpus(tmp.path());
    let d = tmp.path().to_str().unwrap();
    let dir = tmp.path();

    // Test patterns: (label, pattern, is_regex)
    // is_regex = true means it contains regex metacharacters
    let patterns: Vec<(&str, &str, bool)> = vec![
        // Pure literal patterns (should work the same before and after)
        ("literal: authenticate", "authenticate", false),
        ("literal: verify_token", "verify_token", false),
        ("literal: Session", "Session", false),
        // Regex patterns (broken before, fixed now)
        (r"regex: fn\s+\w+", r"fn\s+\w+", true),
        (r"regex: pub\s+fn\s+\w+", r"pub\s+fn\s+\w+", true),
        (r"regex: impl\s+\w+", r"impl\s+\w+", true),
        (r"regex: TODO|FIXME", "TODO|FIXME", true),
        (r"regex: \d+", r"\d+", true),
        (r"regex: \bfn\b", r"\bfn\b", true),
        (r"regex: struct\s+\w+", r"struct\s+\w+", true),
        (r"regex: Result<\w+", r"Result<\w+", true),
        (r"regex: \.unwrap\(\)", r"\.unwrap\(\)", true),
    ];

    eprintln!("\n");
    eprintln!("╔══════════════════════════════════════════════════════════════════════════════════════════╗");
    eprintln!("║                        BitScout Regex Accuracy Report                                  ║");
    eprintln!("╠══════════════════════════════════════════════════════════════════════════════════════════╣");
    eprintln!("║  Pattern                    │ real rg │ BS(old/literal) │ BS(new/regex) │ Accuracy      ║");
    eprintln!("╠══════════════════════════════════════════════════════════════════════════════════════════╣");

    let mut total_real = 0usize;
    let mut total_old_correct = 0usize;
    let mut total_new_correct = 0usize;
    let mut total_patterns = 0usize;
    let mut old_perfect = 0usize;
    let mut new_perfect = 0usize;

    for (label, pattern, _is_regex) in &patterns {
        total_patterns += 1;

        // Real rg output (ground truth)
        let (_, ro) = run_cmd(&rg, &["--no-heading", pattern, d]);
        let real_lines = norm_lines(&ro, dir);
        let real_count = real_lines.len();

        // BitScout with regex (new behavior, via dispatch = full pipeline)
        let (_, bo) = run_bs("rg", &["--no-heading", pattern, "."], dir);
        let new_lines = norm_lines(&bo, dir);
        let new_count = new_lines.len();

        // Simulate old behavior: literal-only matching at engine level
        let old_count = literal_match_count(pattern, &dir.canonicalize().unwrap());

        // Accuracy: how many of real rg's lines does each mode find?
        let new_matching = real_lines.intersection(&new_lines).count();
        let new_extra = new_lines.difference(&real_lines).count();

        let accuracy = if real_count == 0 {
            if new_count == 0 {
                "N/A (0)".to_string()
            } else {
                "EXTRA".to_string()
            }
        } else {
            format!("{:.1}%", new_matching as f64 / real_count as f64 * 100.0)
        };

        total_real += real_count;

        // For old: if old count == real count, consider it correct (for literals)
        // For regex patterns old would get 0, which is wrong
        if old_count == real_count {
            old_perfect += 1;
            total_old_correct += real_count;
        } else {
            total_old_correct += old_count.min(real_count);
        }

        if new_matching == real_count && new_extra == 0 {
            new_perfect += 1;
            total_new_correct += real_count;
        } else {
            total_new_correct += new_matching;
        }

        let old_marker = if old_count == real_count { " " } else { "X" };
        let new_marker = if new_matching == real_count && new_extra == 0 {
            "✓"
        } else {
            "!"
        };

        eprintln!(
            "║  {:<27} │ {:>5}   │ {:>5}  {}        │ {:>5}  {}      │ {:>8}      ║",
            label, real_count, old_count, old_marker, new_count, new_marker, accuracy
        );
    }

    let old_accuracy = if total_real > 0 {
        total_old_correct as f64 / total_real as f64 * 100.0
    } else {
        0.0
    };
    let new_accuracy = if total_real > 0 {
        total_new_correct as f64 / total_real as f64 * 100.0
    } else {
        0.0
    };

    eprintln!("╠══════════════════════════════════════════════════════════════════════════════════════════╣");
    eprintln!(
        "║  TOTAL ({} patterns)           │ {:>5}   │ exact: {}/{}     │ exact: {}/{}    │             ║",
        total_patterns, total_real, old_perfect, total_patterns, new_perfect, total_patterns
    );
    eprintln!(
        "║  Line-level accuracy          │  100%   │     {:.1}%          │    {:.1}%        │             ║",
        old_accuracy, new_accuracy
    );
    eprintln!("╠══════════════════════════════════════════════════════════════════════════════════════════╣");
    eprintln!(
        "║  Improvement: {:.1}% -> {:.1}%  (+{:.1}pp)    Perfect: {}/{} -> {}/{}                        ║",
        old_accuracy,
        new_accuracy,
        new_accuracy - old_accuracy,
        old_perfect,
        total_patterns,
        new_perfect,
        total_patterns
    );
    eprintln!("╚══════════════════════════════════════════════════════════════════════════════════════════╝");

    // The new regex mode must be strictly better
    assert!(
        new_accuracy >= old_accuracy,
        "New accuracy ({:.1}%) must be >= old accuracy ({:.1}%)",
        new_accuracy,
        old_accuracy
    );
    // Every regex pattern should now match real rg exactly
    assert_eq!(
        new_perfect, total_patterns,
        "Expected all {} patterns to match real rg exactly, but only {} did",
        total_patterns, new_perfect
    );
}

// ===================================================================
// grep accuracy report
// ===================================================================

#[test]
fn test_grep_accuracy_report() {
    let tmp = TempDir::new().unwrap();
    create_corpus(tmp.path());
    let d = tmp.path().to_str().unwrap();
    let dir = tmp.path();

    let patterns: Vec<(&str, &str, &[&str], &[&str])> = vec![
        // (label, pattern, real_grep_extra_args, bs_extra_args)
        ("literal: authenticate", "authenticate", &["-rn"], &["-rn"]),
        ("literal: Session", "Session", &["-rn"], &["-rn"]),
        ("regex: [0-9]+", "[0-9][0-9]*", &["-rn"], &["-rn"]),  // BRE for real grep
        ("regex: foo|bar (ERE)", "foo|bar", &["-rn", "-E"], &["-rn"]), // our engine is ERE-like
    ];

    // For grep, regex digits: our engine uses \d+ or [0-9]+ natively
    // But the comparison should use compatible patterns

    eprintln!("\n");
    eprintln!("╔═══════════════════════════════════════════════════════════════════════╗");
    eprintln!("║                    grep Accuracy Report                              ║");
    eprintln!("╠═══════════════════════════════════════════════════════════════════════╣");
    eprintln!("║  Pattern                    │ real grep │ BS(new/regex)  │ Match?     ║");
    eprintln!("╠═══════════════════════════════════════════════════════════════════════╣");

    let mut all_match = true;

    for (label, pattern, grep_args, bs_args) in &patterns {
        let mut real_args: Vec<&str> = grep_args.to_vec();
        real_args.push(pattern);
        real_args.push(d);

        let (_, ro) = run_cmd(real_grep(), &real_args);
        let real_lines = norm_lines(&ro, dir);

        // For BS, use the same pattern but through our dispatch
        // Our grep uses Rust regex (ERE-like) so [0-9][0-9]* works too
        let bs_pattern = if *pattern == "[0-9][0-9]*" {
            "[0-9]+" // equivalent ERE
        } else {
            pattern
        };
        let mut bs_full: Vec<&str> = bs_args.to_vec();
        bs_full.push(bs_pattern);
        bs_full.push(".");

        let (_, bo) = run_bs("grep", &bs_full, dir);
        let new_lines = norm_lines(&bo, dir);

        let matching = real_lines.intersection(&new_lines).count();
        let extra = new_lines.difference(&real_lines).count();
        let exact = matching == real_lines.len() && extra == 0;
        if !exact {
            all_match = false;
        }

        let marker = if exact { "✓ exact" } else { "✗ DIFF" };
        eprintln!(
            "║  {:<27} │ {:>7}   │ {:>7}        │ {:>8}   ║",
            label,
            real_lines.len(),
            new_lines.len(),
            marker
        );
    }

    eprintln!("╚═══════════════════════════════════════════════════════════════════════╝");
    assert!(all_match, "Not all grep patterns matched exactly");
}

// ===================================================================
// BM25 accuracy report: verify --bm25 doesn't change result sets
// ===================================================================

#[test]
fn test_bm25_accuracy_report() {
    let rg = match real_rg() {
        Some(p) => p,
        None => {
            eprintln!("SKIP: rg not found");
            return;
        }
    };

    let tmp = TempDir::new().unwrap();
    create_corpus(tmp.path());
    let d = tmp.path().to_str().unwrap();
    let dir = tmp.path();

    let patterns: Vec<(&str, &str)> = vec![
        ("literal: authenticate", "authenticate"),
        ("literal: Session", "Session"),
        ("literal: token", "token"),
        (r"regex: fn\s+\w+", r"fn\s+\w+"),
        (r"regex: pub\s+fn", r"pub\s+fn"),
        (r"regex: TODO|FIXME", "TODO|FIXME"),
        (r"regex: \d+", r"\d+"),
        (r"regex: struct\s+\w+", r"struct\s+\w+"),
    ];

    eprintln!("\n");
    eprintln!("╔═════════════════════════════════════════════════════════════════════════════════════════════════╗");
    eprintln!("║                           BM25 Scoring Accuracy Report                                        ║");
    eprintln!("╠═════════════════════════════════════════════════════════════════════════════════════════════════╣");
    eprintln!("║  Pattern                    │ real rg │ BS plain │ BS --bm25 │ BS --bm25=full │ Score range    ║");
    eprintln!("╠═════════════════════════════════════════════════════════════════════════════════════════════════╣");

    let mut all_match = true;

    for (label, pattern) in &patterns {
        // Ground truth: real rg
        let (_, ro) = run_cmd(&rg, &["--no-heading", pattern, d]);
        let real_lines = norm_lines(&ro, dir);

        // BitScout plain (no --bm25)
        let (_, bo_plain) = run_bs("rg", &["--no-heading", pattern, "."], dir);
        let plain_lines = norm_lines(&bo_plain, dir);

        // BitScout --bm25 (TF mode)
        let (_, bo_bm25) = run_bs("rg", &["--no-heading", "--bm25", pattern, "."], dir);
        let bm25_raw_lines: Vec<&str> = bo_bm25.lines().filter(|l| !l.is_empty()).collect();

        // Extract scores and content from bm25 output
        let mut scores: Vec<f64> = Vec::new();
        let mut bm25_content_lines = BTreeSet::new();
        for line in &bm25_raw_lines {
            let s = line.trim_end();
            if let Some(bracket_end) = s.find(']') {
                let score_str = &s[1..bracket_end];
                if let Ok(score) = score_str.parse::<f64>() {
                    scores.push(score);
                }
                let content = &s[bracket_end + 2..]; // skip "] "
                let canonical = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
                let cs = canonical.to_str().unwrap();
                let ds = dir.to_str().unwrap();
                bm25_content_lines.insert(
                    content.replace(cs, "<D>").replace(ds, "<D>").replace("//", "/")
                );
            }
        }

        // BitScout --bm25=full
        let (_, bo_full) = run_bs("rg", &["--no-heading", "--bm25=full", pattern, "."], dir);
        let full_raw_lines: Vec<&str> = bo_full.lines().filter(|l| !l.is_empty()).collect();
        let mut full_scores: Vec<f64> = Vec::new();
        let mut full_content_lines = BTreeSet::new();
        for line in &full_raw_lines {
            let s = line.trim_end();
            if let Some(bracket_end) = s.find(']') {
                let score_str = &s[1..bracket_end];
                if let Ok(score) = score_str.parse::<f64>() {
                    full_scores.push(score);
                }
                let content = &s[bracket_end + 2..];
                let canonical = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
                let cs = canonical.to_str().unwrap();
                let ds = dir.to_str().unwrap();
                full_content_lines.insert(
                    content.replace(cs, "<D>").replace(ds, "<D>").replace("//", "/")
                );
            }
        }

        // Verify: bm25 content should match plain content exactly
        let plain_exact = plain_lines == real_lines;
        let bm25_exact = bm25_content_lines == real_lines;
        let full_exact = full_content_lines == real_lines;

        if !bm25_exact || !full_exact {
            all_match = false;
        }

        // Score range
        let score_range = if scores.is_empty() {
            "N/A".to_string()
        } else {
            let min = scores.iter().cloned().fold(f64::INFINITY, f64::min);
            let max = scores.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
            format!("{:.2}-{:.2}", min, max)
        };

        let p_mark = if plain_exact { "✓" } else { "✗" };
        let b_mark = if bm25_exact { "✓" } else { "✗" };
        let f_mark = if full_exact { "✓" } else { "✗" };

        eprintln!(
            "║  {:<27} │ {:>5}   │ {:>5} {}  │ {:>5} {}   │ {:>5} {}        │ {:>12}  ║",
            label,
            real_lines.len(),
            plain_lines.len(), p_mark,
            bm25_content_lines.len(), b_mark,
            full_content_lines.len(), f_mark,
            score_range
        );
    }

    eprintln!("╠═════════════════════════════════════════════════════════════════════════════════════════════════╣");
    eprintln!("║  ✓ = exact match with real rg    ✗ = mismatch                                                ║");
    eprintln!("║  Score range shows min-max BM25 TF scores across all matching lines                           ║");
    eprintln!("╚═════════════════════════════════════════════════════════════════════════════════════════════════╝");

    assert!(all_match, "BM25 mode changed the result set — should be identical to plain mode");
}

#[test]
fn test_bm25_score_differentiation() {
    // Verify that BM25 scores actually differentiate files by relevance.
    // A file with many occurrences of a term should score differently than one with few.
    let tmp = TempDir::new().unwrap();

    fs::create_dir_all(tmp.path().join("src")).unwrap();

    // File A: high relevance for "token" (many occurrences)
    fs::write(tmp.path().join("src/token_heavy.rs"), r#"
fn validate_token(token: &str) -> bool {
    let token_parts: Vec<&str> = token.split('.').collect();
    if token_parts.len() != 3 { return false; }
    let token_header = token_parts[0];
    let token_payload = token_parts[1];
    let token_sig = token_parts[2];
    verify_token_signature(token_header, token_payload, token_sig)
}
fn verify_token_signature(h: &str, p: &str, s: &str) -> bool { true }
"#).unwrap();

    // File B: low relevance for "token" (one occurrence in many lines)
    fs::write(tmp.path().join("src/misc.rs"), r#"
fn main() {
    let config = load_config();
    let server = start_server();
    let db = connect_database();
    let cache = init_cache();
    let logger = setup_logging();
    let router = create_router();
    let middleware = add_middleware();
    let token = get_auth_token();
    let handler = register_handlers();
    let listener = bind_listener();
    start_listening();
}
fn load_config() {}
fn start_server() {}
fn connect_database() {}
fn init_cache() {}
fn setup_logging() {}
fn create_router() {}
fn add_middleware() {}
fn get_auth_token() -> String { String::new() }
fn register_handlers() {}
fn bind_listener() {}
fn start_listening() {}
"#).unwrap();

    let (_, stdout) = run_bs("rg", &["--no-heading", "--bm25", "token", "."], tmp.path());

    // Parse scores per file
    let mut file_scores: std::collections::HashMap<String, Vec<f64>> = std::collections::HashMap::new();
    for line in stdout.lines().filter(|l| !l.is_empty()) {
        let s = line.trim_end();
        if let Some(bracket_end) = s.find(']') {
            let score: f64 = s[1..bracket_end].parse().unwrap_or(0.0);
            let rest = &s[bracket_end + 2..];
            // Extract filename from path:line:content
            if let Some(colon) = rest.find(':') {
                let path = &rest[..colon];
                let filename = Path::new(path).file_name()
                    .map(|f| f.to_string_lossy().to_string())
                    .unwrap_or_default();
                file_scores.entry(filename).or_default().push(score);
            }
        }
    }

    eprintln!("\n");
    eprintln!("╔══════════════════════════════════════════════════════════════╗");
    eprintln!("║              BM25 Score Differentiation Test                ║");
    eprintln!("╠══════════════════════════════════════════════════════════════╣");

    for (file, scores) in &file_scores {
        let avg = scores.iter().sum::<f64>() / scores.len() as f64;
        eprintln!("║  {:<20} │ lines: {:>2} │ avg score: {:.4}          ║", file, scores.len(), avg);
    }
    eprintln!("╚══════════════════════════════════════════════════════════════╝");

    // token_heavy.rs should have a higher BM25 score than misc.rs
    // because "token" appears more frequently relative to doc length
    let heavy_scores = file_scores.get("token_heavy.rs");
    let misc_scores = file_scores.get("misc.rs");

    assert!(heavy_scores.is_some(), "token_heavy.rs should have matches");
    assert!(misc_scores.is_some(), "misc.rs should have matches");

    let heavy_avg = heavy_scores.unwrap().iter().sum::<f64>() / heavy_scores.unwrap().len() as f64;
    let misc_avg = misc_scores.unwrap().iter().sum::<f64>() / misc_scores.unwrap().len() as f64;

    eprintln!("\ntoken_heavy.rs avg: {:.4}, misc.rs avg: {:.4}", heavy_avg, misc_avg);
    assert!(
        heavy_avg > misc_avg,
        "token_heavy.rs (avg={:.4}) should score higher than misc.rs (avg={:.4})",
        heavy_avg, misc_avg
    );
}
