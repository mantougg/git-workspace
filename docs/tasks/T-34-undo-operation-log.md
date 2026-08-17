# T-34 统一 Undo / 操作日志

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[T-05 Task Queue 硬化](./T-05-task-queue.md)。

| 项 | 值 |
|---|---|
| 阶段 | Phase 2 · Multi-Repo Engine（P1） |
| 优先级 | P1 |
| 状态 | ✅ 已完成 |
| 依赖 | T-05 |
| 对应 Roadmap | §46/§47 操作安全（Safety First 可恢复） |

## 目标

在 Safety First「可恢复」原则下，补上事前确认、事后 reflog 之外的第 3 层：**操作日志 + 反向操作**，对可逆批量操作支持一键撤销。

## 需求范围

- [x] 操作日志：谁 / 什么操作 / 哪个仓库 / 操作前 ref 快照（落库，复用 T-03 单写者）
- [x] 对可逆操作生成反向操作（如 Checkout All → 一键撤销回原 ref）
- [x] 初期只覆盖高危批量操作（Checkout / Reset / Delete Branch / Rebase）
- [x] 撤销前确认 + 影响范围展示（§46 Dangerous 分级）

## 架构 / 性能注意点

- ref 快照只存 oid（纯数据），不存 libgit2 句柄（全局约束 §3）。
- 日志写走 T-03 单写者，批量落库，不逐操作 INSERT。
- 撤销本质是反向批量操作，复用 T-05 任务队列。

## 验收标准

- [x] 高危批量操作前记录 ref 快照
- [x] 一键撤销恢复操作前状态（可验证 ref 回退）
- [x] 操作日志可查询（仓库 / 时间 / 操作类型）

## 进度

### 状态

- 当前状态：已完成
- 最近更新：2026-08-17 完成

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-17 | 🟦 | 开始开发：操作日志落库 + ref 快照埋点 + 反向操作撤销 + 日志查询 UI |
| 2026-08-17 | ✅ | 完成：`core/operation_log.rs`（`operation_logs`/`operation_log_items` 表 DAO 模块内实现 + `snapshot_head`/`snapshot_branch` 纯数据快照 + `record_operation_best_effort` 批量单事务 + 查询过滤 workspace/仓库/类型/日期分页 + `preview_undo`/`run_undo`：checkout→检出原分支、delete→按 before_oid 重建、reset→hard 回滚（脏工作区拒绝）、rebase→回滚到 pre-rebase head（进行中拒绝），状态前移守卫 + `persist_undo_results` 幂等）+ `commands/operation_log.rs` 4 command（list/get_detail/preview_undo/undo，DB 锁不跨 repo IO）；埋点接入 `batch_branch_op`（Checkout/Delete 提交前快照，Create 不记）、`history::reset_to`、`merge_rebase::start_rebase`；前端 types/api + OperationLogView（多条件筛选分页 + 展开懒加载明细 + 撤销预览确认列影响范围 + 部分失败明细）；schema V7 落库。验证：`core::operation_log` 7 tests passed（撤销回退 + 安全守卫 + 快照）；`cargo test --lib` 165 passed（仅余 batch dry_run 2 个沙箱 git clone 环境限制失败）；`vue-tsc --noEmit` 通过；IPC golden 补登记 7 个类型 |

### 子任务清单

- [x] 操作日志 schema 与落库
- [x] ref 快照记录（高危操作前）
- [x] 反向操作生成与一键撤销
- [x] 撤销确认流 + 日志查询 UI
