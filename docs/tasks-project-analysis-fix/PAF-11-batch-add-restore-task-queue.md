# PAF-11 batch_add / batch_restore 收编任务队列

| 项 | 值 |
|---|---|
| 优先级 | P1 |
| 状态 | ⬜ 未开始 |
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

- [ ] 多仓 add/restore 走任务队列，进度事件可见、可定位失败仓
- [ ] 部分失败语义与 fetch/pull 一致（PartialSuccess 聚合）
- [ ] batch_restore 入操作日志
- [ ] `cargo test --lib` 通过

## 进度

### 状态

- 当前状态：⬜ 未开始
- 最近更新：2026-09-13 录入

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-09-13 | ⬜ | 分析报告事实核查批次录入 |
