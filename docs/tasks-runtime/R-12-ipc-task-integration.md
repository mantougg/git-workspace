# R-12 Runtime IPC / Event API 与 Task Engine 集成

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[R-09 Build Engine](./R-09-build-engine.md)、[R-10 Runtime Launcher 与 Process Manager](./R-10-launcher-process-manager.md)、[R-11 Runtime 日志引擎](./R-11-log-engine.md)；任务框架复用 T-05 / T-24。

| 项 | 值 |
|---|---|
| 阶段 | Phase 1 · 构建运行闭环 |
| 优先级 | P0 |
| 状态 | 🟦 进行中 |
| 依赖 | R-09, R-10, R-11, T-05 |
| 对应源文档 | §63 IPC API、§64 Event API、§65 Task Engine 集成、§66 并发策略 |

## 目标

定义并实现 Runtime 模块完整 IPC 命令集与事件集，把 Runtime 生命周期接入现有 Task Engine（子任务进度可见、可取消），并以 Runtime Task Scheduler 统一限流。

## 需求范围

- [ ] IPC 命令全集（§63）：`list_projects / inspect_project / resolve_dependencies / get_dependency_graph / create_config / update_config / delete_config / build / start / stop / restart / list_processes / process_status / get_logs / clear_logs / start_environment / stop_environment`
- [ ] Event 全集（§64）：`project_discovered / dependency_resolved / build_started / build_progress / build_completed / process_started / process_output / process_stopped / process_failed / health_changed / file_changed / restart_started / restart_completed`
- [ ] Task Engine 集成（§65）：Runtime Start 拆解为子任务 Validate JDK / Validate Maven / Resolve Dependencies / Generate Reactor / Build / Start，进度对齐 UI 阶段显示（Preparing ✓ / Resolving ✓ / Building ▓ / Starting ○）
- [ ] Runtime Task Scheduler（§66）：最大并发 Build = 2 / Dependency Resolve = 4 起步（可配置），排队、取消、超时
- [ ] IPC 类型单一事实来源：Rust serde 结构 + golden-file 快照测试（沿用全局约束）

## 架构 / 性能注意点

- 高频事件（`process_output` / `build_progress`）必须批量聚合推送，不阻塞 UI（全局约束与 R-11 背压对齐）。
- 大 payload（依赖图、日志查询）分页 / 流式，禁止一次性全量返回。
- 长任务全部托管于 T-05 Task Queue：可取消、有超时、崩溃后可恢复状态。
- 并发上限是硬约束；超限任务排队而非拒绝，队列深度有界。

## 验收标准

- [ ] §63 全部 IPC 命令可用，类型有 golden 快照
- [ ] 启动一个应用的完整生命周期事件序列正确（build_* → process_* → health_changed）
- [ ] 同时启动多个应用时 Build 并发不超限，其余排队执行
- [ ] 任务进度事件与 UI 阶段一一对应（Preparing/Resolving/Building/Starting）
- [ ] 取消进行中的 Start 任务，已启动的子任务正确回滚/终止

## 进度

### 状态

- 当前状态：进行中
- 最近更新：2026-08-23 开始开发

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-23 | 🟦 开始开发 | 启动 R-12：§63 IPC 命令全集、§64 Event 全集（高频聚合推送）、Start 流水线接入 T-05 Task Engine 子任务拆解、Runtime Task Scheduler 限流（Build=2 / Resolve=4 起步可配置）、serde + golden 快照 |

### 子任务清单

- [ ] IPC 命令定义与实现（含 golden 快照）
- [ ] Event 定义与聚合推送
- [ ] Start 流水线的 Task Engine 子任务拆解
- [ ] Runtime Task Scheduler（限流/排队/取消）
- [ ] 单元/集成测试
