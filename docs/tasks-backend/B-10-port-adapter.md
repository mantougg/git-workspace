# B-10 按需引入 Port/Adapter（BuildExecutor / ProcessSupervisor / RuntimeRepository）

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[B-02](./B-02-runtime-service.md)、[B-03](./B-03-runtime-process-manager.md)，**且至少一条触发条件成立**（见下）。设计约束见 [backend-module-split-plan.md](../backend-module-split-plan.md) §6 Phase 5、§10.3、§3。

| 项 | 值 |
|---|---|
| 阶段 | Phase 5 · 依赖边界收敛 |
| 优先级 | P2（条件触发） |
| 状态 | ⬜ 未开始 |
| 依赖 | B-02, B-03 + 触发条件 |
| 对应设计文档 | §6 Phase 5、§10.3、§3 目标依赖方向、§7 数据库边界 |

## 目标

在**真实替换需求出现**时，才把 Runtime、Git、Task 对 SQLite、外部命令、OS 进程和 Tauri 事件的直接依赖收敛到明确的 trait 边界。本任务不是常规排期任务——没有触发条件就不启动。

## 触发条件（§6 Phase 5，满足其一才启动）

- 需要多个 Build Engine（如 Maven + Gradle，对应 R-22）；
- 需要 Fake Process Supervisor 做纯单元测试；
- 需要替换事件出口（Tauri event / 日志事件 / 测试 recorder）；
- 需要收敛业务模块直接持有 SQLite 连接的范围。

## 需求范围（按触发条件裁剪，不做全量）

- [ ] 优先候选接口（§6 Phase 5）：`BuildExecutor`（execute）、`ProcessSupervisor`（start/stop）、`RuntimeRepository`（仅 Runtime 元数据与进程行）
- [ ] 如引入 Repository：按领域拆 `RuntimeRepository` / `MavenIndexRepository` / `TaskRepository`，**禁止**包揽所有表的 `DatabaseService`（§7 / §10.4）
- [ ] trait 落在真实边界上：Build Port → Maven CLI Adapter；Process Port → OS Process Adapter；Event Port → Tauri Event Adapter（§3）
- [ ] 调用方迁移保持 IPC 契约不变（全局约束 §1）

## 架构 / 性能注意点

- **克制是第一原则**（§10.3）：不为每个 DAO 函数、路径工具、serde DTO 加 trait；先拆职责（B-02~B-09）再从最难测试/最可能多实现的边界引入接口。
- trait 对象安全与异步方案按现有代码风格选择（`async-trait` 是否已在依赖中需先确认，不假设可用）。
- 引入 trait 后命令行/事件/SQL 行为不变；fake 实现只服务测试。

## 验收标准

- [ ] 启动时在设计文档 §6 Phase 5 下记录具体触发条件与裁剪范围
- [ ] 只为触发条件涉及的边界引入 trait（diff 走查确认无机械接口化）
- [ ] IPC 契约 / 事件 / DB 行为不变；四件套全绿
- [ ] 若引入 Repository：按领域拆分且事务边界清晰（§7）

## 进度

### 状态

- 当前状态：未开始（条件触发）
- 最近更新：2026-08-29

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-29 | ⬜ | 触发条件评估（B-09 完成后例行核查），**结论：均未成立，不启动**。①多 Build Engine：R-22 Gradle ⬜ 未开始，`build/mod.rs::engine_for` 已预留扩展位且注释明确「Gradle 由 R-22 预留、不提前实现」；②Fake Process Supervisor：无任何任务文档提出该需求，B-03 测试以真实 fixture 覆盖；③替换事件出口：全部 5 条任务线（tasks/runtime/fix/ai/desktop）索引检索零命中；④收敛 SQLite 持有范围：无驱动需求，§10.4 明确无需要时不建 Repository。启动时须先在设计文档 §6 Phase 5 下记录成立的触发条件与裁剪范围。 |

### 子任务清单

- [ ] 记录触发条件与裁剪范围
- [ ] 目标 trait 定义与现有实现适配
- [ ] 调用方迁移
- [ ] 测试（含 fake 实现，如有）
- [ ] 四件套验证 + diff 走查
