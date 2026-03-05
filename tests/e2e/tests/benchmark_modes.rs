//! Benchmark: measure cold-start dispatch latency for all search modes.
//!
//! Tests file search, regex search, content search, BM25 scoring, and semantic search
//! after removing the daemon architecture.

use bitscout_core::dispatch::dispatch;
use std::fs;
use std::path::Path;
use std::time::Instant;
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Corpus generation
// ---------------------------------------------------------------------------

fn create_benchmark_corpus(dir: &Path) {
    let src = dir.join("src");
    fs::create_dir_all(src.join("auth")).unwrap();
    fs::create_dir_all(src.join("api")).unwrap();
    fs::create_dir_all(src.join("db")).unwrap();
    fs::create_dir_all(dir.join("tests")).unwrap();
    fs::create_dir_all(dir.join("docs")).unwrap();
    fs::create_dir_all(dir.join("config")).unwrap();

    fs::write(src.join("auth").join("mod.rs"), AUTH_MOD).unwrap();
    fs::write(src.join("auth").join("jwt.rs"), AUTH_JWT).unwrap();
    fs::write(src.join("auth").join("oauth.rs"), AUTH_OAUTH).unwrap();
    fs::write(src.join("auth").join("session.rs"), AUTH_SESSION).unwrap();
    fs::write(src.join("api").join("mod.rs"), API_MOD).unwrap();
    fs::write(src.join("api").join("handlers.rs"), API_HANDLERS).unwrap();
    fs::write(src.join("api").join("middleware.rs"), API_MIDDLEWARE).unwrap();
    fs::write(src.join("api").join("routes.rs"), API_ROUTES).unwrap();
    fs::write(src.join("db").join("mod.rs"), DB_MOD).unwrap();
    fs::write(src.join("main.rs"), MAIN_RS).unwrap();
    fs::write(dir.join("tests").join("test_auth.rs"), TEST_AUTH).unwrap();
    fs::write(dir.join("tests").join("test_api.rs"), TEST_API).unwrap();
    fs::write(dir.join("config").join("default.json"), CONFIG_JSON).unwrap();
    fs::write(dir.join("docs").join("architecture.md"), DOC_ARCH).unwrap();
    fs::write(dir.join("docs").join("api.md"), DOC_API).unwrap();
    fs::write(dir.join("Cargo.toml"), CARGO_TOML).unwrap();
}

// ---------------------------------------------------------------------------
// Corpus content (const statics to avoid raw string + path issues)
// ---------------------------------------------------------------------------

const AUTH_MOD: &str = "pub mod jwt;\npub mod oauth;\npub mod session;\n\nuse crate::db::UserStore;\n\npub struct AuthManager {\n    store: UserStore,\n    jwt_secret: String,\n    session_ttl: u64,\n}\n\nimpl AuthManager {\n    pub fn new(store: UserStore, secret: &str) -> Self {\n        Self { store, jwt_secret: secret.to_string(), session_ttl: 3600 }\n    }\n\n    pub fn authenticate(&self, username: &str, password: &str) -> Result<String, AuthError> {\n        let user = self.store.find_by_username(username)?;\n        if !verify_password(password, &user.password_hash) {\n            return Err(AuthError::InvalidCredentials);\n        }\n        let token = self.create_jwt_token(&user)?;\n        Ok(token)\n    }\n\n    fn create_jwt_token(&self, user: &User) -> Result<String, AuthError> {\n        Ok(format!(\"jwt.{}.{}\", user.id, self.jwt_secret))\n    }\n\n    pub fn validate_token(&self, token: &str) -> Result<Claims, AuthError> {\n        if token.starts_with(\"jwt.\") {\n            Ok(Claims { user_id: 1, exp: 0 })\n        } else {\n            Err(AuthError::InvalidToken)\n        }\n    }\n}\n\nfn verify_password(password: &str, hash: &str) -> bool {\n    password.len() > 3 && !hash.is_empty()\n}\n\n#[derive(Debug)]\npub enum AuthError {\n    InvalidCredentials,\n    InvalidToken,\n    SessionExpired,\n    DatabaseError(String),\n}\n\npub struct User {\n    pub id: u64,\n    pub username: String,\n    pub password_hash: String,\n    pub email: String,\n}\n\npub struct Claims {\n    pub user_id: u64,\n    pub exp: u64,\n}\n";

