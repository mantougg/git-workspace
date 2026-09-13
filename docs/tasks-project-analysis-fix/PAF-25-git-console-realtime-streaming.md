# PAF-25 Git Console 实时流式接线（run_git_streaming → git_op_output）

| 项 | 值 |
|---|---|
| 优先级 | P1 |
| 状态 | ✅ 已完成 |
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

- [x] 长 push/fetch 期间 Git Console 实时滚动输出（on_line → 100ms 窗口聚合 → `git_op_output` 实时事件）
- [x] 任务收尾的汇总与状态展示不回归（`git_command_result` 照常发送；流式任务收尾不再重复发 meta/输出行，非流式任务格式不变）
- [x] `cargo test --lib` 通过

## 进度

### 状态

- 当前状态：✅ 已完成
- 最近更新：2026-09-13 修复完成

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-09-13 | ⬜ | 分析报告事实核查批次录入 |
| 2026-09-13 | ✅ | 与 PAF-08 同批实施。修复：① worker 网络任务分支接入 `fetch/pull/push/clone_streaming`，`ConsoleStreamer` 桥接 on_line → `git_op_output`（100ms 窗口聚合进度行防事件风暴，T-06 思路）；命令标题行 `$ git fetch <remote>` 先于输出发出，失败/取消/超时补 `✘` 结论行；② 完整输出仍累积进 task output（`git_command_result`、DAG/batch 汇总、task_items 格式不变）；③ 收尾 `git_op_output` 段对已流式任务跳过（防重复），非流式任务改用提取的 `emit_git_op_output` 助手（行为不变）；④ `network_console_command` 提取共用标题行映射。验证：`cargo test --lib` 通过。 |
