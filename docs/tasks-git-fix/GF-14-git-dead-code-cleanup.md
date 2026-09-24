# GF-14 git 死代码清理（v-if=false 死块 / 死 api wrapper / 无引用组件）

> 状态：✅ 已完成
> 优先级：P2
> 来源：2026-09-24 Git 使用体验全景盘点。

## 问题描述

三类死代码，均已被静态分析确认零引用：

1. **RepositoryList `v-if="false"` 死块**：仓库选择器 + quick chips 整段被条件禁用
   （F-09f 自认「用途不明、可用性存疑」），但 `selectorQuery/quickChips/toggleChip`
   状态与模板仍在，误导后续维护者。
2. **死 api wrapper**：`sync_fetch/sync_pull/sync_push` 三个 wrapper 全 `src/` 零调用
   （批量路径全走 `batch_*`；单仓路径被 `smart_pull` 取代）。注意：GF-07 若为单仓操作
   接线，则这三个 wrapper 是接线点而非死代码——**本任务与 GF-07 串行，GF-07 方案定稿
   后再决定删还是留**。
3. **无引用组件**：`BatchActionBar.vue`、`RepoCard.vue`、`RepoTable.vue` 三个仓库卡片/
   表格组件完全无引用。

## 定位线索（证据）

- 死块：`src/views/RepositoryList.vue:353-377`（`v-if="false"`，2026-09-24 核验起始行为 353）
- 死 wrapper：`src/api/git_ops.ts:26 / :30 / :38`
- 无引用组件：`src/components/repo/BatchActionBar.vue`、`src/components/repo/RepoCard.vue`、`src/components/repo/RepoTable.vue`（全 `src/` grep 无 import）

## 修复范围 checklist

- [x] 1. 删除 `RepositoryList.vue` `v-if="false"` 死块及配套状态（`selectorQuery` / `quickChips` / `toggleChip` 确认无其他引用后）。
- [x] 2. 按 GF-07 结论处理 sync_* 死 wrapper（接线或删除）。
- [x] 3. 删除三个无引用组件（先 git log 确认无近期计划引用；如属规划中组件，移入 GF-20 备注而非删除）。
- [x] 4. `pnpm build` + `cargo check` 绿。

## 复现结论与实现说明（2026-09-24）

1. **死块（成立，行号 353→361 漂移）**：`v-if="false"` batch-row = 选择器输入 + quick chips + 匹配计数。删除该 UI 块与 `.selector-input` / `.selector-count` CSS。
   **配套状态辨析（与原任务文档预判不同，如实记录）**：
   - `selectorQuery` / `selectorPaths` / `selectorActive` / debounce watch / `batchTargetRepos` **不是死代码**——`batchTargetRepos()`（`selectorActive` 命中时返回 `selectorPaths`）是 Workspace Stash 目标集合与批量分支操作的目标源；`applyRoutePrefill` 写入 `selectorQuery`（Dashboard 快捷操作 `?selector=@status:dirty` 等）。**保留**（原 F-09f 注释「后续决定去向」的答案：UI 隐藏、状态经路由 prefill 驱动）。
   - `quickChips` + `toggleChip` + watch 内的 chip 同步循环**仅服务死 UI**（template/函数零其他引用）——**已删**。
2. **sync_* wrapper（原预判被后续任务改变）**：GF-07 给 `sync_fetch/sync_pull/sync_push` 接上流式+取消后，GF-18 命令面板已实际调用 `syncFetch` / `syncPush`（`registry.ts` git:fetch-current / git:push-current）——**wrapper 是活的，保留不删**。GF-07 定稿结论「保留 wrapper 并接线」至此闭环。
3. **三个无引用组件（成立）**：`BatchActionBar.vue`（b233d98 smart-pull 批次动作条）、`RepoCard.vue` / `RepoTable.vue`（62572ef D-10 桌面化早期产物）全 `src/` grep 零引用；git log 无近期计划引用、不属 GF-20 规划项——**已删**（`git rm`）。
4. 验证：`pnpm build`（vue-tsc --noEmit + vite build）通过；`cargo check` 无本次相关改动（纯前端任务，后端未动）。功能零回归：删除项均不参与运行；`batchTargetRepos`/prefill 选择器链路经 build + 引用核对保留。

## 验收标准

1. 删除项全部经 grep 确认零引用（quickChips/toggleChip 死 UI 链 + 三组件）；构建绿。
2. 功能零回归（活状态 selectorQuery 链路保留，Dashboard prefill → batchTargetRepos 不断）。

## 进度

| 日期 | 记录 |
|---|---|
| 2026-09-24 | 建任务：死代码盘点（均已 grep 核验）。依赖 GF-07 对 sync_* wrapper 的处置结论。 |
| 2026-09-24 | 复现+修复：死块 UI/quickChips/toggleChip/selector CSS 删；selectorQuery 活状态辨析后保留（prefill+batchTargetRepos 消费）；sync_* wrapper 因 GF-18 接线转活保留；三组件 git log 确认无规划引用后 git rm。验证：`pnpm build` 通过。状态 → ✅。 |
