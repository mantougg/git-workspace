<!-- gitnexus:start -->
# GitNexus — Code Intelligence

This project is indexed by GitNexus as **git-workspace** (12431 symbols, 27724 relationships, 300 execution flows). Use the GitNexus MCP tools to understand code, assess impact, and navigate safely.

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

<!-- gitnexus:end -->

<!-- bug-fixes:start -->
# Bug Analysis & Fixes（2026-09-19）

> 来源：docs/bug-analysis-2026-09-19.md（13 个问题深度分析与修复）。
> 测试用例：docs/test-cases-2026-09-19.md（79 个用例覆盖全部 13 个问题域）。

## 已修复的关键问题

### 前端

| 问题 | 文件 | 修复 |
|------|------|------|
| Git Console 只有 "ready" | `src/components/terminal/XtermView.vue:132` | 虚拟会话（`__xxx__`）跳过 `terminal.writeln("Terminal ready.")` |
| 文件监听不刷新变更树 | `src/views/RepositoryList.vue:1478` | 监听 `repo_status_changed_batch` 事件，800ms 防抖调用 `loadChanges()` |
| 终端 Ctrl+Shift+V 不工作 | `src/components/terminal/XtermView.vue:152` | 新增 Ctrl+Shift+V 粘贴处理器（大小写兼容） |

### 后端（Rust）

| 问题 | 文件 | 修复 |
|------|------|------|
| 终端启动无 PID | `src-tauri/src/process/pty.rs:567` | `get_session_pid()` 方法传播 PTY child PID |
| 终端启动无端口 | `src-tauri/src/runtime/service/queries.rs:284` | `register_terminal_process` 后 spawn 后台线程延迟 3s 扫描端口 |
| Windows 凭证存储失败 | `src-tauri/src/ai/credentials.rs:279` | 新增 `FileCredentialStore`（XChaCha20-Poly1305 加密），OS→File→Session 三级回退 |
| repo_tools 阻塞 UI | `src-tauri/src/commands/repo_tools.rs` | `submodule_op`/`lfs_op`/`run_hook` 改为 `spawn_blocking` |

### 已有实现（确认无需修改）

| 问题 | 文件 | 说明 |
|------|------|------|
| cleaner 阻塞 UI | `src-tauri/src/commands/cleaner.rs:111` | 已通过 `std::thread::spawn` 非阻塞执行 |
| PID 传播 | `src-tauri/src/runtime/service/queries.rs:258` | `register_terminal_process` 已接受 `pty_pid` 参数 |

## 架构关键点

- **终端启动 vs 托管启动**：两种独立模式。终端启动是降级模式（无健康检查/日志持久化/AI 诊断），但已有 PID 传播和端口检测。
- **文件 watcher vs 变更树**：两套独立数据路径。watcher 更新 `RepoStatus`（轻量），变更树需要 `RepoChanges`（完整 libgit2 扫描）。桥接方式是监听 `repo_status_changed_batch` 事件触发 `loadChanges()`。
- **凭证存储回退链**：OS Credential Store → FileCredentialStore（`.gitworkspace/credentials/`）→ SessionStore（内存）。Windows 凭证管理器不可用时自动回退到文件存储。
- **Tauri 命令线程模型**：同步命令内禁止裸 `tokio::spawn`（会 panic）。长阻塞操作用 `std::thread::spawn` 或改为 `async` + `spawn_blocking`。

## 待复现问题

- **问题 1（Pull 拉不下来）**：代码审查显示三条 pull 路径（smart_pull/batch_pull/sync_pull）实现正确，需在实际环境复现。
<!-- bug-fixes:end -->
