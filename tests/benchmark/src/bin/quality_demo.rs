//! Text extraction quality & BM25 relevance ranking demo
//! Run: cargo run --release -p bitscout-benchmark --bin quality_demo

use bitscout_core::extract::pipeline::extract_text;
use bitscout_core::search::bm25::Bm25Scorer;
use bitscout_core::search::engine::{SearchEngine, SearchOptions};
use flate2::write::GzEncoder;
use flate2::Compression;
use std::fs;
use std::io::Write;
use std::path::Path;
use tempfile::TempDir;

fn separator(title: &str) {
    println!("\n============================================================");
    println!("  {title}");
    println!("============================================================");
}

fn show_extracted_text(label: &str, path: &Path) {
    println!(
        "\n  [{label}] {}",
        path.file_name().unwrap().to_string_lossy()
    );
    match extract_text(path) {
        Ok(text) => {
            let lines: Vec<&str> = text.lines().collect();
            let total = lines.len();
            println!("  Extracted {} lines, {} bytes", total, text.len());
            println!("  --- Preview (first 8 lines) ---");
            for (i, line) in lines.iter().take(8).enumerate() {
                let truncated = if line.len() > 80 { &line[..80] } else { line };
                println!("    {:>3}| {truncated}", i + 1);
            }
            if total > 8 {
                println!("    ... ({} more lines)", total - 8);
            }
        }
        Err(e) => println!("  ERROR: {e}"),
    }
}

fn demo_bm25_ranking(dir: &Path) {
    separator("BM25 Relevance Ranking Demo");
    println!("  Query: \"authentication\"");
    println!();

    let engine = SearchEngine::new(dir).unwrap();
    let results = engine
        .search(
            "authentication",
            &SearchOptions {
                context_lines: 1,
                max_results: 100,
                ..Default::default()
            },
        )
        .unwrap();

    // Collect per-file stats for BM25 scoring
    struct FileStats {
        path: String,
        tf: usize,       // term frequency in this file
        doc_len: usize,  // lines in this file
        preview: String, // first matching line
    }

    let mut file_stats: std::collections::HashMap<String, FileStats> =
        std::collections::HashMap::new();
    let mut total_doc_len: usize = 0;
    let mut doc_count: usize = 0;

    // Count files and aggregate stats
    for entry in bitscout_core::fs::tree::FileTree::scan(dir)
        .unwrap()
        .files()
    {
        if let Ok(text) = extract_text(&entry.path) {
            let lines = text.lines().count();
            total_doc_len += lines;
            doc_count += 1;
        }
    }

    for r in &results {
        let fname = r.path.file_name().unwrap().to_string_lossy().to_string();
        let entry = file_stats.entry(fname.clone()).or_insert(FileStats {
            path: fname,
            tf: 0,
            doc_len: 0,
            preview: r.line_content.clone(),
        });
        entry.tf += 1;
        // Get doc length
        if entry.doc_len == 0 {
            if let Ok(text) = extract_text(&r.path) {
                entry.doc_len = text.lines().count();
            }
        }
    }

    let avg_doc_len = total_doc_len as f64 / doc_count.max(1) as f64;
    let scorer = Bm25Scorer::new(doc_count, avg_doc_len);
    let df = file_stats.len(); // document frequency

    let mut scored: Vec<(String, f64, usize, usize, String)> = file_stats
        .values()
        .map(|fs| {
            let score = scorer.score(fs.tf, fs.doc_len, df);
            (
                fs.path.clone(),
                score,
                fs.tf,
                fs.doc_len,
                fs.preview.clone(),
            )
        })
        .collect();

    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

    println!(
        "  {:>4} | {:>7} | {:>3} | {:>5} | File / Preview",
        "Rank", "BM25", "TF", "Lines"
    );
    println!(
        "  {:->4}-+-{:->7}-+-{:->3}-+-{:->5}-+-{:->40}",
        "", "", "", "", ""
    );

    for (i, (path, score, tf, doc_len, preview)) in scored.iter().take(15).enumerate() {
        let short_preview = if preview.len() > 38 {
            format!("{}...", &preview[..35])
        } else {
            preview.clone()
        };
        println!(
            "  {:>4} | {:>7.3} | {:>3} | {:>5} | {} — {}",
            i + 1,
            score,
            tf,
            doc_len,
            path,
            short_preview.trim()
        );
    }

    println!("\n  BM25 params: k1=1.2, b=0.75, N={doc_count}, avgDL={avg_doc_len:.1}, df={df}");
    println!("  Higher score = more relevant (rare term + high density + short doc)");
}

