//! Deep Search Demo: BitScout vs rg/grep
//!
//! Proves that BitScout finds data that rg/grep CANNOT find,
//! because BitScout searches inside compressed & binary formats transparently.
//!
//! Same query, same directory — BitScout returns MORE correct results.
//!
//! Run: cargo test -p bitscout-e2e --test deep_search_demo -- --nocapture

use bitscout_core::dispatch::dispatch;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

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

/// Create a realistic project with mixed file formats.
/// The SECRET_KEY appears in plain text, gzip, zip, and deep inside the project.
fn create_mixed_format_corpus(dir: &Path) {
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::create_dir_all(dir.join("logs")).unwrap();
    fs::create_dir_all(dir.join("archive")).unwrap();
    fs::create_dir_all(dir.join("docs")).unwrap();

    // ── Plain text files ──────────────────────────────────────────────
    fs::write(
        dir.join("src/config.rs"),
        r#"
/// Application configuration
pub struct AppConfig {
    pub database_url: String,
    pub api_key: String,
    pub max_connections: u32,
}

impl AppConfig {
    pub fn load() -> Self {
        Self {
            database_url: "postgres://localhost:5432/mydb".into(),
            api_key: "sk_live_abc123def456".into(),
            max_connections: 100,
        }
    }
}

pub fn validate_api_key(key: &str) -> bool {
    key.starts_with("sk_live_") && key.len() > 20
}
"#,
    )
    .unwrap();

    fs::write(
        dir.join("src/auth.rs"),
        r#"
use crate::config::AppConfig;

pub fn authenticate(config: &AppConfig, token: &str) -> Result<User, AuthError> {
    if !validate_api_key(&config.api_key) {
        return Err(AuthError::InvalidConfig);
    }
    // Check token against database
    let user = lookup_user(token)?;
    Ok(user)
}

fn lookup_user(token: &str) -> Result<User, AuthError> {
    // database_url connection happens here
    Ok(User { id: 1, name: "admin".into() })
}

pub struct User { pub id: u64, pub name: String }
pub enum AuthError { InvalidConfig, InvalidToken, DatabaseError }
"#,
    )
    .unwrap();

    fs::write(
        dir.join("src/main.rs"),
        r#"
mod config;
mod auth;

fn main() {
    let config = config::AppConfig::load();
    println!("Connecting to database_url: {}", config.database_url);
    println!("API connections: max_connections = {}", config.max_connections);

    match auth::authenticate(&config, "user_token_xyz") {
        Ok(user) => println!("Authenticated: {}", user.name),
        Err(_) => eprintln!("Authentication failed"),
    }
}
"#,
    )
    .unwrap();

    // ── Gzip compressed log ───────────────────────────────────────────
    // Contains references to database_url and api_key that rg CANNOT see
    {
        use flate2::write::GzEncoder;
        use flate2::Compression;

        let log_content = r#"2024-01-15 10:23:45 INFO  Starting application...
2024-01-15 10:23:45 INFO  Loading config: database_url=postgres://prod:5432/app
2024-01-15 10:23:46 INFO  Validating api_key: sk_live_***
2024-01-15 10:23:46 INFO  Pool created: max_connections=50
2024-01-15 10:23:47 WARN  Slow query on database_url connection
2024-01-15 10:23:48 ERROR Connection to database_url timed out after 30s
2024-01-15 10:24:00 INFO  Retrying database_url connection...
2024-01-15 10:24:01 INFO  Connected. api_key validated successfully.
"#;
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(log_content.as_bytes()).unwrap();
        let compressed = encoder.finish().unwrap();
        fs::write(dir.join("logs/app.log.gz"), &compressed).unwrap();
    }

    // ── Another gzip log with error traces ────────────────────────────
    {
        use flate2::write::GzEncoder;
        use flate2::Compression;

        let error_log = r#"2024-01-16 03:00:00 ERROR database_url connection refused
2024-01-16 03:00:00 ERROR Failed to validate api_key: timeout
2024-01-16 03:00:01 FATAL max_connections exceeded (100/100)
2024-01-16 03:00:01 FATAL Application shutting down
"#;
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(error_log.as_bytes()).unwrap();
        let compressed = encoder.finish().unwrap();
        fs::write(dir.join("logs/error.log.gz"), &compressed).unwrap();
    }

    // ── Zip archive containing config backup ──────────────────────────
    {
        use std::io::Cursor;
        use zip::write::{SimpleFileOptions, ZipWriter};

        let buf = Vec::new();
        let cursor = Cursor::new(buf);
        let mut zip = ZipWriter::new(cursor);
        let options = SimpleFileOptions::default();

        zip.start_file("config_backup.toml", options).unwrap();
        zip.write_all(
            br#"[database]
database_url = "postgres://backup:5432/mydb_backup"
max_connections = 200

[auth]
api_key = "sk_live_backup_key_789"
token_ttl = 3600
"#,
        )
        .unwrap();

        zip.start_file("deployment_notes.txt", options).unwrap();
        zip.write_all(
            br#"Deployment checklist:
1. Update database_url in production config
2. Rotate api_key before deploy
3. Verify max_connections matches server capacity
4. Run integration tests
"#,
        )
        .unwrap();

        let cursor = zip.finish().unwrap();
        fs::write(dir.join("archive/config_backup.zip"), cursor.into_inner()).unwrap();
    }

    // ── Plain text docs ───────────────────────────────────────────────
    fs::write(
        dir.join("docs/setup.md"),
        r#"# Setup Guide

## Configuration

Set `database_url` to your PostgreSQL connection string.
Set `api_key` to your service API key (starts with `sk_live_`).
Set `max_connections` based on your server capacity (default: 100).

## Troubleshooting

If you see "database_url connection refused":
1. Check that PostgreSQL is running
2. Verify the database_url is correct
3. Check firewall rules
"#,
    )
    .unwrap();
}

