# PAF-24 PTY 生命周期加固（close 持锁 2s / 死会话回收 / pid=0 / 注释不符）

| 项 | 值 |
|---|---|
| 优先级 | P1 |
| 状态 | ✅ 已完成 |
| 来源 | 项目全景分析报告（docs/project-analysis-2026-09-13.md）P1-28，核查智能体逐项验证 |
| 关联任务 | TM-01（PTY 后端）、F-12 |

## 问题描述

`src-tauri/src/process/pty.rs`：

1. **close() 持锁等待**（:465-499）：持有 sessions Mutex 做最长
   CLOSE_GRACE_TIMEOUT=2s（:38）的 50ms 轮询，期间
   terminal_open/write/resize/list 全部阻塞，连续关多个 tab 时 IPC 排队。
2. **死会话不回收**（:552-632）：reader 线程收尾只 emit `terminal_exit`，
   不从 sessions 表移除；:551 文档注释宣称「子进程退出 → terminal_exit
   事件 + 会话清理」与实际实现不符。
3. **pid=0 孤儿路径**（:347 `unwrap_or(0)` + close :479-482 对 pid==0 直接
   返回）：spawn 失败时 shell 可能成为无人清理的孤儿。
4. Windows 无退出码（:621-625，恒 None）——记录为已知限制，文档化即可。

## 定位与修复建议

- close 先从表中摘牌（释放锁）再做 grace 等待与强杀；
- reader 线程退出时从表移除会话（或 list 惰性过滤 + 定期清理）；
- pid==0 时 log::warn 并尽力清理资源；
- 修正 :551 注释。

## 验收标准

- [x] 连续关闭多个 tab 不卡顿（close 摘牌后立即释放表锁再做 2s 优雅等待；`close_does_not_block_list_while_grace_waiting`：trap 忽略 SIGTERM 的 shell 场景下并发 list < 500ms 返回）
- [x] shell 自行退出后 `terminal_list` 不再返回死会话（reader 收尾从会话表摘除；`smoke_dead_session_reclaimed_from_table`）
- [x] 13/13 pty 测试 + `cargo test --lib` 通过（原 11 个 + 新增 2 个回归）

## 进度

### 状态

- 当前状态：✅ 已完成
- 最近更新：2026-09-13 修复完成

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-09-13 | ⬜ | 分析报告事实核查批次录入 |
| 2026-09-13 | ✅ | 复核四项证据全部成立。修复：① `close()` 先 `remove` 摘牌、`sessions` 守卫限定作用域释放，再做 terminate_process + 最长 2s 优雅轮询与 `kill_process_tree`——open/write/resize/list 不再被持锁阻塞（pid==0 早退路径同步移入摘牌之后）；② `TerminalManager.sessions` 包 `Arc`，reader 线程持有引用，`reader_thread_loop` 退出时 emit `terminal_exit` 后从会话表 `remove` 本会话（drop PtySession 释放 master/writer；close 已摘牌时 remove 返回 None 为正常路径），:551 注释同步修正为实际行为；③ `open()` 的 `unwrap_or(0)` 改 `unwrap_or_else` + `log::warn!`，`close()` pid==0 路径告警「可能遗留孤儿进程」后摘牌清理资源；④ Windows exit_code 恒 None 在 reader 文档注释标为已知限制。验证：`cargo test --lib process::pty` 13 passed（新增死会话回收、close 不阻塞 list 两个回归）。 |
