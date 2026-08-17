# T-22 Workspace Change Set

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[T-09 Branch Manager](./T-09-branch-manager.md)、[T-20 Batch Operations 增强](./T-20-batch-ops.md)。

| 项 | 值 |
|---|---|
| 阶段 | Phase 2 · Multi-Repo Engine（P1） |
| 优先级 | P1 |
| 状态 | ✅ 已完成 |
| 依赖 | T-09, T-20 |
| 对应 Roadmap | §17 Workspace Change Set、§41 schema（change_sets / change_set_repositories） |

## 目标

实现 Workspace Change Set——跨仓库关联同一特性的变更集合，统一展示与批量操作，是产品核心差异化功能。

## 需求范围

- [x] 创建 Change Set（如 `Feature: AI Review`）
- [x] 关联多个仓库（repo-a → feature/ai-review 等）
- [x] 统一汇总：Repositories / Files / Added / Deleted / Commits
- [x] 操作：View All Diff / AI Review / Commit All / Push All / Create PRs（跳 T-29）
- [x] 落库 `change_sets` / `change_set_repositories`
- [x] 与 T-20 选择器 / T-21 批量分支联动

## 架构 / 性能注意点

- Change Set 是「仓库集合 + 关联分支」的轻量元数据，汇总统计基于 T-02 状态缓存与 commit 元数据，不重复扫描。
- View All Diff 需聚合多仓库 diff，走 T-04 缓存 + 分页加载，避免一次性拉取全部。

## 验收标准

- [x] Change Set 正确关联多仓库并汇总 Files/Added/Deleted/Commits
- [x] Commit All / Push All 在关联仓库上生效，进度与部分失败可感知
- [x] View All Diff 聚合展示流畅
- [x] 数据持久化，重启后可恢复

## 进度

### 状态

- 当前状态：已完成
- 最近更新：2026-08-17 完成

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-17 | 🟦 | 开始开发 |
| 2026-08-17 | ✅ | 完成：`core/change_set.rs`（CRUD/关联/软删过滤/目标分支规范化，全事务化；`change_stats` 基于 T-04 `head_or_empty_tree` 处理 unborn HEAD，untracked 行数直接读工作区文件补 libgit2 不加载内容之差，runtime 路径排除）+ `commands/change_set.rs` 7 command（summary 走 T-02 状态缓存 + rayon 并行 diff-stat，单仓库失败降级不阻断）；Commit All / Push All 复用 T-05/T-20 任务队列（`batch_commit`/`batch_push` + TaskPanel 进度与部分失败），View All Diff 按仓库懒加载 + 缓存，Add 仓库接 T-20 选择器（`select_repos`）；前端 types/api/store + ChangeSetView（汇总卡片 / 成员表 / 目标分支设置 / 各操作对话框）；schema V6 落库 `change_sets`/`change_set_repositories`（级联删除）。验证：`cargo test --lib core::change_set` 7 passed（修复 2 个 change_stats 失败：unborn HEAD 显式空树、untracked 文件直接读行数）；`cargo test --lib` 165 passed（仅余 batch dry_run 2 个沙箱 git clone 环境限制失败）；`vue-tsc --noEmit` 通过；IPC golden 补登记 36 类型含本任务 7 个 |

### 子任务清单

- [x] Change Set 数据模型与落库
- [x] 创建 / 关联仓库 UI
- [x] 汇总统计与聚合 Diff
- [x] Commit All / Push All 编排
