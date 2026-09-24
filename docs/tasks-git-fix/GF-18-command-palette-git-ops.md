# GF-18 命令面板/快捷键未覆盖已有 git 操作（cherry-pick/merge/rebase/stash/worktree/repo-tools）

> 状态：⬜ 未开始
> 优先级：P2
> 来源：2026-09-24 Git 使用体验全景盘点。

## 问题描述

命令注册表目前覆盖的 git 命令以「导航 + 少量动作」为主：fetch/pull/push/commit/sync/
branch-create + 纯路由跳转（branch/stash/reset/reflog/worktree）+ ai-review。大量**已有
UI 的 git 操作没有命令注册**，无法从命令面板触达、也无法绑快捷键：

- Cherry-pick / Revert / Reset（GitGraph 有）
- Merge / Rebase / Smart Pull（BranchManager 有）
- Stash save/apply/pop/drop、Worktree create/remove
- Repo Tools（submodule/LFS/hooks）
- Create PR / CI 状态

desktop-skin-plan.md:350-351 明确「命令与快捷键统一走命令注册表，禁止视图内各自绑定；
命令面板只编排已有能力，不新增业务逻辑」——当前状态与规范有差距。

## 定位线索（证据）

- 现有 git 命令：`src/commands/registry.ts:115-206`（`getGitCommands`）
- 动作源：`src/views/GitGraph.vue`（cherry-pick/revert/reset，含右键菜单 `:391-404`）、
  `src/views/BranchManager.vue`（merge/rebase/smart-pull）、`src/views/StashManager.vue`、
  `src/views/WorktreeManager.vue`、`src/views/RepoToolsView.vue`
- 规范依据：[../desktop-skin-plan.md:350-351](../desktop-skin-plan.md)、:391
- 与 GF-03 的边界：GF-03 修「nav:false 路由导致 diff/冲突不可达」，本任务补「有 UI 无命令」的操作编排。

## 修复范围 checklist

- [ ] 1. 为上述操作注册命令（作用于「当前仓库」上下文，复用现有 store/当前 repo 状态；无当前仓库时命令置灰或提示选择仓库）。
- [ ] 2. 给高频操作绑默认快捷键（不与既有冲突；清单见 `src/commands/shortcuts.ts:14-35`）。
- [ ] 3. 命令实现只做编排调用（调现有 api + 刷新），不在注册表里写新业务逻辑。

## 验收标准

1. 命令面板可搜到并执行：cherry-pick、merge、rebase、stash save/apply/pop、worktree create、submodule/LFS/hooks 操作（作用于当前仓库）。
2. 新快捷键无冲突、无死键（keys 唯一性走现有注册校验）。
3. `pnpm build` 通过。

## 进度

| 日期 | 记录 |
|---|---|
| 2026-09-24 | 建任务：体验盘点发现（registry.ts:115-206 覆盖 vs 全量 UI 动作差距）。待修复。 |
