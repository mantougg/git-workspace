# GF-11 git 长列表无虚拟滚动 + GitGraph「加载更多」全量重取（O(n²)）

> 状态：⬜ 未开始
> 优先级：P1
> 来源：2026-09-24 Git 使用体验全景盘点。

## 问题描述

两类叠加的性能问题：

1. **翻页成本 O(n²)**：GitGraph 的「加载更多」是 `get_commit_history(repoPath, prevCount + PAGE_SIZE)`
   ——每次翻页全量重拉并整体替换 `commits`。翻到第 k 页时，累计传输/解析量是 O(k²)。
2. **列表零虚拟滚动**：提交图（CommitGraph，每行 3~5 个 SVG line/path）、变更树
   （ChangeTree，主界面）、ChangeSet 列表、分支本地/远程列表、Reflog、Stash、
   冲突列表全是 `v-for` 直渲。千级提交 / 千文件工作区下明显卡顿。

项目里已有成熟虚拟列表组件（`src/components/common/VirtualList.vue`，diff 组件在用），
属「有轮子没用开」。

## 定位线索（证据）

- 全量重拉：`src/views/GitGraph.vue:315`（`prevCount + PAGE_SIZE`）
- CommitGraph 全量渲染：`src/components/graph/CommitGraph.vue:6-107`
- ChangeTree 无 virtual-scroll：`src/components/repo/ChangeTree.vue:3-18`（`n-tree` 未开 `virtual-scroll`）
- 其余全量 v-for：`src/views/ChangeSetView.vue:33`、`src/views/BranchManager.vue:90,:120,:137`、`src/views/Reflog.vue:22`、`src/views/StashManager.vue:27`、`src/views/ConflictResolver.vue:35,:64`
- 后端已有缓存基础：`src-tauri/src/commands/graph.rs:56-76`（SQLite commit 元数据缓存，命中免 `find_commit`）——加分页参数后缓存命中路径不受影响。

## 修复范围 checklist

- [ ] 1. 后端 `get_commit_history` 增 `offset/limit`（或 cursor）分页参数；前端 GitGraph 翻页改增量追加。
- [ ] 2. CommitGraph 接 VirtualList（固定行高，参照 diff 组件用法）。
- [ ] 3. ChangeTree `n-tree` 开 `virtual-scroll`。
- [ ] 4. 其余长列表按仓库规模排优先级：分支列表（>100 分支仓库常见）→ 冲突列表 → 其余。

## 不做（范围控制）

- 不一次性重写所有列表（分批，先在进度文档记录排序与理由）。
- 不改提交图 lazy heap walk 算法本身（已优化过，见 project-analysis）。

## 验收标准

1. 千 commit 仓库：翻到第 10 页时历史请求的累计数据量与页数线性相关（DevTools Network 核对），DOM 行数恒定。
2. 主界面千文件变更树滚动流畅（帧率对比：修复前 `startFrameMeter` 采样 vs 修复后）。
3. 既有提交图/变更树相关单测无回归；`pnpm build` 通过。

## 进度

| 日期 | 记录 |
|---|---|
| 2026-09-24 | 建任务：体验盘点发现（`:315` 全量重拉 + 全路径零虚拟滚动）。待复现与修复。 |
