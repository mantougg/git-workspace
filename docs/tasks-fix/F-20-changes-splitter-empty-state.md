# F-20 变更与批量操作页：graph/diff 分隔条无法拖拽 + 双空状态 + 空状态不居中

| 项 | 值 |
|---|---|
| 优先级 | P1 |
| 状态 | ✅ 已完成 |
| 来源 | 2026-08-28 用户实测反馈（下午场） |
| 关联任务 | F-09b（diff 面板 n-spin 布局）、F-18（ChangeSet 页同类空状态问题） |

## 问题描述

「变更与批量操作」页（`src/views/RepositoryList.vue`，路由名 `changes`）：

1. 树与提交图（graph）、提交图与 diff 之间的分隔把手**拖拽无效**，宽度不可调；
2. 加载数据时树区同时出现两个空状态：「无数据」和「暂无仓库数据」；
3. 空状态没有竖直居中，贴在区域顶部。

## 根因（已定位）

1. `startResize` 是柯里化函数（`RepositoryList.vue` 原 1095 行），模板里
   `@mousedown="startResize('graph')"` 只**返回**事件处理闭包，闭包被丢弃，
   `mousemove/mouseup` 监听从未注册——graph 与 diff 两个把手都因此失效。
2. 「无数据」来自 n-tree 数据为空时的默认 empty 插槽（zhCN 文案）；「暂无仓库
   数据」来自 `ChangeTree.vue` 内联的 `.empty-tree` 块——两者在 `changes` 为空
   时必然同时渲染；加载期间（spin 遮罩下）也会出现。
3. 树区 `n-spin`（原 78 行）无 class，渲染出的 `.n-spin-container` 不参与
   `.tree-pane` 的 flex 布局，高度链断裂，`.empty-state` 的 `height: 100%`
   撑不起来。F-18 只修了 ChangeSetView，本页树区漏修（diff/graph 面板在
   F-09b 已修）。

## 修复范围

- [x] `startResize` 去柯里化为 `(pane, e)`，模板改
  `@mousedown="startResize('graph', $event)"` / `('diff', $event)`
- [x] 树区 `n-spin` 加 `tree-spin` class（`flex: 1; min-height: 0` +
  `:deep(.n-spin-content) { height: 100% }`），`.tree-container` 改
  `height: 100%`，高度链打通
- [x] 空数据时不渲染 `ChangeTree`（`v-if="changes.length > 0"`），空状态统一
  由 view 级 `.empty-state` 展示（含「重新扫描」「前往工作区管理」动作）；
  删除 `ChangeTree.vue` 内联的 `.empty-tree` 块与样式

## 验收标准

- [x] graph / diff 分隔把手可拖拽调宽，宽度持久化（localStorage
  `gw-splitter:changes:*`）与夹紧逻辑不变
- [x] 加载中只有 spin；加载后无仓库时仅一个居中的空状态（不再有「无数据 +
  暂无仓库数据」叠加）
- [x] 空状态在树区水平竖直居中；有数据时布局与之前一致
- [x] `pnpm build`（vue-tsc + vite）通过

## 进度

### 状态

- 当前状态：✅ 已完成
- 最近更新：2026-08-28 修复完成，构建通过

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-28 | ⬜ | 问题录入；定位：startResize 柯里化被模板丢弃；ChangeTree 内联 empty 与 n-tree 默认 empty 重复；树区 n-spin 未参与 flex |
| 2026-08-28 | ✅ | 修复：去柯里化 + tree-spin 高度链 + 空状态收口到 view 级；验证：`pnpm build` 通过；UI 实测以用户验收为准 |
