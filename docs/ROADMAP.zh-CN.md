# 路线图

[English](ROADMAP.md)

## 当前状态（v0.1）

BitScout 是一个功能完整的 AI Agent 搜索加速器：
- BusyBox 模式通过 symlink 替代 `rg`/`grep`/`find`/`fd`/`cat`
- 透明搜索 `.gz`、`.zip`、`.pdf`、`.docx`、`.xlsx` 内容
- BM25 相关性评分（`--bm25`、`--bm25=full`）
- LSA 语义搜索（`--semantic`）— 无需 Embedding 模型，SIMD 加速
- SHA256 CAS 内容缓存 + LRU 淘汰
- 与原始工具 100% 兼容，冷启动快 3-81 倍

## 下一步：持久化记忆（`/memory`）— v0.2

> 下一版本的首要目标。

面向 AI Agent 的跨服务长期记忆模块。基于自定义文件数据格式构建，非键值数据库。提供统一的记忆层，在不同工具（Claude Code、Cursor、OpenClaw 等）之间共享，切换 Agent 不丢失上下文。

- [ ] 自定义记忆文件格式 — 专为记忆存储和检索设计的二进制格式
- [ ] 项目本地初始化 — 从项目结构自动引导记忆上下文
- [ ] 自动上下文注入 — 搜索时自动浮现相关记忆
- [ ] 结构化读写 — 通过 CLI 和编程 API 提供存取接口
- [ ] 跨服务可移植 — 统一记忆层可在任意编程工具或 Agent 间共享
- [ ] 新型检索算法 — 超越关键词匹配的记忆优化检索
- [ ] 基于 RL 的反馈回路 — 通过强化学习根据 Agent 使用信号进行记忆巩固和排序

## 短期

### 性能
- [ ] 增量 FileTree — 通过 `mtime` 比较跳过未变更目录
- [ ] 并行文件扫描 — 基于 rayon 的多线程目录遍历
- [ ] SIMD 加速 Aho-Corasick — 用手写 AVX2/NEON 替换 `aho-corasick` crate
- [ ] LSA 索引缓存 — 持久化 SVD 结果避免重复计算

### 搜索质量
- [ ] 改善小语料 LSA — 根据语料规模自适应 SVD 秩选择
- [ ] 标识符感知分词 — 将 `camelCase` 和 `snake_case` 拆分为独立单词
- [ ] 语言感知停用词 — 移除语言特定的样板 token（fn、let、const、import）
- [ ] 查询词加权 — 稀有词对语义评分贡献更大

### 兼容性
- [ ] `rg` 多行搜索（`-U` + regex）
- [ ] `grep -E` 扩展正则支持
- [ ] `find -mtime`、`-size` 谓词
- [ ] `.tar.gz` 和 `.tar.bz2` 归档搜索

## 中期

### Agent 集成
- [ ] MCP（Model Context Protocol）服务器模式 — 将搜索暴露为 MCP 工具
- [ ] LSP 式 daemon 模式 — 通过 Unix Domain Socket 的持久化进程，支持会话级缓存
- [ ] 所有命令的结构化 JSON 输出 — 面向 Agent 的机器可读结果
- [ ] Watch 模式 — 文件系统监听器实现增量索引更新

### 格式支持
- [ ] `.pptx` 演示文稿文本提取
- [ ] `.epub` 电子书文本提取
- [ ] `.sqlite` / `.db` 数据库内容搜索
- [ ] Source map 支持 — 将压缩的 JS 映射回原始源码

### 评分
- [ ] BM25 + LSA 混合评分 — 结合关键词精确度和语义召回率
- [ ] 文件重要性加权 — 最近和高频访问的文件排名更高
- [ ] 依赖感知排序 — 被更多文件 import 的文件排名更高

## 长期

### 智能化
- [ ] 项目知识图谱 — 构建代码实体和关系的持久化图
- [ ] 跨项目学习 — 在相似项目间共享 LSA 索引
- [ ] Agent 反馈回路 — 从 Agent 实际使用的结果中学习

### 平台
- [ ] Windows 支持（MSVC 构建）
- [ ] WebAssembly 构建 — 在浏览器 IDE 中运行
- [ ] 包管理器 — Homebrew、cargo install、apt/rpm

## 明确不做的事

以下功能不在项目范围内：

- **Embedding 模型** — BitScout 设计为无模型；语义搜索仅依赖 LSA/RP
- **Daemon 优先架构** — 每次调用冷启动是核心设计选择
- **IDE 插件** — BitScout 是面向 Agent 的 CLI 工具，不是 GUI 扩展
- **完整替代 ripgrep** — 不支持的 flag 按设计 fallback 到真实命令
