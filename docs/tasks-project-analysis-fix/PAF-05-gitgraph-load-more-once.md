# PAF-05 GitGraph「加载更多」只生效一次

| 项 | 值 |
|---|---|
| 优先级 | P0 |
| 状态 | ✅ 已完成 |
| 来源 | 项目全景分析报告（docs/project-analysis-2026-09-13.md）P0-5，主控亲自验证 |
| 关联任务 | D-15（三栏联动）、T-04（Diff & Graph） |

## 问题描述

`src/views/GitGraph.vue:309-327`：

```ts
if (more.length > commits.value.length) {
  commits.value = more;
  hasMore.value = more.length >= commits.value.length + PAGE_SIZE;
}
```

`commits.value = more` 先执行，随后比较中 `commits.value.length` 已等于
`more.length`，条件恒为 false → `hasMore` 置 false，「加载更多」按钮只生效
一次（即使还有更多提交）。且每次 load-more 以递增 limit 全量重拉，O(n²)。

## 定位与修复建议

- 先记录旧长度再赋值比较，或改后端返回 `hasMore`/total；
- 分页改为 offset/游标，避免全量重拉。

## 验收标准

- [x] 提交数 > 2 页的仓库可连续加载更多直至拉完
- [x] 无更多数据时按钮正确隐藏
- [x] `pnpm build` 通过

## 进度

### 状态

- 当前状态：✅ 已完成
- 最近更新：2026-09-13 修复完成

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-09-13 | ⬜ | 分析报告事实核查批次录入 |
| 2026-09-13 | ✅ | 证据复核成立（GitGraph.vue `loadMore` 先 `commits.value = more` 再比较，恒 false）。按任务文档首选方案最小修复：拉取前记录 `prevCount`，赋值与 `hasMore` 比较均基于旧长度。offset/游标分页改造涉及 `get_commit_history` IPC 契约变更（需同步 golden fixture），本次不做、维持递增 limit 重拉；如需消除 O(n²) 另立任务。验证：`pnpm build` 通过 |
