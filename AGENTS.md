<!-- gitnexus:start -->
# GitNexus — Code Intelligence

This project is indexed by GitNexus as **git-workspace** (8399 symbols, 18591 relationships, 300 execution flows). Use the GitNexus MCP tools to understand code, assess impact, and navigate safely.

> Index stale? Run `node .gitnexus/run.cjs analyze` from the project root — it auto-selects an available runner. No `.gitnexus/run.cjs` yet? `npx gitnexus analyze` (npm 11 crash → `npm i -g gitnexus`; #1939).

## Always Do

- **MUST run impact analysis before editing any symbol.** Before modifying a function, class, or method, run `impact({target: "symbolName", direction: "upstream"})` and report the blast radius (direct callers, affected processes, risk level) to the user.
- **MUST run `detect_changes()` before committing** to verify your changes only affect expected symbols and execution flows. For regression review, compare against the default branch: `detect_changes({scope: "compare", base_ref: "master"})`.
- **MUST warn the user** if impact analysis returns HIGH or CRITICAL risk before proceeding with edits.
- When exploring unfamiliar code, use `query({query: "concept"})` to find execution flows instead of grepping. It returns process-grouped results ranked by relevance.
- When you need full context on a specific symbol — callers, callees, which execution flows it participates in — use `context({name: "symbolName"})`.

## Never Do

- NEVER edit a function, class, or method without first running `impact` on it.
- NEVER ignore HIGH or CRITICAL risk warnings from impact analysis.
- NEVER rename symbols with find-and-replace — use `rename` which understands the call graph.
- NEVER commit changes without running `detect_changes()` to check affected scope.

## Resources

| Resource | Use for |
|----------|---------|
| `gitnexus://repo/git-workspace/context` | Codebase overview, check index freshness |
| `gitnexus://repo/git-workspace/clusters` | All functional areas |
| `gitnexus://repo/git-workspace/processes` | All execution flows |
| `gitnexus://repo/git-workspace/process/{name}` | Step-by-step execution trace |

## CLI

| Task | Read this skill file |
|------|---------------------|
| Understand architecture / "How does X work?" | `.claude/skills/gitnexus/gitnexus-exploring/SKILL.md` |
| Blast radius / "What breaks if I change X?" | `.claude/skills/gitnexus/gitnexus-impact-analysis/SKILL.md` |
| Trace bugs / "Why is X failing?" | `.claude/skills/gitnexus/gitnexus-debugging/SKILL.md` |
| Rename / extract / split / refactor | `.claude/skills/gitnexus/gitnexus-refactoring/SKILL.md` |
| Tools, resources, schema reference | `.claude/skills/gitnexus/gitnexus-guide/SKILL.md` |
| Index, status, clean, wiki CLI commands | `.claude/skills/gitnexus/gitnexus-cli/SKILL.md` |

<!-- gitnexus:end -->

<!-- platform:start -->
# 平台兼容性开发规范（Windows / macOS / Linux）

> 本项目的目标平台包含 Windows、macOS、Linux。下面每条都是真实踩坑后的硬规则
> （R-14 修复了 5 处 Windows 平台 bug，模式全部沉淀于此）。**新增/修改任何涉及
> 路径、进程、可执行文件检测的代码前，先对照本节。**

## 1. 文件路径

- **禁止字符串相等比较路径**。Windows 上 `PathBuf::join` 会保留 push 部分的分隔符：
  `Path::new(r"C:\a").join("b/c")` 的结果是 `C:\a\b/c`（**混合分隔符**）。而 R-02 的
  SQLite 索引（`path_key`）统一把路径存为**正斜杠**。因此「DB 路径 vs 用户配置/发现路径」
  的匹配必须两侧归一化：`p.replace('\\', "/")` 后再比较或做 `ends_with`。
  - 参照实现：`src-tauri/src/runtime/build/pipeline/mod.rs::find_root_project`、
    `src-tauri/src/runtime/service/mod.rs::find_project`、
    `src-tauri/src/runtime/launch/manager/mod.rs::infer_main_class`、
    `exec_resolve` 的 `known_paths` 增量 diff。
  - 新增路径匹配点时，直接复用/仿写归一化，不要手写裸 `==`。
- **拼接用 `Path::join`，不要字符串拼接**；向用户/日志展示路径用 `display()`，
  展示与比较分离（显示可用原始形式，比较必须归一化）。
- Windows 的 `to_string_lossy()` 结果可能是 `\\?\` 前缀（verbatim 路径），
  需要展示/比较前经 `strip_windows_verbatim_prefix`（见 `maven/index/mod.rs::path_key`）。
- 大小写：Windows 与 macOS（默认）文件系统大小写不敏感。路径**相等比较**建议
  归一化分隔符即可；如做集合/去重语义，可考虑小写化（当前实现以分隔符归一化为主，
  改动需在任务文档说明边界）。

## 2. 可执行文件检测（PATH / PATHEXT）

- **Windows 可执行扩展名：`.exe` / `.cmd` / `.bat`**（PATHEXT 语义）。mise、sdkman
  等工具链会把 Unix shell 脚本（无扩展名）与 `mvn.cmd` 放在同一目录——**必须先命中
  扩展名候选**，否则会选中不可执行的 sh 脚本（CreateProcess 报 os error 193）。
  - 参照实现：`src-tauri/src/java/detect.rs::find_in_path`（目录内按
    `.exe` → `.cmd` → `.bat` → 裸名 顺序）。
  - 检测 mvn / java / 任意 CLI 一律走 `find_in_path`，不要直接 `Command::new(裸名)`
    后靠错误码兜底。
- **执行 `.cmd` / `.bat` 必须经 `cmd /C`**（`needs_cmd_c`，见
  `maven/detect_exec.rs`）；Unix 用 `sh -c`。脚本执行器按平台分支
  （参照 `runtime/build/pipeline/mod.rs::user_script_command`）。
- PATH 分隔符：Windows `;`，Unix `:`（`find_in_path` 已处理，新增遍历 PATH 的代码
  不得硬编码）。

## 3. 进程与系统命令

- **端口占用检测**：Windows `netstat -ano` + `tasklist`；Unix `lsof` + `/proc/<pid>/comm`
  （`process/port.rs`）。解析函数保持纯函数（输入输出样例可单测），系统调用只留
  `detect_port_occupier` 一个入口。
- **进程树终止 / 优雅停止**：统一走 `process/kill_tree.rs`（sysinfo 跨平台）；
  Windows 无 SIGTERM 语义，优雅停止用 `terminate_process`，超时升级整树终止。
- **子进程 spawn**：Windows 必须设 `CREATE_NO_WINDOW`（`process/streaming.rs` 已有）；
  禁止依赖 shell 特定行为（`ls`、`pgrep` 等）编写业务逻辑。
- **子进程输出监控（F-12）**：流式监控循环在输出 reader 全部断开后**不得阻塞
  在 `child.wait()`**——被监控进程在 reader 死后仍可能存活，阻塞期间取消/超时
  信号将无人轮询（Stop 杀不掉 JVM 的根因）。reader 必须按字节读 +
  `from_utf8_lossy`：中文 Windows 下 JVM 默认 GBK 输出，`read_line` 遇非法
  UTF-8 会直接杀死 reader（丢光后续日志，管道写满后还会卡死被监控进程）。
  参照实现：`process/streaming.rs`。
- **超长命令行（F-11）**：Windows CreateProcess 上限 32767 字符。`java -cp
  <数百 jar>` 会超限（os error 206）——走 `runtime/build/pathing_jar.rs`
  的 pathing jar（manifest Class-Path，JDK 8/17/21 均兼容；`@argfile` 需
  JDK 9+ 不可用）。新增拼超长命令的代码必须先过 `estimate_command_len`
  阈值判断。
- 长命令预览/追溯时，路径分隔符保持平台原生展示即可，不要为「统一」重写路径。

## 4. 环境与工具链

- **构建与运行必须同源 JDK**：真实集成测试（Spring Boot 3.x 需要 JDK 17+）应把
  当前 `JAVA_HOME` 注册进 R-04 注册表并绑定到配置，否则会出现「构建用 17、启动用
  系统默认 8」的版本错配（参照 `runtime/launch/manager/mod.rs::boot_fixture`）。
- 环境相关测试（mvn / JDK / 网络下载）**探测不到就 skip 并打印原因**，不要硬失败；
  但产品逻辑（如 MavenNotFound）必须返回可行动错误。

## 5. 测试断言

- 断言含路径的命令预览/输出时，先 `replace('\\', "/")` 归一化再断言（参照
  `runtime/launch/manager/mod.rs` 的 `bound_jdk_is_used_for_launch_command`）。
- 涉及 temp 目录的断言不要依赖具体盘符/前缀；测试 fixture 目录用
  `std::env::temp_dir()` 而非硬编码 `/tmp` 或 `C:\`。
- 平台差异用 `#[cfg(windows)]` / `#[cfg(not(windows))]` 分支，注释说明另一平台行为；
  不要用运行时字符串探测代替编译期分支（除非确有必要）。

<!-- platform:end -->

<!-- app-footer:start -->
# 应用底部版本栏规则（F-07）

- 版本信息展示在 StatusBar 最右槽位，格式：`v<版本号> by <作者>`（如 `v0.1.0 by mantougg`）。
- **版本号与作者的唯一数据源是根目录 `package.json`**（`version` / `author` 字段），
  经 `vite.config.ts` 的 `define` 构建期注入为 `__APP_VERSION__` / `__APP_AUTHOR__`
  （类型声明在 `src/vite-env.d.ts`），前端展示在 `src/components/shell/StatusBar.vue`。
- 发版本时只改 `package.json` 的 `version`（`src-tauri/tauri.conf.json` 的 `version`
  需同步保持一致）；换作者只改 `author`。**禁止在组件里硬编码版本号或作者名。**
<!-- app-footer:end -->

<!-- desktop-skin:start -->
# Desktop Skin 约定

> 来源：docs/desktop-skin-plan.md（F-10 桌面化 UI 改造方案）。
> 任务跟踪：docs/tasks-desktop/README.md（D-01~D-16）。

## 样式规范

- **新 UI 一律使用 tokens 变量**（`src/styles/tokens.scss`），禁止硬编码色值、像素间距、字号。
- 颜色取 `--gw-*` 语义色，间距取 `--gw-space-*`，字号取 `--gw-text-*`，圆角取 `--gw-radius-*`。
- 等宽文本（路径、分支名、commit hash、日志）使用 `--gw-font-mono`。

## 组件规范

- 新面板一律使用 `Panel` / `PanelHeader` / `Toolbar` 骨架组件（二期落地后）。
- 正文字号 / 圆角 / 组件密度以 Naive UI themeOverrides 为准，视图内不单独覆盖。
- 新页面外壳遵循 docs/desktop-skin-plan.md §5.9 统一模式。

## 导航规范

- **工作区切换全局唯一入口为 StatusBar**；视图内不得再出现工作区选择器。
- 视图内不自绘返回按钮，由骨架统一提供。
- 导航按钮统一在 SideNav，视图 toolbar 只保留作用于当前视图数据的操作按钮。
- 命令与快捷键统一走命令注册表（2.5 期起），禁止在视图内各自绑定快捷键。

## 代码落点

| 类型 | 位置 |
|------|------|
| 骨架组件 | `src/components/shell/` |
| Design Tokens | `src/styles/tokens.scss` |
| 命令注册表 | `src/commands/`（2.5 期起） |
| Vue 视图 | `src/views/` |
| 主题 composable | `src/composables/useTheme.ts` |
<!-- desktop-skin:end -->
