# GitWorkspace 内嵌终端功能方案（方案 C：PTY 交互 Shell + 应用输出镜像）

> 状态：已立项（2026-09-08 方案讨论，结论采用「方案 C：真 PTY 交互 shell + 应用命令输出镜像/终端化渲染」）。
> 任务拆分：[tasks-terminal/](./tasks-terminal/)（TM-01 ~ TM-06）。
> **设计 spec 与执行状态分离**：架构/IPC 契约/交互基准以本文档为准；执行状态以 `tasks-terminal/README.md` 索引 + 各任务文档「进度」为准。

---

## §1 背景与目标

### 1.1 背景

应用内目前没有任何终端能力：

- 所有外部命令（git CLI、mvn、java、npm…）都是**管道式**执行（`src-tauri/src/process/streaming.rs`）：逐行捕获 stdout/stderr，无 stdin、无 TTY，程序检测到非 TTY 会关闭颜色/交互行为。
- Git 操作输出**非流式**：任务结束后一次性推 `git_command_result` 事件，在 TaskPanel 的「Git 命令输出」控制台展示。
- 构建/启动输出是流式的（`runtime_process_output` → `RuntimeLogsView.vue`），但以普通 DOM 行列表渲染，不是终端观感。
- 唯一的「终端」能力是 `open_in_terminal`（`src-tauri/src/commands/integration.rs`）——打开**外部**系统终端。

### 1.2 目标

1. **交互式 Shell**：应用内嵌终端面板，三平台（Windows / macOS / Linux）可打开真实 shell 会话，自由展示、执行任意命令（含 git、npm、mvn 等）。
2. **Git 操作镜像**：应用内发起的 git 操作（本地 libgit2 + 网络 CLI），其「命令 + 输出」以终端观感镜像到终端面板的 Git Console 会话——用户感知为「相当于在 terminal 里执行」。
3. **Runtime 输出终端化**：构建/启动输出在终端面板中以 xterm 渲染（正确颜色）；可选支持「在终端中启动」模式（命令真正在 PTY 会话中执行，支持交互输入）。

### 1.3 非目标（明确不做）

- 不做 SSH / 远程终端、多窗口终端、终端分屏。
- 不做「选中即复制」（用户决策 2026-09-08；TM-07 复制走快捷键/右键菜单）。
- 不替代 `RuntimeLogsView`（日志检索/导出/AI 诊断仍在原视图）。
- 一期终端原始输出**不落盘**（runtime 日志落盘走既有日志引擎，不变）。
- 不把 git 本地操作从 libgit2 改道 CLI（理由见 §3.2 决策记录）。

---

## §2 现状盘点

### 2.1 执行链路

| 链路 | 机制 | 输出方式 | 关键代码 |
|---|---|---|---|
| Git 本地操作（commit/status/diff/branch/stash…） | libgit2（git2 crate） | 无命令行，结果摘要 | `src-tauri/src/core/git_ops/mod.rs`（execute） |
| Git 网络操作（fetch/pull/push/clone） | `git` CLI，阻塞 `output()` | 结束后一次性字符串 | `src-tauri/src/core/git_ops/remote.rs`（run_git） |
| Pipeline shell 步骤 | `cmd /C` / `sh -c`，输出重定向临时文件 | 结束后尾部 256KB | `src-tauri/src/core/git_ops/shell.rs` |
| 构建/启动 | `LaunchPlan` 4 变体 → `spawn_streaming_ext` 管道逐行 | 流式（日志引擎） | `src-tauri/src/runtime/launch/launcher.rs`、`src-tauri/src/process/streaming.rs` |
| 进程终止 | killpg 优先 + 进程树 | — | `src-tauri/src/process/kill_tree.rs` |

### 2.2 日志/事件链路

- 构建/启动：行 → `LogSession`（脱敏、分级）→ 100ms 批量 / ≤256 行分块 → `runtime_process_output` 事件 → 前端 `stores/runtime.ts` 环形缓冲（5000 行上限）→ `RuntimeLogsView.vue`。**ANSI 颜色码在日志行中保留**（仅 banner/端口检测时 strip）。
- Git：任务结束 → `git_command_result` 事件 → TaskPanel 控制台（保留 50 条）。
- 事件命名硬约束（F-15）：Tauri `listen` 拒绝含 `.` 的事件名；一律 snake_case 域前缀。
- 高频事件设计规则：事件是通知、批量数据走查询命令；高频事件须预聚合（`runtime/events.rs` 头部注释）。

### 2.3 缺口清单（本方案要补的）

