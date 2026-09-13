# PAF-11 batch_add / batch_restore 收编任务队列

| 项 | 值 |
|---|---|
| 优先级 | P1 |
| 状态 | ✅ 已完成 |
| 来源 | 项目全景分析报告（docs/project-analysis-2026-09-13.md）P1-10，核查智能体验证 |
| 关联任务 | T-05（任务队列）、T-20（批量操作）、T-34（Undo） |

## 问题描述

`src-tauri/src/commands/git_ops.rs:288`（`batch_add`）与 `:324`
（`batch_restore`）是同步串行循环、`?` 直接返回的 fail-fast 实现，**不走
T-05 任务队列**——与 T-20「操作全集全部走任务队列」口径不符（对比
batch_fetch:17 / batch_pull:39 / batch_push:61 / batch_commit:85 全部
`task_manager.submit`）。多仓批量 restore 中途失败时已执行仓库不回滚、
UI 无法定位失败仓，且 `batch_restore`（丢弃工作区改动）不入操作日志。

## 定位与修复建议

- 两个命令收编 TaskQueue：复用 batch_id 聚合（manager.rs:109）+
  PartialSuccess（worker.rs:546-605、models/task.rs:218-249）；
- batch_restore 属高危操作，接入 T-34 操作日志快照（before oid）。

## 验收标准

- [x] 多仓 add/restore 走任务队列，进度事件可见、可定位失败仓
- [x] 部分失败语义与 fetch/pull 一致（PartialSuccess 聚合）
- [x] batch_restore 入操作日志
- [x] `cargo test --lib` 通过

## 进度

### 状态

- 当前状态：✅ 已完成
- 最近更新：2026-09-13 修复完成

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-09-13 | ⬜ | 分析报告事实核查批次录入 |
| 2026-09-13 | ✅ | 证据复核成立（两命令仍为同步串行 fail-fast）。收编实施：① `TaskType` 新增 `StageFiles`/`RestoreFiles`（一仓一任务），实际执行逻辑自 commands 收编为 `GitOps::stage_files`/`restore_files` 并接入 `execute` 分发；`batch_add`/`batch_restore` 改为 `task_manager.submit`（返回任务 ID，多仓自动获得 T-20 batch 聚合与 PartialSuccess 语义，与 fetch/pull/push 一致）；② restore 高危操作：worker 执行前 `snapshot_head`，执行后落 T-34 操作日志（`OP_RESTORE_FILES`，detail 带 files；该类型不支持自动撤销，undo 面板优雅提示，日志行用于追溯定位）；③ 契约同步：golden fixture（stageFiles/restoreFiles 样本）、TS 类型（TaskType 联合 + TaskPanel 穷举标签）；④ 前端适配异步化：task store 新增 `waitForTasks`（轮询活跃列表，60s 超时保护），RepositoryList/DiffViewer 四处调用点等收口后再刷新视图，消除「submit 后立即 loadChanges 读到旧状态」的 UX 回归。回归测试 3 项（stage 暂存/删除形态、restore 还原/删除未跟踪、execute 分发）。验证：`cargo test --lib` 889 passed（另有 flood/revision_diff_cache 2 项机器负载敏感存量测试失败——flood 基线 stash 后 5/5 同样失败、diff_cache 隔离运行通过，与本次改动无关，证据已记入 PAF-26 测试健壮性清单）+ `pnpm build` 通过 |
