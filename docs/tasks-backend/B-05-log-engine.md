# B-05 拆 Log Engine（logs/engine.rs → engine/）

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[B-01](./B-01-baseline-test-extraction.md)（测试已外移）。设计约束见 [backend-module-split-plan.md](../backend-module-split-plan.md) §4.5、§6 Phase 3。

| 项 | 值 |
|---|---|
| 阶段 | Phase 3 · 数据索引与日志引擎 |
| 优先级 | P1 |
| 状态 | ⬜ 未开始 |
| 依赖 | B-01 |
| 对应设计文档 | §2.2 日志引擎问题、§4.5 目标目录、§6 Phase 3 |

## 目标

把约 755 行生产代码的 `runtime/logs/engine.rs` 按「会话 / worker / 查询 / 存储」拆成 `runtime/logs/engine/` 子模块：实时捕获与历史查询共享格式但不共享生命周期（§2.2）。`runtime::logs::RuntimeLogEngine` 公共路径不变。

## 需求范围

- [ ] 目标结构（§4.5）：`engine/{mod.rs, session.rs, worker.rs, query.rs, storage.rs, tests.rs}`
- [ ] 迁移顺序（§6 Phase 3）：先拆查询（`query.rs`）和存储（`storage.rs`），再拆 worker/session（后台线程，高风险）
- [ ] `session.rs`：LogSession、Ring（内存环形缓冲）、SessionMsg
- [ ] `worker.rs`：批量聚合、脱敏后落盘、事件发送、文件滚动
- [ ] `query.rs`：search / tail / export / clear，保持流式读取
- [ ] `storage.rs`：日志目录、段文件、容量上限、路径安全
- [ ] `mod.rs`：RuntimeLogEngine 公共门面 + re-export

## 架构 / 性能注意点

- 实时捕获路径必须保持轻量（§4.5）：捕获线程只做脱敏和发送消息；文件写入、分析、事件聚合由 worker 执行。
- `query.rs` 必须流式读取，不把整个日志文件加载到内存（§4.5）。
- 日志在**落盘前**完成脱敏（T-08 红线）；应用日志保持只读（§6 Phase 3 验收重点）。
- 文件滚动与容量上限行为不变；路径安全（防逃逸）逻辑进 `storage.rs` 并保留测试。

## 验收标准

- [ ] 日志搜索 / 导出 / tail 仍为流式读取（无全量加载，既有测试通过）
- [ ] 落盘前脱敏行为不变（测试断言）
- [ ] 捕获线程不直接做 IO/分析（代码走查确认）
- [ ] 滚动与容量上限行为不变；应用日志只读
- [ ] 四件套全绿；公共 re-export 不变，调用方零修改

## 进度

### 状态

- 当前状态：未开始
- 最近更新：—

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|

### 子任务清单

- [ ] `storage.rs`（目录/段文件/路径安全）
- [ ] `query.rs`（search/tail/export/clear）
- [ ] `session.rs`（LogSession/Ring/SessionMsg）
- [ ] `worker.rs`（聚合/落盘/事件/滚动）
- [ ] `mod.rs` 门面与 re-export
- [ ] 测试归位与四件套验证
