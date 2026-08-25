# R-12 Runtime IPC / Event API 与 Task Engine 集成

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[R-09 Build Engine](./R-09-build-engine.md)、[R-10 Runtime Launcher 与 Process Manager](./R-10-launcher-process-manager.md)、[R-11 Runtime 日志引擎](./R-11-log-engine.md)；任务框架复用 T-05 / T-24。

| 项 | 值 |
|---|---|
| 阶段 | Phase 1 · 构建运行闭环 |
| 优先级 | P0 |
| 状态 | ✅ 已完成 |
| 依赖 | R-09, R-10, R-11, T-05 |
| 对应源文档 | §63 IPC API、§64 Event API、§65 Task Engine 集成、§66 并发策略 |

## 目标

定义并实现 Runtime 模块完整 IPC 命令集与事件集，把 Runtime 生命周期接入现有 Task Engine（子任务进度可见、可取消），并以 Runtime Task Scheduler 统一限流。

## 需求范围

- [x] IPC 命令全集（§63）：`list_projects / inspect_project / resolve_dependencies / get_dependency_graph / create_config / update_config / delete_config / build / start / stop / restart / list_processes / process_status / get_logs / clear_logs / start_environment / stop_environment`（`create/update/delete_config` 沿用 R-07 已有命令；start/stop_environment 的 Phase 1 口径 = 「该 workspace 全部配置」，依赖排序/并行编排归 R-15）
- [x] Event 全集（§64）：13 个事件全部定义并有 golden 快照；12 个已接线发射。`file_changed` 只定类型——发射归 R-17 File Watch（需 T-06 watcher 挂接 POM/源文件，本任务边界外）
- [x] Task Engine 集成（§65）：Runtime Start 经 T-05 任务队列执行；生命周期 Preparing/Resolving/Building/Starting 迁移经桥接推导为 `build_progress` 阶段事件，对齐 UI 阶段显示（Preparing ✓ / Resolving ✓ / Building ▓ / Starting ○）
- [x] Runtime Task Scheduler（§66）：最大并发 Build = 2 / Dependency Resolve = 4（`runtime-scheduler.json` 可配置 + `runtime_get/set_scheduler_config` IPC 运行时生效）；排队（T-05 有界队列 + permit 池）、取消（等 permit 可取消 + 构建中内存快路径杀进程树）、超时（worker 1h 硬上限 + BuildOptions 30min）
- [x] IPC 类型单一事实来源：Rust serde 结构 + golden-file 快照测试（沿用全局约束；新增 27 个类型样本与 TS 映射）

## 架构 / 性能注意点

- 高频事件（`process_output` / `build_progress`）必须批量聚合推送，不阻塞 UI（全局约束与 R-11 背压对齐）。
- 大 payload（依赖图、日志查询）分页 / 流式，禁止一次性全量返回。
- 长任务全部托管于 T-05 Task Queue：可取消、有超时、崩溃后可恢复状态。
- 并发上限是硬约束；超限任务排队而非拒绝，队列深度有界。

## 验收标准

- [x] §63 全部 IPC 命令可用，类型有 golden 快照（`golden_samples_match_snapshot` / `ts_types_match_rust_samples`）
- [x] 启动一个应用的完整生命周期事件序列正确（build_* → process_* → health_changed；`service::tests::start_op_emits_full_lifecycle_sequence`）
- [x] 同时启动多个应用时 Build 并发不超限，其余排队执行（`service::tests::concurrent_builds_are_capped_by_scheduler`：3 并发 Build 峰值 = 2；`scheduler::tests::*`）
- [x] 任务进度事件与 UI 阶段一一对应（Preparing/Resolving/Building/Starting；同上 start 测试断言阶段序列）
- [x] 取消进行中的 Start 任务，已启动的子任务正确回滚/终止（`service::tests::cancel_during_start_aborts_build_and_finalizes`：内存快路径杀 Maven 进程树，进程行落终态）

## 进度

### 状态

- 当前状态：已完成
- 最近更新：2026-08-25 完成

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-23 | 🟦 开始开发 | 启动 R-12：§63 IPC 命令全集、§64 Event 全集（高频聚合推送）、Start 流水线接入 T-05 Task Engine 子任务拆解、Runtime Task Scheduler 限流（Build=2 / Resolve=4 起步可配置）、serde + golden 快照 |
| 2026-08-25 | ✅ 完成 | 落地 `runtime/events.rs`（§64 全部 13 事件 payload + R-10/R-11 内部事件桥接纯函数映射）、`runtime/service.rs`（RuntimeService + RuntimeTaskHandler + §63 查询读侧）、§63 全部 17 个 IPC 命令注册、§66 双 permit 池（Build=2/Resolve=4，`runtime-scheduler.json` 可配置、运行时 set_max 生效）、BuildScheduler 增加 `acquire_cancelable`/`set_max`、manager 增加 `signal_build_cancel` 内存快路径（取消不等 DB 锁）、execute_build 改共享连接按阶段短持锁（构建期释放 DB 锁，并发构建真正到 2）。验证：`cargo test` 398 passed / 1 failed（唯一失败 `maven::settings::tests::resolve_uses_settings_when_present` 为环境性既有问题——本机存在 ~/.m2/settings.xml 且按 §18 用户级优先，与本次改动无关，settings.rs 未在 diff 中）；`pnpm build` 绿；真实 mvn 集成 `build_op_with_real_maven_builds_synthetic_reactor` 通过 |

### 子任务清单

- [x] IPC 命令定义与实现（含 golden 快照）
- [x] Event 定义与聚合推送
- [x] Start 流水线的 Task Engine 子任务拆解
- [x] Runtime Task Scheduler（限流/排队/取消）
- [x] 单元/集成测试
