# GF-19 StatusBar 分支槽位空占位（Git 视图不显示当前分支）

> 状态：⬜ 未开始
> 优先级：P2（低成本、每天可见的桌面体验）
> 来源：2026-09-24 Git 使用体验全景盘点；需求依据 project-analysis:115 + desktop-skin-plan.md:211。

## 问题描述

StatusBar 有分支槽位的模板（`v-if="currentBranch"`），但 `currentBranch` 恒为
`ref(null)`——槽位永远不显示。用户在日常操作中看不到自己当前在哪个仓库的哪个分支上，
git 类视图的上下文感知因此缺一环。

## 定位线索（证据）

- 模板：`src/components/shell/StatusBar.vue:13-18`（`v-if="currentBranch"` 的 slot + divider）
- 空占位：`src/components/shell/StatusBar.vue:224`（`const currentBranch = ref<string | null>(null)`）
- 规范要求：desktop-skin-plan.md:211「StatusBar 分支槽位仅在 Git 类视图显示当前仓库分支；无上下文时隐藏槽位」
- 分析报告已知坑：project-analysis:115「StatusBar 分支槽位空占位（`currentBranch = ref(null)`，预留接口）」
- 数据来源：当前仓库状态 store（watcher 驱动的 `repo_status_changed_batch` 已含分支信息）+ checkout 操作后的刷新（`checkout_branch` 命令 `src-tauri/src/commands/branch.rs:58` 已通知 R-21 联动引擎）。

## 修复范围 checklist

- [ ] 1. 接当前仓库 + 当前分支：从现有当前仓库状态读取，watcher 事件 / checkout 后刷新。
- [ ] 2. 按 desktop-skin-plan:211：仅在 Git 类视图显示；无当前仓库上下文时隐藏槽位（不显示占位）。
- [ ] 3. 交互从简：点击复制分支名 或 跳转 BranchManager（二选一，实现时定并记录；不做下拉切换，避免与工作区切换唯一入口规则冲突——AGENTS.md 导航规范）。

## 验收标准

1. Git 视图下 StatusBar 显示当前仓库当前分支；切换仓库/切换分支后即时更新。
2. 非 Git 视图（Runtime 等）或无仓库上下文时槽位隐藏。
3. `pnpm build` 通过。

## 进度

| 日期 | 记录 |
|---|---|
| 2026-09-24 | 建任务：project-analysis 已知坑 + desktop-skin-plan 已规定行为。待修复。 |