fn main() {
    println!("╔══════════════════════════════════════════════════════════╗");
    println!("║     BitScout — Text Quality & Relevance Demo           ║");
    println!("╚══════════════════════════════════════════════════════════╝");

    let tmp = TempDir::new().unwrap();
    let root = tmp.path();

    // ── 1. Create test files of various formats ──
    separator("1. Text Extraction Quality by Format");

    // Plain text
    let plain_path = root.join("auth_module.rs");
    fs::write(
        &plain_path,
        r#"use jwt::{decode, Validation};

pub struct AuthService {
    secret: String,
}

impl AuthService {
    pub fn verify_token(&self, token: &str) -> Result<Claims, AuthError> {
        let data = decode::<Claims>(token, self.secret.as_bytes(), &Validation::default())?;
        Ok(data.claims)
    }

    pub fn create_session(&self, user_id: u64) -> Session {
        Session::new(user_id, Duration::hours(24))
    }
}
"#,
    )
    .unwrap();
    show_extracted_text("Plain Text (.rs)", &plain_path);

    // Gzip
    let gz_path = root.join("database_config.sql.gz");
    let sql_content = r#"-- Database migration: authentication tables
CREATE TABLE users (
    id SERIAL PRIMARY KEY,
    email VARCHAR(255) UNIQUE NOT NULL,
    password_hash BYTEA NOT NULL,
    created_at TIMESTAMP DEFAULT NOW()
);

CREATE TABLE sessions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id INTEGER REFERENCES users(id),
    token TEXT NOT NULL,
    expires_at TIMESTAMP NOT NULL
);

CREATE INDEX idx_sessions_token ON sessions(token);
CREATE INDEX idx_users_email ON users(email);

-- Seed data for authentication testing
INSERT INTO users (email, password_hash) VALUES
    ('admin@example.com', '\xdeadbeef'),
    ('user@example.com', '\xcafebabe');