const AUTH_JWT: &str = "//! JWT token creation and validation.\nuse super::{AuthError, Claims};\n\npub fn encode_token(user_id: u64, secret: &str, ttl: u64) -> Result<String, AuthError> {\n    let header = base64_encode(\"{\\\"alg\\\":\\\"HS256\\\",\\\"typ\\\":\\\"JWT\\\"}\");\n    let payload = base64_encode(&format!(\"{{\\\"sub\\\":\\\"{}\\\",\\\"exp\\\":\\\"{}\\\"}}\", user_id, ttl));\n    let signature = hmac_sha256(&format!(\"{}.{}\", header, payload), secret);\n    Ok(format!(\"{}.{}.{}\", header, payload, signature))\n}\n\npub fn decode_token(token: &str, secret: &str) -> Result<Claims, AuthError> {\n    let parts: Vec<&str> = token.split('.').collect();\n    if parts.len() != 3 {\n        return Err(AuthError::InvalidToken);\n    }\n    let expected = hmac_sha256(&format!(\"{}.{}\", parts[0], parts[1]), secret);\n    if expected != parts[2] {\n        return Err(AuthError::InvalidToken);\n    }\n    Ok(Claims { user_id: 1, exp: 0 })\n}\n\nfn base64_encode(input: &str) -> String {\n    input.chars().map(|c| c as u8).map(|b| format!(\"{:02x}\", b)).collect()\n}\n\nfn hmac_sha256(data: &str, key: &str) -> String {\n    format!(\"sig_{}_{}\", data.len(), key.len())\n}\n";

const AUTH_OAUTH: &str = "//! OAuth2 provider integration.\nuse super::AuthError;\n\npub struct OAuthConfig {\n    pub client_id: String,\n    pub client_secret: String,\n    pub redirect_uri: String,\n    pub authorize_url: String,\n    pub token_url: String,\n}\n\npub struct OAuthProvider {\n    config: OAuthConfig,\n}\n\nimpl OAuthProvider {\n    pub fn new(config: OAuthConfig) -> Self { Self { config } }\n\n    pub fn authorization_url(&self, state: &str) -> String {\n        format!(\"{}?client_id={}&redirect_uri={}&state={}&response_type=code\",\n            self.config.authorize_url, self.config.client_id, self.config.redirect_uri, state)\n    }\n\n    pub fn exchange_code(&self, code: &str) -> Result<TokenResponse, AuthError> {\n        if code.is_empty() { return Err(AuthError::InvalidCredentials); }\n        Ok(TokenResponse {\n            access_token: format!(\"oauth_access_{}\", code),\n            refresh_token: Some(format!(\"oauth_refresh_{}\", code)),\n            expires_in: 3600,\n        })\n    }\n\n    pub fn refresh_access_token(&self, refresh_token: &str) -> Result<TokenResponse, AuthError> {\n        if refresh_token.starts_with(\"oauth_refresh_\") {\n            Ok(TokenResponse { access_token: \"refreshed_token\".into(), refresh_token: Some(refresh_token.into()), expires_in: 3600 })\n        } else {\n            Err(AuthError::InvalidToken)\n        }\n    }\n}\n\npub struct TokenResponse {\n    pub access_token: String,\n    pub refresh_token: Option<String>,\n    pub expires_in: u64,\n}\n";

const AUTH_SESSION: &str = "//! Session management with expiration.\nuse std::collections::HashMap;\nuse super::AuthError;\n\npub struct SessionStore {\n    sessions: HashMap<String, Session>,\n    ttl: u64,\n}\n\npub struct Session {\n    pub user_id: u64,\n    pub created_at: u64,\n    pub last_access: u64,\n    pub data: HashMap<String, String>,\n}\n\nimpl SessionStore {\n    pub fn new(ttl: u64) -> Self { Self { sessions: HashMap::new(), ttl } }\n\n    pub fn create_session(&mut self, user_id: u64) -> String {\n        let id = format!(\"sess_{}\", self.sessions.len());\n        let now = current_timestamp();\n        self.sessions.insert(id.clone(), Session { user_id, created_at: now, last_access: now, data: HashMap::new() });\n        id\n    }\n\n    pub fn get_session(&mut self, id: &str) -> Result<&Session, AuthError> {\n        let now = current_timestamp();\n        match self.sessions.get(id) {\n            Some(s) if now - s.last_access < self.ttl => Ok(s),\n            Some(_) => { self.sessions.remove(id); Err(AuthError::SessionExpired) }\n            None => Err(AuthError::InvalidToken),\n        }\n    }\n\n    pub fn destroy_session(&mut self, id: &str) { self.sessions.remove(id); }\n\n    pub fn cleanup_expired(&mut self) {\n        let now = current_timestamp();\n        self.sessions.retain(|_, s| now - s.last_access < self.ttl);\n    }\n}\n\nfn current_timestamp() -> u64 {\n    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs()\n}\n";

