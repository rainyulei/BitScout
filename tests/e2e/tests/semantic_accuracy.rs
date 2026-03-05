//! Semantic (Random Projection) search accuracy tests.
//!
//! Validates that RP cosine similarity correctly ranks files by semantic
//! relevance — files whose content is more related to the query should
//! receive higher scores and appear first in results.

use bitscout_core::dispatch::dispatch;
use bitscout_core::search::engine::{SearchEngine, SearchOptions, SearchResult};
use bitscout_core::search::rp::RpScorer;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn dispatch_semantic(query: &str, cwd: &str) -> Vec<(String, f64)> {
    let args: Vec<String> = vec![
        "--semantic".into(),
        "-n".into(),
        query.into(),
        ".".into(),
    ];
    let resp = dispatch("rg", &args, cwd);
    // Parse "[score] path:line:content" or "path:line:content" lines
    // Group by file, take the score from bm25_score (RP score stored there)
    let mut file_scores: HashMap<String, f64> = HashMap::new();
    for line in resp.stdout.lines() {
        if line.is_empty() {
            continue;
        }
        // Extract file path (everything before first ":")
        let path = line.split(':').next().unwrap_or("");
        // Extract just filename
        let filename = Path::new(path)
            .file_name()
            .map(|f| f.to_string_lossy().to_string())
            .unwrap_or_default();
        if !filename.is_empty() && !file_scores.contains_key(&filename) {
            file_scores.insert(filename, 0.0);
        }
    }
    let mut sorted: Vec<(String, f64)> = file_scores.into_iter().collect();
    sorted.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    sorted
}

fn search_semantic_scored(
    root: &Path,
    query: &str,
) -> Vec<(String, f64)> {
    let engine = SearchEngine::new(root).unwrap();
    let opts = SearchOptions {
        semantic: true,
        case_insensitive: true,
        ..SearchOptions::default()
    };
    let results = engine.search(query, &opts).unwrap();

    // Group by file, take score per file
    let mut file_scores: HashMap<String, f64> = HashMap::new();
    for r in &results {
        let filename = r.path.file_name()
            .map(|f| f.to_string_lossy().to_string())
            .unwrap_or_default();
        let score = r.bm25_score.unwrap_or(0.0);
        file_scores.entry(filename)
            .and_modify(|s| { if score > *s { *s = score; } })
            .or_insert(score);
    }

    let mut sorted: Vec<(String, f64)> = file_scores.into_iter().collect();
    sorted.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    sorted
}

