# B-02 拆 RuntimeService（service.rs → service/）

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[B-01](./B-01-baseline-test-extraction.md)（测试已外移）。设计约束见 [backend-module-split-plan.md](../backend-module-split-plan.md) §4.1、§6 Phase 1。GitNexus：移动公共符号前必须跑 `impact`。

| 项 | 值 |
|---|---|
| 阶段 | Phase 1 · Runtime 核心 |
| 优先级 | P0 |
| 状态 | ⬜ 未开始 |
| 依赖 | B-01 |
| 对应设计文档 | §2.2 RuntimeService 问题、§4.1 目标目录、§6 Phase 1 |

## 目标

把约 1,700 行生产代码的 RuntimeService God Object 按职责拆成 `runtime/service/` 子模块：DTO、查询、单服务操作、Environment 编排、任务分发、取消。公共路径 `runtime::RuntimeService` 等保持不变。

## 需求范围

- [ ] 目标结构（§4.1）：`service/{mod.rs, dto.rs, queries.rs, operations.rs, environment.rs, task_handler.rs, cancellation.rs, tests.rs}`
- [ ] 迁移映射（§4.1 表）：`SchedulerConfig` 与请求/返回 DTO → `dto.rs`；`list_projects` 到日志/进程查询 → `queries.rs`；`operation_task_request` / `exec_build` / `exec_start` / `exec_stop` / `exec_restart` / `exec_resolve` → `operations.rs`；`start_environment_requests` / `exec_start_environment` / `exec_stop_environment` → `environment.rs`；`impl RuntimeTaskHandler` → `task_handler.rs`；`CancelWatch` / `build_options_of` / `start_options_of` → `cancellation.rs`
- [ ] 迁移顺序（§6 Phase 1）：mod.rs 骨架与 re-export → DTO/Scheduler 配置 → 查询 → 单服务操作 → Environment 编排 → 最后 TaskHandler + CancelWatch；每步一个职责组，立即编译+测试
- [ ] `mod.rs` 只保留 `RuntimeService` 字段、`new/assemble`、共享构造逻辑和跨子模块 `pub(super)` 辅助方法
- [ ] re-export 兼容：`runtime::RuntimeService`、`runtime::RuntimeOperationRequest`、`runtime::SchedulerConfig` 等路径不变（§5.3）

## 架构 / 性能注意点

- `environment.rs` 是多服务拓扑编排，**不放入** RuntimeProcessManager（§4.1）。
- `cancellation.rs` 的 CancelWatch 必须继续覆盖「取消早于 start 注册句柄」的竞态（§6 Phase 1 验收重点）。
- 共享辅助（如 `find_project` 路径归一化匹配）留在 `mod.rs` 或显式工具模块，路径比较遵守归一化规则（全局约束 §6）。
- 查询与写操作分文件后，查询路径不得新增缓存失效写入（全局约束 §5）。

## 验收标准

- [ ] `commands/runtime.rs` 不需要改变业务调用方式（§6 Phase 1）
- [ ] Build / Start / Restart / Resolve 仍通过 TaskManager 执行；事件名和 payload 不变
- [ ] CancelWatch 竞态测试继续通过
- [ ] `mod.rs` ≤ 约 400 行，以公共类型/装配/re-export 为主（§11）
- [ ] 四件套全绿；`detect_changes()` 无超预期影响
- [ ] 文档同步：更新根 `AGENTS.md` 等平台规范中对 `runtime/service.rs::find_project` 等路径的引用（§9 第 6 条）

## 进度

### 状态

- 当前状态：未开始
- 最近更新：—

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|

### 子任务清单

- [ ] `service/mod.rs` 骨架 + 公共 re-export（impact 分析先行）
- [ ] `dto.rs`（SchedulerConfig + DTO）
- [ ] `queries.rs`（只读查询）
- [ ] `operations.rs`（Build/Start/Stop/Restart/Resolve）
- [ ] `environment.rs`（多服务编排）
- [ ] `task_handler.rs` + `cancellation.rs`
- [ ] 测试归位与四件套验证 + 文档同步