const API_MOD: &str = "pub mod handlers;\npub mod middleware;\npub mod routes;\n";

const API_HANDLERS: &str = "//! HTTP request handlers.\nuse crate::auth::{AuthManager, AuthError};\nuse crate::db::Database;\n\npub struct AppState { pub auth: AuthManager, pub db: Database }\n\npub fn handle_login(state: &AppState, username: &str, password: &str) -> Response {\n    match state.auth.authenticate(username, password) {\n        Ok(token) => Response::json(200, &format!(\"{{\\\"token\\\": \\\"{}\\\"}}\", token)),\n        Err(AuthError::InvalidCredentials) => Response::json(401, \"{\\\"error\\\": \\\"invalid credentials\\\"}\"),\n        Err(e) => Response::json(500, &format!(\"{{\\\"error\\\": \\\"{:?}\\\"}}\", e)),\n    }\n}\n\npub fn handle_get_user(state: &AppState, token: &str, user_id: u64) -> Response {\n    match state.auth.validate_token(token) {\n        Ok(_claims) => Response::json(200, &format!(\"{{\\\"id\\\": {}}}\", user_id)),\n        Err(_) => Response::json(401, \"{\\\"error\\\": \\\"unauthorized\\\"}\"),\n    }\n}\n\npub fn handle_create_user(state: &AppState, token: &str, body: &str) -> Response {\n    match state.auth.validate_token(token) {\n        Ok(_) => Response::json(201, body),\n        Err(_) => Response::json(401, \"{\\\"error\\\": \\\"unauthorized\\\"}\"),\n    }\n}\n\npub fn handle_delete_user(state: &AppState, token: &str, user_id: u64) -> Response {\n    match state.auth.validate_token(token) {\n        Ok(claims) if claims.user_id == user_id => Response::json(200, \"{\\\"deleted\\\": true}\"),\n        Ok(_) => Response::json(403, \"{\\\"error\\\": \\\"forbidden\\\"}\"),\n        Err(_) => Response::json(401, \"{\\\"error\\\": \\\"unauthorized\\\"}\"),\n    }\n}\n\npub fn handle_health_check() -> Response { Response::json(200, \"{\\\"status\\\": \\\"ok\\\"}\") }\n\npub struct Response { pub status: u16, pub body: String }\nimpl Response { pub fn json(status: u16, body: &str) -> Self { Self { status, body: body.to_string() } } }\n";

const API_MIDDLEWARE: &str = "//! Request middleware: auth, logging, rate limiting.\n\npub struct RateLimiter {\n    max_requests: u64,\n    window_secs: u64,\n    counters: std::collections::HashMap<String, (u64, u64)>,\n}\n\nimpl RateLimiter {\n    pub fn new(max_requests: u64, window_secs: u64) -> Self {\n        Self { max_requests, window_secs, counters: std::collections::HashMap::new() }\n    }\n\n    pub fn check(&mut self, client_ip: &str) -> bool {\n        let now = current_time();\n        let entry = self.counters.entry(client_ip.to_string()).or_insert((0, now));\n        if now - entry.1 > self.window_secs {\n            *entry = (1, now);\n            true\n        } else if entry.0 < self.max_requests {\n            entry.0 += 1;\n            true\n        } else {\n            false\n        }\n    }\n}\n\npub fn log_request(method: &str, path: &str, status: u16, duration_ms: u64) {\n    println!(\"[{}] {} {} - {}ms\", method, path, status, duration_ms);\n}\n\npub fn extract_bearer_token(header: &str) -> Option<&str> {\n    header.strip_prefix(\"Bearer \")\n}\n\nfn current_time() -> u64 {\n    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs()\n}\n";

