# PAF-08 git 网络任务超时后 spawn_blocking 线程无取消机制

| 项 | 值 |
|---|---|
| 优先级 | P1 |
| 状态 | ⬜ 未开始 |
| 来源 | 项目全景分析报告（docs/project-analysis-2026-09-13.md）P1-3（核查修正后表述） |
| 关联任务 | T-05（任务队列）、T-20（批量网络操作） |

## 问题描述

`task/worker.rs`：TASK_TIMEOUT=300s（:19）、MAX_RETRIES=2（:17）；超时分支
`:297-301` 仅对 Runtime/NodeInstall 类任务置 cancel flag；git 任务执行体
`:278` 不接收 cancel 参数，无取消机制。核查修正：泄漏的**不是** 8 个 async
worker（超时后它们回到循环继续取任务），而是 `spawn_blocking` 提交到 tokio
blocking 线程池（默认 512）的线程——网络挂起期间等效无限占用，且可重试
网络任务（:315-318）超时重试会再占新线程。另 `core/git_ops/remote.rs:191-222`
阻塞版 `run_git` 无超时；`commands/git_ops.rs:170-192` 的
sync_fetch/pull/push 在 Tauri 同步命令主线程执行，完全无超时保护。

## 定位与修复建议

- git CLI 路径统一走 `run_git_streaming`（remote.rs:228，已支持 timeout）并
  在超时后 kill 子进程；
- libgit2 无法中断的路径考虑改走 CLI 或接受限制并文档化；
- sync_* 命令加超时包装。

## 验收标准

- [ ] fetch/push 卡死任务超时后底层 git 进程被清理，blocking 线程回收
- [ ] 批量网络操作取消语义与 T-05 验收一致
- [ ] `cargo test --lib` 通过

## 进度

### 状态

- 当前状态：⬜ 未开始
- 最近更新：2026-09-13 录入

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-09-13 | ⬜ | 分析报告事实核查批次录入 |
