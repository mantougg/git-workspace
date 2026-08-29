# B-09 拆 Operation Log（operation_log.rs → operation_log/）

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[B-01](./B-01-baseline-test-extraction.md)。设计约束见 [backend-module-split-plan.md](../backend-module-split-plan.md) §4.6、§6 Phase 4（设计文档 2026-08-29 修订：§4.6 正式归入 Phase 4）。

| 项 | 值 |
|---|---|
| 阶段 | Phase 4 · 支撑模块 |
| 优先级 | P1 |
| 状态 | ⬜ 未开始 |
| 依赖 | B-01 |
| 对应设计文档 | §4.6 目标目录、§6 Phase 4、§2.2（operation_log.rs 需拆记录/查询/Undo） |

## 目标

把约 755 行生产代码的 `core/operation_log.rs` 按「模型 / 记录 / 查询 / Undo 计划 / Undo 执行」拆成 `core/operation_log/` 子模块，继续保持 Undo 计划与执行的分离。

## 需求范围

- [ ] 目标结构（§4.6）：`operation_log/{mod.rs, model.rs, record.rs, query.rs, undo_plan.rs, undo_execute.rs, tests.rs}`
- [ ] `model.rs`：OperationLog*、Undo* DTO
- [ ] `record.rs`：snapshot、record_operation
- [ ] `query.rs`：分页和详情查询
- [ ] `undo_plan.rs`：Undo 计划和预览
- [ ] `undo_execute.rs`：Undo 执行和工作区状态保护
- [ ] `mod.rs`：公共类型和 re-export，调用方零修改

## 架构 / 性能注意点

- **Undo 预览不得修改 Git**（§4.6）：`undo_plan.rs` 纯只读。
- **执行前必须重新检查**当前 HEAD、分支和工作区状态（§4.6）：防止操作记录对应的状态已被用户改变；该校验逻辑进 `undo_execute.rs` 并保留测试。
- Undo 执行走现有 Git 能力与任务通道，不新增执行路径。
- 操作记录的写入保持短事务（全局约束 §5）。

## 验收标准

- [ ] 操作记录 / 分页查询 / 详情行为不变（既有测试全绿）
- [ ] Undo 预览只读（无 Git 副作用，测试断言）
- [ ] Undo 执行前的状态重新校验生效（状态已变时拒绝执行，测试断言）
- [ ] Undo 执行结果与 T-34 语义一致（可 Undo 的操作日志闭环不变）
- [ ] 四件套全绿；公共入口不变

## 进度

### 状态

- 当前状态：未开始
- 最近更新：—

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|

### 子任务清单

- [ ] `model.rs`（DTO）
- [ ] `record.rs`（snapshot / record_operation）
- [ ] `query.rs`（分页 / 详情）
- [ ] `undo_plan.rs`（只读预览）
- [ ] `undo_execute.rs`（执行 + 状态重校验）
- [ ] 测试归位与四件套验证
