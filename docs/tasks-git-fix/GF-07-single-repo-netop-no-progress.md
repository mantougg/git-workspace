# GF-07 单仓网络操作无进度无取消 + `push_branch` 同步命令阻塞 IPC 线程

> 状态：⬜ 未开始
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

- 空 on_line 闭包：`src-tauri/src/commands/git_ops.rs:203 / :215 / :232 / :244 / :261 / :337`
- 同步直连 CLI：`src-tauri/src/commands/branch.rs:92`（`push_branch`，`GitOps::with_default_ssh().push_branch(...)`，无 async/无超时）
- 300s 硬超时：`src-tauri/src/commands/git_ops.rs:20`（TASK_TIMEOUT 常量区）
- 批次对照实现（流式 + 镜像）：`src-tauri/src/task/worker.rs:472-537`（`run_git_streaming`，100ms 聚合 emit `git_op_output`）
- 前端消费方：`src/api/branch.ts`（push_branch wrapper）、`src/api/git_ops.ts`（sync_* wrapper，当前零视图引用，见 GF-14）

## 修复范围 checklist

- [ ] 1. `sync_fetch/sync_pull/sync_push/smart_pull` 的 `on_line` 接流式：emit 与批次一致的 `git_op_output` 事件（或新事件名），前端在 Git Console / 操作位置展示增量输出。
- [ ] 2. `push_branch` 改 `async` + `spawn_blocking`（或直接入任务队列），加超时与取消；**注意 AGENTS.md F-43 线程模型规则**（异步化后的调用链不得引入裸 `tokio::spawn`）。
- [ ] 3. 前端单仓网络操作展示进度（Console tab 镜像或按钮 loading + 输出区），提供取消入口（与 TaskPanel 现有取消体验对齐）。

## 不做（范围控制）

- 不重写凭据链路（凭据走系统 GCM / ssh-agent 的设计不变，见 `core/git_ops/mod.rs:24-28`）。
- 不动任务队列本身（超时/重试/取消已成熟）。

## 验收标准

1. 单仓 pull/push 大远程有实时输出（不再静默转圈）；取消按钮可中断。
2. `push_branch` 异步化后：远程不可达时界面仍可交互（不再阻塞 IPC 线程），错误可行动（GF-08 联动）。
3. 验证线程模型合规：沿调用链 grep 无裸 `tokio::spawn` / `Handle::current` / `block_on`。
4. `cargo test --lib`（GW_TEST_MANIFEST=1）通过。

## 进度

| 日期 | 记录 |
|---|---|
| 2026-09-24 | 建任务：体验盘点发现（空闭包 + 同步 push_branch，均已静态核验）。待复现与修复。 |
