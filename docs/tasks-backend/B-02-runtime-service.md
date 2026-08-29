# B-02 拆 RuntimeService（service.rs → service/）

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[B-01](./B-01-baseline-test-extraction.md)（测试已外移）。设计约束见 [backend-module-split-plan.md](../backend-module-split-plan.md) §4.1、§6 Phase 1。GitNexus：移动公共符号前必须跑 `impact`。

| 项 | 值 |
|---|---|
| 阶段 | Phase 1 · Runtime 核心 |
| 优先级 | P0 |
| 状态 | ✅ 已完成 |
| 依赖 | B-01 |
| 对应设计文档 | §2.2 RuntimeService 问题、§4.1 目标目录、§6 Phase 1 |

## 目标

把约 1,700 行生产代码的 RuntimeService God Object 按职责拆成 `runtime/service/` 子模块：DTO、查询、单服务操作、Environment 编排、任务分发、取消。公共路径 `runtime::RuntimeService` 等保持不变。

## 需求范围

- [x] 目标结构（§4.1）：`service/{mod.rs, dto.rs, queries.rs, operations.rs, environment.rs, task_handler.rs, cancellation.rs, tests.rs}`
- [x] 迁移映射（§4.1 表）：`SchedulerConfig` 与请求/返回 DTO → `dto.rs`；`list_projects` 到日志/进程查询 → `queries.rs`；`operation_task_request` / `exec_build` / `exec_start` / `exec_stop` / `exec_restart` / `exec_resolve` → `operations.rs`；`start_environment_requests` / `exec_start_environment` / `exec_stop_environment` → `environment.rs`；`impl RuntimeTaskHandler` → `task_handler.rs`；`CancelWatch` / `build_options_of` / `start_options_of` → `cancellation.rs`（映射表未覆盖项的归属决策见时间线影响分析条目）
- [x] 迁移顺序（§6 Phase 1）：mod.rs 骨架与 re-export → DTO/Scheduler 配置 → 查询 → 单服务操作 → Environment 编排 → 最后 TaskHandler + CancelWatch；每步一个职责组，立即编译（check --tests）+ 最终统一四件套
- [x] `mod.rs` 只保留 `RuntimeService` 字段、`new/assemble`、共享构造逻辑和跨子模块 `pub(super)` 辅助方法（303 行）
- [x] re-export 兼容：`runtime::RuntimeService`、`runtime::RuntimeOperationRequest`、`runtime::SchedulerConfig` 等路径不变（§5.3；`mod.rs` `pub use` 保持 `runtime::service::*` 与 `runtime::*` 双路径）

## 架构 / 性能注意点

- `environment.rs` 是多服务拓扑编排，**不放入** RuntimeProcessManager（§4.1）。✓ 独立 525 行子模块
- `cancellation.rs` 的 CancelWatch 必须继续覆盖「取消早于 start 注册句柄」的竞态（§6 Phase 1 验收重点）。✓ 结构体 + `start`/`Drop` 逐字搬迁，`cancel_during_start_aborts_build_and_finalizes` 等竞态测试通过
- 共享辅助（如 `find_project` 路径归一化匹配）留在 `mod.rs` 或显式工具模块，路径比较遵守归一化规则（全局约束 §6）。✓ `find_project` 留 mod.rs，归一化逻辑逐字节保留
- 查询与写操作分文件后，查询路径不得新增缓存失效写入（全局约束 §5）。✓ queries.rs 仅读侧 + 日志用例组（clear_logs 为 §4.1 范围成员，非新增写）

## 验收标准

- [x] `commands/runtime.rs` 不需要改变业务调用方式（§6 Phase 1）——本任务 diff 中 `commands/`、`state.rs`、`lib.rs` 零改动
- [x] Build / Start / Restart / Resolve 仍通过 TaskManager 执行；事件名和 payload 不变（task_handler.rs 分发逻辑逐字搬迁；ipc_golden 快照测试通过）
- [x] CancelWatch 竞态测试继续通过（`service::` 17/17 通过）
- [x] `mod.rs` ≤ 约 400 行，以公共类型/装配/re-export 为主（§11）——实际 303 行
- [x] 四件套全绿；`detect_changes()` 无超预期影响——与 B-01 相同边界口径（见下）；`check` 绿、`test --lib` 495 总数不变（488 通过 / 4 失败 / 3 忽略，失败集与 B-01 收尾完全一致且逐个已归因）、clippy 全目标 103 与拆分前持平且 service/ 文件零 lint、fmt 新文件零 hunk。GitNexus MCP 本会话不可用：以源码搜索影响分析 + git diff 范围审计替代（diff 仅含 `service/` 内文件与任务文档）
- [x] 文档同步：根 `AGENTS.md` 参照引用已在 B-01 中更新为 `runtime/service/mod.rs::find_project` 等（`find_project` 留在 mod.rs，路径继续有效）；本任务无新增死链

