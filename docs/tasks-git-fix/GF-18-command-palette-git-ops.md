# GF-18 命令面板/快捷键未覆盖已有 git 操作（cherry-pick/merge/rebase/stash/worktree/repo-tools）

> 状态：✅ 已完成
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

- [x] 1. 为上述操作注册命令（作用于「当前仓库」上下文，复用现有 store/当前 repo 状态；无当前仓库时命令置灰或提示选择仓库）。
- [x] 2. 给高频操作绑默认快捷键（不与既有冲突；清单见 `src/commands/shortcuts.ts:14-35`）。
- [x] 3. 命令实现只做编排调用（调现有 api + 刷新），不在注册表里写新业务逻辑。

## 实现说明（2026-09-24）

- **复现结论**：核实既有注册表已含纯导航命令（`git:branch`/`git:stash`/`git:worktree`/`git:reset`（标题含 Cherry-pick/Revert）/`git:reflog`，`nav:repo-tools` 由路由自动生成）；缺口是**作用于当前仓库的直接操作命令**与**自包含对话框的唤起入口**。
- 实现分为两类（均只编排，不写业务逻辑）：
  1. **单仓直接操作**（`requireCurrentRepo` 守卫，无仓库抛错由命令面板展示；网络操作走 GF-07 流式镜像，Git Console 自动弹出）：
     - `git:fetch-current`（sync_fetch）、`git:pull-current`（smart_pull——conflict 态自动跳 conflict-resolver，仓库已留在冲突态）、`git:push-current`（sync_push）。
  2. **自包含对话框 prefill 打开**（视图保留全部业务逻辑/确认流；沿用 changes 页 `?action=` 既有预填模式）：
     - `git:stash-save` → stash-manager `?save=1`（打开新建 stash 对话框：message + includeUntracked）；
     - `git:create-pr` → branch-manager `?pr=1`（`openCreatePr()`，load 后调用使分支选项/远程信息就绪，含 token 预检）；
     - `git:rebase` → branch-manager `?rebase=1`（RebaseDialog）；
     - `git:worktree-create` → worktree-manager `?create=1`（新建 Worktree 对话框）。
- **刻意不做直接命令的操作**（需在视图中选择目标，属交互流）：merge（需选源分支，`git:branch` 导航）、cherry-pick/revert/reset（需选提交，`git:reset` 导航，标题已含）、stash apply/pop/drop（需选 stash index，`git:stash` 导航）、submodule/LFS/hooks（repo-tools 页内 tab，`nav:repo-tools` 自动注册已可达）。任务文档「冲突解决器」等由 GF-03 已注册（`git:diff`/`git:open-conflict-resolver`）。
- 快捷键：`Shift+Alt+F`（fetch 当前仓库）、`Shift+Alt+P`（pull 当前仓库）。选型理由：Ctrl+Shift+P 已被命令面板占用，Ctrl+Shift+O/T/W/N/R/Del 为浏览器/系统保留，Ctrl+Shift+C/V 终端占用；Shift+Alt 系 WebView 几乎不保留。**push 刻意不绑快捷键**（写操作保持显式触发）。唯一性人工核对 SHORTCUT_MAP 全表无冲突。
- 视图改动：StashManager/BranchManager/WorktreeManager 各加 onMounted prefill（load 之后执行）+ useRoute；合计 <15 行/视图。

## 验收标准

1. 命令面板可搜到并执行：cherry-pick、merge、rebase、stash save/apply/pop、worktree create、submodule/LFS/hooks 操作（作用于当前仓库）。
2. 新快捷键无冲突、无死键（keys 唯一性走现有注册校验）。
3. `pnpm build` 通过。

## 进度

| 日期 | 记录 |
|---|---|
| 2026-09-24 | 建任务：体验盘点发现（registry.ts:115-206 覆盖 vs 全量 UI 动作差距）。待修复。 |
| 2026-09-24 | 复现：既有纯导航命令已覆盖 merge/cherry-pick/reset/stash/worktree/repo-tools 的「到达」；缺口为当前仓库直接操作与自包含对话框唤起。修复：registry 新增 fetch/pull/push-current（GF-07 流式镜像，smart_pull 冲突自动跳解决器）、stash-save/create-pr/rebase/worktree-create prefill；视图 prefill 落 StashManager/BranchManager/WorktreeManager；快捷键 Shift+Alt+F / Shift+Alt+P（push 刻意不绑）。验证：`pnpm build`（vue-tsc+vite）通过；快捷键唯一性人工核对。状态 → ✅。 |
