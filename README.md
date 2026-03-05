# BitScout

[中文版](README.zh-CN.md)

> SIMD-accelerated search toolkit for AI Agents — drop-in replacement for `rg`, `grep`, `find`, `fd`, `cat` with built-in semantic search

## Why BitScout?

AI coding agents (Claude Code, Cursor, etc.) spawn a new process for every `rg`/`grep`/`find` call. Each invocation pays the full fork+exec cost, cannot search inside compressed or binary formats, and has zero semantic understanding.

BitScout solves all three problems in a single binary:

- **BusyBox-style cold start** — one binary, detected via `argv[0]` symlinks, no daemon
- **Transparent format support** — searches inside `.gz`, `.zip`, `.pdf`, `.docx`, `.xlsx` automatically
- **Embedding-free semantic search** — LSA (Latent Semantic Analysis) with SIMD-accelerated scoring, no external models

### Feature Comparison

| Capability | rg/grep | BitScout |
|---|---|---|
| Plain text search | Yes | Yes (fully compatible) |
| Regex support | Yes | Yes (fully compatible) |
| Search `.gz` files | No | Yes, auto-decompress |
| Search `.zip` archives | No | Yes, auto-extract |
| Search `.docx` / `.xlsx` | No | Yes, auto-extract text |
| Search `.pdf` | No | Yes, auto-extract text |
| BM25 relevance scoring | No | Yes (`--bm25`) |
| Semantic search | No | Yes (`--semantic`, LSA) |
| Cold-start overhead | fork+exec | ~0.1ms single binary |
| Content caching | No | Yes (SHA256 CAS + LRU) |

## Quick Start

```bash
# Build
cargo build --release

# Install BusyBox-style symlinks
./target/release/bitscout install

# Or create symlinks manually
ln -s target/release/bitscout target/release/rg

# Use — fully compatible with rg/grep/find/fd/cat
rg "fn main" src/
grep -rn "TODO" .
find . -name "*.rs"
cat compressed.log.gz

# Semantic search
rg --semantic "error handling" src/

# BM25 relevance scoring
rg --bm25 "database" src/

# Run tests
cargo test
```

## Supported Commands

| Command | Accelerated Flags | Notes |
|---|---|---|
| `rg` | `--json`, `-n`, `-i`, `-l`, `-c`, `-C`, `-A`, `-B`, `-g`, `-t`, `--color`, `-F`, `-U`, `--bm25`, `--semantic` | Regex by default, `-F` for literal |
| `grep` | `-r`, `-n`, `-i`, `-l`, `-c`, `-H`, `-h`, `-w`, `-F`, `--include`, `--bm25` | Regex by default, `-F` for literal |
| `find` | `-name`, `-iname`, `-type`, `-path` | Glob matching |
| `fd` | `-e`, `-t`, `-i`, `-F` | Regex filename matching |
| `cat` | `-n` | Transparent `.gz`/`.zip`/`.pdf`/`.docx` reading |

Unsupported flags automatically fall back to the real command — zero impact.

## Scoring Modes

### BM25 (`--bm25`)

Classic BM25 text relevance scoring with two modes:

- `--bm25` — fast mode, TF scoring only
- `--bm25=full` — full mode with IDF weighting

### Semantic (`--semantic`)

LSA-based semantic search, no embedding models required:

- **Truncated SVD** on TF-IDF matrix discovers latent semantic relationships between terms
- **Zero dependencies** — pure Rust SVD + Random Projection, no `rand`/`ndarray`/`lapack`
- **SIMD accelerated** — vector ops and cosine similarity use AVX2/NEON kernels
- **Cold-start friendly** — builds index from project corpus on each invocation, no persistence needed

```bash
rg --semantic "authentication flow" src/
rg --semantic "database connection pool" .
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
│  │  • LSA semantic search (--semantic)                │  │
│  └────────────────────┬───────────────────────────────┘  │
│                       │                                  │
│  ┌────────────────────▼───────────────────────────────┐  │
│  │   CAS Content Cache (SHA256 + LRU, 256MB limit)   │  │
│  │   ~/.bitscout/cache/content/                       │  │
│  └────────────────────────────────────────────────────┘  │
└──────────────────────────────────────────────────────────┘
```

## Project Structure

```
crates/
  bitscout-core/       # Core engine
    src/
      search/          # SearchEngine, Matcher, BM25, LSA, RP, SIMD
      extract/         # Content extraction (pdf, gz, zip, docx)
      cache/           # SHA256 CAS cache + LRU eviction
      compat/          # Command parsers (rg/grep/find/fd)
      dispatch.rs      # Unified dispatch entry point
      fs/              # FileTree scanner
  bitscout-cli/        # BusyBox-style CLI entry point
  bitscout-memory/     # Persistent knowledge store
tests/
  e2e/                 # Conformance + accuracy + benchmark tests
```

## Tech Stack

- **Language**: Rust (~10,000 lines)
- **Matching**: Aho-Corasick (literal) + `regex` crate (regex patterns)
- **Scoring**: BM25 (TF/IDF) + LSA (SVD + cosine similarity) + Random Projection
- **SIMD**: AVX2/FMA (x86_64), NEON (aarch64), scalar fallback
- **File I/O**: mmap (`memmap2`)
- **Extraction**: `flate2` (gzip), `zip` (zip/docx/xlsx), PDF text extraction
- **Caching**: SHA256 content-addressable storage + LRU eviction (`filetime`)

## Documentation

- [Testing Report](docs/TESTING.md)
- [Roadmap](docs/ROADMAP.md)

## License

MIT