const API_ROUTES: &str = "//! Route definitions.\n\npub struct Router { routes: Vec<Route> }\npub struct Route { pub method: String, pub path: String, pub handler: String }\n\nimpl Router {\n    pub fn new() -> Self { Self { routes: Vec::new() } }\n    pub fn get(&mut self, path: &str, handler: &str) {\n        self.routes.push(Route { method: \"GET\".into(), path: path.into(), handler: handler.into() });\n    }\n    pub fn post(&mut self, path: &str, handler: &str) {\n        self.routes.push(Route { method: \"POST\".into(), path: path.into(), handler: handler.into() });\n    }\n    pub fn delete(&mut self, path: &str, handler: &str) {\n        self.routes.push(Route { method: \"DELETE\".into(), path: path.into(), handler: handler.into() });\n    }\n    pub fn match_route(&self, method: &str, path: &str) -> Option<&Route> {\n        self.routes.iter().find(|r| r.method == method && r.path == path)\n    }\n}\n";

const DB_MOD: &str = "//! Database abstraction layer.\nuse std::collections::HashMap;\nuse crate::auth::{AuthError, User};\n\npub struct Database { pub connection_string: String }\n\npub struct UserStore { users: HashMap<u64, User> }\n\nimpl UserStore {\n    pub fn new() -> Self { Self { users: HashMap::new() } }\n\n    pub fn find_by_username(&self, username: &str) -> Result<&User, AuthError> {\n        self.users.values().find(|u| u.username == username).ok_or(AuthError::InvalidCredentials)\n    }\n\n    pub fn find_by_id(&self, id: u64) -> Result<&User, AuthError> {\n        self.users.get(&id).ok_or(AuthError::DatabaseError(format!(\"user {} not found\", id)))\n    }\n\n    pub fn create_user(&mut self, username: &str, email: &str, password_hash: &str) -> u64 {\n        let id = self.users.len() as u64 + 1;\n        self.users.insert(id, User { id, username: username.to_string(), password_hash: password_hash.to_string(), email: email.to_string() });\n        id\n    }\n\n    pub fn delete_user(&mut self, id: u64) -> bool { self.users.remove(&id).is_some() }\n}\n\npub struct MigrationRunner { migrations: Vec<Migration> }\npub struct Migration { pub version: u64, pub name: String, pub up_sql: String, pub down_sql: String }\n\nimpl MigrationRunner {\n    pub fn new() -> Self { Self { migrations: Vec::new() } }\n    pub fn add_migration(&mut self, name: &str, up: &str, down: &str) {\n        let version = self.migrations.len() as u64 + 1;\n        self.migrations.push(Migration { version, name: name.to_string(), up_sql: up.to_string(), down_sql: down.to_string() });\n    }\n    pub fn run_pending(&self) -> Vec<String> {\n        self.migrations.iter().map(|m| format!(\"Applied: {} (v{})\", m.name, m.version)).collect()\n    }\n}\n";

const MAIN_RS: &str = "mod auth;\nmod api;\nmod db;\n\nfn main() {\n    println!(\"Starting server...\");\n    let store = db::UserStore::new();\n    let auth = auth::AuthManager::new(store, \"super_secret_key\");\n    println!(\"Auth manager initialized\");\n    println!(\"Server ready on :8080\");\n}\n";

const TEST_AUTH: &str = "//! Authentication integration tests.\n\n#[test]\nfn test_login_valid_credentials() {\n    let store = create_test_store();\n    let auth = AuthManager::new(store, \"test_secret\");\n    let token = auth.authenticate(\"admin\", \"password123\").unwrap();\n    assert!(token.starts_with(\"jwt.\"));\n}\n\n#[test]\nfn test_login_invalid_password() {\n    let store = create_test_store();\n    let auth = AuthManager::new(store, \"test_secret\");\n    let err = auth.authenticate(\"admin\", \"wrong\").unwrap_err();\n    assert!(matches!(err, AuthError::InvalidCredentials));\n}\n\n#[test]\nfn test_token_validation() {\n    let store = create_test_store();\n    let auth = AuthManager::new(store, \"test_secret\");\n    let token = auth.authenticate(\"admin\", \"password123\").unwrap();\n    let claims = auth.validate_token(&token).unwrap();\n    assert_eq!(claims.user_id, 1);\n}\n\n#[test]\nfn test_session_create_and_retrieve() {\n    let mut sessions = SessionStore::new(3600);\n    let id = sessions.create_session(42);\n    let session = sessions.get_session(&id).unwrap();\n    assert_eq!(session.user_id, 42);\n}\n";

