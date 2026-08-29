# B-09 拆 Operation Log（operation_log.rs → operation_log/）

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[B-01](./B-01-baseline-test-extraction.md)。设计约束见 [backend-module-split-plan.md](../backend-module-split-plan.md) §4.6、§6 Phase 4（设计文档 2026-08-29 修订：§4.6 正式归入 Phase 4）。

| 项 | 值 |
|---|---|
| 阶段 | Phase 4 · 支撑模块 |
| 优先级 | P1 |
| 状态 | ✅ 已完成 |
| 依赖 | B-01 |
| 对应设计文档 | §4.6 目标目录、§6 Phase 4、§2.2（operation_log.rs 需拆记录/查询/Undo） |

## 目标

把约 755 行生产代码的 `core/operation_log.rs` 按「模型 / 记录 / 查询 / Undo 计划 / Undo 执行」拆成 `core/operation_log/` 子模块，继续保持 Undo 计划与执行的分离。

## 需求范围

- [x] 目标结构（§4.6）：`operation_log/{mod.rs, model.rs, record.rs, query.rs, undo_plan.rs, undo_execute.rs, tests.rs}`
- [x] `model.rs`：OperationLog*、Undo* DTO
- [x] `record.rs`：snapshot、record_operation
- [x] `query.rs`：分页和详情查询
- [x] `undo_plan.rs`：Undo 计划和预览
- [x] `undo_execute.rs`：Undo 执行和工作区状态保护
- [x] `mod.rs`：公共类型和 re-export，调用方零修改

## 架构 / 性能注意点

- **Undo 预览不得修改 Git**（§4.6）：`undo_plan.rs` 纯只读。
- **执行前必须重新检查**当前 HEAD、分支和工作区状态（§4.6）：防止操作记录对应的状态已被用户改变；该校验逻辑进 `undo_execute.rs` 并保留测试。
- Undo 执行走现有 Git 能力与任务通道，不新增执行路径。
- 操作记录的写入保持短事务（全局约束 §5）。

## 验收标准

- [x] 操作记录 / 分页查询 / 详情行为不变（既有测试全绿）
- [x] Undo 预览只读（无 Git 副作用，测试断言）
- [x] Undo 执行前的状态重新校验生效（状态已变时拒绝执行，测试断言）
- [x] Undo 执行结果与 T-34 语义一致（可 Undo 的操作日志闭环不变）
- [x] 四件套全绿；公共入口不变

## 进度

### 状态

- 当前状态：已完成
- 最近更新：2026-08-29

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-29 | 🟦 | 开始开发；GitNexus impact：record_operation_best_effort LOW（3 调用方）、run_undo CRITICAL（6 调用方中 5 个为本模块测试，公共路径 re-export 不变，调用方零修改） |
| 2026-08-29 | ✅ | 完成：`git mv` 保留历史后按 model → record → query → undo_plan → undo_execute → tests 顺序迁移，每组移动后跑四件套。1181 行 → `mod.rs` 39 行（门面 + re-export）+ model 119 / record 124 / query 137 / undo_plan 244 / undo_execute 161 / tests 477。GitNexus impact：`record_operation_best_effort` LOW（batch/history/merge_rebase 3 调用方）、`run_undo` CRITICAL（6 调用方中 5 个为本模块测试 + undo_operation command，公共路径经 mod.rs re-export 不变）。可见性按 §5.2：`UndoPlan`/`plan_item`/`reset_mode`/`repo_name_of`/`short_oid` pub(super)（undo_plan ↔ undo_execute 子模块共享），`worktree_dirty`/`plan_*`/`execute_item`/`undo_*` 保持模块私有，无字段改 pub。结构决策：`persist_undo_results` 归入 undo_execute.rs（Undo 闭环 plan→execute→persist 同侧；git 执行不触 DB，写回由 command 持连接在执行完成后调用，DB 锁不跨 repo IO 语义不变）；OP_* 常量归入 model.rs（op_type 词汇表）。Undo 预览只读 + 执行前 plan_item 重校验语义原样保留（`undo_refuses_when_state_moved_on` 断言绿）。`detect_changes()`：LOW、受影响执行流 0。测试总数不变（494），operation_log 域 7/7 全绿；全量仅余 maven::settings ×2 基线失败（本机 ~/.m2 干扰，B-08 已记录）。fmt/clippy 口径同 B-06/B-07/B-08：本任务触碰文件（core/operation_log/ 全部 7 文件）零告警；全仓 rustc 1.98 工具链预存漂移不属本任务范围。公共入口不变：全部外部调用方（commands/batch、history、merge_rebase、commands/operation_log、models/ipc_golden）零修改。 |

### 子任务清单

- [x] `model.rs`（DTO）
- [x] `record.rs`（snapshot / record_operation）
- [x] `query.rs`（分页 / 详情）
- [x] `undo_plan.rs`（只读预览）
- [x] `undo_execute.rs`（执行 + 状态重校验）
- [x] 测试归位与四件套验证