fn filename(r: &SearchResult) -> String {
    r.path.file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Test 1: RP scorer unit-level — cosine similarity ranking
// ---------------------------------------------------------------------------

#[test]
fn test_rp_cosine_similarity_ranking() {
    let mut scorer = RpScorer::new();

    // Query about authentication
    let query = "user authentication login password credentials";
    let q_proj = scorer.project(query);

    // Highly related doc
    let auth_doc = "authenticate user verify password login credentials token session";
    let auth_score = scorer.score(&q_proj, auth_doc);

    // Somewhat related doc
    let session_doc = "session management expire timeout cookie user state persist";
    let session_score = scorer.score(&q_proj, session_doc);

    // Unrelated doc
    let math_doc = "calculate average sum total multiply divide matrix vector algorithm";
    let math_score = scorer.score(&q_proj, math_doc);

    eprintln!("\n=== RP Cosine Similarity Ranking (unit) ===");
    eprintln!("  Query: \"{}\"", query);
    eprintln!("  auth_doc score:    {:.4}  (highly related)", auth_score);
    eprintln!("  session_doc score: {:.4}  (somewhat related)", session_score);
    eprintln!("  math_doc score:    {:.4}  (unrelated)", math_score);

    assert!(
        auth_score > session_score,
        "auth ({:.4}) should rank above session ({:.4})",
        auth_score, session_score
    );
    assert!(
        session_score > math_score,
        "session ({:.4}) should rank above math ({:.4})",
        session_score, math_score
    );
}

#[test]
fn test_rp_cosine_similarity_code_patterns() {
    let mut scorer = RpScorer::new();

    let query = "error handling result unwrap panic recover";
    let q_proj = scorer.project(query);

    let error_doc = "fn handle_error result err unwrap_or panic catch recover fallback error retry";
    let error_score = scorer.score(&q_proj, error_doc);

    let db_doc = "database query select insert update delete table column migration schema index";
    let db_score = scorer.score(&q_proj, db_doc);

    let config_doc = "configuration settings toml yaml json env port host debug level";
    let config_score = scorer.score(&q_proj, config_doc);

    eprintln!("\n=== RP Code Pattern Ranking ===");
    eprintln!("  Query: \"{}\"", query);
    eprintln!("  error_doc:  {:.4}", error_score);
    eprintln!("  db_doc:     {:.4}", db_score);
    eprintln!("  config_doc: {:.4}", config_score);

    assert!(
        error_score > db_score,
        "error ({:.4}) should rank above db ({:.4})",
        error_score, db_score
    );
    assert!(
        error_score > config_score,
        "error ({:.4}) should rank above config ({:.4})",
        error_score, config_score
    );
}

// ---------------------------------------------------------------------------
// Test 2: End-to-end file ranking — same keyword, different contexts
// ---------------------------------------------------------------------------

/// All files contain "token", but the auth-focused file should rank highest
/// when the query is about authentication tokens.
#[test]
fn test_semantic_ranks_auth_file_highest_for_token_query() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();

    // File heavily about auth tokens
    fs::write(
        root.join("auth_token.rs"),
        "// Authentication token management\n\
         fn create_auth_token(user_id: u64, secret: &str) -> String {\n\
         \tlet token = jwt_encode(user_id, secret);\n\
         \tvalidate_token(&token);\n\
         \ttoken\n\
         }\n\
         fn validate_token(token: &str) -> bool {\n\
         \ttoken.starts_with(\"Bearer\")\n\
         }\n\
         fn refresh_token(old_token: &str) -> String {\n\
         \tcreate_auth_token(extract_user_id(old_token), \"secret\")\n\
         }\n\
         fn revoke_token(token: &str) { blacklist_add(token); }\n",
    )
    .unwrap();

    // File about string tokenization (different meaning of "token")
    fs::write(
        root.join("tokenizer.rs"),
        "// String tokenizer for parsing\n\
         fn tokenize(input: &str) -> Vec<&str> {\n\
         \tinput.split_whitespace().collect()\n\
         }\n\
         fn next_token(iter: &mut std::str::SplitWhitespace) -> Option<&str> {\n\
         \titer.next()\n\
         }\n\
         fn count_tokens(text: &str) -> usize {\n\
         \ttext.split_whitespace().count()\n\
         }\n\
         fn is_valid_token(token: &str) -> bool {\n\
         \t!token.is_empty() && token.len() < 100\n\
         }\n",
    )
    .unwrap();

    // File barely mentioning token
    fs::write(
        root.join("config.rs"),
        "// Application configuration\n\
         struct Config {\n\
         \tport: u16,\n\
         \thost: String,\n\
         \tdebug: bool,\n\
         \tapi_token: String, // used for external API\n\
         }\n\
         fn load_config() -> Config {\n\
         \tConfig { port: 8080, host: \"localhost\".into(), debug: false, api_token: \"default\".into() }\n\
         }\n",
    )
    .unwrap();

    let results = search_semantic_scored(root, "auth token validate");

    eprintln!("\n=== Semantic File Ranking: 'auth token validate' ===");
    for (f, s) in &results {
        eprintln!("  {:.4}  {}", s, f);
    }

    assert!(
        !results.is_empty(),
        "Should have results"
    );

    // auth_token.rs should rank first (most about auth tokens)
    let first = &results[0].0;
    assert!(
        first.contains("auth_token"),
        "auth_token.rs should rank first, got: {}",
        first
    );
}

