# B-03 拆 RuntimeProcessManager（manager.rs → manager/）

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[B-01](./B-01-baseline-test-extraction.md)（测试已外移；建议在 [B-02](./B-02-runtime-service.md) 完成后开始）。设计约束见 [backend-module-split-plan.md](../backend-module-split-plan.md) §4.2、§6 Phase 2。本任务并发与平台风险最高，严格按顺序小步移动。

| 项 | 值 |
|---|---|
| 阶段 | Phase 2 · Runtime 核心 |
| 优先级 | P0 |
| 状态 | ✅ 已完成 |
| 依赖 | B-01（建议 B-02 后） |
| 对应设计文档 | §2.2 ProcessManager 问题、§4.2 目标目录、§6 Phase 2 |

## 目标

把约 1,400 行生产代码的 RuntimeProcessManager 按「状态机 / 控制 / 监控 / 指标 / 输出」分离到 `runtime/launch/manager/` 子模块；生命周期状态迁移仍由 `RuntimeProcessManager` 统一执行，不出现多个可修改生命周期的入口。

## 需求范围

- [x] 目标结构（§4.2）：`manager/{mod.rs, types.rs, start.rs, control.rs, monitor.rs, metrics.rs, output.rs, tests.rs}`
- [x] 迁移顺序（§6 Phase 2，先纯逻辑后高风险）：纯类型与退出分类（`types.rs`）→ metrics sampler（`metrics.rs`）→ output sink 与启动探测（`output.rs`）→ monitor/finalize（`monitor.rs`）→ stop/kill/reconcile（`control.rs`）→ 最后 start/build 准备流程（`start.rs`）——每步 `check --tests` 门禁，最终统一四件套
- [x] 职责边界（§4.2）：`start.rs` = 如何启动；`control.rs` = 用户如何控制；`monitor.rs` = 进程实际发生了什么；进程记录 SQL 归 `store.rs`（已有），不在 `monitor.rs` 拼 SQL；`metrics.rs` 只读 OS 指标按节流写回
- [x] `mod.rs` 保留 `RuntimeProcessManager`、`RuntimeProcessDeps`、共享状态与构造（294 行：struct/ctor/Drop + transit 状态迁移核心 + 查询 + re-export）；re-export 兼容 `runtime::RuntimeProcessManager`（§5.3；`StartOptions`/`EnvironmentOverrides`/`DEFAULT_*` 经 `pub use types::` 保持 `launch::*` 与 `runtime::*` 双路径）

## 架构 / 性能注意点

- 生命周期不变（§6 Phase 2）：`Created → Preparing → Resolving → Building → Starting → Running → Stopping → Stopped/Failed` ✓（`transit`/`transit_lenient` 状态迁移核心留在 mod.rs，由单一类型统一执行）
- **F-12 红线**：reader 全部断开后 monitor 仍能轮询取消和超时，不得阻塞在 `child.wait()`；按字节读 + `from_utf8_lossy`（GBK 兼容）✓（monitor.rs 逐字搬迁，`stop_kills_sigterm_ignoring_process_that_closed_streams` 等 F-12 回归测试通过）
- Windows：`terminate_process` + 进程树终止走 `process/kill_tree.rs`；平台分支不得散落到多个 manager 子模块（全局约束 §6）✓（`cfg(windows)` 分支随用例与实现留在原位置）
- PID 与 start_time 校验（防 PID 复用误杀）不移动到不安全的公共层、不扩大可见性（§6 Phase 2）✓（`ActiveProcess` 等私有类型以 `pub(super)` 共享于 manager 内部，未出现新的 crate 级 `pub`）
- 指标采样不得为采样创建额外子进程（§6 Phase 2）✓（sampler 仍为 sysinfo 进程内读取）
- `infer_main_class` 等被根 AGENTS.md 引用的符号移动后需文档同步（§9 第 6 条）✓（AGENTS.md 已更新为 `manager/start.rs::infer_main_class`）

## 验收标准

- [x] 生命周期状态机行为不变（既有测试全绿，含 monitor 取消/超时竞态——`service::` 与 `manager::tests::` 全部通过）
- [x] reader 断开后不阻塞 `child.wait()`（F-12 回归测试通过）
- [x] Windows 终止策略不变（`cfg(windows)` 分支保留，kill_tree 路径不变）
- [x] Stop/Force Kill/孤儿接管（reconcile）行为不变
- [x] 四件套全绿；`detect_changes()` 无超预期影响；AGENTS.md 等文档引用已同步——与 B-01/B-02 相同边界口径：`check` 绿；`test --lib` 495 总数不变（490 通过 / 2 失败 / 3 忽略，仅剩既有 `maven::settings` 环境失败）；clippy 全目标 103 与拆分前持平、manager 新文件零 lint（tests.rs 的 `manual_inspect` 为 B-01 已归因既有项）；7 个新文件 fmt 零 hunk。GitNexus MCP 本会话不可用：以源码搜索影响分析 + git diff 范围审计替代（diff 仅含 `launch/manager/` 内文件与任务文档/AGENTS.md 引用行）

