# TM-04 Git 输出流式化 + Git Console 镜像

> **开发前必读**：[../terminal-feature-plan.md](../terminal-feature-plan.md) §4.4 / §4.2（`git_op_output` 契约）+ [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：TM-03。Git 执行链路边界：`src-tauri/src/core/git_ops/`（网络 CLI / 本地 libgit2 分工见 `mod.rs` 头部注释）。

| 项 | 值 |
|---|---|
| 阶段 | 二期 · Git 输出镜像 |
| 优先级 | P1 |
| 状态 | ⬜ 未开始 |
| 依赖 | TM-03 |
| 对应方案 | §4.4 Git 输出镜像 |

## 目标

应用内发起的 git 操作以终端观感镜像到终端面板的「Git Console」tab：网络操作（fetch/pull/push/clone）获得**实时流式输出**，本地 libgit2 操作合成 `$ 命令` 标题行 + 结果摘要。既有任务追踪/TaskPanel/操作日志链路**全部保持不变**。

## 需求范围

### 后端流式化

- [ ] `core/git_ops/remote.rs::run_git` 从阻塞 `cmd.output()` 升级为 `spawn_streaming`（保留 CREATE_NO_WINDOW、cancel/timeout 语义），逐行 emit `git_op_output { repoPath, repoName, command, stream, line }`
- [ ] fetch/pull/push/clone 的远程进度行实时到达前端（stderr 进度行原样转发）
- [ ] 任务结束仍发 `git_command_result`（TaskPanel 兼容保留，双通道并存）
- [ ] `GitOps::execute` 各 libgit2 分支合成 `stream: "meta"` 行：`$ git commit -m "…"` 样式可读描述 + 成功/失败摘要行（文案列表在任务文档时间线补充定稿）

### 前端 Git Console tab

- [ ] 终端面板内置「Git Console」特殊 tab（不可关闭、无 PTY），聚合全部仓库的 `git_op_output` 流，xterm 渲染
- [ ] `meta` 行用 `--gw-accent` 取色区分；按操作时间顺序插入，单次操作多行间不穿插（行级缓冲按 command 分组 flush）
- [ ] 缓冲上限（如 5000 行），超限从头截断

### 回归保障

- [ ] `git_command_result` 消费方（TaskPanel 控制台）行为不变
- [ ] 批量操作（T-20）下多仓库输出按仓库名可辨识
- [ ] `cargo test` 中 git_ops 相关测试适配（mock runner 注入点如受影响需同步）

## 验收标准

- [ ] 真实 fetch/pull/push（含需要网络进度的场景）在 Git Console 实时滚动，与命令行观感一致
- [ ] commit/branch/stash 等 libgit2 操作出现 `$ 命令` + 结果行；失败操作有错误摘要
- [ ] TaskPanel「Git 命令输出」控制台功能不回归
- [ ] `cargo check` + 相关 `cargo test` + `pnpm build` 通过

## 进度

### 状态

- 当前状态：⬜ 未开始
- 最近更新：2026-09-08 录入

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-09-08 | ⬜ | 任务拆解录入（来源：terminal-feature-plan.md §6 二期） |
