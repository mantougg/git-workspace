# PAF-02 Runtime spawn 超时后进程存活但记录终态，不可停止（孤儿进程）

| 项 | 值 |
|---|---|
| 优先级 | P0 |
| 状态 | ⬜ 未开始 |
| 来源 | 项目全景分析报告（docs/project-analysis-2026-09-13.md）P0-2，主控亲自逐链验证 |
| 关联任务 | R-10（启动器与进程管理）、F-12（Stop JVM 泄漏） |

## 问题描述

链路（全部实证）：

1. `runtime/launch/manager/start.rs:140` `spawn_monitor` 先行启动；
2. `:143-153` `wait_pid_or_outcome(handle, 10s)` 超时走 `Timeout` 分支；
3. `:151` `abort_before_spawn`（`:415-431`）把 DB 行置为**终态 Failed**
   且 `cancelled: false`，monitor 线程无 spawn 前取消检查；
4. spawn 随后真正成功（Windows Defender 冷扫描 java.exe / .cmd shim 使
   spawn 超过 10 秒是现实场景），进程存活运行；
5. 用户点 Stop/Kill 时 `control.rs:19/:100` 的 `row.status.is_terminal()`
   直接早退——**UI 永远无法停止这个进程**。

## 定位与修复建议

- Timeout 分支不落终态：保持 Stopping/Starting，或对「行终态但进程存活」
  加对账兜底；
- `abort_before_spawn` 后 monitor 拿到 pid 时应检测取消标记并立即 kill；
- 评估 10s 窗口是否过短（可随首轮启动放宽或按平台区分）。

## 验收标准

- [ ] 模拟 spawn 确认超 10s 的场景，超时后 Stop 仍能终止进程
- [ ] 不再出现「Failed 终态 + 活进程」并存的记录
- [ ] 回归测试（fake runner 延迟 pid 回填）通过
- [ ] `cargo test --lib` 通过

## 进度

### 状态

- 当前状态：⬜ 未开始
- 最近更新：2026-09-13 录入

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-09-13 | ⬜ | 分析报告事实核查批次录入 |
