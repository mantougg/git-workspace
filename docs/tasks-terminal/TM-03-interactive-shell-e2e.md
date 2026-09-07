# TM-03 交互式 Shell 端到端（联通 + 三平台冒烟）

> **开发前必读**：[../terminal-feature-plan.md](../terminal-feature-plan.md) §4.1~§4.3 / §5 + [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：TM-01、TM-02。

| 项 | 值 |
|---|---|
| 阶段 | 一期 · 终端基础 |
| 优先级 | P0 |
| 状态 | 🟦 进行中 |
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

- [ ] 大输出洪峰不卡 UI（验证后端批量 + 前端写缓冲；如 `yes` / `npm install` 级输出滚动流畅）
- [ ] 交互式程序可用：方向键历史、`Ctrl-C` 中断、`less`/`top` 类全屏程序、vim 打开退出
- [ ] shell 探测失败（如 Windows 老系统无 ConPTY）显示可行动错误提示
- [ ] 应用退出后再启动，无残留子进程（ps/任务管理器核对）

### 三平台冒烟 checklist（每项在本机/虚拟机实测勾选）

- [ ] Linux：bash/zsh 打开，`git status`、中文 echo、缩放、Ctrl-C、关闭 tab 进程回收
- [ ] macOS：`$SHELL` 探测（zsh），同上流程
- [ ] Windows：pwsh/powershell/cmd 探测顺序，GBK 中文输出不乱码（base64 字节路径），`git status`、缩放、关闭 tab 进程树回收

## 验收标准

- [ ] 三平台冒烟 checklist 全部实测通过（时间线逐平台记录）— 需用户协助验证 macOS/Windows
- [ ] 交互式程序（vim/less/历史/Ctrl-C）在三平台可用
- [ ] 关闭面板/应用后无孤儿进程（unix 进程组消失；Windows pid 不存在）
- [x] `pnpm build` + `cargo check` + `cargo test pty` 通过

## 进度

### 状态

- 当前状态：🟦 进行中
- 最近更新：2026-09-08 开始开发

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-09-08 | ⬜ | 任务拆解录入（来源：terminal-feature-plan.md §6 一期-3） |
| 2026-09-08 | 🟦 | 开始开发：联通前后端（移除 mock，走真实 PTY） |
| 2026-09-08 | 🟦 | 联通完成：store writeCallback 机制 + XtermView 注册回调 + 暂停/恢复缓冲 + pnpm build + cargo check 通过 |
