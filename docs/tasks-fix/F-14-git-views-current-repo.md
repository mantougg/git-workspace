# F-14 Git 视图「未指定仓库路径」（当前仓库全局状态缺失）

| 项 | 值 |
|---|---|
| 优先级 | P0 |
| 状态 | ✅ 已完成（2026-08-28） |
| 来源 | 2026-08-28 用户实测反馈问题 1 |
| 关联任务 | D-05 导航清理、F-02 |

## 问题描述

Git 分组的所有视图（提交图 / 分支 / Stash / Worktree / Reflog）从 SideNav
进入时都弹「未指定仓库路径」警告并跳回变更页，等于这些页面全部不可用。

## 根因（已定位）

这些视图的 `repoPath` **只**从 `route.query.repo` 读取（如
`src/views/GitGraph.vue:212`、`BranchManager.vue:354`、`StashManager.vue:135`、
`WorktreeManager.vue:177`、`Reflog.vue:132`、`DiffViewer.vue:312`）。
Desktop Shell 改造（ca47b39）把 Git 视图提升为 SideNav 一级导航，但
SideNav 跳转（`SideNav.vue:13`）与快捷键命令（`commands/registry.ts`）都
不带 `repo` query——传参链断了。「当前仓库」的全局状态从未存在过
（`src/stores/repository.ts` 只有列表无选中态）。

## 修复范围

- [x] `src/stores/repository.ts` 新增全局 `currentRepoPath` 状态
- [x] `RepositoryList.vue` 选中仓库变化时同步写入 store（保持「变更页勾选
  的仓库 = 全局当前仓库」语义）；既有 `viewXxx` 带 query 跳转保持不变
- [x] 各 Git 视图 repo 解析顺序改为：`route.query.repo` → store
  `currentRepoPath`，解析成功回写 store；两者皆无才 warning
- [x] DiffViewer 保持任务页语义（commit/base/other 仍走 query），仅 repo
  参数补 store 回落

## 验收标准

- [x] 在变更页勾选仓库后，从 SideNav 直接进入提交图/分支/Stash/Worktree/
  Reflog 均能展示该仓库数据，无警告
- [x] 从变更页点按钮带 query 跳转的行为与之前一致
- [x] 未选任何仓库时进入 Git 视图仍给出提示（不崩）

## 进度

### 状态

- 当前状态：✅ 已完成
- 最近更新：2026-08-28 修复完成并实测验证

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-28 | ⬜ | 问题录入；定位：Git 视图只认 route.query.repo，SideNav/快捷键不带 query，全局「当前仓库」状态从未存在 |
| 2026-08-28 | ✅ | 修复：`stores/repository.ts` 新增 `currentRepoPath`；`RepositoryList.vue` watch 选中态同步写入；GitGraph/BranchManager/StashManager/WorktreeManager/Reflog/DiffViewer 六个视图改为 query 优先 → store 回落并回写。验证（安装包 CDP 实测）：变更页勾选仓库 → SideNav 直达提交图，无 query 即加载该仓库分支/提交，无警告不跳转；未选仓库时仍提示并回跳变更页。`pnpm build` 通过 |
