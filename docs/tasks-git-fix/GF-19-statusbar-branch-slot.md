# GF-19 StatusBar 分支槽位空占位（Git 视图不显示当前分支）

> 状态：✅ 已完成
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

- [x] 1. 接当前仓库 + 当前分支：从现有当前仓库状态读取，watcher 事件 / checkout 后刷新。
- [x] 2. 按 desktop-skin-plan:211：仅在 Git 类视图显示；无当前仓库上下文时隐藏槽位（不显示占位）。
- [x] 3. 交互从简：点击复制分支名 或 跳转 BranchManager（二选一，实现时定并记录；不做下拉切换，避免与工作区切换唯一入口规则冲突——AGENTS.md 导航规范）。

## 实现说明（2026-09-24）

- 数据源：`repoStore`（F-14 全局当前仓库 `currentRepoPath` + `repositories[].status.branch`）。列表可信性守卫：`repositoriesWorkspaceId` 必须等于当前工作区 id（防切换工作区后用到旧仓库路径）；路径匹配两侧归一化分隔符（AGENTS.md 平台规范）。
- 数据保鲜：StatusBar 自监听 `repo_status_changed_batch` → `repoStore.updateStatus`（此前只有变更页/总览页监听，Git 类视图上分支切换后 store 会陈旧）；当前仓库不在已加载列表时（SideNav 直达等）对 `currentRepoPath` 一次性 `refreshRepositoryStatus` 兜底（watch immediate），失败静默（常驻组件不弹错误 toast）。
- 视图判定：`isGitView` = `route.meta.group === "Git"` 或路由名 ∈ {changes, diff-viewer, conflict-resolver}（三个 repo 上下文任务页）。非 Git 视图（Runtime/设置）或无上下文 → 槽位隐藏。
- 游离 HEAD：`status.isDetached` 显示「HEAD 游离」（与 BranchManager/HealthView 文案一致）。
- 交互：**点击复制分支名**（不跳 BranchManager——避免与「工作区切换唯一入口在 StatusBar」产生第二个跳转语义混淆），复制成功 `message.success` 反馈。

## 验收标准

1. Git 视图下 StatusBar 显示当前仓库当前分支；切换仓库/切换分支后即时更新。
2. 非 Git 视图（Runtime 等）或无仓库上下文时槽位隐藏。
3. `pnpm build` 通过。

## 进度

| 日期 | 记录 |
|---|---|
| 2026-09-24 | 建任务：project-analysis 已知坑 + desktop-skin-plan 已规定行为。待修复。 |
| 2026-09-24 | 复现成立（`currentBranch = ref(null)` 恒空，模板 v-if 永不显示）。修复：StatusBar 接 repoStore（workspace 守卫 + 路径归一化），自监听 repo_status_changed_batch 保鲜，SideNav 直达场景 refreshRepositoryStatus 兜底；isGitView 限定 Git 类视图；点击复制分支名。验证：`pnpm build`（vue-tsc + vite）通过。状态 → ✅。 |
