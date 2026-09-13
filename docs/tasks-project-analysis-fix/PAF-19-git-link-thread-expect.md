# PAF-19 git_link 常驻线程 expect ×3，SQL 抖动即线程死亡

| 项 | 值 |
|---|---|
| 优先级 | P1 |
| 状态 | ⬜ 未开始 |
| 来源 | 项目全景分析报告（docs/project-analysis-2026-09-13.md）P1-22，主控亲自验证 |
| 关联任务 | R-21（Git 联动） |

## 问题描述

`runtime/git_link.rs:167/:172/:174` 三处 `expect("repo_status ...")` 位于
常驻 `runtime-git-link` 线程循环（`dirty_for_app`）内——任何一次 SQL
prepare/IO 错误（磁盘抖动、DB 忙）即 panic，线程死亡且无人重启，Git 联动
（分支复核、dirty 提示）从此静默失效直到应用重启。

## 定位与修复建议

- 三处 expect 降级为 `log::error!` + 本轮跳过（继续下个 tick）；
- 顺手核对同文件其他线程内 expect。

## 验收标准

- [ ] 注入临时 SQL 错误后线程存活、下个 tick 恢复正常
- [ ] `cargo test --lib` 通过

## 进度

### 状态

- 当前状态：⬜ 未开始
- 最近更新：2026-09-13 录入

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-09-13 | ⬜ | 分析报告事实核查批次录入 |