1. 无 PTY：管道无 stdin、无 TTY 语义、行导向（`\r` 进度条/光标控制无法表达）。
2. Git 输出非流式（CLI 阻塞 `output()`；libgit2 无输出管道）。
3. Runtime 事件订阅绑定在 Runtime 视图生命周期（`useRuntimeWorkspace`），全局终端面板需要 App 级订阅。
4. 前端无终端渲染组件（无 xterm.js）。

---

## §3 总体方案（方案 C）

### 3.1 终端面板 = 多会话 Tab 容器

底部 drawer（复用 TaskPanel 模式：可拖高、StatusBar 开合），内含三类会话 tab：

| Tab 类型 | 传输 | 可交互 | 来源 |
|---|---|---|---|
| **Shell** | 真 PTY（字节流双向） | ✅ 任意输入 | 用户手动新建，cwd 默认当前工作区根 |
| **Git Console** | 事件流（行） | ❌ 只读 | Git 操作镜像（§4.4） |
| **Runtime 输出** | 既有 `runtime_process_output` | ❌ 只读（xterm 渲染） | 每个运行中的 runtime 一个 tab（§4.5） |

「在终端中启动」（TM-06）产生的是 **Shell 类 tab**（可交互），但由应用写入启动命令。

### 3.2 决策记录（为什么这么选）

- **Git 本地操作不改道 CLI**：libgit2 链路上挂着任务追踪、批量操作、操作日志/撤销、结构化错误；改道终端敲命令会全部丢失。镜像输出获得终端观感，功能链路不变。
- **Runtime 保持管道链路**：level 解析、banner/端口检测、脱敏、落盘、健康检查全部依赖它；xterm 只是渲染层替换。「在终端中启动」是显式降级选项，能力差异在 UI 明示。
- **PTY 后端选 `portable-pty`**：纯 Rust、单一 crate 覆盖三平台（Windows ConPTY / unix forkpty），是 wezterm/alacritty 系的事实标准；不引入系统依赖。
- **前端选 `@xterm/xterm` + `@xterm/addon-fit`**：xterm.js 官方拆分后的现名包，支持 `write(Uint8Array)`（跨块 UTF-8 安全）。
- **PTY 输出事件传输用 base64**：字节块 base64 → 前端解码为 `Uint8Array` → `term.write(bytes)`。避免 lossy String 在多字节字符跨块时产生替换符。

---

## §4 架构设计

### 4.1 后端 PTY 层

新模块 `src-tauri/src/process/pty.rs`（与 `streaming.rs` 平级，同属 `process/`）：

- `TerminalManager`（注册进 `AppState`）：`HashMap<SessionId, PtySession>`；会话 id 用 UUID/自增。
- `PtySession`：`portable_pty::PtyPair`（master 持有 writer + reader、slave spawn 子进程）、子进程 pid、写锁（`Mutex<Box<dyn Write + Send>>`）。
- **reader 线程**：阻塞读 master → 聚合成块（≤50ms flush 或 ≥8KiB 立即 flush）→ `terminal_output` 事件。子进程退出 → `terminal_exit` 事件 + 会话清理。
- **关闭语义**：`terminal_close` → 先 `terminate_process`（unix 组 SIGTERM），超时升级 `kill_process_tree`（复用 `kill_tree.rs`，禁止另起实现）。
- unix：PTY 子进程已是 session leader（forkpty 语义），与 AGENTS.md 的 `process_group(0)` 约定天然一致；Windows ConPTY 无进程组概念，树杀走 parent-chain（与 kill_tree 现状一致）。
- Windows：**ConPTY 路径不得叠加 `CREATE_NO_WINDOW`**（portable-pty 内部管理 spawn，与 `Command` 路径的约定不冲突；在代码注释中说明）。
- 应用退出时清理全部会话（`AppState` drop / tauri teardown 钩子）。

### 4.2 IPC 契约（前后端共同遵守，联调基准）

Tauri commands（注册在 `src-tauri/src/commands/terminal.rs`，沿用 camelCase payload 约定）：

| Command | 参数 | 返回 | 说明 |
|---|---|---|---|
| `terminal_open` | `{ cwd?: string, shell?: string, cols: number, rows: number }` | `{ sessionId: string }` | shell 缺省按 §5 探测；cwd 缺省当前工作区根 |
| `terminal_write` | `{ sessionId: string, dataBase64: string }` | `()` | 用户输入 → PTY master |
| `terminal_resize` | `{ sessionId: string, cols: number, rows: number }` | `()` | xterm fit 时同步 |
| `terminal_close` | `{ sessionId: string }` | `()` | 优雅→强杀 |
| `terminal_list` | `()` | `{ sessions: TerminalSessionInfo[] }` | 面板重开时恢复会话列表 |
| `terminal_list_shells` | `()` | `{ shells: { id: string, label: string, path: string }[] }` | 探测可用 shell profile（TM-07 新建 tab 下拉，按 §5.1 顺序） |