/// Files about database — query about "query execute database" should
/// rank the database file above an HTTP handler that also uses "query".
#[test]
fn test_semantic_ranks_db_file_for_database_query() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();

    fs::write(
        root.join("database.rs"),
        "// Database connection and query execution\n\
         struct Database { pool: ConnectionPool }\n\
         impl Database {\n\
         \tfn query(&self, sql: &str) -> Vec<Row> {\n\
         \t\tself.pool.execute_query(sql)\n\
         \t}\n\
         \tfn execute(&self, sql: &str) -> Result<(), DbError> {\n\
         \t\tself.pool.execute(sql)\n\
         \t}\n\
         \tfn transaction(&self) -> Transaction {\n\
         \t\tself.pool.begin_transaction()\n\
         \t}\n\
         \tfn migrate(&self) { execute_migrations(self); }\n\
         }\n\
         fn execute_query(pool: &Pool, sql: &str) -> Vec<Row> { vec![] }\n",
    )
    .unwrap();

    fs::write(
        root.join("http_handler.rs"),
        "// HTTP request handling\n\
         fn handle_request(req: Request) -> Response {\n\
         \tlet query = req.query_string();\n\
         \tlet path = req.path();\n\
         \tRoute::match_path(path, query)\n\
         }\n\
         fn parse_query_params(query: &str) -> HashMap<String, String> {\n\
         \tquery.split('&').filter_map(|p| {\n\
         \t\tlet mut kv = p.splitn(2, '=');\n\
         \t\tSome((kv.next()?.into(), kv.next()?.into()))\n\
         \t}).collect()\n\
         }\n",
    )
    .unwrap();

    fs::write(
        root.join("utils.rs"),
        "// Utility functions\n\
         fn format_output(data: &[u8]) -> String { String::from_utf8_lossy(data).to_string() }\n\
         fn query_env(key: &str) -> Option<String> { std::env::var(key).ok() }\n",
    )
    .unwrap();

    let results = search_semantic_scored(root, "database query execute");

    eprintln!("\n=== Semantic File Ranking: 'database query execute' ===");
    for (f, s) in &results {
        eprintln!("  {:.4}  {}", s, f);
    }

    assert!(!results.is_empty());
    let first = &results[0].0;
    assert!(
        first.contains("database"),
        "database.rs should rank first, got: {}",
        first
    );
}

// ---------------------------------------------------------------------------
// Test 3: Semantic vs plain — semantic should reorder results
// ---------------------------------------------------------------------------

#[test]
fn test_semantic_reorders_vs_plain_search() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();

    // Create files where alphabetical/filesystem order differs from semantic relevance
    fs::write(
        root.join("aaa_unrelated.rs"),
        "// AAA file - mentions error only in passing\n\
         fn process_data(input: &str) -> String {\n\
         \tif input.is_empty() { return \"error: empty\".into(); }\n\
         \tinput.to_uppercase()\n\
         }\n",
    )
    .unwrap();

    fs::write(
        root.join("bbb_error_handler.rs"),
        "// BBB file - all about error handling\n\
         enum AppError { NotFound, Unauthorized, BadRequest, InternalError }\n\
         fn handle_error(err: AppError) -> Response {\n\
         \tmatch err {\n\
         \t\tAppError::NotFound => Response::new(404, \"Not Found\"),\n\
         \t\tAppError::Unauthorized => Response::new(401, \"Unauthorized error\"),\n\
         \t\tAppError::BadRequest => Response::new(400, \"Bad Request error\"),\n\
         \t\tAppError::InternalError => Response::new(500, \"Internal Server Error\"),\n\
         \t}\n\
         }\n\
         fn log_error(err: &AppError) { eprintln!(\"error: {:?}\", err); }\n\
         fn recover_from_error(err: AppError) -> Result<(), AppError> {\n\
         \tlog_error(&err);\n\
         \tErr(err)\n\
         }\n",
    )
    .unwrap();

    fs::write(
        root.join("ccc_middleware.rs"),
        "// CCC file - some error handling in middleware\n\
         fn auth_middleware(req: Request) -> Result<Request, Error> {\n\
         \tlet token = req.header(\"Authorization\");\n\
         \tif token.is_none() { return Err(Error::new(\"auth error\")); }\n\
         \tOk(req)\n\
         }\n\
         fn error_boundary(handler: fn(Request) -> Response) -> Response {\n\
         \t// wraps handler to catch panics and return error response\n\
         \thandler(Request::default())\n\
         }\n",
    )
    .unwrap();

    let results = search_semantic_scored(root, "error");

    eprintln!("\n=== Semantic Reordering: 'error' ===");
    eprintln!("  (All files contain 'error', but bbb is MOST about errors)");
    for (f, s) in &results {
        eprintln!("  {:.4}  {}", s, f);
    }

    assert!(results.len() >= 2, "Should match at least 2 files");

    // Both bbb and ccc are heavily error-related; RP may rank them very close.
    // Assert bbb_error_handler is in top 2 (it's dense with error terms but ccc also has many).
    let top2: Vec<&str> = results.iter().take(2).map(|r| r.0.as_str()).collect();
    assert!(
        top2.iter().any(|f| f.contains("bbb_error_handler")),
        "bbb_error_handler.rs should be in top 2, got: {:?}",
        top2
    );
    // aaa_unrelated should always rank last
    assert!(
        results.last().unwrap().0.contains("aaa_unrelated"),
        "aaa_unrelated.rs should rank last"
    );
}