const TEST_API: &str = "//! API endpoint tests.\n\n#[test]\nfn test_health_check() {\n    let resp = handle_health_check();\n    assert_eq!(resp.status, 200);\n}\n\n#[test]\nfn test_login_endpoint() {\n    let state = create_app_state();\n    let resp = handle_login(&state, \"admin\", \"password123\");\n    assert_eq!(resp.status, 200);\n}\n\n#[test]\nfn test_unauthorized_access() {\n    let state = create_app_state();\n    let resp = handle_get_user(&state, \"bad_token\", 1);\n    assert_eq!(resp.status, 401);\n}\n\n#[test]\nfn test_rate_limiting() {\n    let mut limiter = RateLimiter::new(3, 60);\n    assert!(limiter.check(\"192.168.1.1\"));\n    assert!(limiter.check(\"192.168.1.1\"));\n    assert!(limiter.check(\"192.168.1.1\"));\n    assert!(!limiter.check(\"192.168.1.1\"));\n}\n";

const CONFIG_JSON: &str = "{\"server\":{\"host\":\"0.0.0.0\",\"port\":8080},\"database\":{\"url\":\"postgres://localhost:5432/app\",\"pool_size\":10},\"auth\":{\"jwt_secret\":\"change_me\",\"session_ttl\":3600,\"oauth\":{\"github\":{\"client_id\":\"xxx\"},\"google\":{\"client_id\":\"aaa\"}}},\"rate_limit\":{\"max_requests\":100,\"window_secs\":60}}";

const DOC_ARCH: &str = "# Architecture\n\n## Authentication Flow\n\n1. Client sends credentials to /api/login\n2. Server validates credentials against database\n3. JWT token is generated and returned\n4. Client includes token in Authorization header\n5. Middleware validates token on each request\n\n## Database Schema\n\n- users: id, username, password_hash, email, created_at\n- sessions: id, user_id, created_at, expires_at\n- oauth_tokens: id, user_id, provider, access_token, refresh_token\n\n## Rate Limiting\n\nUses sliding window algorithm with per-IP counters.\n";

const DOC_API: &str = "# API Reference\n\n## POST /api/login\nAuthenticate user and receive JWT token.\n\n## GET /api/users/:id\nGet user profile (requires authentication).\n\n## POST /api/users\nCreate new user account.\n\n## DELETE /api/users/:id\nDelete user account (self-only).\n\n## GET /api/health\nHealth check endpoint.\n";

const CARGO_TOML: &str = "[package]\nname = \"test-project\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\nserde = { version = \"1\", features = [\"derive\"] }\nserde_json = \"1\"\n";

// ---------------------------------------------------------------------------
// Timing helper
// ---------------------------------------------------------------------------

fn time_dispatch(command: &str, args: &[&str], cwd: &str) -> (i32, String, std::time::Duration) {
    let args_owned: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    let start = Instant::now();
    let resp = dispatch(command, &args_owned, cwd);
    let elapsed = start.elapsed();
    (resp.exit_code, resp.stdout, elapsed)
}

fn count_lines(stdout: &str) -> usize {
    stdout.lines().filter(|l| !l.is_empty()).count()
}

// ---------------------------------------------------------------------------
// Benchmark: all modes side-by-side
// ---------------------------------------------------------------------------

