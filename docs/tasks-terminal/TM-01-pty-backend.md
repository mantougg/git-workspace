# TM-01 PTY 会话后端（portable-pty + IPC 契约）

> **开发前必读**：[../terminal-feature-plan.md](../terminal-feature-plan.md) §4.1 / §4.2 / §5（架构与 IPC 契约基准）+ [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：无。

| 项 | 值 |
|---|---|
| 阶段 | 一期 · 终端基础 |
| 优先级 | P0 |
| 状态 | ⬜ 未开始 |
| 依赖 | — |
| 对应方案 | §4.1 后端 PTY 层 / §4.2 IPC 契约 / §5.2 |

## 目标

在 `src-tauri` 引入 `portable-pty`，实现跨平台 PTY 会话后端：会话创建/写入/缩放/关闭的 Tauri commands + `terminal_output` / `terminal_exit` 事件，供 TM-02 前端对接。本任务只交付后端 + 自测，不含前端。

## 需求范围

### 依赖与模块

- [ ] `src-tauri/Cargo.toml` 新增 `portable-pty`（选定版本后锁进 Cargo.lock；不引入其他终端 crate）
- [ ] 新模块 `src-tauri/src/process/pty.rs`，经 `process/mod.rs` 导出；`TerminalManager` 注册进 `AppState`
- [ ] 新命令模块 `src-tauri/src/commands/terminal.rs`，在 `lib.rs` 注册 5 个 command

### 会话管理

- [ ] `PtySession`：持有 PtyPair（master writer/reader、slave spawn）、子进程 pid、写锁 `Mutex<Box<dyn Write + Send>>`
- [ ] 会话表 `HashMap<String, PtySession>`，sessionId 用 UUID；`terminal_list` 返回存活会话（`TerminalSessionInfo`，契约字段见方案 §4.2）
- [ ] Shell 探测按方案 §5.1 顺序（走 `find_in_path`，禁止裸名兜底）；探测失败返回可行动错误
- [ ] cwd 缺省当前工作区根；env 继承应用进程环境

### 读写与事件

- [ ] reader 线程：阻塞读 master → 聚合（≤50ms flush 或 ≥8KiB 立即 flush）→ `terminal_output { sessionId, dataBase64 }` 事件
- [ ] 子进程退出 → `terminal_exit { sessionId, exitCode }`（信号终止为 null）+ 会话清理
- [ ] `terminal_write`：base64 解码后写 master（bytes，不经 String）
- [ ] `terminal_resize`：`pty_size` 同步到 pair

### 生命周期与终止

- [ ] `terminal_close`：`terminate_process` 优雅 → 超时升级 `kill_process_tree`（复用 `kill_tree.rs`，禁止另起实现）
- [ ] 应用退出钩子全量清理会话
- [ ] Windows ConPTY 路径不叠加 `CREATE_NO_WINDOW`（注释说明）；unix 依赖 forkpty 独立 session（注释说明）

### 测试

- [ ] 纯函数单测：base64 编解码、shell 探测候选序列、会话表增删
- [ ] 集成冒烟：开 PTY → 写 `echo hello` → 断言读到 `hello`；resize 不 panic；close 后进程无残留（探测不到 shell 时 skip 并打印原因）

## 验收标准

- [ ] 5 个 command 与 2 个事件的名称/payload 与方案 §4.2 **完全一致**（含 camelCase、事件名无 `.`）
- [ ] `cargo test pty` 通过；`cargo check` 无警告
- [ ] 集成冒烟在本机（Linux）通过；Windows/macOS 分支代码经 cfg 审查
- [ ] 无 `read_line` / `from_utf8_lossy` 出现在 PTY 路径；kill 逻辑复用 `kill_tree.rs`

## 进度

### 状态

- 当前状态：⬜ 未开始
- 最近更新：2026-09-08 录入

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-09-08 | ⬜ | 任务拆解录入（来源：terminal-feature-plan.md §6 一期-1） |
