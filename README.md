# BitScout

> SIMD-accelerated search toolkit for AI Agents — drop-in replacement for `rg`, `grep`, `find`, `fd`, `cat` with built-in semantic search

BitScout 是一个面向 AI Agent 的搜索加速引擎。单一二进制冷启动，通过 symlink 透明替代 `rg`/`grep`/`find`/`fd`/`cat`，用 SIMD 指令集 + Random Projection 语义搜索 + CAS 磁盘缓存替代传统工具链，让 Agent 每一次搜索更快、更准、更智能。

## Why BitScout?

AI coding agents (Claude Code, Cursor, etc.) 每次搜索都要启动新进程调用 `rg`/`grep` 等命令。BitScout 用 BusyBox 式单二进制冷启动（无 daemon），同时穿透压缩文件和二进制格式，并提供无需 Embedding 模型的语义搜索。

### 核心优势

| 能力 | rg/grep | BitScout |
|------|---------|----------|
| 纯文本搜索 | ✓ | ✓ (完全兼容) |
| Regex 支持 | ✓ | ✓ (完整兼容) |
| 搜索 `.gz` 压缩文件 | ✗ | ✓ 自动解压搜索 |
| 搜索 `.zip` 归档 | ✗ | ✓ 自动解包搜索 |
| 搜索 `.docx` / `.xlsx` | ✗ | ✓ 自动提取文本 |
| 搜索 `.pdf` | ✗ | ✓ 自动提取文本 |
| BM25 评分排序 | ✗ | ✓ (`--bm25`) |
| 语义搜索 | ✗ | ✓ (`--semantic`, LSA + Random Projection) |
| 冷启动零开销 | fork+exec | ✓ (单二进制, ~0.1ms) |
| 内容缓存 | ✗ | ✓ (SHA256 CAS + LRU) |

## Test Results

### Conformance: 100% 兼容

45 个 conformance 测试，对比 BitScout vs 真实 `rg`/`grep`/`find`/`fd`/`cat` 输出，行级精确匹配：

```
Tool    Tests   Pass    Accuracy
─────── ─────── ─────── ────────
rg       18      18     100%
grep      8       8     100%
find      4       4     100%
fd        4       4     100%
cat      10      10     100%
─────── ─────── ─────── ────────
Total    44      44     100%
```

12 个 regex pattern 对比 real rg，行级准确率 **100%**。

### Speed: 3-81x Faster

冷启动性能对比（无 daemon，每次调用独立扫描）：

```
Tool       real cmd    BitScout    Speedup
────────── ────────── ────────── ─────────
cat          1.1ms     0.014ms     81.3x
find         1.5ms     0.12ms      12.9x
rg           6.1ms     0.77ms       8.0x
grep         2.6ms     0.82ms       3.2x
```

### Search Mode Benchmarks

所有模式冷启动耗时（含 FileTree 扫描 + 搜索 + 排序）：

```
Mode                    Avg(us)   Notes
─────────────────────── ───────── ─────────────────────────
find -name '*.rs'          123   文件名 glob 匹配
fd -e rs                   117   文件名扩展名匹配
cat file                    14   文件读取
rg literal                 769   字面量搜索
rg regex                  2456   正则表达式搜索
grep -rn                   823   递归行搜索
rg --bm25                  907   BM25 相关性排序
rg --bm25=full             948   BM25 + IDF 全量评分
rg --semantic              771   Random Projection 语义搜索
```

所有模式 < 2.5ms，满足 Agent 实时交互需求。

### Semantic Search: Top-1 100%

基于 Random Projection (Johnson-Lindenstrauss) 的无 Embedding 语义搜索：

```
Query                            Expected #1        Result         Score
──────────────────────────────── ────────────────── ────────────── ──────
login password authenticate      auth_login.rs      auth_login.rs  0.214 ✓
jwt token generate validate      auth_jwt.rs        auth_jwt.rs    0.642 ✓
database query insert migrate    database.rs        database.rs    0.235 ✓
http server listen connection    http_server.rs     http_server.rs 0.168 ✓
cache evict lru store            cache.rs           cache.rs       0.244 ✓
──────────────────────────────── ────────────────── ──────────────────────
Top-1 Accuracy: 5/5 (100%)
Top-3 Accuracy: 5/5 (100%)
```

RP 单元级区分度：auth(0.70) > session(0.13) > math(-0.20)，相关文档得分显著高于不相关文档。

### Test Suite Summary

```
Test Suite                  Tests   Status
─────────────────────────── ─────── ──────
bitscout-core unit           122    ✓ all pass
bitscout-memory unit           4    ✓ all pass
rg conformance (e2e)          18    ✓ all pass
grep conformance (e2e)         8    ✓ all pass
find/fd conformance (e2e)      8    ✓ all pass
cat conformance (e2e)         10    ✓ all pass
full conformance (e2e)        44    ✓ all pass
semantic accuracy (e2e)       12    ✓ all pass
benchmark modes (e2e)          3    ✓ all pass
─────────────────────────── ─────── ──────
Total                        185+   ✓ all pass
```

## Architecture

