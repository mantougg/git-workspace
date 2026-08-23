# R-08 Runtime Benchmark 基线存档

> 本目录保存 Runtime Benchmark 的 JSON 基线（`runtime_<repos>x<modules>.json`），随仓库版本化，供后续性能敏感任务（R-09 / R-17 / R-18 …）对比回归。
> 重新生成：`cd src-tauri && cargo run --release --example benchmark -- runtime [repos] [modules]`，或 `runtime --matrix` 跑 §96 全矩阵（10/50/100 × 10/50/100）。
> 报告格式、§99 判定与对比口径见 `src-tauri/src/benchmark/runtime.rs` 与 `../R-08-idea-comparison.md`。

## 首次基线（2026-08-21）

机器：Linux 7.0.0-28-generic · i5-1135G7 @ 2.40GHz（8 核）· release 构建 · commit 工作区版本（R-08 引入）。

| repos × modules | POMs | Discovery (cold) | POM Cache Hit（命中数） | Graph Cache Hit | Config Load | File→Detection |
|---|---:|---:|---:|---:|---:|---:|
| 10 × 10 | 110 | 50 ms ✅ | 6 ms ✅（110/110） | 0 ms ✅ | 0 ms ✅ | 0 ms ✅ |
| 10 × 50 | 510 | 32 ms ✅ | 27 ms ✅（510/510） | 1 ms ✅ | 0 ms ✅ | 0 ms ✅ |
| 10 × 100 | 1010 | 64 ms ✅ | 51 ms ❌（1010/1010） | 2 ms ✅ | 0 ms ✅ | 0 ms ✅ |
| 50 × 10 | 550 | 53 ms ✅ | 54 ms ❌（550/550） | 1 ms ✅ | 0 ms ✅ | 0 ms ✅ |
| 50 × 50 | 2550 | 270 ms ✅ | 260 ms ❌（1811/2550） | 7 ms ✅ | 0 ms ✅ | 0 ms ✅ |
| 50 × 100 | 5050 | 562 ms ❌ | 535 ms ❌（1521/5050） | 14 ms ✅ | 0 ms ✅ | 0 ms ✅ |
| 100 × 10 | 1100 | 165 ms ✅ | 158 ms ❌（1100/1100） | 2 ms ✅ | 0 ms ✅ | 0 ms ✅ |
| 100 × 50 | 5100 | 890 ms ❌ | 1070 ms ❌（1507/5100） | 18 ms ✅ | 0 ms ✅ | 0 ms ✅ |
| 100 × 100 | 10100 | 2135 ms ❌ | 1946 ms ❌（740/10100） | 30 ms ✅ | 0 ms ✅ | 1 ms ✅ |

资源包线（10×10）：Idle RSS 9.4 MB / Peak RSS 18.5 MB / 线程峰值 11 / 子进程 0（符合「测量阶段零 Maven/Java 子进程」预期）。

## 基线发现（反馈给后续优化任务）

1. **§99 目标在 ≤ ~1000 POM 规模达标**：Discovery < 500ms 在 5050 POM 起超线（562ms → 2135ms）；POM Cache Hit < 50ms 在 ~1000 POM 起超线（51ms+）。
2. **PomCache 容量 2048 在 ≥2550 POM 时发生淘汰**（命中率 1811/2550 → 740/10100），大规模下「缓存命中重载」名存实亡——容量策略需随工程规模调整（属 R-01/R-02 后续优化，非 R-08 范围）。
3. **缓存命中重载路径仍全量重建 effective model**（读盘 + hash + effective 重建），是 POM Cache Hit 超线的主因；冷 Discovery 的规模瓶颈同理。
4. Graph Cache / Closure / Config / File-Detection 在 10100 POM 下仍远低于预算（≤30ms），SQLite + moka 组合无规模问题。
5. Build / Application Start 指标字段已预留（`build_ms` / `app_start_ms`），待 R-09 / R-10 接入。
