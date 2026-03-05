# BitScout

[English](README.md)

> SIMD 加速的 AI Agent 搜索工具包 — `rg`、`grep`、`find`、`fd`、`cat` 的透明替代，内置语义搜索

## 为什么需要 BitScout？

AI 编程助手（Claude Code、Cursor 等）每次搜索都要启动新进程调用 `rg`/`grep` 等命令。每次调用都要付出 fork+exec 的开销，无法搜索压缩文件和二进制格式，也没有语义理解能力。

BitScout 用一个二进制文件解决所有问题：

- **BusyBox 式冷启动** — 单一二进制，通过 `argv[0]` symlink 检测命令，无需 daemon
- **透明格式支持** — 自动搜索 `.gz`、`.zip`、`.pdf`、`.docx`、`.xlsx` 内容
- **无 Embedding 语义搜索** — 基于 LSA（潜在语义分析）+ SIMD 加速评分，不依赖外部模型

### 功能对比

| 能力 | rg/grep | BitScout |
|---|---|---|
| 纯文本搜索 | ✓ | ✓（完全兼容）|
| Regex 支持 | ✓ | ✓（完整兼容）|
| 搜索 `.gz` 压缩文件 | ✗ | ✓ 自动解压搜索 |
| 搜索 `.zip` 归档 | ✗ | ✓ 自动解包搜索 |
| 搜索 `.docx` / `.xlsx` | ✗ | ✓ 自动提取文本 |
| 搜索 `.pdf` | ✗ | ✓ 自动提取文本 |
| BM25 评分排序 | ✗ | ✓（`--bm25`）|
| 语义搜索 | ✗ | ✓（`--semantic`，LSA）|
| 冷启动开销 | fork+exec | ~0.1ms 单二进制 |
| 内容缓存 | ✗ | ✓（SHA256 CAS + LRU）|

## 快速开始

```bash
# 构建
cargo build --release

# BusyBox 模式安装（创建 symlinks）
./target/release/bitscout install

# 或手动创建 symlink
ln -s target/release/bitscout target/release/rg

# 使用 — 与 rg/grep/find/fd/cat 完全兼容
rg "fn main" src/
grep -rn "TODO" .
find . -name "*.rs"
cat compressed.log.gz

# 语义搜索
rg --semantic "error handling" src/

# BM25 相关性排序
rg --bm25 "database" src/

# 运行测试
cargo test
```

## 支持的命令

| 命令 | 加速的 Flag | 备注 |
|---|---|---|
| `rg` | `--json`, `-n`, `-i`, `-l`, `-c`, `-C`, `-A`, `-B`, `-g`, `-t`, `--color`, `-F`, `-U`, `--bm25`, `--semantic` | 默认 regex，`-F` 切 literal |
| `grep` | `-r`, `-n`, `-i`, `-l`, `-c`, `-H`, `-h`, `-w`, `-F`, `--include`, `--bm25` | 默认 regex，`-F` 切 literal |
| `find` | `-name`, `-iname`, `-type`, `-path` | glob 匹配 |
| `fd` | `-e`, `-t`, `-i`, `-F` | 默认 regex 匹配文件名 |
| `cat` | `-n` | 支持 .gz/.zip/.pdf/.docx 透明读取 |

不支持的 flag 自动 fallback 到真实命令，零影响。

## 评分模式

### BM25（`--bm25`）

经典 BM25 文本相关性评分，支持两种模式：

- `--bm25` — 快速模式，仅 TF 评分
- `--bm25=full` — 完整模式，含 IDF 加权

### 语义搜索（`--semantic`）

基于 LSA 的语义搜索，无需 Embedding 模型：

- **截断 SVD** — 对 TF-IDF 矩阵做截断 SVD，发现词汇间的潜在语义关联
- **零依赖** — 纯 Rust 实现 SVD + Random Projection，不依赖 `rand`/`ndarray`/`lapack`
- **SIMD 加速** — 向量运算和 cosine similarity 均用 AVX2/NEON 内核
- **冷启动友好** — 每次调用从项目语料构建索引，无需持久化

```bash
rg --semantic "authentication flow" src/
rg --semantic "database connection pool" .
```

## 架构

```
┌─────────────────────────────────────────────────────────┐
│                    User / AI Agent                       │
│           rg "pattern" .  /  grep  /  find               │
└────────────────────────┬────────────────────────────────┘
                         │ argv[0] 检测
┌────────────────────────▼────────────────────────────────┐
│              BitScout CLI（BusyBox 模式）                 │
│                                                          │
│   argv[0]=rg → rg_compat     argv[0]=bitscout → subcmd  │
│   argv[0]=grep → grep_compat   bitscout search ...      │
│   argv[0]=find → find_compat   bitscout install         │
│   argv[0]=fd → find_compat                               │
│   argv[0]=cat → cat handler                              │
│                                                          │
│   不支持的 flag → fallback 到真实命令                      │
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
│  │                评分模式                              │  │
│  │  • 字面量/正则匹配（默认）                            │  │
│  │  • BM25 相关性评分（--bm25）                         │  │
│  │  • LSA 语义搜索（--semantic）                        │  │
│  └────────────────────┬───────────────────────────────┘  │
│                       │                                  │
│  ┌────────────────────▼───────────────────────────────┐  │
│  │   CAS 内容缓存（SHA256 + LRU，256MB 上限）          │  │
│  │   ~/.bitscout/cache/content/                       │  │
│  └────────────────────────────────────────────────────┘  │
└──────────────────────────────────────────────────────────┘
```

## 项目结构

```
crates/
  bitscout-core/       # 核心引擎
    src/
      search/          # SearchEngine, Matcher, BM25, LSA, RP, SIMD
      extract/         # 内容提取（pdf, gz, zip, docx）
      cache/           # SHA256 CAS 缓存 + LRU 淘汰
      compat/          # 命令解析器（rg/grep/find/fd）
      dispatch.rs      # 统一调度入口
      fs/              # FileTree 文件扫描
  bitscout-cli/        # BusyBox 式 CLI 入口
  bitscout-memory/     # 持久化知识库存储
tests/
  e2e/                 # 端到端 conformance + 准确率 + 基准测试
```

## 技术栈

- **语言**：Rust（约 10,000 行）
- **匹配**：Aho-Corasick（字面量）+ `regex` crate（正则）
- **评分**：BM25（TF/IDF）+ LSA（SVD + cosine similarity）+ Random Projection
- **SIMD**：AVX2/FMA（x86_64）、NEON（aarch64）、scalar fallback
- **文件 I/O**：mmap（`memmap2`）
- **提取**：`flate2`（gzip）、`zip`（zip/docx/xlsx）、PDF 文本提取
- **缓存**：SHA256 内容寻址存储 + LRU 淘汰（`filetime`）

## 文档

- [测试报告](docs/TESTING.zh-CN.md)
- [路线图](docs/ROADMAP.zh-CN.md)

## 许可证

MIT
