# Testing Report

[中文版](TESTING.zh-CN.md)

## Summary

| Test Suite | Tests | Status |
|---|---|---|
| bitscout-core unit | 122 | All pass |
| rg conformance (e2e) | 18 | All pass |
| grep conformance (e2e) | 8 | All pass |
| find/fd conformance (e2e) | 8 | All pass |
| cat conformance (e2e) | 10 | All pass |
| full conformance (e2e) | 44 | All pass |
| semantic accuracy (e2e) | 12 | All pass |
| benchmark modes (e2e) | 3 | All pass |
| **Total** | **181+** | **All pass** |

## Running Tests

```bash
cargo test                                             # All tests
cargo test -p bitscout-core --lib                      # Core unit tests (122)
cargo test -p bitscout-e2e --test full_conformance     # Conformance (44)
cargo test -p bitscout-e2e --test semantic_accuracy    # Semantic accuracy (12)
cargo test -p bitscout-e2e --test benchmark_modes      # Performance benchmarks (3)
```

## Conformance: 100% Compatible

44 conformance tests compare BitScout output against real `rg`/`grep`/`find`/`fd`/`cat`, line-by-line exact match:

```
Tool    Tests   Pass    Accuracy
------- ------- ------- --------
rg       18      18     100%
grep      8       8     100%
find      4       4     100%
fd        4       4     100%
cat      10      10     100%
------- ------- ------- --------
Total    44      44     100%
```

12 regex patterns compared against real rg, line-level accuracy **100%**.

## Speed: 3-81x Faster

Cold-start performance comparison (no daemon, independent scan per invocation):

```
Tool       real cmd    BitScout    Speedup
---------- ---------- ---------- ---------
cat          1.1ms     0.014ms     81.3x
find         1.5ms     0.12ms      12.9x
rg           6.1ms     0.77ms       8.0x
grep         2.6ms     0.82ms       3.2x
```

### All Search Modes

Cold-start latency including FileTree scan + search + scoring:

```
Mode                    Avg(us)   Notes
----------------------- --------- -------------------------
find -name '*.rs'          123   Filename glob matching
fd -e rs                   117   Filename extension matching
cat file                    14   File read
rg literal                 769   Literal search
rg regex                  2456   Regex search
grep -rn                   823   Recursive line search
rg --bm25                  907   BM25 relevance scoring
rg --bm25=full             948   BM25 + IDF full scoring
rg --semantic              771   LSA semantic search
```

All modes < 2.5ms, meeting real-time interaction requirements for AI agents.

## Semantic Search Accuracy: Top-1 100%

LSA semantic search (embedding-free, based on truncated SVD of TF-IDF):

```
Query                            Expected #1        Result         Score
-------------------------------- ------------------ -------------- ------
login password authenticate      auth_login.rs      auth_login.rs  0.214
jwt token generate validate      auth_jwt.rs        auth_jwt.rs    0.642
database query insert migrate    database.rs        database.rs    0.235
http server listen connection    http_server.rs     http_server.rs 0.168
cache evict lru store            cache.rs           cache.rs       0.244
-------------------------------- ------------------ ----------------------
Top-1 Accuracy: 5/5 (100%)
Top-3 Accuracy: 5/5 (100%)
```

RP unit-level discrimination: auth(0.70) > session(0.13) > math(-0.20) — relevant documents score significantly higher than unrelated ones.

### Semantic Test Coverage

| Test | Description |
|---|---|
| RP cosine similarity ranking | Verifies correct ordering: highly related > somewhat related > unrelated |
| RP code pattern ranking | Error handling terms rank above database/config terms |
| Auth file ranking for token query | Auth-focused files outrank tokenizer and config files |
| Database file ranking | Database file ranks first for DB-related queries |
| Semantic reordering vs plain search | Semantic results differ from filesystem order |
| Multi-word query ranking | Auth files appear in top-2 for "authenticate session" |
| Score variance | Meaningful spread between related and unrelated documents |
| Deterministic scoring | Same query always produces same scores |
| Comprehensive accuracy report | 5 diverse queries with expected rankings |
| LSA cross-vocabulary | Co-occurring terms enable semantic bridging |
| LSA synonym discovery | Co-occurring pairs (error/exception, success/ok) become similar |
| Pure LSA cross-vocabulary | Auth files rank above DB files without external embeddings |
