# F-44 终端启动命令在 PowerShell 下解析失败（行首引号路径缺 `&` 调用运算符）

| 项 | 值 |
|---|---|
| 优先级 | P1 |
| 状态 | 🟦 修复中 |
| 来源 | 2026-09-20 用户反馈：Runtime「在终端中启动」后端，PowerShell 报 `ParserError: 表达式或语句中存在意外的标记"-XX"` |
| 关联任务 | TM-06（终端启动）、F-42（终端面板默认 shell = pwsh） |

## 问题描述

Runtime 总览对 Spring Boot 应用点「在终端中启动」，终端（PowerShell 7.6.6）原样回显
命令后立即解析失败，JVM 从未拉起：

```
PS D:\AWork\Code\IPD\docs\03原型\hussar-web> "C:\Program Files\Java\jdk-1.8\bin\java.exe" -XX:TieredStopAtLevel=1 … -cp …\pathing-e2459fdcec83117c.jar com.jxdinfo.hussar.example.HussarApplication
ParserError:
Line |
   1 |  "C:\Program Files\Java\jdk-1.8\bin\java.exe" -XX:TieredStopAtLevel=1  …
     |                                               ~~~
     | 表达式或语句中存在意外的标记"-XX"。
```

复现前提（两个条件同时成立）：

1. `java.exe` 路径含空格（如 `C:\Program Files\Java\jdk-1.8`）——组装时被加双引号；
2. 终端默认 shell 是 PowerShell（Windows 探测顺序 pwsh → powershell → cmd，见
   `process/pty.rs::shell_candidates`）。

托管启动（`spawn` 直起进程，不经 shell 解析）不受影响；仅 TM-06 终端启动（把命令行
字符串写入交互式 PTY）触发。

## 根因（已定位）

机制链（逐环经源码验证）：

1. `RuntimeDashboard.vue::onLaunchInTerminal`（`src/views/RuntimeDashboard.vue:1268`）
   → IPC `runtime_compute_launch_preview` →
   `RuntimeProcessManager::compute_launch_plan`（`runtime/launch/manager/start.rs:311`）
   → `launcher::plan_shell_command`（`runtime/launch/launcher.rs:128`）生成
   `"C:\Program Files\Java\jdk-1.8\bin\java.exe" -XX:… -cp … MainClass`
   ——含空格路径经 `shell_quote_arg` 加双引号（`launcher.rs:186`），这对 cmd/sh
   是正确引用，但**不是 PowerShell 的调用语法**。
2. 前端 `stores/terminal.ts:504 launchInTerminal` → IPC
   `runtime_start_in_terminal`（`commands/terminal.rs:96`）：命令字符串原样
   `format!("{}\r", full_command)` 写入 PTY；PTY 的 shell 由
   `detect_default_shell()`（`process/pty.rs:91`）在 `open` 内部解析，Windows 首选
   `pwsh`（PowerShell 7）。
3. PowerShell 语法：**行首为 `"` 的字符串是表达式（string literal），不是命令
   调用**——后续 token `-XX:…` 即报「表达式或语句中存在意外的标记」。引号路径
   必须加调用运算符：`& "path" args`。cmd 与 POSIX sh 无此要求（引号首词原生
   作为命令词）。
4. 附带发现（同函数内潜在 bug）：`assemble_command_with_env`
   （`commands/terminal.rs:148`）Windows 分支生成 `set A=b && cmd`——cmd 语法；
   PowerShell 下 `set` 是 `Set-Variable` 别名（`set A=b` 不注入进程环境且报错），
   `&&` 仅 pwsh 7+ 支持（Windows PowerShell 5.1 不支持）。当前 UI 未传 env
   （`launchInTerminal` 第三参无调用方），未实际触发，随本修复一并按 shell 分流。

## 修复范围

- [x] `runtime_start_in_terminal` 在 open 前显式解析默认 shell
      （`detect_default_shell` 提为 `pub(crate)`），按 shell 类型适配命令行，
      并把**同一个** shell 路径显式传给 `open`（消除「写入方与 open 各自解析
      可能不一致」的窗口）
- [x] 命令组装按 shell 分流（纯函数）：
      `process/pty.rs` 新增 `ShellKind`（PowerShell / Cmd / Posix）+
      `shell_kind(path)`（按可执行文件名 stem 分类，大小写不敏感）；
      `commands/terminal.rs` 的 `assemble_command_with_env` 重写为
      `assemble_command_for_shell(command, env, kind)`：
      PowerShell → 行首引号补 `& `、env 用 `$env:K='V'; …`（`;` 连接兼容 5.1）；
      Cmd → 维持 `set K=V && …`；Posix → 维持 `K=V …`
- [x] 单测覆盖：三 shell ×（引号 / 非引号首词）×（无 env / 有 env / env 含单引号转义）
      （commands/terminal.rs 新增 9 例 + pty.rs 新增 shell_kind 分类 1 例）
- [x] `AGENTS.md` 平台规范沉淀「写入 PTY 的命令行必须按目标 shell 语法适配」

## 验收标准

- [x] `GW_TEST_MANIFEST=1 cargo test --lib` 相关模块（commands::terminal /
      process::pty）全绿，含新增用例
      （commands::terminal 9/9 全绿；process::pty 23/24——唯一失败
      `smoke_dead_session_reclaimed_from_table` 经 stash 验证在改动前的
      HEAD 同样失败，属本机 pwsh 环境型预存在失败，与本修复无关。
      另观察到 runtime::launch 的 real_maven 3 项在本机失败
      （fixture Maven 构建 exit 1，构建期错误与本改动路径无交集）、
      real_node_vite 1 项 flaky（重跑即过），均为预存在环境型失败）
- [ ] 真机实测：JDK 在 `C:\Program Files\Java\jdk-1.8`（含空格路径）的应用
      「在终端中启动」→ PowerShell 正常拉起 JVM，应用可访问

## 进度

### 状态

- 当前状态：🟦 修复中（代码已完成并含单测；原始复现案例待真机实测）
- 最近更新：2026-09-20 修复完成，`commands::terminal` + `process::pty` 测试全绿；待真机实测

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-09-20 | ⬜ | 问题录入；定位：`plan_shell_command` 生成引号路径命令 → `runtime_start_in_terminal` 原样写入 PTY → 默认 shell 是 pwsh，PowerShell 把行首引号字符串当表达式，缺 `&` 调用运算符 |
| 2026-09-20 | 🟦 | 开始修复 |
| 2026-09-20 | 🟦 | 修复完成：`runtime_start_in_terminal` 先经 `detect_default_shell` 解析 shell（提为 pub(crate)）再按 `shell_kind` 适配——PowerShell 行首引号补 `& `、env 改 `$env:K='V'; `（原 `set K=V &&` 在 PowerShell 下本就错误，随本修复一并分流）；适配后的同一 shell 路径显式传给 open。新增 10 例单测全绿；验证：`GW_TEST_MANIFEST=1 cargo test --lib -- commands::terminal process::pty`（唯一失败项为预存在环境问题，已在 HEAD 复核）。AGENTS.md §3 沉淀 PTY 命令行适配规则。真机实测待做 |
