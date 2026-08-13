# T-22 Workspace Change Set

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[T-09 Branch Manager](./T-09-branch-manager.md)、[T-20 Batch Operations 增强](./T-20-batch-ops.md)。

| 项 | 值 |
|---|---|
| 阶段 | Phase 2 · Multi-Repo Engine（P1） |
| 优先级 | P1 |
| 状态 | ⬜ 未开始 |
| 依赖 | T-09, T-20 |
| 对应 Roadmap | §17 Workspace Change Set、§41 schema（change_sets / change_set_repositories） |

## 目标

实现 Workspace Change Set——跨仓库关联同一特性的变更集合，统一展示与批量操作，是产品核心差异化功能。

## 需求范围

- [ ] 创建 Change Set（如 `Feature: AI Review`）
- [ ] 关联多个仓库（repo-a → feature/ai-review 等）
- [ ] 统一汇总：Repositories / Files / Added / Deleted / Commits
- [ ] 操作：View All Diff / AI Review / Commit All / Push All / Create PRs（跳 T-29）
- [ ] 落库 `change_sets` / `change_set_repositories`
- [ ] 与 T-20 选择器 / T-21 批量分支联动

## 架构 / 性能注意点

- Change Set 是「仓库集合 + 关联分支」的轻量元数据，汇总统计基于 T-02 状态缓存与 commit 元数据，不重复扫描。
- View All Diff 需聚合多仓库 diff，走 T-04 缓存 + 分页加载，避免一次性拉取全部。

## 验收标准

- [ ] Change Set 正确关联多仓库并汇总 Files/Added/Deleted/Commits
- [ ] Commit All / Push All 在关联仓库上生效，进度与部分失败可感知
- [ ] View All Diff 聚合展示流畅
- [ ] 数据持久化，重启后可恢复

## 进度

### 状态

- 当前状态：未开始
- 最近更新：—

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| — | — | — |

### 子任务清单

- [ ] Change Set 数据模型与落库
- [ ] 创建 / 关联仓库 UI
- [ ] 汇总统计与聚合 Diff
- [ ] Commit All / Push All 编排
