# F-51 终端启动（ConPTY）后端日志中文乱码（JVM 输出 UTF-8，控制台代码页 GBK）

| 项 | 值 |
|---|---|
| 优先级 | P1 |
| 状态 | 🟦 修复中 |
| 来源 | 2026-09-20 用户反馈：终端面板「在终端中启动」后端，启动日志中文乱码：`璇锋眰璺緞:/workstationConfig/...` |
| 关联任务 | TM-06（终端启动）、F-44/F-45（同一路径的命令行适配） |

## 问题描述

Runtime 应用「在终端中启动」后，PTY 里的 Spring Boot 日志中文全部乱码：

```
2026-09-20 14:25:33.780  INFO ... : 璇锋眰璺緞:/workstationConfig/sysShortcut/getShortcutData,褰撳墠鎿嶄綔浜猴細瓒呯骇绠＄悊鍛橈紝...
```

乱码形态鉴定：`璇锋眰璺緞` = 「请求路径」的 UTF-8 字节
（`E8AFB7 E6B182 E8B7AF E5BE84`）被按 GBK（CP936）解码——即 **JVM 输出的
是正确的 UTF-8，链路中有一环用 GBK 去读它**。

## 根因（已定位）

- 终端启动走真实 ConPTY：`runtime_start_in_terminal`
  （`commands/terminal.rs:96`）→ `TerminalManager::open` →
  `slave.spawn_command`（`process/pty.rs:381`）。
- Windows 控制台输出代码页默认 936（GBK）。conhost 把 JVM 写出的 UTF-8 字节
  按 CP936 转成 UTF-16（此刻已乱），再编码成 UTF-8 发给 PTY master；
  `reader_thread_loop`（`process/pty.rs:645`）只透传字节，无法挽回。
- 该应用启动参数带 `-Dfile.encoding=UTF-8`（见 F-45 案例 fixture
  `launcher.rs:710`），JVM 日志输出 UTF-8 → 与 CP936 错配。
- 对照组：**托管启动不乱码**——`process/streaming.rs` 按字节读 +
  `from_utf8_lossy`（F-12），期望的就是 UTF-8。两条输出链路完全不同，
  解释了为何只有终端面板启动乱。

## 修复范围

- [x] `commands/terminal.rs`：Windows 下 `runtime_start_in_terminal` 写入
      PTY 的命令行前加代码页切换前缀——新增纯函数 `utf8_console_prefix`
      （cmd：`chcp 65001 >nul && `；PowerShell：`chcp 65001 | Out-Null; `，
      `;` 连接兼容 Windows PowerShell 5.1；Posix 返回 None），调用点仅在
      `cfg!(windows)` 下拼接前缀；`assemble_command_for_shell` 本身不动
- [x] 对齐原理：控制台 CP 切到 65001 后，Windows 上 JVM 的
      `sun.stdout.encoding`/`stdout.encoding`（跟随控制台 CP）与显式
      `-Dfile.encoding=UTF-8` 均输出 UTF-8，双向对齐
- [x] 单测：`utf8_console_prefix_per_shell_kind`（三种 shell 前缀形态）、
      `utf8_prefix_composes_with_env_and_call_operator`（chcp 前缀在最前，
      env 注入与 F-44 `&` 调用运算符相对顺序不变）
- [x] AGENTS.md 平台规范 §3 补充 ConPTY 代码页规则

不在本次范围：交互式 shell tab（`terminal_open`）不动——那是用户自己的 shell
会话，切代码页会改变用户环境状态；托管启动（streaming.rs）本就不乱。

## 验收标准

- [x] 单测全绿：`GW_TEST_MANIFEST=1 cargo test --lib -- commands::terminal`
      （12/12）
- [ ] 真机实测：同一应用「在终端中启动」，日志中文正常显示（原复现案例）

## 进度

### 状态

- 当前状态：🟦 修复中（代码已完成并含单测；待真机实测原始复现案例）
- 最近更新：2026-09-20 修复完成，`commands::terminal` 12/12 全绿；待真机实测

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-09-20 | ⬜ | 问题录入；乱码鉴定为 UTF-8 字节被 GBK 解码；定位 ConPTY 控制台代码页 936 与 JVM UTF-8 输出错配 |
| 2026-09-20 | 🟦 | 开始修复 |
| 2026-09-20 | 🟦 | 修复完成：`runtime_start_in_terminal` 在 Windows 下经纯函数 `utf8_console_prefix` 加 `chcp 65001` 前缀（cmd/PowerShell 分流）；新增 2 例单测，12/12 全绿；AGENTS.md §3 沉淀规则；真机实测待做 |
