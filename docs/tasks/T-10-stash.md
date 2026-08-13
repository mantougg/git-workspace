# T-10 Stash

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[T-02 Status Engine](./T-02-status-engine.md)。

| 项 | 值 |
|---|---|
| 阶段 | Phase 1 · 完整 Git Client（P0） |
| 优先级 | P0 |
| 状态 | ⬜ 未开始 |
| 依赖 | T-02 |
| 对应 Roadmap | §7 Stash、§41 schema（stashes） |

## 目标

实现单仓库 Stash 完整能力（保存/应用/弹出/丢弃/清空/查看 diff/从 stash 建分支）。

## 需求范围

- [ ] Stash Changes / Stash Including Untracked
- [ ] Apply / Pop / Drop / Clear
- [ ] Show Diff（复用 T-04 Diff 查看器）
- [ ] Create Branch From Stash
- [ ] Stash 列表与元数据（时间、message、包含仓库）落库 `stashes`
- [ ] Drop / Clear 走 §46 Warning 级确认

## 架构 / 性能注意点

- stash 的 diff 展示复用 diff 缓存（T-04），stash 数据量通常小，无需额外性能设计。
- Workspace 级 Stash（多仓库一次性暂存）属于 T-21，本任务只做单仓库，但数据模型预留 workspace stash 关联字段。

## 验收标准

- [ ] 六类操作（含 include-untracked）全部可用且结果正确
- [ ] Show Diff 正确显示 stash 内容
- [ ] Drop / Clear 有二次确认，误操作可感知
- [ ] stash 列表持久化，重启后仍可见

## 进度

### 状态

- 当前状态：未开始
- 最近更新：—

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| — | — | — |

### 子任务清单

- [ ] stash 操作 command（libgit2 / git CLI 选型）
- [ ] stash 列表 UI 与元数据落库
- [ ] Show Diff 集成
- [ ] Create Branch From Stash