"#;
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(sql_content.as_bytes()).unwrap();
    let compressed = encoder.finish().unwrap();
    fs::write(&gz_path, &compressed).unwrap();
    show_extracted_text("Gzip (.sql.gz)", &gz_path);
    println!(
        "  Compression: {} bytes → {} bytes ({:.0}% ratio)",
        sql_content.len(),
        compressed.len(),
        compressed.len() as f64 / sql_content.len() as f64 * 100.0
    );

    // ZIP archive with multiple source files
    let zip_path = root.join("project_backup.zip");
    {
        let buf = Vec::new();
        let cursor = std::io::Cursor::new(buf);
        let mut zw = zip::ZipWriter::new(cursor);
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);

        zw.start_file("src/main.rs", opts).unwrap();
        zw.write_all(b"fn main() {\n    let app = Router::new()\n        .route(\"/login\", post(handle_authentication));\n    app.listen(\"0.0.0.0:3000\").await;\n}\n").unwrap();

        zw.start_file("src/auth.rs", opts).unwrap();
        zw.write_all(b"pub async fn handle_authentication(req: Request) -> Response {\n    let creds = req.json::<Credentials>().await?;\n    let token = create_jwt(&creds.username)?;\n    Response::json(&TokenResponse { token })\n}\n").unwrap();

        zw.start_file("Cargo.toml", opts).unwrap();
        zw.write_all(b"[package]\nname = \"auth-server\"\nversion = \"0.1.0\"\n\n[dependencies]\ntokio = { version = \"1\", features = [\"full\"] }\nserde = { version = \"1\", features = [\"derive\"] }\njsonwebtoken = \"9\"\n").unwrap();

        let data = zw.finish().unwrap().into_inner();
        fs::write(&zip_path, &data).unwrap();
    }
    show_extracted_text("ZIP (.zip)", &zip_path);

    // DOCX
    let docx_path = root.join("security_review.docx");
    {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
<w:body>
<w:p><w:r><w:t>Security Review: Authentication System v2.0</w:t></w:r></w:p>
<w:p><w:r><w:t>Date: March 2026 | Reviewer: Security Team</w:t></w:r></w:p>
<w:p><w:r><w:t>The authentication module uses JWT tokens with RS256 signing. Token expiration is set to 24 hours with refresh token rotation enabled. Multi-factor authentication (MFA) is required for admin accounts.</w:t></w:r></w:p>
<w:p><w:r><w:t>Findings:</w:t></w:r></w:p>
<w:p><w:r><w:t>1. Password hashing uses bcrypt with cost factor 12 — PASS</w:t></w:r></w:p>
<w:p><w:r><w:t>2. Session tokens are stored in HttpOnly cookies — PASS</w:t></w:r></w:p>
<w:p><w:r><w:t>3. Rate limiting on authentication endpoints — NEEDS IMPROVEMENT</w:t></w:r></w:p>
<w:p><w:r><w:t>4. CSRF protection via SameSite cookies — PASS</w:t></w:r></w:p>
<w:p><w:r><w:t>Recommendation: Implement progressive delays after failed authentication attempts to prevent brute-force attacks.</w:t></w:r></w:p>
</w:body></w:document>"#;

        let buf = Vec::new();
        let cursor = std::io::Cursor::new(buf);
        let mut zw = zip::ZipWriter::new(cursor);
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        zw.start_file("word/document.xml", opts).unwrap();
        zw.write_all(xml.as_bytes()).unwrap();
        let data = zw.finish().unwrap().into_inner();
        fs::write(&docx_path, &data).unwrap();
    }
    show_extracted_text("DOCX (.docx)", &docx_path);

    // XLSX
    let xlsx_path = root.join("auth_metrics.xlsx");
    {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
<si><t>Metric</t></si>
<si><t>Value</t></si>
<si><t>Login Success Rate</t></si>
<si><t>98.7%</t></si>
<si><t>MFA Adoption</t></si>
<si><t>73.2%</t></si>
<si><t>Average Authentication Latency</t></si>
<si><t>42ms</t></si>
<si><t>Failed Login Attempts (24h)</t></si>
<si><t>1,247</t></si>
<si><t>Active Sessions</t></si>
<si><t>15,892</t></si>
</sst>"#;

        let buf = Vec::new();
        let cursor = std::io::Cursor::new(buf);
        let mut zw = zip::ZipWriter::new(cursor);
        let opts = zip::write::SimpleFileOptions::default();
        zw.start_file("xl/sharedStrings.xml", opts).unwrap();
        zw.write_all(xml.as_bytes()).unwrap();
        let data = zw.finish().unwrap().into_inner();
        fs::write(&xlsx_path, &data).unwrap();
    }
    show_extracted_text("XLSX (.xlsx)", &xlsx_path);

    // ── 2. Cross-format search ──
    separator("2. Cross-Format Search: \"authentication\"");

    let engine = SearchEngine::new(root).unwrap();
    let results = engine
        .search(
            "authentication",
            &SearchOptions {
                context_lines: 0,
                max_results: 100,
                ..Default::default()
            },
        )
        .unwrap();

    println!(
        "  Found {} matches across {} file types:\n",
        results.len(),
        {
            let mut types: std::collections::HashSet<String> = std::collections::HashSet::new();
            for r in &results {
                let ext = r
                    .path
                    .extension()
                    .map(|e| e.to_string_lossy().to_string())
                    .unwrap_or("none".into());
                types.insert(ext);
            }
            types.len()
        }
    );

    for r in &results {
        let fname = r.path.file_name().unwrap().to_string_lossy();
        let ext = r
            .path
            .extension()
            .map(|e| e.to_string_lossy().to_string())
            .unwrap_or("?".into());
        let ftype = match ext.as_str() {
            "rs" => "PlainText",
            "gz" => "Gzip",
            "zip" => "ZIP",
            "docx" => "DOCX",
            "xlsx" => "XLSX",
            _ => "Unknown",
        };
        let line_preview = if r.line_content.len() > 65 {
            format!("{}...", &r.line_content[..62])
        } else {
            r.line_content.clone()
        };
        println!(
            "  [{:>9}] {}:{} — {}",
            ftype,
            fname,
            r.line_number,
            line_preview.trim()
        );
    }

    // ── 3. BM25 Ranking ──
    separator("3. BM25 Relevance Ranking");

    // Create a more varied corpus for BM25 demo
    let bm25_dir = TempDir::new().unwrap();
    let bd = bm25_dir.path();

    // File A: Authentication is THE topic (high TF, short doc)
    fs::write(
        bd.join("auth_core.rs"),
        r#"
pub fn authenticate(user: &str, pass: &str) -> bool { check_credentials(user, pass) }
pub fn authenticate_token(token: &str) -> bool { verify_jwt(token) }
pub fn authenticate_api_key(key: &str) -> bool { lookup_key(key) }
fn check_credentials(u: &str, p: &str) -> bool { true }
"#,
    )
    .unwrap();

    // File B: Authentication mentioned once in a huge file (low TF, long doc)
    let mut big_file = String::new();
    for i in 0..200 {
        big_file.push_str(&format!(
            "fn utility_function_{i}() {{ /* some code */ }}\n"
        ));
    }
    big_file.push_str("fn setup() { let _ = authenticate(\"user\", \"pass\"); }\n");
    for i in 200..400 {
        big_file.push_str(&format!("fn helper_{i}() {{ /* more code */ }}\n"));
    }
    fs::write(bd.join("big_utils.rs"), &big_file).unwrap();

    // File C: Authentication in a config context (medium TF, medium doc)
    fs::write(
        bd.join("config.yaml"),
        r#"
server:
  port: 8080
  host: 0.0.0.0

authentication:
  method: jwt
  secret_key: "${AUTH_SECRET}"
  token_expiry: 86400
  authenticate_on_startup: true

database:
  host: localhost
  port: 5432
  name: myapp
  pool_size: 10
"#,
    )
    .unwrap();

    // File D: DOCX with authentication content
    {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
<w:body>
<w:p><w:r><w:t>API Authentication Guide</w:t></w:r></w:p>
<w:p><w:r><w:t>To authenticate with the API, include the Bearer token in the Authorization header.</w:t></w:r></w:p>
<w:p><w:r><w:t>All endpoints require authentication except /health and /status.</w:t></w:r></w:p>
</w:body></w:document>"#;
        let buf = Vec::new();
        let cursor = std::io::Cursor::new(buf);
        let mut zw = zip::ZipWriter::new(cursor);
        let opts = zip::write::SimpleFileOptions::default();
        zw.start_file("word/document.xml", opts).unwrap();
        zw.write_all(xml.as_bytes()).unwrap();
        let data = zw.finish().unwrap().into_inner();
        fs::write(bd.join("api_guide.docx"), &data).unwrap();
    }

    // File E: No authentication mentions (noise)
    fs::write(
        bd.join("models.rs"),
        r#"
pub struct User { pub id: u64, pub name: String, pub email: String }
pub struct Product { pub id: u64, pub title: String, pub price: f64 }
pub struct Order { pub id: u64, pub user_id: u64, pub total: f64 }
"#,
    )
    .unwrap();

    demo_bm25_ranking(bd);

    // ── 4. Current limitations ──
    separator("4. Current Limitations & V2 Roadmap");
    println!("  IMPLEMENTED:");
    println!("    [x] SIMD Aho-Corasick literal matching (via memchr)");
    println!("    [x] BM25 relevance scoring (standalone, not yet wired into engine ranking)");
    println!("    [x] Multi-format extraction: PlainText, Gzip, ZIP, DOCX, XLSX, PDF");
    println!("    [x] SHA256 CAS caching for binary extractions");
    println!();
    println!("  NOT YET IMPLEMENTED (V2):");
    println!("    [ ] SimHash + Hamming distance — lightweight similarity search");
    println!("    [ ] BM25 integrated into SearchEngine result ranking");
    println!("    [ ] Vectorscan multi-pattern parallel matching");
    println!("    [ ] SPLADE query expansion");
    println!("    [ ] Token-aware content slicing");

    println!("\n╔══════════════════════════════════════════════════════════╗");
    println!("║                    Demo Complete                        ║");
    println!("╚══════════════════════════════════════════════════════════╝");
}