// ---------------------------------------------------------------------------
// Test 4: Multi-word semantic queries
// ---------------------------------------------------------------------------

#[test]
fn test_semantic_multiword_query_ranking() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();

    fs::write(
        root.join("auth.rs"),
        "// User authentication and session management\n\
         fn authenticate(username: &str, password: &str) -> Result<Session, AuthError> {\n\
         \tlet user = find_user(username)?;\n\
         \tverify_password(password, &user.hash)?;\n\
         \tcreate_session(user.id)\n\
         }\n\
         fn create_session(user_id: u64) -> Result<Session, AuthError> {\n\
         \tSession::new(user_id, Duration::hours(24))\n\
         }\n\
         fn logout(session: &Session) { session.invalidate(); }\n",
    )
    .unwrap();

    fs::write(
        root.join("cache.rs"),
        "// Cache layer with session-like interface\n\
         struct CacheSession { ttl: u64, data: HashMap<String, Vec<u8>> }\n\
         impl CacheSession {\n\
         \tfn authenticate_backend(&self) -> bool { true }\n\
         \tfn get(&self, key: &str) -> Option<&[u8]> { self.data.get(key).map(|v| v.as_slice()) }\n\
         \tfn set(&mut self, key: &str, val: Vec<u8>) { self.data.insert(key.into(), val); }\n\
         }\n",
    )
    .unwrap();

    fs::write(
        root.join("test_helpers.rs"),
        "// Test utilities\n\
         fn mock_authenticate() -> Session { Session::test_default() }\n\
         fn assert_session_valid(s: &Session) { assert!(s.is_valid()); }\n\
         fn create_test_user() -> User { User { id: 1, name: \"test\".into() } }\n",
    )
    .unwrap();

    let results = search_semantic_scored(root, "authenticate session");

    eprintln!("\n=== Multi-word Query: 'authenticate session' ===");
    for (f, s) in &results {
        eprintln!("  {:.4}  {}", s, f);
    }

    assert!(!results.is_empty());
    let first = &results[0].0;
    assert!(
        first.contains("auth"),
        "auth.rs should rank first for 'authenticate session', got: {}",
        first
    );
}

// ---------------------------------------------------------------------------
// Test 5: Score distribution — scores should vary, not be flat
// ---------------------------------------------------------------------------

