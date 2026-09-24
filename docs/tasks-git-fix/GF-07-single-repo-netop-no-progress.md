# GF-07 单仓网络操作无进度无取消 + `push_branch` 同步命令阻塞 IPC 线程

> 状态：✅ 已完成
> 优先级：P1（远程挂起即界面假死，属可复现的高危体验回归点）
> 来源：2026-09-24 Git 使用体验全景盘点。

## 问题描述

批次操作（batch_fetch/pull/push）有 Git Console 实时输出镜像、可取消、5 分钟超时；
但单仓网络操作完全是另一副样子：

1. `sync_fetch` / `sync_pull` / `sync_push` / `smart_pull` 的 `on_line` 回调全是
   **空闭包**——单仓操作无任何进度、无取消通道，前端只能转圈等 300s 硬超时。
2. `push_branch`（分支管理页 Push 用的就是它）是**同步命令直接跑 `git push` CLI**，
   无超时、阻塞 WebView IPC 回调线程——远程挂起时整个界面失去响应。

单/批体验割裂，且后者是真实的界面假死风险。

## 定位线索（证据）

> 2026-09-24 复现核验结论（修复前）：
> - 空 `on_line` 闭包：**成立**。`commands/git_ops.rs` 的 `sync_fetch` /
>   `sync_pull` / `sync_push` / `smart_pull`（含 fetch 与两处 pull 回退）全部
>   传 `&mut |_, _| {}`（原 :203 / :215 / :232 / :244 / :261 / :337，PAF-08 后
>   行号有漂移、语义不变）。
> - 同步直连 CLI：**成立**。`commands/branch.rs:92`（原行号）`push_branch` 为
>   `pub fn` 同步命令直接跑 `GitOps::with_default_ssh().push_branch(...)`，
>   无 async / 无超时 / 无取消。
> - 300s 硬超时：**漂移后仍成立**。`TASK_TIMEOUT` 常量在 `task/worker.rs:21`；
>   `commands/git_ops.rs:20` 现为 `SYNC_GIT_TIMEOUT`（PAF-08 引入，300s，
>   经 `run_git_streaming` 杀进程树）。
> - 批次对照实现：**漂移后仍成立**。无独立 `run_git_streaming` 函数在
>   worker.rs；流式 + 100ms 聚合 emit `git_op_output` 的实现在
>   `task/worker.rs` 的 `ConsoleStreamer`（CONSOLE_FLUSH_INTERVAL = 100ms）。
>   底层 `run_git_streaming` 在 `core/git_ops/remote.rs:233`。
> - 前端消费方：`src/api/branch.ts`（push_branch wrapper，调用方
>   BranchManager.vue Push 按钮）、`src/api/git_ops.ts`——**部分漂移**：
>   `sync_fetch/pull/push` 仍零视图引用（GF-14），但 `smart_pull` 有 3 个
>   调用方（BranchManager / RepositoryList / BatchActionBar），不在"零引用"之列。

- 空 on_line 闭包：`src-tauri/src/commands/git_ops.rs:203 / :215 / :232 / :244 / :261 / :337`
- 同步直连 CLI：`src-tauri/src/commands/branch.rs:92`（`push_branch`，`GitOps::with_default_ssh().push_branch(...)`，无 async/无超时）
- 300s 硬超时：`src-tauri/src/commands/git_ops.rs:20`（TASK_TIMEOUT 常量区）
- 批次对照实现（流式 + 镜像）：`src-tauri/src/task/worker.rs:472-537`（`run_git_streaming`，100ms 聚合 emit `git_op_output`）
- 前端消费方：`src/api/branch.ts`（push_branch wrapper）、`src/api/git_ops.ts`（sync_* wrapper，当前零视图引用，见 GF-14）

## 修复范围 checklist

- [x] 1. `sync_fetch/sync_pull/sync_push/smart_pull` 的 `on_line` 接流式：emit 与批次一致的 `git_op_output` 事件（或新事件名），前端在 Git Console / 操作位置展示增量输出。
- [x] 2. `push_branch` 改 `async` + `spawn_blocking`（或直接入任务队列），加超时与取消；**注意 AGENTS.md F-43 线程模型规则**（异步化后的调用链不得引入裸 `tokio::spawn`）。
- [x] 3. 前端单仓网络操作展示进度（Console tab 镜像或按钮 loading + 输出区），提供取消入口（与 TaskPanel 现有取消体验对齐）。

## 不做（范围控制）

- 不重写凭据链路（凭据走系统 GCM / ssh-agent 的设计不变，见 `core/git_ops/mod.rs:24-28`）。
- 不动任务队列本身（超时/重试/取消已成熟）。

## 验收标准

