# PAF-07 stop 在 pid 未回填时强杀也是 no-op，restart 撞 Stopping 报 Conflict

| 项 | 值 |
|---|---|
| 优先级 | P1 |
| 状态 | ⬜ 未开始 |
| 来源 | 项目全景分析报告（docs/project-analysis-2026-09-13.md）P1-2（经核查修正后结论） |
| 关联任务 | R-10、PAF-02（spawn 超时孤儿） |

## 问题描述

`runtime/launch/manager/control.rs:29-49`：`stop()` 时
`handle.pid()` 为 None（还在 Building 或 spawn 未回填）→ `terminate` 被
跳过；**强杀升级分支同样有 `if let Some(pid)` 守卫**，pid 持续为 None 时
强杀也是 no-op，stop 返回后行停留在 Stopping。`restart()`（:134-145）随即
`start()` 撞 `find_active`——Stopping 非终态（lifecycle.rs:64-66），返回
Conflict「已在运行」，用户只能干等。

## 定位与修复建议

- stop 时等待 `wait_pid_or_outcome` 短窗口拿 pid 再 terminate；
- pid 始终缺失时按 DB 行/进程名兜底清理，保证 stop 能收口到终态；
- restart 等待 stop 落终态后再 start（或对 Stopping 行做 join 语义）。

## 验收标准

- [ ] spawn 慢（pid 未回填）时点 Stop 不长时间无响应、行最终落终态
- [ ] 紧跟的 Restart 不报 Conflict
- [ ] 回归测试通过

## 进度

### 状态

- 当前状态：⬜ 未开始
- 最近更新：2026-09-13 录入

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-09-13 | ⬜ | 分析报告事实核查批次录入 |
