use bitscout_core::search::engine::{SearchEngine, SearchOptions};
use flate2::write::GzEncoder;
use flate2::Compression;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::Command;
use std::time::Instant;
use tempfile::TempDir;

const RG_PATH: &str = "/Users/rainlei/.local/share/claude/versions/2.1.63";
const RG_PREFIX: &str = "--ripgrep";
const ITERATIONS: usize = 5;

fn generate_corpus(root: &Path, num_files: usize, lines_per_file: usize) {
    let keywords = [
        "authenticate", "authorize", "validate", "session",
        "token", "middleware", "handler", "controller",
        "database", "connection", "transaction", "query",
    ];

    for i in 0..num_files {
        let ext = match i % 5 {
            0 => "rs", 1 => "py", 2 => "js", 3 => "ts", _ => "go",
        };
        let mut content = String::new();
        for line_num in 0..lines_per_file {
            let keyword = keywords[(i + line_num) % keywords.len()];
            content.push_str(&format!(
                "fn {keyword}_{i}_{line_num}(arg: &str) -> Result<(), Error> {{ /* impl */ }}\n"
            ));
        }
        if i % 10 == 0 {
            content.push_str("fn UNIQUE_SEARCH_TARGET_NEEDLE() { /* find me */ }\n");
        }
        fs::write(root.join(format!("file_{i:04}.{ext}")), &content).unwrap();
    }

    for dir_idx in 0..5 {
        let dir = root.join(format!("subdir_{dir_idx}"));
        fs::create_dir_all(&dir).unwrap();
        for i in 0..num_files / 10 {
            let content = format!(
                "// nested\nfn nested_{dir_idx}_{i}() {{ let x = authenticate(); }}\n"
            );
            fs::write(dir.join(format!("nested_{i}.rs")), &content).unwrap();
        }
    }
}

fn generate_gz_files(root: &Path, count: usize) {
    let gz_dir = root.join("compressed");
    fs::create_dir_all(&gz_dir).unwrap();
    for i in 0..count {
        let content = format!(
            "fn compressed_{i}() {{\n    let auth = UNIQUE_SEARCH_TARGET_NEEDLE();\n}}\n"
        );
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(content.as_bytes()).unwrap();
        let compressed = encoder.finish().unwrap();
        fs::write(gz_dir.join(format!("archive_{i}.rs.gz")), &compressed).unwrap();
    }
}

fn generate_docx_files(root: &Path, count: usize) {
    let docx_dir = root.join("documents");
    fs::create_dir_all(&docx_dir).unwrap();
    for i in 0..count {
        let xml = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
<w:body>
<w:p><w:r><w:t>Document {i}: UNIQUE_SEARCH_TARGET_NEEDLE for testing.</w:t></w:r></w:p>
</w:body></w:document>"#
        );
        let buf = Vec::new();
        let cursor = std::io::Cursor::new(buf);
        let mut zw = zip::ZipWriter::new(cursor);
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        zw.start_file("word/document.xml", opts).unwrap();
        zw.write_all(xml.as_bytes()).unwrap();
        let data = zw.finish().unwrap().into_inner();
        fs::write(docx_dir.join(format!("report_{i}.docx")), &data).unwrap();
    }
}

fn time_rg(pattern: &str, dir: &Path) -> (std::time::Duration, usize) {
    let start = Instant::now();
    let output = Command::new(RG_PATH)
        .args([RG_PREFIX, "--no-heading", "-c", pattern])
        .arg(dir)
        .output()
        .expect("rg failed");
    let elapsed = start.elapsed();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let count: usize = stdout
        .lines()
        .filter_map(|l| l.rsplit(':').next()?.parse::<usize>().ok())
        .sum();
    (elapsed, count)
}

fn time_bitscout(pattern: &str, dir: &Path) -> (std::time::Duration, usize) {
    let start = Instant::now();
    let engine = SearchEngine::new(dir).unwrap();
    let results = engine
        .search(pattern, &SearchOptions { max_results: 100000, ..Default::default() })
        .unwrap();
    let elapsed = start.elapsed();
    (elapsed, results.len())
}

