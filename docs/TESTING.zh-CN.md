# 测试报告

[English](TESTING.md)

## 总览

| 测试套件 | 数量 | 状态 |
|---|---|---|
| bitscout-core 单元测试 | 122 | 全部通过 |
| bitscout-memory 单元测试 | 4 | 全部通过 |
| rg 兼容性（e2e）| 18 | 全部通过 |
| grep 兼容性（e2e）| 8 | 全部通过 |
| find/fd 兼容性（e2e）| 8 | 全部通过 |
| cat 兼容性（e2e）| 10 | 全部通过 |
| 完整兼容性（e2e）| 44 | 全部通过 |
| 语义准确度（e2e）| 12 | 全部通过 |
| 性能基准（e2e）| 3 | 全部通过 |
| **合计** | **185+** | **全部通过** |

## 运行测试

```bash
cargo test                                             # 全部测试
cargo test -p bitscout-core --lib                      # 核心单元测试 (122)
cargo test -p bitscout-e2e --test full_conformance     # 兼容性对照 (44)
cargo test -p bitscout-e2e --test semantic_accuracy    # 语义准确度 (12)
cargo test -p bitscout-e2e --test benchmark_modes      # 性能基准 (3)
```

## 兼容性：100%

44 个 conformance 测试，对比 BitScout 与真实 `rg`/`grep`/`find`/`fd`/`cat` 输出，行级精确匹配：

```
工具    测试数   通过    准确率
------- ------- ------- --------
rg       18      18     100%
grep      8       8     100%
find      4       4     100%
fd        4       4     100%
cat      10      10     100%
------- ------- ------- --------
合计     44      44     100%
```

12 个 regex pattern 对比 real rg，行级准确率 **100%**。

## 性能：3-81 倍加速

冷启动性能对比（无 daemon，每次调用独立扫描）：

```
工具       原始命令    BitScout    加速比
---------- ---------- ---------- ---------
cat          1.1ms     0.014ms     81.3x
find         1.5ms     0.12ms      12.9x
rg           6.1ms     0.77ms       8.0x
grep         2.6ms     0.82ms       3.2x
```

### 全模式基准

冷启动耗时（含 FileTree 扫描 + 搜索 + 排序）：

```
模式                    平均(us)  说明
----------------------- --------- -------------------------
find -name '*.rs'          123   文件名 glob 匹配
fd -e rs                   117   文件名扩展名匹配
cat file                    14   文件读取
rg literal                 769   字面量搜索
rg regex                  2456   正则表达式搜索
grep -rn                   823   递归行搜索
rg --bm25                  907   BM25 相关性排序
rg --bm25=full             948   BM25 + IDF 全量评分
rg --semantic              771   LSA 语义搜索
```

所有模式 < 2.5ms，满足 AI Agent 实时交互需求。

## 语义搜索准确度：Top-1 100%

基于 LSA 的无 Embedding 语义搜索（TF-IDF 矩阵截断 SVD）：

```
查询                              期望 #1            结果           分数
-------------------------------- ------------------ -------------- ------
login password authenticate      auth_login.rs      auth_login.rs  0.214
jwt token generate validate      auth_jwt.rs        auth_jwt.rs    0.642
database query insert migrate    database.rs        database.rs    0.235
http server listen connection    http_server.rs     http_server.rs 0.168
cache evict lru store            cache.rs           cache.rs       0.244
-------------------------------- ------------------ ----------------------
Top-1 准确率: 5/5 (100%)
Top-3 准确率: 5/5 (100%)
```

RP 单元级区分度：auth(0.70) > session(0.13) > math(-0.20)，相关文档得分显著高于不相关文档。

### 语义测试覆盖

| 测试 | 描述 |
|---|---|
| RP 余弦相似度排序 | 验证排序正确：高相关 > 中等相关 > 不相关 |
| RP 代码模式排序 | 错误处理相关词排在数据库/配置词之上 |
| Token 查询的 Auth 文件排序 | Auth 文件排在 tokenizer 和 config 文件之上 |
| 数据库文件排序 | 数据库文件在 DB 相关查询中排名第一 |
| 语义重排序 vs 普通搜索 | 语义结果与文件系统顺序不同 |
| 多词查询排序 | Auth 文件在 "authenticate session" 查询中进入 top-2 |
| 分数方差 | 相关与不相关文档之间有有意义的分数差距 |
| 确定性评分 | 相同查询始终产生相同分数 |
| 综合准确度报告 | 5 个多样化查询的期望排名 |
| LSA 跨词汇 | 共现词实现语义桥接 |
| LSA 同义词发现 | 共现词对（error/exception、success/ok）变得相似 |
| 纯 LSA 跨词汇 | 无外部 Embedding 时 Auth 文件排在 DB 文件之上 |
