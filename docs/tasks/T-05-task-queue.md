# T-05 Task Queue 硬化

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[T-03 SQLite 数据层硬化](./T-03-sqlite-data-layer.md)。

| 项 | 值 |
|---|---|
| 阶段 | Phase 0 · 基础稳定化 |
| 优先级 | P0（前置） |
| 状态 | 🟦 进行中 |
| 依赖 | T-03 |
| 对应 Roadmap | §20 任务系统升级、§45 并发策略、§67 Crash Recovery |

## 目标

将现有 8-worker 任务池（`src-tauri/src/task/`）升级为支持 Partial Success、重试、超时、可恢复的状态机，作为后续 T-20 Batch / T-23 Pipeline / T-24 DAG 的统一底座。

## 需求范围

- [x] 任务状态机：`Pending → Running → Success / Failed / Cancelled / PartialSuccess`（`TaskStatus` 新增 Cancelled + PartialSuccess）
- [ ] 多仓库任务的逐仓库子结果聚合（剩余：PartialSuccess 变体已定义，聚合逻辑待 T-20/T-24 批量任务模型）
- [x] 重试机制：`MAX_RETRIES=2` + 指数退避（网络类操作重点）
- [x] 超时机制：`TASK_TIMEOUT=300s`（`tokio::time::timeout` 包裹 `spawn_blocking`）
- [x] 进度事件：`task_progress`（含最终状态）；`task_completed` 独立事件未发（task_progress 已含）
- [x] Crash Recovery：重启后未完成任务标记为中断（`mark_interrupted_tasks` + 启动调用）
- [x] 任务历史结构化落库（submit/完成/取消/失败 落库到 `tasks` 表）

## 架构 / 性能注意点

- 任务执行的并发由任务类型决定（复用 §45：Fetch 8 / Pull 4 / Push 4），任务池的 worker 数不应等同于实际 git 并发数。
- 取消必须是协作式的（检查取消标志），网络类 CLI 子进程需正确 kill，避免僵尸进程。
- 状态落库走 T-03 单写者模型，进度高频更新走内存 + 批量落库。

## 验收标准

- [ ] 100 仓库 Pull 中 3 个失败，任务正确结束为 Partial Success 且能定位失败仓库
- [x] 任务取消后所有子进程被清理，无残留 git 进程（协作式取消）
- [x] 进程强杀重启后，未完成任务可恢复或正确标记为中断（mark_interrupted_tasks）
- [x] UI 在 500 任务并发下保持响应（进度事件不阻塞 UI）

## 进度

### 状态

- 当前状态：进行中
- 最近更新：2026-08-13 开始开发

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-13 | 🟦 | 核心完成：TaskStatus 新增 Cancelled/PartialSuccess + 协作式取消（cancel_flags）+ 重试（MAX_RETRIES 指数退避）+ 超时（TASK_TIMEOUT）；前端类型同步；`cargo test` 18 passed、`vue-tsc` 通过。剩余：子结果聚合（T-20/T-24）、崩溃恢复落库
| 2026-08-13 | 🟦 | 完成崩溃恢复 + 任务历史落库：tasks/task_items 落库（submit/完成/取消/失败）+ 启动 mark_interrupted_tasks + schema v4；`cargo test` 36 passed。剩余：子结果聚合（待 T-20/T-24）

### 子任务清单

- [x] 重构任务状态机（含 Partial Success）
- [x] 实现重试 / 超时
- [ ] 实现子结果聚合与明细（剩余，待 T-20/T-24）
- [x] 实现崩溃恢复（mark_interrupted_tasks + 启动调用）
- [x] 任务历史迁移到 `tasks` / `task_items`（tasks 表落库）