Events：

| 事件名 | Payload | 说明 |
|---|---|---|
| `terminal_output` | `{ sessionId: string, dataBase64: string }` | reader 线程批量（≤50ms / ≥8KiB flush） |
| `terminal_exit` | `{ sessionId: string, exitCode: number \| null }` | null = 信号终止 |
| `git_op_output`（TM-04） | `{ repoPath: string, repoName: string, command: string, stream: "stdout"\|"stderr"\|"meta", line: string }` | Git 镜像行；`meta` 为合成的 `$ 命令` 标题行/分隔行 |

`TerminalSessionInfo = { sessionId, kind: "shell" \| "runtime", title, cwd, alive }`。

### 4.3 前端面板

- 组件落点 `src/components/terminal/`：`TerminalPanel.vue`（drawer + tab 容器，模式仿 `TaskPanel.vue`：底部、可拖高 240px~85vh）、`XtermView.vue`（xterm 封装：挂载、fit、write、主题）、`TerminalTabs.vue`（tab 条 + 新建/关闭）。
- Store `src/stores/terminal.ts`：会话列表、activeTab、面板开合；`listen(terminal_output/terminal_exit)` 在面板首次打开时注册、App 生命周期内保持。
- 入口：StatusBar 新增「终端」槽位（仿任务槽位）+ 命令注册表 `terminal.toggle` / `terminal.newShell`（快捷键走注册表，禁止视图内私绑）。
- xterm 主题从 `--gw-*` tokens 取色（背景/前景/光标/选区），跟随亮暗主题切换；等宽字体用 `--gw-font-mono`。
- xterm 渲染写缓冲：事件回调里合并多个 chunk 后一次性 `write`，避免高频小写。
- `@xterm/xterm` 体积按需：组件懒加载（`defineAsyncComponent`）。

### 4.4 Git 输出镜像（TM-04）

- `core/git_ops/remote.rs::run_git` 从阻塞 `output()` 升级为 `spawn_streaming`（复用 cancel/timeout 语义），逐行 emit `git_op_output`；任务结束仍发 `git_command_result`（TaskPanel 兼容保留）。
- libgit2 本地操作：在 `GitOps::execute` 各分支合成 `meta` 行（`$ git commit -m "…"` 样式的可读描述）+ 结果摘要行，同样走 `git_op_output`。
- 前端 Git Console tab 按时间聚合全部仓库的 `git_op_output` 流，xterm 渲染（meta 行用 accent 色）。
- 网络 CLI 操作从此获得**实时进度**（fetch/pull/push 的远程进度行）——这是本任务独立的价值点。

### 4.5 Runtime 输出终端化（TM-05）与「在终端中启动」（TM-06）

- **TM-05**：终端面板为每个活跃 runtime 开只读 tab，数据源是 `stores/runtime.ts` 的 `logBuffers`（行内 ANSI 保留，`writeln` 即可）。前提：`store.subscribe()` 从 `useRuntimeWorkspace` 上移到 App 级（注意 5000 行环形上限已有，内存可控）；`RuntimeLogsView` 保持原样不受影响。同时新增**面板操作工具条**（对标 IDEA Run 面板）：activeTab 为受管 runtime tab 时显示 启动 / 重启 / 停止 按钮，调用 `stores/runtime.ts` 既有 `start/stop/restart`（走任务队列，TaskPanel 照常追踪），按钮可用性跟随该 runtime 运行态；shell / Git Console tab 不显示该组按钮。
- **TM-06**：新增 `runtime_start_in_terminal(runtimeName)`：构建走原链路产出 `LaunchPlan`，然后开一个 Shell tab 并写入 `plan.preview`（现成的可读命令串）+ 回车执行。UI 明示降级：此模式下无健康检查/端口检测/日志落盘，Stop = 关闭该 PTY 会话（kill 进程树）。`LaunchPlan.preview` 缺失或含需脱敏 env 时禁止该模式并提示。

---

## §5 平台兼容性细则

### 5.1 Shell 探测顺序（统一走 `find_in_path`，禁止裸名兜底）

