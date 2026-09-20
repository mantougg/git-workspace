# F-45 终端启动命令在 PowerShell 下被拆参数（`-Dspring.*` 裸 token 在第一个 `.` 处断开）

| 项 | 值 |
|---|---|
| 优先级 | P1 |
| 状态 | 🟦 修复中 |
| 来源 | 2026-09-20 用户反馈：Runtime「在终端中启动」后端，PowerShell 7.6.6 报 `错误: 找不到或无法加载主类 .output.ansi.enabled=always` |
| 关联任务 | TM-06（终端启动）、F-44（同一路径的 `&` 调用运算符）、F-42（默认 shell = pwsh） |

## 问题描述

Runtime 总览对 Spring Boot 应用点「在终端中启动」，PowerShell 原样回显命令后
JVM 立即失败——**命令没拼错，参数被 shell 拆开了**：

```
PS D:\AWork\Code\IPD\docs\03原型\hussar-web> & "C:\Program Files\Java\jdk-1.8\bin\java.exe" -XX:TieredStopAtLevel=1 -Dspring.output.ansi.enabled=always … -cp D:\AWork\Code\IPD\.gitworkspace\runtime\IPD原型后端\classpath\pathing-e2459fdcec83117c.jar com.jxdinfo.hussar.example.HussarApplication
错误: 找不到或无法加载主类 .output.ansi.enabled=always
```

即 `-Dspring.output.ansi.enabled=always` 变成了 `-Dspring` 与
`.output.ansi.enabled=always` 两个参数，后者落在主类位置上。

F-44 已让行首引号路径带上 `&` 调用运算符，故命令能进到 PowerShell 解析阶段；
本问题发生在**参数级**，是同一路径（TM-06 把命令行字符串写进交互 PTY）的
第二层 shell 适配缺口。

## 根因（已定位，本机 pwsh 7.6.6 + JDK 1.8 实测复现）

机制链（逐环经源码验证 + 命令行实测）：

1. `RuntimeDashboard.vue::onLaunchInTerminal`（`src/views/RuntimeDashboard.vue:1271`）
   → IPC `runtime_compute_launch_preview` →
   `RuntimeProcessManager::compute_launch_plan`（`runtime/launch/manager/start.rs:311`）
   → `launcher::plan_shell_command`（`runtime/launch/launcher.rs:129`）。
   该函数的 `shell_quote_arg` 只对「含空格 / tab / 双引号」的参数加引号；
   `-Dspring.output.ansi.enabled=always` 三者皆无 → **裸写**。
2. `stores/terminal.ts:504 launchInTerminal` → IPC `runtime_start_in_terminal`
   （`commands/terminal.rs:96`）：命令字符串原样 `format!("{}\r", full_command)`
   写入 PTY。
3. **PTY 里的 shell 把这一行当 PowerShell 源码重新解析**（F-44 已确认默认
   shell 是 pwsh）。PowerShell 参数模式对原生命令 token 的处理（实测）：

   | 命令里写的裸 token | java 实际收到的 argv | 结果 |
   |---|---|---|
   | `-Dspring.output.ansi.enabled=always` | `-Dspring` + `.output.ansi.enabled=always` | 主类解析失败（本 bug） |
   | `-Dfoo.bar=baz` | `-Dfoo` + `.bar=baz` | 同上 |
   | `-Dfoo=bar.baz` | `-Dfoo=bar` + `.baz` | 同上 |
   | `--server.port=8080` | 完整单参数 | 正常 |
   | `-XX:TieredStopAtLevel=1`（无 `.`） | 完整单参数 | 正常 |
   | `-Dfoo=a,b` | —— | ParserError「参数列表中缺少参数」 |
   | `-cp a.jar;b.jar` | 在 `;` 处断开 | `Unrecognized option` |
   | `-Dfoo=a$HOME` | `$HOME` 被展开 | 静默拿到错误值 |

   规律：**单个 `-` 前缀且含 `.` 的裸 token 在第一个 `.` 处被拆开**（`--` 双
   横线前缀不受影响）。Spring Boot 默认注入的 `-Dspring.output.ansi.enabled` /
   `-Dfile.encoding` / `-Dspring.jmx.enabled` … 全部命中，JVM 报出的只是第一
   个被拆的那个。
4. 同类隐患：多条目 classpath 经 `std::env::join_paths` 用 `;` 连接
   （`launcher.rs:157`），无空格时同样是裸 token → 在 `;` 处断开。

## 修复范围

- [x] `runtime/launch/launcher.rs`：`shell_quote_arg` 的引号判定从
      「空格/tab/双引号」改为 `arg_needs_quoting` 字符集——空白、`"` `'`
      `` ` `` `$`、PowerShell 拆分字符 `.` `,` `;` `|` `&` `<` `>`、解析错误
      触发字符 `(` `)` `{` `}`、以及 cmd 元字符 `%` `^` `!`；命中即整体加
      双引号。内含双引号的转义从 `\"`（POSIX 语义，PowerShell 不认）改为
      `""`（PowerShell 与 cmd CRT 一致）
- [x] 单测：`launcher` 新增 5 例（字符集判定、F-45 案例引号守卫、剥引号后
      argv 序列、Script 危险参数），`commands::terminal` 新增 1 例
      （`plan_shell_command` + `assemble_command_for_shell` 组合）。
      其中 `plan_shell_command_classpath_quotes_every_unsafe_token` 是回归
      守卫：断言「会被 shell 拆开的 token 全都加了引号」
- [x] `AGENTS.md` 平台规范 §3 补充 PowerShell 参数模式拆参规则

不在本次范围：托管启动（`spawn` 直起进程，不经 shell 解析）不受影响；
`plan_preview`（展示/落库用）保持不加引号的人类可读形式不变。

## 验收标准

- [x] 单测全绿：`GW_TEST_MANIFEST=1 cargo test --lib -- runtime::launch::launcher commands::terminal`
      （20/20；同批 `process::pty` 中 `smoke_dead_session_reclaimed_from_table`
      为本机预存在失败，与本次改动无关——本修复未触碰 `process/pty.rs`）
- [x] 本机实测修复形态：对 F-45 的 argv 集合逐 token 加引号后写入 pwsh，JVM
      收到完整参数、主类正常加载（`-version` 场景与真实 classpath 场景均验证）
- [ ] 真机实测：JDK 在 `C:\Program Files\Java\jdk-1.8`（含空格路径）的应用
      「在终端中启动」→ PowerShell 正常拉起 JVM，应用可访问
      （F-44 的真机实测项一并覆盖）

## 进度

### 状态

- 当前状态：🟦 修复中（代码已完成并含单测；原始复现案例待真机实测）
- 最近更新：2026-09-20 修复完成，`runtime::launch::launcher` +
      `commands::terminal` 测试全绿；待真机实测

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-09-20 | ⬜ | 问题录入；定位：`plan_shell_command` 对 `-Dspring.*` 裸写 → 写入 PTY → pwsh 参数模式在第一个 `.` 处把 token 拆成两个参数 → JVM 把 `.output.ansi.enabled=always` 当主类 |
| 2026-09-20 | 🟦 | 开始修复；本机 pwsh 7.6.6 + JDK 1.8 逐字符实测出 PowerShell 参数模式的拆分/展开字符集 |
| 2026-09-20 | 🟦 | 修复完成：`shell_quote_arg` 改用 `arg_needs_quoting` 字符集 + `""` 引号转义；新增 6 例单测全绿；AGENTS.md §3 沉淀规则；真机实测待做 |
