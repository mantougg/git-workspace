# GF-23 RepositoryList 侧栏提交图预览面板高度链断裂（裁切不可滚动）

> 状态：⬜ 未开始
> 优先级：P2
> 来源：GF-11 审核牵出（2026-09-24）。

## 问题描述

RepositoryList 中栏「提交图预览面板」（D-15/D-16，`graphRepoPath` 非空时显示）的
`n-spin` 容器 `.graph-pane-spin` **缺 `:deep(.n-spin-content){height:100%}`**——
同一模式在 tree 面板（F-20）与 diff 面板（F-09b）都已修复，唯独 graph 面板漏掉。
后果：预览面板内 CommitGraph 的高度链断裂，spin 内容塌陷为内容高度，被
`overflow:hidden` 裁切且不可滚动——提交图预览只能看到顶部几行。

## 定位线索（证据）

- `.graph-pane-spin` 缺高度链：`src/views/RepositoryList.vue` graph-pane 段
  （GF-11 审核时静态核对；对照 `.tree-spin` / `.diff-pane-spin` 的
  `:deep(.n-spin-content){height:100%}` 修复模式，F-20/F-09b）
- 参照修复：`src/views/RepositoryList.vue` 的 `.tree-spin`、`.diff-pane-spin`
- 被裁切内容：`src/components/graph/CommitGraph.vue`（GF-11 起VirtualList 定高窗口，
  容器无确定高度时窗口测量为 0/内容高度）

## 修复范围 checklist

- [ ] 1. `.graph-pane-spin` 补 `:deep(.n-spin-content){height:100%}`（对齐 tree/diff 面板模式）。
- [ ] 2. 核对 `.graph-pane` / `.graph-pane-spin` 高度链到面板根，预览面板内提交图可滚动。
- [ ] 3. `pnpm build` 通过；主界面提交图预览面板人工抽检（滚动 + 加载更多仍在位）。

## 验收标准

1. 预览面板内千 commit 提交图可滚动查看，底部 footer（已加载 N 条）可见。
2. tree/diff 面板行为无回归。
3. `pnpm build` 通过。

## 进度

| 日期 | 记录 |
|---|---|
| 2026-09-24 | 建任务：GF-11 审核提交图预览面板时发现（`.graph-pane-spin` 缺高度链，同模式 tree/diff 已修）。待修复。 |
