# GF-10 Workspace Stash 多仓串行执行且无进度无取消

> 状态：⬜ 未开始
> 优先级：P2
> 来源：2026-09-24 Git 使用体验全景盘点。

## 问题描述

Workspace Stash（工作区快照，保存/恢复跨仓库 stash）对每个仓库**串行**执行
`git stash`，且**无进度事件、无取消**。在几十个仓库的工作区里，这是全应用唯一一个
「黑屏等待」的写操作——其余批量写操作（fetch/pull/push/commit/stage/branch op）都走
任务队列，有 TaskPanel 进度、Console 镜像、可取消。

## 定位线索（证据）

- 串行循环：`src-tauri/src/core/workspace_stash.rs:106-118`（`stash_repos`，`for path in repo_paths`）
- 命令层无进度：`src-tauri/src/commands/workspace_stash.rs:47`（save 调 `stash_repos`，前后仅 DB 操作）
- restore 同路：`src-tauri/src/commands/workspace_stash.rs:112`（`restore_workspace_stash`）
- 对照：批量任务队列治理 `src-tauri/src/task/worker.rs`（进度 emit、cancel flag 杀进程树 `:472-476`）
- 前端入口：`src/api/workspaceStash.ts` → RepositoryList Workspace Stash 面板

## 修复范围 checklist

- [ ] 1. save/restore 改为入任务队列（新增 TaskType 或复用批量任务），TaskPanel 可见逐仓进度与结果。
- [ ] 2. 支持取消；**必须定义部分完成的语义**（哪些仓已 stash / 哪些未动，失败时给出可恢复清单——恢复预检 `check_workspace_stash` 已有安全网）。
- [ ] 3. 保留现有安全预检：`check_workspace_stash`（分支不匹配需 `allow_branch_mismatch`）在入队前执行。

## 不做（范围控制）

- 不改串行→并行的 git 执行模型（stash 跨仓无依赖，但并行会放大机器负载与错误面；先补可观测性，并行化另立任务评估）。
- 不动 DB 记录结构。

## 验收标准

1. 多仓（≥10）workspace stash 保存/恢复：TaskPanel 显示逐仓进度，耗时与当前串行版本持平即可，但等待期间界面可交互、可取消。
2. 中途取消：已完成的仓库状态正确（不半途损坏），给出后续处理提示。
3. `cargo test --lib`（GW_TEST_MANIFEST=1）通过；既有 workspace_stash 单测无回归。

## 进度

| 日期 | 记录 |
|---|---|
| 2026-09-24 | 建任务：体验盘点发现（串行 for 循环 + 无进度事件，均已静态核验）。待复现与修复。 |