## 进度

### 状态

- 当前状态：已完成
- 最近更新：2026-08-29，全部需求范围完成并通过验收

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-29 | 🟦 开始开发 | 前置 B-01 完成（已提交 `2c99d80`）。生产代码约 1,722 行（测试已外移）。按 §4.1 目标结构 + §6 Phase 1 顺序执行。 |
| 2026-08-29 | 🟦 影响分析（GitNexus MCP 不可用，源码搜索兜底） | 公共面：`runtime::service::{RuntimeService, RuntimeOperationRequest, SchedulerConfig, RuntimeLogQuery, ProjectInspection, DependencyGraphView, ClosurePreview, scheduler_config_path, build_options_of, start_options_of, RuntimeServiceOverrides}`（经 `runtime/mod.rs` re-export）；调用方：`AppState.runtime`（state.rs）、`lib.rs`（装配 `watch_shared_parts` / `reconcile_on_startup` / TaskHandler 接线）、`commands/runtime.rs` 经 state 访问。映射表未覆盖项的归属决策：`stop_blocking`→operations（同步 Stop 操作）；`script_approval_*` 三兄弟→operations（管理操作，queries 保持纯读）；`reconcile_on_startup`→mod.rs（启动装配生命周期）；`watch_shared_parts`→mod.rs（共享依赖装配访问器）；`clear_logs` 随日志用例组留 queries.rs（§4.1 范围成员，全局约束仅禁新增缓存失效写）；`resolve_task_request` 与 `operation_task_request` 同组→operations。 |
| 2026-08-29 | 🟦 六步迁移（每组后 check --tests） | Step1 dto（SchedulerConfig+5 DTO，`sanitized` 改 `pub(super)`）；Step2 queries（13 个读侧方法）；Step3 operations（14 方法 + `display_path` + `MAX_PROJECT_DISCOVERED_EVENTS`，`exec_*` 六方法改 `pub(super)` 供 task_handler 分发）；Step4 environment（7 方法 + `start_environment_service` 自由函数，`exec_start/stop_environment` 改 `pub(super)`）；Step5 task_handler（`impl RuntimeTaskHandler`）+ cancellation（选项映射 + CancelWatch + CANCEL_* 常量，`CancelWatch`/`start` 改 `pub(super)`）。方法体逐字搬迁；跨子模块可见性一律 `pub(super)`，公共路径经 mod.rs re-export 不变。 |
| 2026-08-29 | 🟦 导入修剪与 mod.rs 收敛 | 按编译器警告迭代修剪 7 个文件导入（152→4→0）；tests.rs 改为显式导入（+11 行，不再依赖 mod.rs 私有导入透传）；mod.rs 清理孤儿分隔注释、重写模块文档说明拆分布局，最终 303 行。 |
| 2026-08-29 | ✅ 完成验收 | `check` 绿；`test --lib` 总数 495 不变（488 通过 / 4 失败 / 3 忽略，失败集与 B-01 收尾完全一致：settings×2 既有环境、flood 已证基线同样失败、diff cache 隔离通过）；`service::` 测试 17/17 通过（含 CancelWatch 竞态 `cancel_during_start_aborts_build_and_finalizes`）；clippy 全目标 103 与拆分前持平、`service/` 零 lint（修剪中曾引入 dto 重复常量与 2 处空行，已修复）；7 个新文件 fmt 零 hunk（tests.rs 为 B-01 已提交基线，不动）；最终 diff：mod.rs -1482 行、6 个新子模块、tests.rs +11 行导入、任务文档，`commands/`/`state.rs`/`lib.rs` 零改动。GitNexus MCP 不可用：以源码搜索影响分析 + diff 范围审计替代 `detect_changes`。 |

### 子任务清单

- [x] `service/mod.rs` 骨架 + 公共 re-export（impact 分析先行）
- [x] `dto.rs`（SchedulerConfig + DTO）
- [x] `queries.rs`（只读查询）
- [x] `operations.rs`（Build/Start/Stop/Restart/Resolve）
- [x] `environment.rs`（多服务编排）
- [x] `task_handler.rs` + `cancellation.rs`
- [x] 测试归位与四件套验证 + 文档同步
