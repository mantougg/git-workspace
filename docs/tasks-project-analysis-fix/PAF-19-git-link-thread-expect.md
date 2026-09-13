# PAF-19 git_link 常驻线程 expect ×3，SQL 抖动即线程死亡

| 项 | 值 |
|---|---|
| 优先级 | P1 |
| 状态 | ✅ 已完成 |
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

- [x] 注入临时 SQL 错误后线程存活、下个 tick 恢复正常（`dirty_for_app_survives_sql_error_and_recovers`：DROP 表模拟 prepare 失败 → 空快照不 panic → 重建表后恢复正常快照）
- [x] `cargo test --lib` 通过（runtime::git_link 4 passed）

## 进度

### 状态

- 当前状态：✅ 已完成
- 最近更新：2026-09-13 修复完成

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-09-13 | ⬜ | 分析报告事实核查批次录入 |
| 2026-09-13 | ✅ | 复核证据成立（三处 expect 均在常驻线程循环内）。`dirty_for_app` 的 repo_status 查询改为返回 `AppResult` 的闭包：prepare/query_map 失败 → `log::error!` + 返回空快照（与函数内其余错误路径一致），单行读取失败 → 跳过该行；不再有 panic 通道，线程每 tick 重发查询自然恢复。顺手核对：同文件其余 `unwrap` 均为 Mutex lock 与测试代码，无常驻循环内的其他 `expect`。验证：`cargo test --lib runtime::git_link` 4 passed（含新增 SQL 故障注入回归）。 |