> 2026-09-24 核验（修复后，均满足）：
> 1. 单仓 pull/push 输出经 100ms 聚合 emit `git_op_output` 实时进 Git Console
>    （`ConsoleStreamer`，与批次同一事件/同一聚合窗口）；取消按钮见标准 3。
> 2. `push_branch` 为 async + `tauri::async_runtime::spawn_blocking`，
>    `push_branch_streaming` 带 300s 超时（杀 git 进程树）与 cancel flag；
>    远程不可达时 IPC 线程不再阻塞，错误经 stderr 尾部还原成可读 Git 错误
>    原样冒泡（`finish_streaming` → `readable_error`，GF-08 语义不变）。
> 3. `git_op_started` / `git_op_finished` 生命周期事件驱动：Git Console 自动
>    弹出聚焦（不再静默转圈），面板工具条按进行中 op 渲染「取消」按钮
>    （NButton，与 TaskPanel 取消体验对齐），点击走 `cancel_git_op`。
> 4. `GW_TEST_MANIFEST=1 cargo test --lib`：955 passed / 14 failed / 3 ignored。
>    14 个失败与 HEAD 基线（946 passed / 16 failed）逐条比对全部命中预存失败
>    清单（real_maven / node_vite / pty smoke / pathutil / benchmark smoke，
>    均环境依赖且不在本次改动链路）；基线的 16 个失败为预存 14 + 2 个抖动
>    （benchmark smoke、node_vite slow spawn）。新增 7 个测试全过
>    （single_ops 4 + push_branch_streaming 3）。

1. 单仓 pull/push 大远程有实时输出（不再静默转圈）；取消按钮可中断。
2. `push_branch` 异步化后：远程不可达时界面仍可交互（不再阻塞 IPC 线程），错误可行动（GF-08 联动）。
3. 验证线程模型合规：沿调用链 grep 无裸 `tokio::spawn` / `Handle::current` / `block_on`。
4. `cargo test --lib`（GW_TEST_MANIFEST=1）通过。

## 实现说明（2026-09-24）

- 新模块 `src-tauri/src/task/console.rs`：`ConsoleStreamer`（自 worker.rs 迁移，
  worker 与单仓命令共用）+ `emit_git_op_output/started/finished` +
  `finish_streaming`（超时/取消/失败的人话结论行 + stderr 尾部还原可读错误）。
- 新模块 `src-tauri/src/task/single_ops.rs`：`SingleOpRegistry`（op_id →
  AtomicBool，DashMap）+ `SingleOpGuard`（RAII 摘除，防 panic 泄漏）。
  `AppState.single_ops` 持有唯一实例。
- `commands/git_ops.rs`：sync_fetch/sync_pull/sync_push/smart_pull 增
  `op_id: Option<String>` + `AppHandle` + `State<AppState>`，走
  `register_single_op` → `git_op_started` → spawn_blocking 流式 →
  `git_op_finished`；新增 `cancel_git_op` 命令。
- `commands/branch.rs`：`push_branch` 改 async + spawn_blocking，走新增的
  `GitOps::push_branch_streaming`（upstream 解析抽为 `resolve_push_target`，
  非流式 `push_branch` 复用同一解析，两路径目标一致）。
- 前端：`api/git_ops.ts` / `api/branch.ts` wrapper 增可选 `opId` 与
  `cancelGitOp`；`api/terminal.ts` 增 `GIT_OP_EVENTS` 与两个事件类型；
  新 composable `useGitOpMirror`（App 级监听，对齐 useTaskProgress——终端
  面板懒注册 F-42 会丢首批事件）；`stores/terminal.ts` 出 `gitOpsInFlight`
  与 `cancelGitOp`；`TerminalPanel.vue` 工具条渲染取消按钮（naive-ui）。
- 前端调用方零改动即生效：BranchManager 的 Push / Pull、RepositoryList /
  BatchActionBar 的 smartPull 调用签名向后兼容（新参数可选）。

## 进度

| 日期 | 记录 |
|---|---|
| 2026-09-24 | 建任务：体验盘点发现（空闭包 + 同步 push_branch，均已静态核验）。待复现与修复。 |
| 2026-09-24 | 修复完成。根因：单仓网络命令的 `on_line` 全为空闭包（无 Console 镜像、无取消通道），`push_branch` 为同步命令直连 git CLI（无 async/超时/取消，远程挂起阻塞 IPC 线程）。修法：ConsoleStreamer 迁移至 `task/console.rs` 供队列与单仓共用；新增 `single_ops` 取消注册表（op_id→AtomicBool，RAII 摘除）与 `cancel_git_op` 命令；sync_* 与 push_branch 全部改 async + `tauri::async_runtime::spawn_blocking` + `*_streaming`（300s 超时杀进程树）；前端 `useGitOpMirror` App 级监听 `git_op_started/finished`，Git Console 自动弹出聚焦 + 工具条取消按钮。验证：`GW_TEST_MANIFEST=1 cargo test --lib` 955 passed / 14 failed / 3 ignored（14 失败全部命中 HEAD 基线预存失败清单，新增 7 测试全过）；`pnpm build`（vue-tsc + vite）通过；F-43 链路 grep 无裸 tokio::spawn/Handle::current/block_on。README 总表状态由主智能体统一同步。 |