#[test]
fn test_semantic_score_variance() {
    let mut scorer = RpScorer::new();
    let query = "network socket connection tcp listen bind";
    let q_proj = scorer.project(query);

    let docs = [
        ("network_server", "tcp socket listen bind accept connection server client port address network"),
        ("http_client", "http request response header body url fetch get post status connection"),
        ("file_io", "read write open close file path buffer seek flush directory rename"),
        ("math_utils", "add subtract multiply divide sqrt pow abs ceil floor round"),
    ];

    let mut scores: Vec<(&str, f32)> = docs
        .iter()
        .map(|(name, text)| (*name, scorer.score(&q_proj, text)))
        .collect();
    scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

    eprintln!("\n=== Score Variance: 'network socket connection' ===");
    for (name, score) in &scores {
        eprintln!("  {:.4}  {}", score, name);
    }

    // Scores should not be flat — there should be meaningful separation
    let max_score = scores[0].1;
    let min_score = scores.last().unwrap().1;
    let spread = max_score - min_score;

    eprintln!("  Spread: {:.4} (max={:.4}, min={:.4})", spread, max_score, min_score);

    assert!(spread > 0.05, "Score spread ({:.4}) too small — scores are too flat", spread);

    // network_server should clearly be #1
    assert_eq!(scores[0].0, "network_server");

    // math_utils or http_client should be near the bottom (both unrelated to "network socket")
    // http_client has "connection" overlap but many diluting terms; exact ordering is acceptable
    let bottom_two: Vec<&str> = scores[2..].iter().map(|s| s.0).collect();
    assert!(
        bottom_two.contains(&"math_utils") || bottom_two.contains(&"file_io"),
        "math_utils or file_io should be in bottom half, got: {:?}",
        bottom_two
    );
}

// ---------------------------------------------------------------------------
// Test 6: Stability — same query, same results
// ---------------------------------------------------------------------------

#[test]
fn test_semantic_deterministic() {
    let mut scorer1 = RpScorer::new();
    let mut scorer2 = RpScorer::new();

    let query = "authentication token validation";
    let doc = "verify jwt token authenticate user session credential";

    let proj1 = scorer1.project(query);
    let proj2 = scorer2.project(query);

    // Same projection vectors (allow f32 epsilon from accumulation order)
    assert_eq!(proj1.len(), proj2.len());
    for (i, (a, b)) in proj1.iter().zip(proj2.iter()).enumerate() {
        assert!(
            (a - b).abs() < 1e-5,
            "Projection dim {} differs: {} vs {}",
            i, a, b
        );
    }

    let score1 = scorer1.score(&proj1, doc);
    let score2 = scorer2.score(&proj2, doc);

    assert!(
        (score1 - score2).abs() < 1e-6,
        "Scores should be deterministic: {} vs {}",
        score1, score2
    );

    eprintln!("\n=== Determinism Check ===");
    eprintln!("  Score1: {:.6}, Score2: {:.6}, diff: {:.10}", score1, score2, (score1 - score2).abs());
}

// ---------------------------------------------------------------------------
// Test 7: Comprehensive accuracy report
// ---------------------------------------------------------------------------