## 进度

### 状态

- 当前状态：已完成
- 最近更新：2026-08-29，全部需求范围完成并通过验收

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-29 | 🟦 开始开发 | 前置 B-01/B-02 完成。按 §4.2 目标结构 + §6 Phase 2 顺序（types → metrics → output → monitor → control → start）执行。 |
| 2026-08-29 | 🟦 影响分析（GitNexus MCP 不可用，源码搜索兜底） | 公共面：`RuntimeProcessManager`/`RuntimeProcessDeps`/`StartOptions`/`EnvironmentOverrides`/`DEFAULT_START_GRACE`/`DEFAULT_STOP_GRACE`/`DEFAULT_SAMPLE_INTERVAL`（经 `launch/mod.rs` re-export）。调用方：`service`（with_deps/stop_runtime/get_process/list_processes/reconcile_on_startup/cancellation.rs）、`watch.rs`（Arc 共享 + test 用 new）、`task/`（trait 装配）。私有类型（ActiveProcess/Progress/MonitorOutcome/CachedLaunch/Prepared/PidWait/RunWait/Built/classify_exit）随 `types.rs` 上移为 `pub(super)`——manager 内部共享，未扩大 crate 级可见性。 |
| 2026-08-29 | 🟦 六步迁移（每组后 check --tests） | Step1 types（纯类型 + 退出分类，成员 `pub(super)`）；Step2 metrics（DB_FLUSH_EVERY_TICKS + ensure_sampler/sampler_loop，`ensure_sampler` 改 `pub(super)`）；Step3 output（open_log_session 改 `pub(super)` + BuildLogSink/启动横幅正则）；Step4 monitor（spawn_monitor/spawn_adopted_monitor/finalize_exit/finish_early_exit/wait 四原语 + ADOPT_POLL_INTERVAL，均 `pub(super)`）；Step5 control（stop/stop_runtime/signal_build_cancel/kill/restart/reconcile_on_startup/stop_unmanaged，全为 pub 无需调整）；Step6 start（start/start_inner/prepare/infer_main_class/run_build/abort_before_spawn）。方法体逐字搬迁；`super::port_preflight` 在 start.rs 中改为 `crate::runtime::launch::port_preflight`（层级变化）。 |
| 2026-08-29 | 🟦 导入修剪与 mod.rs 收敛 | 按编译器警告迭代修剪（146→1→0）；tests.rs 补 4 个显式导入（Instant/OutputStream/LogPhase/RunStrategy，不再依赖 mod.rs 私有导入透传）；修剪与 seed_cached_launch（cfg(test)）的 LaunchPlan/RunStrategy 导入冲突，最终以 `#[cfg(test)] use` 解决；mod.rs 清孤儿注释、更新模块文档说明七文件布局，最终 294 行。 |
| 2026-08-29 | ✅ 完成验收 | `check` 绿；`test --lib` 495 总数不变（490 通过 / 2 失败 / 3 忽略——仅剩既有 `maven::settings` 环境失败，本机 `~/.m2` 干扰已归因）；F-12 回归、生命周期竞态、reconcile、Windows 分支测试全部通过；clippy 全目标 103 与拆分前持平、manager 新文件零 lint（tests.rs `manual_inspect` 为 B-01 已归因既有项）；7 个新文件 fmt 零 hunk。AGENTS.md `infer_main_class` 引用已同步至 `manager/start.rs`。GitNexus MCP 不可用：以源码搜索影响分析 + diff 范围审计替代 `detect_changes`。 |

### 子任务清单

- [x] `types.rs`（ActiveProcess / Progress / MonitorOutcome / Prepared + 退出分类）
- [x] `metrics.rs`（sampler 线程 + DB 指标刷新）
- [x] `output.rs`（BuildLogSink / 启动横幅 / 端口探测）
- [x] `monitor.rs`（monitor / finalize_exit / 启动宽限）
- [x] `control.rs`（stop / kill / restart / reconcile）
- [x] `start.rs`（prepare / run_build / spawn 前流程）
- [x] 测试归位与四件套验证 + 文档同步
