# PAF-10 Git 操作数据安全前置校验（rebase 脏区 / 分支切换 / merge 前置 / cherry-pick abort）

| 项 | 值 |
|---|---|
| 优先级 | P1 |
| 状态 | ⬜ 未开始 |
| 来源 | 项目全景分析报告（docs/project-analysis-2026-09-13.md）P1-6~P1-9，主控 + 核查智能体双重验证 |
| 关联任务 | T-13、T-15、T-16、T-34 |

## 问题描述

四项已实证的 Git 操作安全缺口：

1. **rebase 启动不查脏工作区**：`core/rebase.rs:164-165` ops 校验后直接
   `history::reset_to(onto, "hard")`，未暂存修改被静默丢弃，且启动阶段的
   rebase 不入 undo log；`rebase_skip`/`rebase_abort`/`merge_abort` 的
   hard reset 同理会连带丢弃冲突期间的手工未暂存修改。
2. **rebase_continue 不校验分支被切换**：`core/rebase.rs:182-211` 只查
   index 冲突与 position，结果 set 到当前 HEAD 所指 ref（:342-344）——
   挂起期间切分支再 Continue，rebase 链写到错误分支。
3. **merge 无互斥/前置校验**：`core/merge.rs:37-73` 不检查已有 MERGE_HEAD
   （`merge_in_progress` 存在但未被调用）、不查脏区、不查自研 rebase 进行中。
4. **多 commit cherry-pick 中途 Abort 语义破损**：
   `src/views/ConflictResolver.vue:312/:476` 调 `abortPick(repoPath)` 不传
   `base_oid`；`core/history.rs:165-171` 在 None 时 reset 到当前 HEAD——
   前 N-1 个已落地的 pick 提交残留，与 UI 文案「恢复到操作前状态」不符。

## 定位与修复建议

- rebase/merge 启动前用 `repo.statuses()` 查脏区 + 已有 MERGE_HEAD/rebase
  状态互斥（结构化 Conflict 错误）；
- rebase_continue 校验当前 HEAD 仍是 rebase 目标分支；
- ConflictResolver abort 透传 PickOutcome 携带的 base_oid；
- hard reset 路径评估是否改 keep 未暂存改动或显式确认。

## 验收标准

- [ ] 脏工作区启动 rebase/merge 被拒绝并提示
- [ ] 挂起期间切分支后 Continue 被拒绝
- [ ] 多 commit cherry-pick 中途 Abort 恢复到操作前 HEAD
- [ ] 各项单测通过

## 进度

### 状态

- 当前状态：⬜ 未开始
- 最近更新：2026-09-13 录入

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-09-13 | ⬜ | 分析报告事实核查批次录入 |