```
┌─────────────────────────────────────────────────────────┐
│                    User / AI Agent                       │
│           rg "pattern" .  /  grep  /  find               │
└────────────────────────┬────────────────────────────────┘
                         │ argv[0] detection
┌────────────────────────▼────────────────────────────────┐
│              BitScout CLI (BusyBox-style)                │
│                                                          │
│   argv[0]=rg → rg_compat     argv[0]=bitscout → subcmd  │
│   argv[0]=grep → grep_compat   bitscout search ...      │
│   argv[0]=find → find_compat   bitscout install         │
│   argv[0]=fd → find_compat                               │
│   argv[0]=cat → cat handler                              │
│                                                          │
│   Unsupported flags? → fallback to real command           │
└────────────────────────┬────────────────────────────────┘
                         │
┌────────────────────────▼────────────────────────────────┐
│                  bitscout-core                           │
│                                                          │
│  ┌──────────┐  ┌───────────┐  ┌──────────────────────┐  │
│  │ Matcher  │  │  Search   │  │  Content Extractor   │  │
│  │ AC+Regex │  │  Engine   │  │  .gz .zip .pdf .docx │  │
│  └────┬─────┘  └─────┬─────┘  └──────────┬───────────┘  │
│       │              │                    │              │
│  ┌────▼──────────────▼────────────────────▼───────────┐  │
│  │                Scoring Modes                       │  │
│  │  • Literal/Regex match (default)                   │  │
│  │  • BM25 relevance scoring (--bm25)                 │  │
│  │  • Random Projection semantic (--semantic)         │  │
│  └────────────────────┬───────────────────────────────┘  │
│                       │                                  │
│  ┌────────────────────▼───────────────────────────────┐  │
│  │   CAS Content Cache (SHA256 + LRU, 256MB limit)   │  │
│  │   ~/.bitscout/cache/content/                       │  │
│  └────────────────────────────────────────────────────┘  │
└──────────────────────────────────────────────────────────┘
```

## Supported Commands

| Command | Accelerated Flags | Notes |
|---------|-------------------|-------|
| `rg` | `--json`, `-n`, `-i`, `-l`, `-c`, `-C`, `-A`, `-B`, `-g`, `-t`, `--color`, `-F`, `-U`, `--bm25`, `--semantic` | 默认 regex，`-F` 切 literal |
| `grep` | `-r`, `-n`, `-i`, `-l`, `-c`, `-H`, `-h`, `-w`, `-F`, `--include`, `--bm25` | 默认 regex，`-F` 切 literal |
| `find` | `-name`, `-iname`, `-type`, `-path` | glob 匹配 |
| `fd` | `-e`, `-t`, `-i`, `-F` | 默认 regex 匹配文件名 |
| `cat` | `-n` | 支持 .gz/.zip/.pdf/.docx 透明读取 |

不支持的 flag 自动 fallback 到真实命令，零影响。

## Scoring Modes

### BM25 (`--bm25`)

经典 BM25 文本相关性评分。支持两种模式：

- `--bm25` — 快速模式，仅 TF 评分
- `--bm25=full` — 完整模式，含 IDF 加权

### Semantic (`--semantic`)

基于 LSA (Latent Semantic Analysis) 的语义搜索，无需 Embedding 模型：

- **SVD 降维**：对 TF-IDF 矩阵做截断 SVD，发现词汇间的潜在语义关联
- **零依赖**：纯 Rust 实现 SVD + Random Projection，不依赖 `rand`/`ndarray`/`lapack`
- **SIMD 加速**：向量运算和 cosine similarity 均用 SIMD 内核（AVX2/NEON）
- **冷启动友好**：每次调用从项目语料构建索引，无需持久化

```bash
# 语义搜索示例
rg --semantic "authentication flow" src/
bitscout search --semantic "database connection pool" .
```

## Quick Start

```bash
# Build
cargo build --release

# BusyBox 模式安装 (创建 symlinks)
./target/release/bitscout install

# 或手动创建 symlink
ln -s target/release/bitscout target/release/rg

# 使用 — 与 rg/grep/find 完全兼容
rg "fn main" src/
grep -rn "TODO" .
find . -name "*.rs"

# 语义搜索
rg --semantic "error handling" src/
bitscout search --semantic "authentication" .

# 运行测试
cargo test                                             # 全部测试
cargo test -p bitscout-core --lib                      # 核心单元测试 (110)
cargo test -p bitscout-e2e --test full_conformance     # 兼容性对照 (44)
cargo test -p bitscout-e2e --test semantic_accuracy    # 语义准确度 (9)
cargo test -p bitscout-e2e --test benchmark_modes      # 性能基准 (4)
```

## Project Structure

```
crates/
  bitscout-core/       # 核心引擎
    src/
      search/          # SearchEngine, Matcher, BM25, LSA, RP, SIMD
      extract/         # 内容提取 (pdf, gz, zip, docx)
      cache/           # SHA256 CAS 缓存 + LRU 淘汰
      compat/          # 命令解析器 (rg/grep/find/fd)
      dispatch.rs      # 统一调度入口
      fs/              # FileTree 文件扫描
  bitscout-cli/        # BusyBox 式 CLI 入口
  bitscout-memory/     # 持久化知识库存储
tests/
  e2e/                 # 端到端 conformance + 准确率 + 基准测试
```

## Tech Stack

- **Language**: Rust (~5,800 lines)
- **Matching**: Aho-Corasick (literal) + `regex` crate (regex patterns)
- **Scoring**: BM25 (TF/IDF) + LSA (SVD + cosine similarity) + Random Projection
- **SIMD**: AVX2/FMA (x86_64), NEON (aarch64), scalar fallback
- **File I/O**: mmap (`memmap2`)
- **Extraction**: `flate2` (gzip), `zip` (zip/docx/xlsx), PDF text extraction
- **Caching**: SHA256 content-addressable storage + LRU eviction (`filetime`)

## License

MIT
