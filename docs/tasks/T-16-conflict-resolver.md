# T-16 Conflict Resolver

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[T-15 Merge / Rebase](./T-15-merge-rebase.md)、[T-04 Diff & Graph 硬化](./T-04-diff-graph.md)。

| 项 | 值 |
|---|---|
| 阶段 | Phase 1 · 完整 Git Client（P0） |
| 优先级 | P0 |
| 状态 | ✅ 已完成 |
| 依赖 | T-15, T-04 |
| 对应 Roadmap | §13 Conflict Resolver、§24 AI Conflict Resolution |

## 目标

实现三方冲突可视化解决器：BASE / OURS / THEIRS / RESULT 四栏，手动编辑与逐文件标记解决，作为 Merge/Rebase/Cherry-pick 冲突的统一出口。

## 需求范围

- [x] 冲突检测：仓库处于 CONFLICT 状态时进入 Resolver
- [x] 三方展示：BASE / OURS / THEIRS / RESULT 四栏 Diff
- [x] 解决操作：Use Ours / Use Theirs / Use Both / Manual Edit / Mark Resolved / Abort
- [x] 冲突文件列表 + 逐文件解决状态
- [x] 全部解决后提示继续（merge --continue / rebase --continue）
- [x] 冲突队列：跨仓库集中列出所有冲突仓库、逐个解决、可整体 abort（Multi-Repo First 延伸）
- [x] 预留 AI 建议入口（T-26 实现）

## 架构 / 性能注意点

- 三方内容来自 libgit2 的 merge 祖先解析（`index_conflicts` / `merge_base`）；大文件冲突按需加载，不一次性读入全部内容。
- **AI 冲突解决（§24）的硬约束**：AI 只给建议 → Diff Preview → 用户确认 → Apply，禁止默认直接覆盖工作区（T-26 继承此约束）。

## 验收标准

- [x] CONFLICT 状态正确识别并进入 Resolver
- [x] Use Ours/Theirs/Both/Mark Resolved 结果与 `git add` 后状态一致
- [x] Abort 完整恢复冲突前状态
- [x] 多文件冲突逐个解决、进度清晰
- [x] 批量操作后多个仓库同时冲突时，冲突队列集中展示与逐个解决

## 进度

### 状态

- 当前状态：已完成
- 最近更新：2026-08-14 完成

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-14 | 🟦 | 开始开发 |
| 2026-08-14 | ✅ | 完成：新增 `core/conflict.rs`（operation_state 统一检测 merge/cherry-pick/revert/rebase + 冲突形态分类 + 三方/工作区内容按需加载 500K 字符截断 + ours/theirs/both/手动内容解决，落 `git add` 语义）+ `history::pick_continue`（cherry-pick/revert 的 continue）+ 5 个 command；前端 `ConflictResolver.vue`（BASE/OURS/THEIRS/RESULT 四栏，RESULT 可编辑 + 逐文件解决进度 + Continue/Abort 按操作类型路由 + 跨仓库冲突队列与整体 Abort）+ GitGraph/BranchManager 横幅「进入解决器」+ RepositoryList 冲突队列入口；T-13/T-15 验收随之闭环；5 个核心单元测试（含三策略与 git add 一致性 + merge_continue 衔接）；IPC golden 登记 ConflictFile/OperationState/ConflictContent；`cargo test` 73 passed、`vue-tsc` + `vite build` 通过 |

### 子任务清单

- [x] 冲突检测与列表
- [x] 三方/结果四栏 Diff UI
- [x] 解决操作（Ours/Theirs/Both/Manual/Mark Resolved/Abort）
- [x] 继续/中止衔接 merge/rebase 状态机
- [x] 预留 AI 建议接口
- [x] 跨仓库冲突队列
