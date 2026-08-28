# F-17 Git 视图「未指定仓库路径」复现（当前仓库缺少工作区兜底）

| 项 | 值 |
|---|---|
| 优先级 | P0 |
| 状态 | 🟦 修复中 |
| 来源 | 2026-08-28 用户实测反馈问题 1 |
| 关联任务 | F-14（前序修复）、D-05 |

## 问题描述

从 SideNav 点击提交图 / 分支 / Stash / Worktree / Reflog 菜单时，弹出
「未指定仓库路径」警告并跳回变更页，这些页面不可用。

## 根因（已定位）

F-14 的回退链为 `route.query.repo` → `repoStore.currentRepoPath`，而
`currentRepoPath` **只在变更页勾选树节点 checkbox 时写入**
（`RepositoryList.vue` watch `selectedRepoPath`）。该状态是内存态：

- 应用重启后 store 为空，用户直接从 SideNav 进 Git 视图必现警告；
- 用户未在变更页勾选（勾选语义隐蔽，单击仓库行不算）时同样必现。

即「当前仓库」缺少最后一级兜底——当前工作区明明有仓库，却不做任何
自动选择直接死路一条。

## 修复范围

- [ ] 新增 `src/composables/useCurrentRepo.ts`：解析顺序
  `route.query.repo` → `repoStore.currentRepoPath` → 当前工作区仓库列表
  第一个（store 列表为空时先 `loadRepositories` 拉取），解析成功回写
  store；工作区无仓库才返回空
- [ ] 六个 Git 视图（GitGraph / BranchManager / StashManager /
  WorktreeManager / Reflog / DiffViewer）的 `onMounted` 改用该 composable
- [ ] 工作区确实无仓库时保持原有提示 + 回跳变更页行为

## 验收标准

- [ ] 应用重启后未勾选任何仓库，直接从 SideNav 进提交图/分支/Stash/
  Worktree/Reflog，自动展示当前工作区第一个仓库的数据，无警告
- [ ] 变更页勾选仓库后再进 Git 视图，展示的仍是勾选的仓库（既有语义不变）
- [ ] 带 `?repo=` query 跳转的行为不变（优先级最高）
- [ ] 工作区没有任何仓库时仍给出提示（不崩）

## 进度

### 状态

- 当前状态：🟦 修复中
- 最近更新：2026-08-28 开始修复

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-28 | ⬜ | 问题录入；定位：F-14 回退链缺工作区级兜底，`currentRepoPath` 依赖变更页勾选且为内存态 |
| 2026-08-28 | 🟦 | 开始修复 |
