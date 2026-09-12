# TM-01 PTY 会话后端（portable-pty + IPC 契约）

> **开发前必读**：[../terminal-feature-plan.md](../terminal-feature-plan.md) §4.1 / §4.2 / §5（架构与 IPC 契约基准）+ [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：无。

| 项 | 值 |
|---|---|
| 阶段 | 一期 · 终端基础 |
| 优先级 | P0 |
| 状态 | ✅ 已完成 |
| 依赖 | — |
| 对应方案 | §4.1 后端 PTY 层 / §4.2 IPC 契约 / §5.2 |

## 目标

在 `src-tauri` 引入 `portable-pty`，实现跨平台 PTY 会话后端：会话创建/写入/缩放/关闭的 Tauri commands + `terminal_output` / `terminal_exit` 事件，供 TM-02 前端对接。本任务只交付后端 + 自测，不含前端。

## 需求范围

### 依赖与模块

- [x] `src-tauri/Cargo.toml` 新增 `portable-pty`（选定版本后锁进 Cargo.lock；不引入其他终端 crate）
- [x] 新模块 `src-tauri/src/process/pty.rs`，经 `process/mod.rs` 导出；`TerminalManager` 注册进 `AppState`
- [x] 新命令模块 `src-tauri/src/commands/terminal.rs`，在 `lib.rs` 注册 6 个 command

### 会话管理

- [x] `PtySession`：持有 PtyPair（master writer/reader、slave spawn）、子进程 pid、写锁 `Mutex<Box<dyn Write + Send>>`
- [x] 会话表 `HashMap<String, PtySession>`，sessionId 用 UUID；`terminal_list` 返回存活会话（`TerminalSessionInfo`，契约字段见方案 §4.2）
- [x] Shell 探测按方案 §5.1 顺序（走 `find_in_path`，禁止裸名兜底）；探测失败返回可行动错误
- [x] cwd 缺省当前工作区根；env 继承应用进程环境

### 读写与事件

- [x] reader 线程：阻塞读 master → 聚合（≤50ms flush 或 ≥8KiB 立即 flush）→ `terminal_output { sessionId, dataBase64 }` 事件
- [x] 子进程退出 → `terminal_exit { sessionId, exitCode }`（信号终止为 null）+ 会话清理
- [x] `terminal_write`：base64 解码后写 master（bytes，不经 String）
- [x] `terminal_resize`：`pty_size` 同步到 pair（当前版本记录日志，后续优化保留 master handle）

### 生命周期与终止

- [x] `terminal_close`：`terminate_process` 优雅 → 超时升级 `kill_process_tree`（复用 `kill_tree.rs`，禁止另起实现）
- [x] 应用退出钩子全量清理会话
- [x] Windows ConPTY 路径不叠加 `CREATE_NO_WINDOW`（注释说明）；unix 依赖 forkpty 独立 session（注释说明）

### 测试

- [x] 纯函数单测：base64 编解码、shell 探测候选序列、会话表增删
- [x] 集成冒烟：开 PTY → 写 `echo hello` → 断言读到 `hello`；resize 不 panic；close 后进程无残留（探测不到 shell 时 skip 并打印原因）

## 验收标准

- [x] 6 个 command 与 2 个事件的名称/payload 与方案 §4.2 **完全一致**（含 camelCase、事件名无 `.`）
- [x] `cargo test pty` 通过（22 tests passed，含 8 个 PTY 测试）；`cargo check` 通过
- [x] 集成冒烟在本机（Linux）通过；Windows/macOS 分支代码经 cfg 审查
- [x] 无 `read_line` / `from_utf8_lossy` 出现在 PTY 路径；kill 逻辑复用 `kill_tree.rs`

## 进度

### 状态

- 当前状态：✅ 已完成
- 最近更新：2026-09-12 Windows ConPTY 根因修复（master 保活 + resize 落地）

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-09-08 | ⬜ | 任务拆解录入（来源：terminal-feature-plan.md §6 一期-1） |
| 2026-09-08 | 🟦 | 开始开发：Cargo.toml 新增 portable-pty，创建 pty.rs 模块 |
| 2026-09-08 | ✅ | 开发完成：TerminalManager + PtySession + shell 探测 + reader 线程 + 6 command + 2 event，cargo check 通过，22 tests 全部通过（含 8 个 PTY 测试） |
| 2026-09-12 | ✅ | Windows 根因修复：open() 原 `drop(pty_pair)` 触发 ConPTY `ClosePseudoConsole` 杀死 shell——reader 只收到 EOF，终端空白/不可交互（Linux/macOS 因 reader/writer dup fd 不受影响，故潜伏至今）；改为仅 drop slave、master 随 PtySession 保活，`resize` 从 no-op 桩落地为真实实现；新增回归 `smoke_reader_receives_shell_output`（12/12 pty 测试通过）；另修复 Windows `cargo test` 无法运行：测试 exe 无应用清单 → comctl32 v5 缺 `TaskDialogIndirect` 加载即 0xc0000139，`build.rs` 在 `GW_TEST_MANIFEST=1` 时注入 `test.manifest`（comctl32 v6 依赖）解决，Windows 跑单测命令：`GW_TEST_MANIFEST=1 cargo test --lib` |
