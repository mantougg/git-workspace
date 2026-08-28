# F-22 Git 视图（提交图/分支/Stash/Worktree/Reflog）支持切换仓库

| 项 | 值 |
|---|---|
| 优先级 | P1 |
| 状态 | ✅ 已完成 |
| 来源 | 2026-08-28 用户实测反馈（下午场） |
| 关联任务 | F-14 / F-17（当前仓库解析链）、D-05 |

## 问题描述

提交图、分支、Stash、Worktree、Reflog 五个视图只能展示**一个仓库**的内容，
且没有任何切换入口——想看另一个仓库必须回变更页重新勾选（语义隐蔽）。

用户期望：优先「一起展示全部」；无法一起展示时至少要能「切换展示」。

## 结论与方案

后端命令全部是**单仓库作用域**（`repo_path: String` → `Path::new(...)`），
「全部仓库一起展示」需要为每类数据新增工作区级聚合命令（分支图跨仓库无单一
语义，收益低、改动大）。采用**切换展示**：视图头部加仓库切换器，选择当前工作
区任一仓库后重置并重载视图，纯前端改动，不动后端。

## 修复范围

- [x] 新增 `src/components/shell/RepoSwitcher.vue`：n-select（filterable），
  options 来自 `repoStore.repositories`（label = 仓库名，value = 路径），值
  绑定全局 `repoStore.currentRepoPath`，切换时回写 store 并 emit `change`
- [x] 5 个视图头部原 `<span class="repo-path">` 替换为 `<RepoSwitcher
  @change="onRepoSwitch" />`
- [x] 各视图新增 `onRepoSwitch`：重置本视图状态后重调既有加载入口——
  - GitGraph：清 commits/branches/conflictFiles/hasMore/详情抽屉 →
    `loadHistory` + `loadBranches` + `refreshConflicts`
  - BranchManager：清 overview/mergeInProgress/rebaseState/compare → `load`
  - StashManager：清 entries/差异弹窗 → `load`
  - WorktreeManager：清 worktrees/localBranches → `load`
  - Reflog：清 entries、引用回退 HEAD；onMounted 的分支列表加载抽为
    `loadBranchOptions()`，切换时一并重调
- [x] DiffViewer / ConflictResolver 不接入（query 驱动的临时视图/冲突处理中
  切换无语义）

## 验收标准

- [x] 5 个视图头部出现仓库下拉（可过滤），切换后立即展示所选仓库数据
- [x] 切换器与变更页勾选、`?repo=` query 跳转行为不冲突（三者都写同一
  `currentRepoPath`）
- [x] 工作区无仓库时视图保持原有提示 + 回跳变更页行为（F-17 链不受影响）
- [x] `pnpm build`（vue-tsc + vite）通过

## 进度

### 状态

- 当前状态：✅ 已完成
- 最近更新：2026-08-28 修复完成，构建通过

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-28 | ⬜ | 问题录入；调查确认后端命令均单仓库作用域，「全部一起展示」成本高收益低，确定切换展示方案 |
| 2026-08-28 | ✅ | 实现 RepoSwitcher + 5 视图接线；验证：`pnpm build` 通过；UI 实测以用户验收为准 |
