# F-12 Stop 无法终止已启动的 JVM（Windows）

| 项 | 值 |
|---|---|
| 优先级 | P0 |
| 状态 | ⬜ 未开始 |
| 来源 | 2026-08-27 F-11 实测验证时牵出 |
| 关联任务 | R-10 Launcher、`process/kill_tree.rs` |

## 问题描述

release.2 真实场景验证（hussar-base-web）中：应用起到 Running（宽限到期、
进程存活），随后 `stop(process_id, 15s)` 返回成功但** JVM 进程仍然存活**
（`process_alive(pid)` = true，需手工杀）。同一 JVM 在更早一次实测中也
观察到 manager.start 超过 start_grace 未返回——疑似与监控/停止链路同源。

## 定位线索

- 停止链路：`runtime/launch/manager.rs::stop` → `process/kill_tree.rs`
  （Windows 无 SIGTERM，优雅停止用 `terminate_process`，超时升级整树终止）
- 监控等待：`manager.rs` 的 RunWait / GraceElapsed 分支（此前观察到超宽限
  不返回的现象，本次 120s 宽限正常返回，问题可能只在停止侧）
- 复现：真实启动一个长驻 JVM（hussar 或 R-10 boot fixture 改为长等待），
  stop 后断言 `!process_alive(pid)`

## 修复范围

- [ ] 复现并定位 stop 返回成功但 JVM 存活的根因（terminate 信号未达？
  pid 跟踪错位？升级杀树未触发？）
- [ ] 修复并补回归测试（stop 后进程必须不存在）
- [ ] 排查 start 超宽限不返回是否与停止链路同源

## 验收标准

- [ ] stop 后被停进程真实不存在（不残留孤儿 JVM）
- [ ] 新增回归测试通过；既有 R-10 停止/强杀测试不回归

## 进度

### 状态

- 当前状态：未开始
- 最近更新：2026-08-27 问题录入

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-27 | ⬜ | 问题录入（F-11 实测牵出：Running 后 stop 15s JVM 仍存活，PID 手工清理） |
