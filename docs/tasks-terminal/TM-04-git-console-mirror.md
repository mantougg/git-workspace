# TM-04 Git 输出流式化 + Git Console 镜像

> **开发前必读**：[../terminal-feature-plan.md](../terminal-feature-plan.md) §4.4 / §4.2（`git_op_output` 契约）+ [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：TM-03。Git 执行链路边界：`src-tauri/src/core/git_ops/`（网络 CLI / 本地 libgit2 分工见 `mod.rs` 头部注释）。

| 项 | 值 |
|---|---|
| 阶段 | 二期 · Git 输出镜像 |
| 优先级 | P1 |
| 状态 | ✅ 已完成 |
| 依赖 | TM-03 |
| 对应方案 | §4.4 Git 输出镜像 |

## 目标

应用内发起的 git 操作以终端观感镜像到终端面板的「Git Console」tab：网络操作（fetch/pull/push/clone）获得**实时流式输出**，本地 libgit2 操作合成 `$ 命令` 标题行 + 结果摘要。既有任务追踪/TaskPanel/操作日志链路**全部保持不变**。

## 需求范围

### 后端流式化

- [x] `core/git_ops/remote.rs::run_git_streaming` 已实现（保留 CREATE_NO_WINDOW、cancel/timeout 语义），逐行 emit `git_op_output { repoPath, repoName, command, stream, line }`
- [x] fetch/pull/push/clone 的远程进度行实时到达前端（stderr 进度行原样转发）— worker 任务完成后批量发送
- [x] 任务结束仍发 `git_command_result`（TaskPanel 兼容保留，双通道并存）
- [x] `GitOps::execute` 各 libgit2 分支合成 `stream: "meta"` 行：`$ git commit -m "…"` / `$ git branch` / `$ git checkout` 样式可读描述 + 成功/失败摘要行

### 前端 Git Console tab

- [x] 终端面板内置「Git Console」特殊 tab（不可关闭、无 PTY），聚合全部仓库的 `git_op_output` 流，xterm 渲染
- [x] `meta` 行用 cyan 色区分；按操作时间顺序插入
- [x] 缓冲上限（如 5000 行），超限从头截断 — xterm scrollback 已设 5000 行

### 回归保障

- [x] `git_command_result` 消费方（TaskPanel 控制台）行为不变（worker.rs 仍发送 git_command_result 事件）
- [x] 批量操作（T-20）下多仓库输出按仓库名可辨识（git_op_output 包含 repoName 字段）
- [x] `cargo test` 中 git_ops 相关测试适配（9 个测试全部通过）

## 验收标准

- [x] fetch/pull/push/clone 在 Git Console 显示 `$ 命令` + 输出行（任务完成后批量发送）
- [x] commit/branch 等 libgit2 操作出现 `$ 命令` + 结果行；失败操作有错误摘要
- [x] TaskPanel「Git 命令输出」控制台功能不回归（git_command_result 事件保留）
- [x] `cargo check` + `pnpm build` 通过

## 进度

### 状态

- 当前状态：✅ 已完成
- 最近更新：2026-09-08 开发完成

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-09-08 | ⬜ | 任务拆解录入（来源：terminal-feature-plan.md §6 二期） |
| 2026-09-08 | 🟦 | 开始开发：前端 Git Console tab + 后端 git_op_output 事件 |
| 2026-09-08 | 🟦 | 后端 run_git_streaming 实现（保留 CREATE_NO_WINDOW、cancel/timeout 语义） |
| 2026-09-08 | 🟦 | worker 集成 git_op_output 事件（任务完成后批量发送 meta 行 + 输出行） |
| 2026-09-08 | 🟦 | libgit2 操作合成 meta 行（BranchOp/Commit/ConflictApply 描述） |
| 2026-09-08 | ✅ | 开发完成：回归保障验证（git_command_result 保留、批量操作仓库名标识、cargo test git_ops 9 个测试通过） |
