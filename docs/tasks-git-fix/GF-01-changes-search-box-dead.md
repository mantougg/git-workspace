# GF-01 主界面搜索框失效（绑定了 state，全文无过滤逻辑）

> 状态：✅ 已完成
> 优先级：P0（用户打开应用第一眼就会试的功能，输入 100% 无响应）
> 来源：2026-09-24 Git 使用体验全景盘点。

## 问题描述

变更与批量操作页（RepositoryList）顶部有搜索框，绑定了 `searchQuery`，
但**全文没有任何地方消费它**——用户在搜索框打字完全无反应，既不报错也不过滤。

## 定位线索（证据）

- 绑定：`src/views/RepositoryList.vue:34`（`v-model:value="searchQuery"`）
- 定义：`src/views/RepositoryList.vue:860`（`const searchQuery = ref("")`）
- 全文检索 `searchQuery` 仅以上两处，无过滤 / 无 computed / 无 watch。
- **加重因素（2026-09-24 核验）**：命令面板的 `action:repo-search`（`src/commands/registry.ts:94-103`，
  绑定 Ctrl+Shift+F）执行 `router.push({ name: "changes", query: { focus: "search" } })`——
  即命令面板专门把用户带到这个搜索框上聚焦，而搜索框本身是死的：一条公开入口指向失灵控件。
- **复现结论（2026-09-24）**：成立。`searchQuery` 仅绑定与定义两处；`searchInputRef` +
  `route.query.focus === "search"` 的聚焦链路（T-31）已能工作，聚焦后输入零响应坐实。

## 修复范围 checklist

- [x] 1. 实现过滤：至少覆盖仓库名与路径；建议同时支持当前分支名、状态关键词（脏/领先/落后）。
- [x] 2. 目录树与平铺两种展示模式都生效（树模式过滤时注意分组/父节点的呈现，勿出现空分组）。
- [x] 3. 无匹配结果显示 `n-empty` 空态；清空输入恢复全量。
- [x] 4. 与扫描进度/刷新流程兼容：过滤词在刷新后保持。

## 实现说明（2026-09-24）

- 过滤策略：新增 `filteredChanges` computed（`RepositoryList.vue`，`totalChangedFiles` 之后）。
  仓库级字段（`repoName` / `relativePath` / 归一化 `repoPath` / `branch` / 游离 / 脏 /
  ↑ahead ↓behind 数量）命中时保留该仓库**全部**变更文件；仅文件路径命中时只带入命中文件
  （`{ ...repo, changes }` 浅拷贝，不改动原数据）。全部 `includes` 大小写不敏感匹配。
- 树模式无空分组：过滤只裁剪 `changes` 数据本身，ChangeTree 的 `buildTree` 对裁剪后数据
  自然成树（无子节点的分组不会出现——文件全被过滤掉时该分组整支消失）。
- 模板改动：ChangeTree 的 `:changes` 改绑 `filteredChanges`；在既有「未发现任何 Git 仓库」
  空态之后新增 `v-else-if`「没有匹配的仓库或文件」空态（带「清除搜索词」按钮）；
  Push 仓库选择弹窗的 `n-data-table :data` 同源改绑 `filteredChanges`。
  统计条 / 批量操作 / 勾选逻辑仍用全量 `changes`，不受过滤词影响。
- 刷新保持：`searchQuery` 为独立 ref，`loadChanges` 只替换 `changes.value`，
  computed 自动重算，过滤词天然保持。

## 不做（范围控制）

- 不做全局跨视图搜索（符号搜索已有 SymbolSearchView，另行规划）。
- 不引入模糊匹配库，`includes` 大小写不敏感匹配即可。

## 验收标准

1. 输入仓库名关键字：列表只显示匹配仓库；输入分支名：显示该分支相关仓库（含未匹配时的空态）。
2. 清空搜索词列表恢复全量，刷新后搜索词仍生效。
3. `pnpm build`（vue-tsc + vite）通过。

## 进度

| 日期 | 记录 |
|---|---|
| 2026-09-24 | 建任务：体验盘点发现（静态证据：`:34` 绑定 + `:860` 定义，全文零消费）。待复现与修复。 |
| 2026-09-24 | 复现结论：成立（searchQuery 仅绑定+定义；focus 聚焦链路 T-31 已在，输入零响应）。修复：新增 `filteredChanges` computed（仓库级命中带全部文件 / 文件级命中只带命中文件 / 大小写不敏感 includes），ChangeTree 与 Push 选择弹窗改绑该 computed，补「没有匹配的仓库或文件」空态（含清除按钮）。验证：`pnpm build`（vue-tsc + vite）通过；detect_changes risk=low 无受影响执行流。状态 → ✅。 |