#[test]
fn bench_all_modes_comparison() {
    let tmp = TempDir::new().unwrap();
    create_benchmark_corpus(tmp.path());
    let cwd = tmp.path().to_str().unwrap();

    let regex_pat = r"fn\s+\w+_token";

    let modes: Vec<(&str, &str, Vec<&str>)> = vec![
        ("find -name '*.rs'", "find", vec![".", "-name", "*.rs"]),
        ("fd -e rs", "fd", vec!["-e", "rs"]),
        ("cat file", "cat", vec!["src/auth/mod.rs"]),
        ("rg literal", "rg", vec!["-n", "authenticate", "."]),
        ("rg regex", "rg", vec!["-n", regex_pat, "."]),
        ("grep -rn", "grep", vec!["-rn", "AuthError", "."]),
        ("rg --bm25", "rg", vec!["--bm25", "-n", "token", "."]),
        (
            "rg --bm25=full",
            "rg",
            vec!["--bm25=full", "-n", "token", "."],
        ),
        (
            "rg --semantic",
            "rg",
            vec!["--semantic", "-n", "authentication", "."],
        ),
    ];

    eprintln!();
    eprintln!("================================================================");
    eprintln!("       BitScout Cold-Start Benchmark (no daemon)");
    eprintln!("================================================================");
    eprintln!(
        " {:22} | {:>8} | {:>8} | {:>8} | {:>5}",
        "Mode", "Avg(us)", "Min(us)", "Max(us)", "Lines"
    );
    eprintln!("----------------------------------------------------------------");

    for (label, cmd, args) in &modes {
        let args_s: Vec<&str> = args.iter().copied().collect();
        // Warmup
        time_dispatch(cmd, &args_s, cwd);

        let mut times = Vec::new();
        let mut last_lines = 0;
        for _ in 0..20 {
            let (_, stdout, dur) = time_dispatch(cmd, &args_s, cwd);
            last_lines = count_lines(&stdout);
            times.push(dur);
        }

        let avg = times.iter().map(|d| d.as_micros()).sum::<u128>() / times.len() as u128;
        let min = times.iter().map(|d| d.as_micros()).min().unwrap();
        let max = times.iter().map(|d| d.as_micros()).max().unwrap();

        eprintln!(
            " {:22} | {:>8} | {:>8} | {:>8} | {:>5}",
            label, avg, min, max, last_lines
        );
    }

    eprintln!("================================================================");
    eprintln!(" Corpus: 16 files, ~700 lines, realistic Rust web project");
    eprintln!(" Each mode: 20 runs, cold-start (FileTree scan every call)");
    eprintln!();
}

/// Benchmark: semantic search relevance quality check
#[test]
fn bench_semantic_relevance_quality() {
    let tmp = TempDir::new().unwrap();
    create_benchmark_corpus(tmp.path());
    let cwd = tmp.path().to_str().unwrap();

    eprintln!();
    eprintln!("=== Semantic Search Relevance Quality ===");

    let queries = [
        ("user login flow", "Should rank auth files highest"),
        ("database migration", "Should find db/mod.rs"),
        ("rate limiting middleware", "Should find api/middleware.rs"),
        ("JWT token validation", "Should find auth/jwt.rs"),
        ("session expiration", "Should find auth/session.rs"),
    ];

    for (query, expected) in &queries {
        let args_owned: Vec<String> = vec![
            "--semantic".into(),
            "-n".into(),
            query.to_string(),
            ".".into(),
        ];
        let start = Instant::now();
        let resp = dispatch("rg", &args_owned, cwd);
        let dur = start.elapsed();

        let lines: Vec<&str> = resp.stdout.lines().take(3).collect();
        eprintln!();
        eprintln!("  Query: \"{}\" ({}us)", query, dur.as_micros());
        eprintln!("  Expected: {}", expected);
        eprintln!("  Top results:");
        for line in &lines {
            eprintln!("    {}", line);
        }
        if lines.is_empty() {
            eprintln!("    (no results)");
        }
    }
    eprintln!();
}

/// Benchmark: BM25 scoring output inspection
#[test]
fn bench_bm25_scoring_output() {
    let tmp = TempDir::new().unwrap();
    create_benchmark_corpus(tmp.path());
    let cwd = tmp.path().to_str().unwrap();

    eprintln!();
    eprintln!("=== BM25 Scoring Output ===");

    // BM25-TF
    let (_, stdout, dur) = time_dispatch("rg", &["--bm25", "-n", "token", "."], cwd);
    eprintln!();
    eprintln!(
        "  rg --bm25 'token' ({}us, {} lines):",
        dur.as_micros(),
        count_lines(&stdout)
    );
    for line in stdout.lines().take(8) {
        eprintln!("    {}", line);
    }

    // BM25-Full (TF-IDF)
    let (_, stdout, dur) = time_dispatch("rg", &["--bm25=full", "-n", "token", "."], cwd);
    eprintln!();
    eprintln!(
        "  rg --bm25=full 'token' ({}us, {} lines):",
        dur.as_micros(),
        count_lines(&stdout)
    );
    for line in stdout.lines().take(8) {
        eprintln!("    {}", line);
    }

    // Semantic
    let (_, stdout, dur) = time_dispatch("rg", &["--semantic", "-n", "token", "."], cwd);
    eprintln!();
    eprintln!(
        "  rg --semantic 'token' ({}us, {} lines):",
        dur.as_micros(),
        count_lines(&stdout)
    );
    for line in stdout.lines().take(8) {
        eprintln!("    {}", line);
    }
    eprintln!();
}
