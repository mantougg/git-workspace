# B-03 拆 RuntimeProcessManager（manager.rs → manager/）

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[B-01](./B-01-baseline-test-extraction.md)（测试已外移；建议在 [B-02](./B-02-runtime-service.md) 完成后开始）。设计约束见 [backend-module-split-plan.md](../backend-module-split-plan.md) §4.2、§6 Phase 2。本任务并发与平台风险最高，严格按顺序小步移动。

| 项 | 值 |
|---|---|
| 阶段 | Phase 2 · Runtime 核心 |
| 优先级 | P0 |
| 状态 | ⬜ 未开始 |
| 依赖 | B-01（建议 B-02 后） |
| 对应设计文档 | §2.2 ProcessManager 问题、§4.2 目标目录、§6 Phase 2 |

## 目标

把约 1,400 行生产代码的 RuntimeProcessManager 按「状态机 / 控制 / 监控 / 指标 / 输出」分离到 `runtime/launch/manager/` 子模块；生命周期状态迁移仍由 `RuntimeProcessManager` 统一执行，不出现多个可修改生命周期的入口。

## 需求范围

- [ ] 目标结构（§4.2）：`manager/{mod.rs, types.rs, start.rs, control.rs, monitor.rs, metrics.rs, output.rs, tests.rs}`
- [ ] 迁移顺序（§6 Phase 2，先纯逻辑后高风险）：纯类型与退出分类（`types.rs`）→ metrics sampler（`metrics.rs`）→ output sink 与启动探测（`output.rs`）→ monitor/finalize（`monitor.rs`）→ stop/kill/reconcile（`control.rs`）→ 最后 start/build 准备流程（`start.rs`）
- [ ] 职责边界（§4.2）：`start.rs` = 如何启动；`control.rs` = 用户如何控制；`monitor.rs` = 进程实际发生了什么；进程记录 SQL 归 `store.rs`（已有），不在 `monitor.rs` 拼 SQL；`metrics.rs` 只读 OS 指标按节流写回
- [ ] `mod.rs` 保留 `RuntimeProcessManager`、`RuntimeProcessDeps`、共享状态与构造；re-export 兼容 `runtime::RuntimeProcessManager`（§5.3）

## 架构 / 性能注意点

- 生命周期不变（§6 Phase 2）：`Created → Preparing → Resolving → Building → Starting → Running → Stopping → Stopped/Failed`。
- **F-12 红线**：reader 全部断开后 monitor 仍能轮询取消和超时，不得阻塞在 `child.wait()`；按字节读 + `from_utf8_lossy`（GBK 兼容）。
- Windows：`terminate_process` + 进程树终止走 `process/kill_tree.rs`；平台分支不得散落到多个 manager 子模块（全局约束 §6）。
- PID 与 start_time 校验（防 PID 复用误杀）不移动到不安全的公共层、不扩大可见性（§6 Phase 2）。
- 指标采样不得为采样创建额外子进程（§6 Phase 2）。
- `infer_main_class` 等被根 AGENTS.md 引用的符号移动后需文档同步（§9 第 6 条）。

## 验收标准

- [ ] 生命周期状态机行为不变（既有测试全绿，含 monitor 取消/超时竞态）
- [ ] reader 断开后不阻塞 `child.wait()`（F-12 回归测试通过）
- [ ] Windows 终止策略不变（`cfg(windows)` 分支保留，kill_tree 路径不变）
- [ ] Stop/Force Kill/孤儿接管（reconcile）行为不变
- [ ] 四件套全绿；`detect_changes()` 无超预期影响；AGENTS.md 等文档引用已同步

## 进度

### 状态

- 当前状态：未开始
- 最近更新：—

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|

### 子任务清单

- [ ] `types.rs`（ActiveProcess / Progress / MonitorOutcome / Prepared + 退出分类）
- [ ] `metrics.rs`（sampler 线程 + DB 指标刷新）
- [ ] `output.rs`（BuildLogSink / 启动横幅 / 端口探测）
- [ ] `monitor.rs`（monitor / finalize_exit / 启动宽限）
- [ ] `control.rs`（stop / kill / restart / reconcile）
- [ ] `start.rs`（prepare / run_build / spawn 前流程）
- [ ] 测试归位与四件套验证 + 文档同步
