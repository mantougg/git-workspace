# GF-12 SmartMergeDialog 丢失冲突语义（baseOid 未用 / conflictType 硬编码）

> 状态：⬜ 未开始
> 优先级：P2
> 来源：2026-09-24 Git 使用体验全景盘点。

## 问题描述

智能合并对话框（Smart Pull / merge 中断恢复用）有两个语义丢失缺陷：

1. `baseOid` prop 声明后**从未使用**——三路合并的 base 信息拿着却没用上。
2. `conflictType` 被**硬编码为 `"both-modified"`**——deleted-by-us / deleted-by-them /
   both-added 等冲突类型全部退化成同一种，图标分支（已写好）永远不触发。用户看不出
   「对方删除了文件」和「普通内容冲突」的区别，而这两者的正确处理方式完全不同。

## 定位线索（证据）

- `src/components/git/SmartMergeDialog.vue:164`（`baseOid` prop 唯一定义处，零消费）
- `src/components/git/SmartMergeDialog.vue:184`（`conflictType` 硬编码 `"both-modified"`）
- 死分支图标：`src/components/git/SmartMergeDialog.vue:268-276`（按类型渲染，因硬编码永不触发）
- 数据源侧：`src-tauri/src/commands/conflict.rs:17-42`（`get_operation_state` / 三路内容）与
  `src-tauri/src/commands/history.rs:76`（`get_conflict_files` 返回 `PickOutcome` 冲突态带 files + base_oid）——真实类型信息在后端已有，需确认逐文件类型是否可得。

## 修复范围 checklist

- [ ] 1. 透传真实 `conflictType`：确认后端可提供的粒度（文件级 or 全局），前端按类型渲染图标与推荐动作（deleted-by-us/them 给 Use Ours/Use Theirs 的默认推荐）。
- [ ] 2. `baseOid` 要么用于三路合并 base 展示（OURS/THEIRS 旁显示 BASE），要么删除该 prop（二选一，实现时定并记录）。
- [ ] 3. both-modified 行为与现有测试零回归。

## 不做（范围控制）

- 不扩展新冲突操作（如「保留双方」的默认策略）。
- 不改后端冲突检测算法。

## 验收标准

1. 构造 delete-by-us / delete-by-them 冲突：解决器显示对应类型图标与正确的默认推荐动作。
2. both-modified 冲突行为与修复前一致。
3. `pnpm build` 通过。

## 进度

| 日期 | 记录 |
|---|---|
| 2026-09-24 | 建任务：体验盘点发现（两处语义丢失，均已静态核验）。待复现与修复。 |
