# F-12 Stop 无法终止已启动的 JVM（Windows）

| 项 | 值 |
|---|---|
| 优先级 | P0 |
| 状态 | ✅ 已完成（2026-08-28） |
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

- [x] 复现并定位 stop 返回成功但 JVM 存活的根因（terminate 信号未达？
  pid 跟踪错位？升级杀树未触发？）
- [x] 修复并补回归测试（stop 后进程必须不存在）
- [x] 排查 start 超宽限不返回是否与停止链路同源

## 验收标准

- [x] stop 后被停进程真实不存在（不残留孤儿 JVM）
- [x] 新增回归测试通过；既有 R-10 停止/强杀测试不回归

## 进度

### 状态

- 当前状态：✅ 已完成
- 最近更新：2026-08-28 修复完成并实测验证

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-27 | ⬜ | 问题录入（F-11 实测牵出：Running 后 stop 15s JVM 仍存活，PID 手工清理） |
| 2026-08-28 | 🟦 | 开始修复 |
| 2026-08-28 | ✅ | **根因**：JVM 的 GBK 中文日志（非法 UTF-8）令 reader 的 `read_line` 报错早死；两侧 reader 全断后监控主循环在 Disconnected 分支阻塞 `child.wait()`，此后 force_kill/timeout 无人轮询——Windows 无 SIGTERM（terminate 恒 false），停止全押在该标志上，故 stop 等满 grace 无功而返、JVM 残留。「start 超宽限不返回」同源：reader 早死令横幅探测失明，只能等满 grace。**修复**：① `streaming.rs` 主循环改 `readers_done` 短睡轮询，reader 全断后 cancel/timeout 仍可达；② reader 改 `read_until` + `from_utf8_lossy`，非法字节只损失为 U+FFFD（顺带修复 GBK 应用日志丢失、管道写满卡死被监控进程）；③ `manager.rs` grace 升级分支去掉 adopted 限制直杀进程树兜底。**验证**：新增回归测试 3+1（`streaming::reader_lossy_decodes_invalid_utf8_and_keeps_reading`、`streaming::cancel_kills_child_after_readers_disconnect`、manager `real_process_windows::stop_kills_process_whose_output_streams_closed_early`，修复前均红；unix 变体 `stop_kills_sigterm_ignoring_process_that_closed_streams`）；全量 `cargo test`（JDK17）418 过（benchmark/concurrency/cancel 3 个失败为并行负载抖动，串行单跑即过，与本改动无关）；真实场景 `cargo test manual_hussar -- --ignored`：hussar-base-web 全量构建 → Running → stop(15s) → Stopped 且 JVM 真实消失（225s 通过）。AGENTS.md 平台规范 §3 已沉淀「reader 断开后不得阻塞 wait + 按字节 lossy 读」规则 |
