use bitscout_core::fs::tree::FileTree;
use bitscout_core::search::engine::{SearchEngine, SearchOptions};
use std::path::Path;
use std::time::Instant;

#[test]
fn bench_cold_vs_hot_with_real_pdfs() {
    let dir = Path::new("/tmp/bitscout_real_test");
    if !dir.exists() {
        eprintln!("SKIP: /tmp/bitscout_real_test not found");
        return;
    }
    
    let pattern = "climate";
    let iters = 20;
    
    eprintln!("\n目录: {:?}", dir);
    eprintln!("搜索: \"{}\"", pattern);
    eprintln!("迭代: {}\n", iters);
    
    // ── 单独测 FileTree::scan 耗时 ──
    // warmup
    let _ = FileTree::scan(dir);
    
    let start = Instant::now();
    for _ in 0..iters {
        let tree = FileTree::scan(dir).unwrap();
        std::hint::black_box(tree.file_count());
    }
    let scan_ms = start.elapsed().as_secs_f64() * 1000.0 / iters as f64;
    
    // ── 冷启动（每次 scan + search） ──
    let start = Instant::now();
    for _ in 0..iters {
        let engine = SearchEngine::new(dir).unwrap();
        let results = engine.search(pattern, &SearchOptions::default()).unwrap();
        std::hint::black_box(results.len());
    }
    let cold_ms = start.elapsed().as_secs_f64() * 1000.0 / iters as f64;
    
    // ── 热索引（一次 scan，多次 search） ──
    let tree = FileTree::scan(dir).unwrap();
    let file_count = tree.file_count();
    
    // warmup
    let engine = SearchEngine::from_tree(tree.clone());
    let warmup_results = engine.search(pattern, &SearchOptions::default()).unwrap();
    let result_count = warmup_results.len();
    
    let start = Instant::now();
    for _ in 0..iters {
        let engine = SearchEngine::from_tree(tree.clone());
        let results = engine.search(pattern, &SearchOptions::default()).unwrap();
        std::hint::black_box(results.len());
    }
    let hot_ms = start.elapsed().as_secs_f64() * 1000.0 / iters as f64;
    
    eprintln!("索引文件数: {}", file_count);
    eprintln!("搜索结果数: {}", result_count);
    eprintln!();
    eprintln!("╔═══════════════════════════════════════════════════╗");
    eprintln!("║     大文件 (27MB, 3x PDF) In-Process Benchmark   ║");
    eprintln!("╠═══════════════════════════════════════════════════╣");
    eprintln!("║  FileTree::scan() 耗时: {:>10.2} ms             ║", scan_ms);
    eprintln!("║  冷启动 (scan+search):  {:>10.2} ms             ║", cold_ms);
    eprintln!("║  热索引 (search only):  {:>10.2} ms             ║", hot_ms);
    eprintln!("║  热索引加速比:          {:>10.1}x               ║", cold_ms / hot_ms);
    eprintln!("╚═══════════════════════════════════════════════════╝");
    eprintln!();
    if cold_ms > 0.0 {
        eprintln!("scan 占冷启动的 {:.0}%", scan_ms / cold_ms * 100.0);
    }
}
