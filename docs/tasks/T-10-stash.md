# T-10 Stash

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[T-02 Status Engine](./T-02-status-engine.md)。

| 项 | 值 |
|---|---|
| 阶段 | Phase 1 · 完整 Git Client（P0） |
| 优先级 | P0 |
| 状态 | ✅ 已完成 |
| 依赖 | T-02 |
| 对应 Roadmap | §7 Stash、§41 schema（stashes） |

## 目标

实现单仓库 Stash 完整能力（保存/应用/弹出/丢弃/清空/查看 diff/从 stash 建分支）。

## 需求范围

- [x] Stash Changes / Stash Including Untracked
- [x] Apply / Pop / Drop / Clear
- [x] Show Diff（复用 T-04 Diff 查看器）
- [x] Create Branch From Stash
- [x] Stash 列表与元数据（时间、message、包含仓库）落库 `stashes`
- [x] Drop / Clear 走 §46 Warning 级确认

## 架构 / 性能注意点

- stash 的 diff 展示复用 diff 缓存（T-04），stash 数据量通常小，无需额外性能设计。
- Workspace 级 Stash（多仓库一次性暂存）属于 T-21，本任务只做单仓库，但数据模型预留 workspace stash 关联字段。

## 验收标准

- [x] 六类操作（含 include-untracked）全部可用且结果正确
- [x] Show Diff 正确显示 stash 内容
- [x] Drop / Clear 有二次确认，误操作可感知
- [x] stash 列表持久化，重启后仍可见

## 进度

### 状态

- 当前状态：已完成
- 最近更新：2026-08-14 完成

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-14 | 🟦 | 开始开发 |
| 2026-08-14 | ✅ | 完成：新增 `core/stash.rs`（libgit2 选型：stash_save2/apply/pop/drop/clear/branch_from_stash + stash_diff 走 `diff_revisions` 复用 T-04 截断）+ `commands/stash.rs` 8 个 command + 落库 `replace_stashes`（replace 式事务）+ schema V5（stashes 预留 `workspace_ref` 字段给 T-21）+ db 版本断言改为跟随 MIGRATIONS.len()（后续加迁移不再 churn 测试）；前端 `StashManager.vue`（列表 + 保存对话框含 include-untracked + Show Diff 复用 UnifiedDiff + Drop/Clear §46 Warning 确认）+ RepositoryList Stash 入口；4 个核心单元测试；IPC golden 登记 StashEntry；`cargo test` 60 passed、`vue-tsc` + `vite build` 通过 |

### 子任务清单

- [x] stash 操作 command（libgit2 / git CLI 选型）
- [x] stash 列表 UI 与元数据落库
- [x] Show Diff 集成
- [x] Create Branch From Stash
