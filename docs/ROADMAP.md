# Roadmap

[中文版](ROADMAP.zh-CN.md)

## Current State (v0.1)

BitScout is a functional single-binary search accelerator for AI agents with:
- BusyBox-style `rg`/`grep`/`find`/`fd`/`cat` replacement via symlinks
- Transparent search inside `.gz`, `.zip`, `.pdf`, `.docx`, `.xlsx`
- BM25 relevance scoring (`--bm25`, `--bm25=full`)
- LSA semantic search (`--semantic`) — embedding-free, SIMD-accelerated
- SHA256 CAS content cache with LRU eviction
- 100% conformance with original tools, 3-81x faster cold-start

## Short Term

### Performance
- [ ] Incremental FileTree — skip unchanged directories via `mtime` comparison
- [ ] Parallel file scanning — rayon-based multi-threaded tree walk
- [ ] SIMD-accelerated Aho-Corasick — replace `aho-corasick` crate with hand-rolled AVX2/NEON
- [ ] Pre-built LSA index cache — persist SVD results to avoid recomputation

### Search Quality
- [ ] Improve LSA on small corpora — adaptive SVD rank selection based on corpus size
- [ ] Identifier-aware tokenization — split `camelCase` and `snake_case` into component words
- [ ] Language-aware stop words — remove language-specific boilerplate tokens (fn, let, const, import)
- [ ] Query term weighting — rare terms contribute more to semantic score

### Compatibility
- [ ] `rg` multiline search (`-U` with regex)
- [ ] `grep -E` extended regex support
- [ ] `find -mtime`, `-size` predicates
- [ ] `.tar.gz` and `.tar.bz2` archive search

## Medium Term

### Agent Integration
- [ ] MCP (Model Context Protocol) server mode — expose search as MCP tool
- [ ] LSP-style daemon mode — persistent process with Unix Domain Socket for session-local caching
- [ ] Structured JSON output for all commands — machine-readable results for agent consumption
- [ ] Watch mode — filesystem watcher for incremental index updates

### Format Support
- [ ] `.pptx` presentation text extraction
- [ ] `.epub` ebook text extraction
- [ ] `.sqlite` / `.db` database content search
- [ ] Source map support — map minified JS back to original source

### Scoring
- [ ] Hybrid BM25 + LSA scoring — combine keyword precision with semantic recall
- [ ] File importance weighting — recent files and frequently-accessed files rank higher
- [ ] Dependency-aware ranking — files imported by many others rank higher

## Long Term

### Intelligence
- [ ] Project knowledge graph — build a persistent graph of code entities and relationships
- [ ] Cross-project learning — share LSA indices across similar projects
- [ ] Agent feedback loop — learn from which results agents actually use

### Platform
- [ ] Windows support (MSVC build)
- [ ] WebAssembly build — run in browser-based IDEs
- [ ] Package managers — Homebrew, cargo install, apt/rpm

## Non-Goals

These are explicitly out of scope:

- **Embedding models** — BitScout is designed to be model-free; semantic search relies on LSA/RP only
- **Daemon-first architecture** — cold-start per-invocation is a core design choice
- **IDE plugin** — BitScout is a CLI tool for agents, not a GUI extension
- **Full ripgrep replacement** — unsupported flags fall back to real commands by design
