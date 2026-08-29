# B-05 拆 Log Engine（logs/engine.rs → engine/）

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[B-01](./B-01-baseline-test-extraction.md)（测试已外移）。设计约束见 [backend-module-split-plan.md](../backend-module-split-plan.md) §4.5、§6 Phase 3。

| 项 | 值 |
|---|---|
| 阶段 | Phase 3 · 数据索引与日志引擎 |
| 优先级 | P1 |
| 状态 | ✅ 已完成 |
| 依赖 | B-01 |
| 对应设计文档 | §2.2 日志引擎问题、§4.5 目标目录、§6 Phase 3 |

## 目标

把约 755 行生产代码的 `runtime/logs/engine.rs` 按「会话 / worker / 查询 / 存储」拆成 `runtime/logs/engine/` 子模块：实时捕获与历史查询共享格式但不共享生命周期（§2.2）。`runtime::logs::RuntimeLogEngine` 公共路径不变。

## 需求范围

- [x] 目标结构（§4.5）：`engine/{mod.rs, session.rs, worker.rs, query.rs, storage.rs, tests.rs}`
- [x] 迁移顺序（§6 Phase 3）：先拆查询（`query.rs`）和存储（`storage.rs`），再拆 worker/session（后台线程，高风险）
- [x] `session.rs`：LogSession、Ring（内存环形缓冲）、SessionMsg
- [x] `worker.rs`：批量聚合、脱敏后落盘、事件发送、文件滚动
- [x] `query.rs`：search / tail / export / clear，保持流式读取
- [x] `storage.rs`：日志目录、段文件、容量上限、路径安全
- [x] `mod.rs`：RuntimeLogEngine 公共门面 + re-export

## 架构 / 性能注意点

- 实时捕获路径必须保持轻量（§4.5）：捕获线程只做脱敏和发送消息；文件写入、分析、事件聚合由 worker 执行。
- `query.rs` 必须流式读取，不把整个日志文件加载到内存（§4.5）。
- 日志在**落盘前**完成脱敏（T-08 红线）；应用日志保持只读（§6 Phase 3 验收重点）。
- 文件滚动与容量上限行为不变；路径安全（防逃逸）逻辑进 `storage.rs` 并保留测试。

## 验收标准

- [x] 日志搜索 / 导出 / tail 仍为流式读取（无全量加载，既有测试通过）
- [x] 落盘前脱敏行为不变（测试断言）
- [x] 捕获线程不直接做 IO/分析（代码走查确认）
- [x] 滚动与容量上限行为不变；应用日志只读
- [x] 四件套全绿；公共 re-export 不变，调用方零修改

## 进度

### 状态

- 当前状态：已完成
- 最近更新：2026-08-29

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-29 | 🟦 | 开始开发：确认 B-01 已将测试外移至 `engine/tests.rs`（生产代码 757 行仍在 `engine/mod.rs`） |
| 2026-08-29 | ✅ | 完成：按 `storage → query → session → worker` 顺序逐职责组迁移，每组跑四件套。生产代码 757 行 → `mod.rs` 218 行（公共类型 + RuntimeLogEngine 门面）+ 4 个子模块（storage 79 / query 232 / session 131 / worker 172）。捕获路径走查确认：`LogSession::log` 仅脱敏 + 级别解析 + 发送（session.rs 无任何文件 IO 导入），分析与写盘全在 worker。验证：全量 `cargo test` 490 通过 / 3 ignored 与基线一致（仅 2 个既有 `maven::settings` 环境失败）；clippy 在 `logs/engine/` 零告警；`detect_changes()` 风险 LOW、受影响执行流 0；`runtime/logs/mod.rs` re-export 面未动，diff 无调用方文件。 |

**GitNexus 备注**：索引中缺失 `runtime/logs/` 全部文件（`RuntimeLogEngine` / `LogSession` 等符号查不到；重新 analyze 后依旧，疑为分析器对该目录的收录缺口）。impact 分析不可用，按全局约束 §4 以源码搜索兜底：外部调用方（service、launch/manager、commands、ipc_golden、runtime/mod.rs re-export）全部经 `runtime::logs::` / `runtime::` 稳定路径访问，无深层 `engine::` 导入。

### 子任务清单

- [x] `storage.rs`（目录/段文件/路径安全）
- [x] `query.rs`（search/tail/export/clear）
- [x] `session.rs`（LogSession/Ring/SessionMsg）
- [x] `worker.rs`（聚合/落盘/事件/滚动）
- [x] `mod.rs` 门面与 re-export
- [x] 测试归位与四件套验证