fn bench(label: &str, pattern: &str, dir: &Path) {
    println!("\n--- {label} ---");
    println!("  Pattern: \"{pattern}\"");

    // Warmup
    let _ = time_rg(pattern, dir);
    let _ = time_bitscout(pattern, dir);

    let mut rg_times = Vec::new();
    let mut bs_times = Vec::new();
    let mut rg_m = 0;
    let mut bs_m = 0;

    for _ in 0..ITERATIONS {
        let (t, m) = time_rg(pattern, dir);
        rg_times.push(t);
        rg_m = m;

        let (t, m) = time_bitscout(pattern, dir);
        bs_times.push(t);
        bs_m = m;
    }

    let rg_avg = rg_times.iter().sum::<std::time::Duration>() / ITERATIONS as u32;
    let rg_min = *rg_times.iter().min().unwrap();
    let bs_avg = bs_times.iter().sum::<std::time::Duration>() / ITERATIONS as u32;
    let bs_min = *bs_times.iter().min().unwrap();

    println!("  {:>18} | {:>10} | {:>10} | {:>8}", "Tool", "Avg", "Min", "Matches");
    println!("  {:->18}-+-{:->10}-+-{:->10}-+-{:->8}", "", "", "", "");
    println!(
        "  {:>18} | {:>7.2}ms | {:>7.2}ms | {:>8}",
        "ripgrep", rg_avg.as_secs_f64() * 1000.0, rg_min.as_secs_f64() * 1000.0, rg_m
    );
    println!(
        "  {:>18} | {:>7.2}ms | {:>7.2}ms | {:>8}",
        "BitScout", bs_avg.as_secs_f64() * 1000.0, bs_min.as_secs_f64() * 1000.0, bs_m
    );

    let ratio = rg_avg.as_secs_f64() / bs_avg.as_secs_f64();
    if ratio > 1.0 {
        println!("  => BitScout {:.1}x FASTER", ratio);
    } else {
        println!("  => rg {:.1}x faster (process startup amortized)", 1.0 / ratio);
    }
}

fn bench_binary_only(label: &str, pattern: &str, dir: &Path) {
    println!("\n--- {label} ---");
    println!("  Pattern: \"{pattern}\"");
    println!("  (rg cannot search .gz/.docx — BitScout exclusive feature)");

    let _ = time_bitscout(pattern, dir);

    let mut bs_times = Vec::new();
    let mut bs_m = 0;
    for _ in 0..ITERATIONS {
        let (t, m) = time_bitscout(pattern, dir);
        bs_times.push(t);
        bs_m = m;
    }

    let bs_avg = bs_times.iter().sum::<std::time::Duration>() / ITERATIONS as u32;
    let bs_min = *bs_times.iter().min().unwrap();

    let (_, rg_m) = time_rg(pattern, dir);

    println!("  BitScout: avg {:.2}ms, min {:.2}ms => {} matches (text + gz + docx)",
        bs_avg.as_secs_f64() * 1000.0, bs_min.as_secs_f64() * 1000.0, bs_m);
    println!("  ripgrep:  {} matches (text only, MISSES {} binary files)",
        rg_m, bs_m.saturating_sub(rg_m));
}

fn main() {
    println!("╔══════════════════════════════════════════════════════════╗");
    println!("║         BitScout vs ripgrep — Benchmark Report          ║");
    println!("╚══════════════════════════════════════════════════════════╝");
    println!();
    println!("Platform: macOS {} | Iterations: {}", std::env::consts::ARCH, ITERATIONS);

    // Generate corpora
    let small = TempDir::new().unwrap();
    generate_corpus(small.path(), 100, 50);
    println!("\nCorpus A (small):  100 files × 50 lines ≈ 5K lines");

    let medium = TempDir::new().unwrap();
    generate_corpus(medium.path(), 1000, 100);
    println!("Corpus B (medium): 1000 files × 100 lines ≈ 100K lines");

    let binary = TempDir::new().unwrap();
    generate_corpus(binary.path(), 200, 50);
    generate_gz_files(binary.path(), 20);
    generate_docx_files(binary.path(), 10);
    println!("Corpus C (binary): 200 text + 20 gz + 10 docx");

    // Run benchmarks
    bench("1. Common keyword — small corpus", "authenticate", small.path());
    bench("2. Rare keyword — medium corpus", "UNIQUE_SEARCH_TARGET_NEEDLE", medium.path());
    bench("3. Common keyword — medium corpus", "authenticate", medium.path());
    bench_binary_only("4. Binary file search — BitScout exclusive", "UNIQUE_SEARCH_TARGET_NEEDLE", binary.path());

    // Benchmark on BitScout's own source
    bench("5. Real project — BitScout source code", "extract_text", Path::new("/Users/rainlei/holiday/BitScout"));

    println!("\n╔══════════════════════════════════════════════════════════╗");
    println!("║                  Benchmark Complete                     ║");
    println!("╚══════════════════════════════════════════════════════════╝");
}
