# PAF-25 Git Console 实时流式接线（run_git_streaming → git_op_output）

| 项 | 值 |
|---|---|
| 优先级 | P1 |
| 状态 | ⬜ 未开始 |
| 来源 | 项目全景分析报告（docs/project-analysis-2026-09-13.md）P1-29 + §4 建议 1，核查智能体验证 |
| 关联任务 | TM-04（Git 输出镜像）、T-05、T-20 |

## 问题描述

TM-04 的设计目标是「网络操作（fetch/pull/push/clone）获得实时流式输出」，
但 `task/worker.rs:401-426` 在任务收尾（final_status 确定后）才批量 emit
`git_op_output`——长 push/fetch 期间 Git Console 静默。流式底座其实已就绪：
`core/git_ops/remote.rs:228` `run_git_streaming` 逐行回调存在，
`fetch_streaming/pull_streaming/push_streaming/clone_streaming`（:116-177）
封装齐全，但**全仓零外部调用方**（worker 走非流式 `run_git`，:191）。

## 定位与修复建议

- TaskType 网络操作执行器改走 `*_streaming`，`on_line` 回调桥接为
  `git_op_output` 事件（注意事件频率聚合，参照 T-06 批量思路防事件风暴）；
- 收尾仍发汇总 meta 行，保持既有格式兼容；
- 与 PAF-08（CLI 超时 kill）共用流式底座，建议同批实施。

## 验收标准

- [ ] 长 push/fetch 期间 Git Console 实时滚动输出
- [ ] 任务收尾的汇总与状态展示不回归
- [ ] `cargo test --lib` 通过

## 进度

### 状态

- 当前状态：⬜ 未开始
- 最近更新：2026-09-13 录入

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-09-13 | ⬜ | 分析报告事实核查批次录入 |