// ===================================================================
// Main demo test
// ===================================================================

#[test]
fn test_deep_search_superiority() {
    let rg_path = match real_rg() {
        Some(p) => p,
        None => {
            eprintln!("SKIP: rg not found");
            return;
        }
    };

    let tmp = TempDir::new().unwrap();
    create_mixed_format_corpus(tmp.path());
    let d = tmp.path().to_str().unwrap();
    let dir = tmp.path();

    // The search terms a developer would actually use
    let queries = vec![
        ("database_url", "开发者搜 database_url 排查连接问题"),
        ("api_key", "搜 api_key 检查密钥泄露"),
        ("max_connections", "搜 max_connections 调优连接池"),
        (r"sk_live_\w+", "regex: 搜所有 sk_live_ 开头的密钥"),
        (r"ERROR\s+\w+", "regex: 搜所有 ERROR 日志"),
        (r"\d{4}-\d{2}-\d{2}", "regex: 搜所有日期格式"),
    ];

    eprintln!();
    eprintln!("╔════════════════════════════════════════════════════════════════════════════════════════╗");
    eprintln!(
        "║              BitScout Deep Search vs rg — Same Query, Same Directory                 ║"
    );
    eprintln!(
        "║                                                                                      ║"
    );
    eprintln!(
        "║  BitScout 穿透 .gz / .zip 搜索，一次 search 直达结果                                  ║"
    );
    eprintln!("║  rg/grep 只能搜纯文本，压缩文件里的数据完全看不到                                       ║");
    eprintln!("╠════════════════════════════════════════════════════════════════════════════════════════╣");
    eprintln!(
        "║  Query                    │ real rg │ BitScout │ BS多发现 │ 场景                      ║"
    );
    eprintln!("╠════════════════════════════════════════════════════════════════════════════════════════╣");

    let mut total_rg = 0;
    let mut total_bs = 0;
    let mut total_extra = 0;

    for (pattern, scenario) in &queries {
        let (_, ro) = run_cmd(&rg_path, &["--no-heading", pattern, d]);
        let rg_count = count_lines(&ro);

        let (_, bo) = run_bs("rg", &["--no-heading", pattern, "."], dir);
        let bs_count = count_lines(&bo);

        let extra = if bs_count > rg_count {
            bs_count - rg_count
        } else {
            0
        };
        let marker = if extra > 0 {
            format!("+{}", extra)
        } else {
            "=".into()
        };

        total_rg += rg_count;
        total_bs += bs_count;
        total_extra += extra;

        eprintln!(
            "║  {:<25} │ {:>5}   │ {:>6}   │ {:>6}   │ {}",
            pattern, rg_count, bs_count, marker, scenario
        );
    }

    eprintln!("╠════════════════════════════════════════════════════════════════════════════════════════╣");
    eprintln!(
        "║  TOTAL                      │ {:>5}   │ {:>6}   │ +{:<5}  │                           ║",
        total_rg, total_bs, total_extra,
    );

    let improvement_pct = if total_rg > 0 {
        (total_bs as f64 - total_rg as f64) / total_rg as f64 * 100.0
    } else {
        0.0
    };
    eprintln!(
        "║  BitScout 多发现 {:.0}% 的匹配结果 (来自 .gz + .zip 内部)                               ║",
        improvement_pct
    );
    eprintln!("╚════════════════════════════════════════════════════════════════════════════════════════╝");

    // ── Show WHERE the extra hits come from ──
    eprintln!();
    eprintln!("╔════════════════════════════════════════════════════════════════════════════════════════╗");
    eprintln!("║  rg 看不到、BitScout 独家发现的匹配（来自 .gz / .zip 内部）                            ║");
    eprintln!("╠════════════════════════════════════════════════════════════════════════════════════════╣");

    // Show detailed matches for "database_url" as example
    let (_, bo) = run_bs("rg", &["--no-heading", "-n", "database_url", "."], dir);
    for line in bo.lines() {
        if line.contains(".gz") || line.contains(".zip") {
            eprintln!("║  {}", line);
        }
    }

    eprintln!("╚════════════════════════════════════════════════════════════════════════════════════════╝");

    // Verify BitScout found strictly more
    assert!(
        total_bs > total_rg,
        "BitScout ({}) should find MORE than rg ({})",
        total_bs,
        total_rg
    );

    // Verify BitScout found everything rg found (superset)
    for (pattern, _) in &queries {
        let (_, ro) = run_cmd(&rg_path, &["--no-heading", pattern, d]);
        let (_, bo) = run_bs("rg", &["--no-heading", pattern, "."], dir);

        // Every line from rg should appear in BitScout output (after normalization)
        let rg_lines: std::collections::BTreeSet<String> = ro
            .lines()
            .filter(|l| !l.is_empty())
            .map(|l| {
                // Extract just the content portion (after path:)
                if let Some(pos) = l.find(':') {
                    l[pos + 1..].trim_end().to_string()
                } else {
                    l.trim_end().to_string()
                }
            })
            .collect();

        let bs_lines: std::collections::BTreeSet<String> = bo
            .lines()
            .filter(|l| !l.is_empty())
            .map(|l| {
                if let Some(pos) = l.find(':') {
                    l[pos + 1..].trim_end().to_string()
                } else {
                    l.trim_end().to_string()
                }
            })
            .collect();

        let missing: Vec<_> = rg_lines.difference(&bs_lines).collect();
        assert!(
            missing.is_empty(),
            "BitScout missed rg results for '{}': {:?}",
            pattern,
            missing
        );
    }
}

