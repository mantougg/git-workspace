# T-21 Workspace Stash & Branch

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[T-09 Branch Manager](./T-09-branch-manager.md)、[T-10 Stash](./T-10-stash.md)。

| 项 | 值 |
|---|---|
| 阶段 | Phase 2 · Multi-Repo Engine（P1） |
| 优先级 | P1 |
| 状态 | ⬜ 未开始 |
| 依赖 | T-09, T-10 |
| 对应 Roadmap | §7 Workspace Stash、§16 Create Branch All / Delete Branch All |

## 目标

实现多仓库级 Stash 与 Branch 编排：一次暂存/恢复整组仓库，一次在整组仓库创建/删除同名分支。

## 需求范围

- [ ] Workspace Stash：选定仓库组 → 保存为 `Workspace Stash #N`（记录各仓库 stash 关联）
- [ ] Restore Workspace：按记录恢复整组仓库
- [ ] Workspace Branch：Create Branch All / Delete Branch All（选定组、同名分支）
- [ ] 落库 `change_sets` / 或独立 `workspace_stashes` 表关联各仓库 stash
- [ ] 与 T-20 选择器复用（按组/标签/状态选仓库）

## 架构 / 性能注意点

- Workspace Stash 本质是「逐仓库 stash + 关联记录」，执行走 T-05 任务队列并发限流；恢复前必须校验每个仓库当前状态可安全恢复。
- Restore 是危险操作（可能覆盖工作区），§46 Warning 级确认并列出影响仓库。

## 验收标准

- [ ] 一次 Stash 能覆盖选定组全部仓库且生成关联记录
- [ ] Restore 完整还原整组仓库工作区
- [ ] Create/Delete Branch All 在整组生效，部分失败可定位
- [ ] 恢复操作有确认且影响范围清晰

## 进度

### 状态

- 当前状态：未开始
- 最近更新：—

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| — | — | — |

### 子任务清单

- [ ] Workspace Stash 数据模型
- [ ] 多仓库 stash/restore 编排
- [ ] 批量建/删分支
- [ ] 与 T-20 选择器联动
