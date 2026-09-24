# GF-14 git 死代码清理（v-if=false 死块 / 死 api wrapper / 无引用组件）

> 状态：⬜ 未开始
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

- [ ] 1. 删除 `RepositoryList.vue:352-377` 死块及配套状态（`selectorQuery` / `quickChips` / `toggleChip` 确认无其他引用后）。
- [ ] 2. 按 GF-07 结论处理 sync_* 死 wrapper（接线或删除）。
- [ ] 3. 删除三个无引用组件（先 git log 确认无近期计划引用；如属规划中组件，移入 GF-20 备注而非删除）。
- [ ] 4. `pnpm build` + `cargo check` 绿。

## 验收标准

1. 删除项全部经 grep 确认零引用；构建绿。
2. 功能零回归（死代码本就不参与运行，以构建绿 + 冒烟主要 git 流程为准）。

## 进度

| 日期 | 记录 |
|---|---|
| 2026-09-24 | 建任务：死代码盘点（均已 grep 核验）。依赖 GF-07 对 sync_* wrapper 的处置结论。 |
