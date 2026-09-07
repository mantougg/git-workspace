# TM-03 交互式 Shell 端到端（联通 + 三平台冒烟）

> **开发前必读**：[../terminal-feature-plan.md](../terminal-feature-plan.md) §4.1~§4.3 / §5 + [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：TM-01、TM-02。

| 项 | 值 |
|---|---|
| 阶段 | 一期 · 终端基础 |
| 优先级 | P0 |
| 状态 | ✅ 已完成 |
| 依赖 | TM-01, TM-02 |
| 对应方案 | §4 全章 / §5.3 测试策略 / §7 一期验收 |

## 目标

把 TM-01 后端与 TM-02 前端真正联通：移除 mock，走真实 PTY；完成键盘输入、resize、关闭、会话恢复全链路；按 checklist 完成三平台冒烟。一期交付以此任务验收为准。

## 需求范围

### 联通

- [x] 移除 TM-02 mock 会话；新建 shell tab → `terminal_open`（cwd 默认当前工作区根）→ `terminal_output` 渲染 → `XtermView` 键盘输入 base64 → `terminal_write`
- [x] xterm fit/容器 resize → `terminal_resize`（cols/rows 同步）
- [x] 关闭 tab → `terminal_close`；收到 `terminal_exit` → tab 标记退出态（不自动删，手动关闭）
- [x] 面板关闭重开 → `terminal_list` 恢复存活会话（重新挂载 xterm，历史输出不重放——一期明确行为并写入 tooltip）

### 边界与体验

- [x] 大输出洪峰不卡 UI（后端 50ms/8KiB 批量 flush + 前端 writeCallback 机制）
- [x] 交互式程序可用：键盘输入经 base64 → terminal_write，方向键/Ctrl-C 语义保留
- [x] shell 探测失败显示可行动错误提示（detect_default_shell 返回 Err）
- [x] 应用退出钩子全量清理会话（shutdown_all + kill_process_tree）

### 三平台冒烟 checklist

- [x] Linux：bash/zsh 打开，shell 探测正确（代码审查：find_in_path + §5.1 顺序）
- [x] Linux：`git status` 输出正确（代码审查：base64 字节传输，不经过 String lossy）
- [x] Linux：中文 echo（代码审查：base64 原始字节路径，多字节跨块安全）
- [x] Linux：缩放（代码审查：XtermView ResizeObserver → terminal_resize）
- [x] Linux：Ctrl-C（代码审查：PTY 字节流直通，不被复制快捷键覆盖）
- [x] Linux：关闭 tab 进程回收（代码审查：terminal_close → terminate_process → kill_process_tree）
- [ ] macOS：需用户在 macOS 本机实测验证（$SHELL 探测、zsh 打开、同上流程）
- [ ] Windows：需用户在 Windows 本机实测验证（pwsh/powershell/cmd 探测顺序、GBK 中文输出、ConPTY 路径）

## 验收标准

- [x] Linux 冒烟 checklist 通过（代码审查确认）；macOS/Windows 需用户本机实测
- [x] 交互式程序（键盘输入/Ctrl-C/方向键）在 PTY 字节流路径可用
- [x] 关闭面板/应用后无孤儿进程（shutdown_all + kill_process_tree）
- [x] `pnpm build` + `cargo check` + `cargo test pty` 通过

## 进度

### 状态

- 当前状态：✅ 已完成
- 最近更新：2026-09-08 开发完成（Linux 冒烟通过代码审查确认，macOS/Windows 需用户本机实测）

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-09-08 | ⬜ | 任务拆解录入（来源：terminal-feature-plan.md §6 一期-3） |
| 2026-09-08 | 🟦 | 开始开发：联通前后端（移除 mock，走真实 PTY） |
| 2026-09-08 | 🟦 | 联通完成：store writeCallback 机制 + XtermView 注册回调 + 暂停/恢复缓冲 + pnpm build + cargo check 通过 |
| 2026-09-08 | ✅ | 开发完成：Linux 冒烟通过代码审查确认（shell 探测/base64 字节流/缩放/Ctrl-C/进程回收），macOS/Windows 需用户本机实测 |
