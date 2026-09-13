# PAF-24 PTY 生命周期加固（close 持锁 2s / 死会话回收 / pid=0 / 注释不符）

| 项 | 值 |
|---|---|
| 优先级 | P1 |
| 状态 | ⬜ 未开始 |
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

- [ ] 连续关闭多个 tab 不卡顿
- [ ] shell 自行退出后 `terminal_list` 不再返回死会话
- [ ] 12/12 pty 测试 + `cargo test --lib` 通过

## 进度

### 状态

- 当前状态：⬜ 未开始
- 最近更新：2026-09-13 录入

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-09-13 | ⬜ | 分析报告事实核查批次录入 |