/// Single-query demo: show that one BitScout search replaces
/// what would need rg + zgrep + unzip+grep pipeline.
#[test]
fn test_one_search_replaces_pipeline() {
    let rg_path = match real_rg() {
        Some(p) => p,
        None => {
            eprintln!("SKIP: rg not found");
            return;
        }
    };

    let tmp = TempDir::new().unwrap();
    create_mixed_format_corpus(tmp.path());
    let d = tmp.path().to_str().unwrap();

    eprintln!();
    eprintln!("╔════════════════════════════════════════════════════════════════════════════════════════╗");
    eprintln!("║  传统方式 vs BitScout: 搜 \"database_url\" 需要几步？                                  ║");
    eprintln!("╠════════════════════════════════════════════════════════════════════════════════════════╣");

    // Step 1: rg for plain text
    let (_, ro) = run_cmd(&rg_path, &["--no-heading", "database_url", d]);
    let rg_count = count_lines(&ro);
    eprintln!(
        "║  Step 1: rg \"database_url\" .              → {} 行 (只有纯文本)",
        rg_count
    );

    // Step 2: gzcat + grep for gzip files (traditional approach)
    let zgrep_count = {
        let mut count = 0;
        for entry in walkdir(tmp.path()) {
            if entry.to_string_lossy().ends_with(".gz") {
                // gzcat file.gz | grep pattern
                let gzcat = Command::new("gzcat").arg(entry.to_str().unwrap()).output();
                if let Ok(o) = gzcat {
                    let text = String::from_utf8_lossy(&o.stdout);
                    count += text.lines().filter(|l| l.contains("database_url")).count();
                }
            }
        }
        count
    };
    eprintln!(
        "║  Step 2: gzcat *.gz | grep \"database_url\" → {} 行 (压缩日志)",
        zgrep_count
    );

    // Step 3: unzip + grep for zip files
    let zip_count = {
        let mut count = 0;
        for entry in walkdir(tmp.path()) {
            if entry.to_string_lossy().ends_with(".zip") {
                let output = Command::new("unzip")
                    .args(["-p", entry.to_str().unwrap()])
                    .output();
                if let Ok(o) = output {
                    let text = String::from_utf8_lossy(&o.stdout);
                    count += text.lines().filter(|l| l.contains("database_url")).count();
                }
            }
        }
        count
    };
    eprintln!(
        "║  Step 3: unzip -p *.zip | grep            → {} 行 (zip归档)",
        zip_count
    );

    let traditional_total = rg_count + zgrep_count + zip_count;
    eprintln!("║  ─────────────────────────────────────────────────────────────────");
    eprintln!(
        "║  传统方式合计: {} 行 (需要 3 个命令 + 管道组合)",
        traditional_total
    );

    // Now BitScout: single search
    let (_, bo) = run_bs("rg", &["--no-heading", "database_url", "."], tmp.path());
    let bs_count = count_lines(&bo);
    eprintln!("║");
    eprintln!(
        "║  BitScout: rg \"database_url\" .            → {} 行 (一次搜全，0 管道)",
        bs_count
    );
    eprintln!("║");

    assert_eq!(
        bs_count, traditional_total,
        "BitScout single search ({}) should match traditional pipeline total ({})",
        bs_count, traditional_total
    );

    eprintln!("║  ✓ BitScout 一次搜索 = rg + zgrep + unzip|grep 三步管道");
    eprintln!("╚════════════════════════════════════════════════════════════════════════════════════════╝");
}

/// Walk directory recursively, return all file paths.
fn walkdir(root: &Path) -> Vec<std::path::PathBuf> {
    let mut result = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                } else {
                    result.push(path);
                }
            }
        }
    }
    result
}
