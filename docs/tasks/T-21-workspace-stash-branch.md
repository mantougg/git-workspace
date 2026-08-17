# T-21 Workspace Stash & Branch

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[T-09 Branch Manager](./T-09-branch-manager.md)、[T-10 Stash](./T-10-stash.md)。

| 项 | 值 |
|---|---|
| 阶段 | Phase 2 · Multi-Repo Engine（P1） |
| 优先级 | P1 |
| 状态 | ✅ 已完成 |
| 依赖 | T-09, T-10 |
| 对应 Roadmap | §7 Workspace Stash、§16 Create Branch All / Delete Branch All |

## 目标

实现多仓库级 Stash 与 Branch 编排：一次暂存/恢复整组仓库，一次在整组仓库创建/删除同名分支。

## 需求范围

- [x] Workspace Stash：选定仓库组 → 保存为 `Workspace Stash #N`（记录各仓库 stash 关联）
- [x] Restore Workspace：按记录恢复整组仓库
- [x] Workspace Branch：Create Branch All / Delete Branch All（选定组、同名分支）
- [x] 落库 `change_sets` / 或独立 `workspace_stashes` 表关联各仓库 stash
- [x] 与 T-20 选择器复用（按组/标签/状态选仓库）

## 架构 / 性能注意点

- Workspace Stash 本质是「逐仓库 stash + 关联记录」，执行走 T-05 任务队列并发限流；恢复前必须校验每个仓库当前状态可安全恢复。
- Restore 是危险操作（可能覆盖工作区），§46 Warning 级确认并列出影响仓库。

## 验收标准

- [x] 一次 Stash 能覆盖选定组全部仓库且生成关联记录
- [x] Restore 完整还原整组仓库工作区
- [x] Create/Delete Branch All 在整组生效，部分失败可定位
- [x] 恢复操作有确认且影响范围清晰

## 进度

### 状态

- 当前状态：已完成
- 最近更新：2026-08-17 完成

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-17 | 🟦 | 开始开发 |
| 2026-08-17 | ✅ | 完成：新增 `core/workspace_stash.rs`（逐仓库 stash 编排复用 T-10 `core/stash`，干净仓库跳过/单仓库失败收集不阻断；恢复前按 oid 重定位 stash + 分支一致性校验，branch_mismatch 需显式放行；`workspace_stashes`/`items` DAO helper 刻意写在本模块不进 `db/dao.rs`（并行开发期），批量插入单事务 + prepared statement）+ `commands/workspace_stash.rs` 6 个 command（save/list/items/check/restore/delete，git 阶段不持 DB 锁）；Create/Delete Branch All 核对 T-20 已有完整实现（`batch_branch_op` + `TaskType::BranchOp` 执行器 + Partial Success 聚合 + 选择器预选 + Delete 双重确认），无缺口；前端 `types/workspaceStash.ts` + `api/workspaceStash.ts` + RepositoryList「Workspace Stash」对话框（保存含 include-untracked、记录列表可展开明细、恢复走 §46 Warning 确认列出影响仓库与校验结果、删除仅删记录并提示各仓库 stash 保留）；3 个核心单元测试（roundtrip/分支不一致守卫/命名级联删除）；`cargo test --lib` 139 passed（另 2 个 change_set 失败为 T-22 并行在改模块，与本任务无关，本任务 3 测试全过）、`vue-tsc --noEmit` 通过、IPC golden 两测试通过 |
| 2026-08-17 | ✅ | 收尾：lib.rs 注册 6 个 workspace_stash 命令；IPC golden 补登记 5 个类型（WorkspaceStashRepoOutcome / SaveWorkspaceStashResult / WorkspaceStashSummary / WorkspaceStashItemEntry / WorkspaceStashCheckItem）；`cargo test --lib` 165 passed（仅余 batch dry_run 2 个沙箱 git clone 环境限制失败） |

### 子任务清单

- [x] Workspace Stash 数据模型
- [x] 多仓库 stash/restore 编排
- [x] 批量建/删分支
- [x] 与 T-20 选择器联动