| 平台 | 顺序 |
|---|---|
| Windows | `pwsh` → `powershell` → `cmd`（`.exe`/`.cmd`/`.bat` 候选） |
| macOS / Linux | `$SHELL` 环境变量 → `zsh` → `bash` → `sh` |

### 5.2 三平台要点

- **Windows**：ConPTY 需 Win10 1809+（portable-pty 内部处理，探测失败返回可行动错误提示「系统版本过低，请使用外部终端」）；中文系统 GBK 输出——base64 原始字节传输 + xterm 字节解析天然规避 F-12 类问题，**禁止在 PTY 路径使用 `read_line`/String lossy**。
- **macOS / Linux**：forkpty 子进程天然独立 session（满足 killpg 语义）；优雅关闭 `terminate_process` → 超时 `kill_process_tree`。
- 全平台：路径展示用 `display()`、比较必须归一化（AGENTS.md 平台规范）；环境相关测试探测不到工具链就 skip 并打印原因。

### 5.3 测试策略

- 纯函数单测：base64 编解码、会话表管理、shell 探测候选序列。
- 集成冒烟（后端）：开 PTY → 写 `echo hello` → 断言读到 `hello`；resize 不 panic；close 后进程树无残留（unix 断言进程组消失，Windows 断言 pid 不存在）。
- 三平台手动冒烟 checklist（写入 TM-03 验收）：打开 shell → 执行 `git status` / 方向键历史 / Ctrl-C / 中文输出 / 窗口缩放 / 关闭 tab 进程回收。

---

## §6 分期与任务拆分

对应 `docs/tasks-terminal/`：

| 期 | 任务 | 内容 |
|---|---|---|
| 一期 · 终端基础 | TM-01 PTY 会话后端 / TM-02 终端面板前端 / TM-03 交互式 Shell 端到端 | portable-pty 接入 + IPC 契约；xterm 面板骨架；三平台联通冒烟 |
| 二期 · Git 镜像 | TM-04 Git 输出流式化 + Git Console | `run_git` 流式化、`git_op_output`、libgit2 合成 meta 行 |
| 三期 · Runtime 终端化 | TM-05 Runtime 输出 xterm tab + 面板操作工具条 / TM-06 在终端中启动 | App 级订阅 + 启动/重启/停止按钮；`runtime_start_in_terminal`（降级模式） |
| 增量 · 打磨 | TM-07 终端交互打磨 | 面板内搜索 / 链接路径识别 / 复制粘贴 / shell profile 选择 / 清屏重开 / 面板最大化（明确不含选中即复制） |

依赖链：TM-01 ─► TM-02 ─► TM-03 ─► TM-04；TM-03 ─► TM-05 ─► TM-06（详见 tasks-terminal/README.md）。

---

## §7 验收总标准

- 一期：三平台可打开交互 shell，执行/中断/缩放/关闭全链路正常，进程无泄漏。
- 二期：fetch/pull/push 实时进度出现在 Git Console；本地 git 操作有 `$ 命令` 镜像行；TaskPanel 原功能不回归。
- 三期：runtime 输出在终端面板正确渲染颜色；面板工具条可对当前 runtime tab 执行 启动/重启/停止，按钮状态与运行态一致；「在终端中启动」可用且降级提示明确。
- 全程：`pnpm build` + `cargo check` + 相关 `cargo test` 通过；无硬编码色值/像素（tokens）；不引入 AGENTS.md 禁止的模式。

## §8 风险与缓解

| 风险 | 缓解 |
|---|---|
| ConPTY 老系统不可用 | 探测失败给可行动错误；一期不做 fallback 终端 |
| 事件洪水（大输出刷爆 IPC） | reader 50ms/8KiB 批量 + 前端写缓冲 + tab 隐藏时暂停渲染 |
| xterm 包体积影响启动 | 组件懒加载，仅打开面板时载入 |
| 「在终端中启动」泄露 env 中的 secret | preview 含敏感 env 时禁用该模式；命令写入前过一遍脱敏器 |
| PTY 会话泄漏 | 应用退出钩子全量清理 + `terminal_list` 恢复视图以存活会话为准 |

## §9 约定沉淀

功能稳定后（三期完成），将终端相关硬规则以 `terminal:start/end` 段落沉淀进根目录 `AGENTS.md`（参照 desktop-skin 段落的先例），候选规则：PTY 路径禁止 `read_line`/lossy 解码、shell 探测顺序、ConPTY 与 CREATE_NO_WINDOW 边界、终端事件批量阈值。