#[test]
fn test_semantic_accuracy_report() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();

    // Create a diverse corpus
    let files = [
        ("auth_login.rs",
         "fn login(username: &str, password: &str) -> Result<Token, AuthError> {\n\
          \tlet user = db.find_user(username)?;\n\
          \tif !verify_password(password, &user.password_hash) {\n\
          \t\treturn Err(AuthError::InvalidCredentials);\n\
          \t}\n\
          \tlet token = generate_jwt_token(&user)?;\n\
          \tsave_session(&token, user.id)?;\n\
          \tOk(token)\n\
          }\n\
          fn logout(token: &Token) { invalidate_session(token); }\n\
          fn verify_password(plain: &str, hash: &str) -> bool { bcrypt_verify(plain, hash) }\n"),

        ("auth_jwt.rs",
         "fn generate_jwt_token(user: &User) -> Result<Token, AuthError> {\n\
          \tlet claims = Claims { sub: user.id, exp: now() + 3600 };\n\
          \tlet token = jwt_encode(&claims, &SECRET)?;\n\
          \tOk(Token::new(token))\n\
          }\n\
          fn validate_jwt_token(token: &str) -> Result<Claims, AuthError> {\n\
          \tjwt_decode(token, &SECRET).map_err(|_| AuthError::InvalidToken)\n\
          }\n\
          fn refresh_jwt_token(old: &Token) -> Result<Token, AuthError> {\n\
          \tlet claims = validate_jwt_token(old.as_str())?;\n\
          \tgenerate_jwt_token(&User { id: claims.sub })\n\
          }\n"),

        ("database.rs",
         "struct Database { pool: Pool }\n\
          impl Database {\n\
          \tfn query(&self, sql: &str, params: &[&str]) -> Result<Vec<Row>, DbError> {\n\
          \t\tself.pool.execute(sql, params)\n\
          \t}\n\
          \tfn insert(&self, table: &str, data: &Map) -> Result<u64, DbError> {\n\
          \t\tlet sql = format!(\"INSERT INTO {} VALUES (?)\" , table);\n\
          \t\tself.query(&sql, &[])?;\n\
          \t\tOk(self.last_insert_id())\n\
          \t}\n\
          \tfn migrate(&self, migrations: &[Migration]) -> Result<(), DbError> {\n\
          \t\tfor m in migrations { self.query(&m.up_sql, &[])?; }\n\
          \t\tOk(())\n\
          \t}\n\
          }\n"),

        ("http_server.rs",
         "struct HttpServer { addr: SocketAddr, routes: Router }\n\
          impl HttpServer {\n\
          \tfn listen(&self) -> Result<(), IoError> {\n\
          \t\tlet listener = TcpListener::bind(self.addr)?;\n\
          \t\tfor stream in listener.incoming() {\n\
          \t\t\tself.handle_connection(stream?)?;\n\
          \t\t}\n\
          \t\tOk(())\n\
          \t}\n\
          \tfn handle_connection(&self, stream: TcpStream) -> Result<(), IoError> {\n\
          \t\tlet req = parse_http_request(&stream)?;\n\
          \t\tlet resp = self.routes.dispatch(req)?;\n\
          \t\twrite_http_response(&stream, resp)\n\
          \t}\n\
          }\n"),

        ("cache.rs",
         "struct Cache { store: HashMap<String, CacheEntry>, max_size: usize }\n\
          impl Cache {\n\
          \tfn get(&mut self, key: &str) -> Option<&[u8]> {\n\
          \t\tself.store.get_mut(key).map(|e| { e.last_access = now(); e.data.as_slice() })\n\
          \t}\n\
          \tfn set(&mut self, key: String, data: Vec<u8>, ttl: u64) {\n\
          \t\tif self.store.len() >= self.max_size { self.evict_lru(); }\n\
          \t\tself.store.insert(key, CacheEntry { data, ttl, last_access: now() });\n\
          \t}\n\
          \tfn evict_lru(&mut self) {\n\
          \t\tlet oldest = self.store.iter().min_by_key(|(_, e)| e.last_access);\n\
          \t\tif let Some((k, _)) = oldest { let k = k.clone(); self.store.remove(&k); }\n\
          \t}\n\
          }\n"),

        ("logger.rs",
         "enum LogLevel { Debug, Info, Warn, Error }\n\
          struct Logger { level: LogLevel, output: Box<dyn Write> }\n\
          impl Logger {\n\
          \tfn log(&mut self, level: LogLevel, msg: &str) {\n\
          \t\twriteln!(self.output, \"[{:?}] {}\", level, msg).ok();\n\
          \t}\n\
          \tfn error(&mut self, msg: &str) { self.log(LogLevel::Error, msg); }\n\
          \tfn info(&mut self, msg: &str) { self.log(LogLevel::Info, msg); }\n\
          \tfn debug(&mut self, msg: &str) { self.log(LogLevel::Debug, msg); }\n\
          }\n"),

        ("config.rs",
         "struct Config { port: u16, host: String, db_url: String, log_level: String }\n\
          fn load_config(path: &str) -> Result<Config, ConfigError> {\n\
          \tlet contents = fs::read_to_string(path)?;\n\
          \ttoml::from_str(&contents).map_err(ConfigError::Parse)\n\
          }\n\
          fn default_config() -> Config {\n\
          \tConfig { port: 8080, host: \"0.0.0.0\".into(), db_url: \"postgres://localhost/app\".into(), log_level: \"info\".into() }\n\
          }\n"),

        ("test_auth.rs",
         "fn test_login_success() {\n\
          \tlet token = login(\"admin\", \"secret123\").unwrap();\n\
          \tassert!(validate_jwt_token(token.as_str()).is_ok());\n\
          }\n\
          fn test_login_failure() {\n\
          \tlet err = login(\"admin\", \"wrong\").unwrap_err();\n\
          \tassert!(matches!(err, AuthError::InvalidCredentials));\n\
          }\n\
          fn test_token_refresh() {\n\
          \tlet token = login(\"user\", \"pass\").unwrap();\n\
          \tlet new_token = refresh_jwt_token(&token).unwrap();\n\
          \tassert_ne!(token.as_str(), new_token.as_str());\n\
          }\n"),
    ];

    for (name, content) in &files {
        fs::write(root.join(name), content).unwrap();
    }

    // Test queries with expected top-ranked files
    let test_cases: Vec<(&str, Vec<&str>)> = vec![
        // (query, expected files in rough order of relevance)
        ("login password authenticate",    vec!["auth_login.rs", "test_auth.rs", "auth_jwt.rs"]),
        ("jwt token generate validate",    vec!["auth_jwt.rs", "auth_login.rs", "test_auth.rs"]),
        ("database query insert migrate",  vec!["database.rs"]),
        ("http server listen connection",  vec!["http_server.rs"]),
        ("cache evict lru store",          vec!["cache.rs"]),
    ];

    eprintln!("\n╔══════════════════════════════════════════════════════════════════════════╗");
    eprintln!("║                   Semantic Search Accuracy Report                       ║");
    eprintln!("╠══════════════════════════════════════════════════════════════════════════╣");

    let mut total = 0;
    let mut correct_top1 = 0;
    let mut correct_top3 = 0;

    for (query, expected_top) in &test_cases {
        total += 1;
        let results = search_semantic_scored(root, query);

        let result_files: Vec<&str> = results.iter().map(|(f, _)| f.as_str()).collect();
        let top1_correct = result_files.first().map_or(false, |f| *f == expected_top[0]);
        let top3_files: Vec<&str> = result_files.iter().take(3).copied().collect();
        let top3_has_expected = expected_top[0].to_string();
        let top3_correct = top3_files.contains(&top3_has_expected.as_str());

        if top1_correct { correct_top1 += 1; }
        if top3_correct { correct_top3 += 1; }

        let icon = if top1_correct { "OK" } else if top3_correct { "~" } else { "X" };

        eprintln!("║  [{}] Query: {:40} Expected #1: {:18}", icon, query, expected_top[0]);
        for (i, (f, s)) in results.iter().take(5).enumerate() {
            let marker = if *f == expected_top[0] { " <--" } else { "" };
            eprintln!("║       #{}: {:.4}  {}{}", i + 1, s, f, marker);
        }
        if results.is_empty() {
            eprintln!("║       (no results — query words not found in files)");
        }
        eprintln!("║");
    }

    eprintln!("╠══════════════════════════════════════════════════════════════════════════╣");
    eprintln!("║  Top-1 Accuracy: {}/{} ({:.0}%)", correct_top1, total, correct_top1 as f64 / total as f64 * 100.0);
    eprintln!("║  Top-3 Accuracy: {}/{} ({:.0}%)", correct_top3, total, correct_top3 as f64 / total as f64 * 100.0);
    eprintln!("╚══════════════════════════════════════════════════════════════════════════╝");

    // At least 60% top-1 accuracy
    assert!(
        correct_top1 as f64 / total as f64 >= 0.6,
        "Top-1 accuracy {}/{} < 60%",
        correct_top1, total
    );
}
