# T-14 Reflog

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[T-09 Branch Manager](./T-09-branch-manager.md)。

| 项 | 值 |
|---|---|
| 阶段 | Phase 1 · 完整 Git Client（P0） |
| 优先级 | P0 |
| 状态 | ⬜ 未开始 |
| 依赖 | T-09 |
| 对应 Roadmap | §11 Reflog |

## 目标

实现 Reflog 查看与恢复操作，降低 Reset / Rebase / 误删除等高风险操作的恢复成本。

## 需求范围

- [ ] 三种 reflog：HEAD / Branch / Remote
- [ ] 列表展示 `HEAD@{0} / HEAD@{1} / ...` 及对应 commit 摘要
- [ ] 操作：Create Branch Here / Reset Here / Restore State / View Commit
- [ ] Reset Here / Restore State 走 §46 Warning/Dangerous 确认
- [ ] 与 T-13 Reset 联动：reset 前提示可记录 reflog 位置

## 架构 / 性能注意点

- reflog 读取是本地轻量操作，按需分页加载即可，无需缓存。
- 恢复操作本质是 reset，复用 T-13 的危险确认与影响范围展示。

## 验收标准

- [ ] HEAD/Branch/Remote 三类 reflog 正确展示
- [ ] Create Branch Here / Reset Here 可用且可回退
- [ ] 误 reset 后可通过 reflog 恢复原状态

## 进度

### 状态

- 当前状态：未开始
- 最近更新：—

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| — | — | — |

### 子任务清单

- [ ] reflog 读取与列表 UI
- [ ] 恢复类操作与确认
- [ ] 与 T-13 联动
