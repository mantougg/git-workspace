# R-10 Runtime Launcher 与 Process Manager

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[R-09 Build Engine](./R-09-build-engine.md)、[R-04 JDK 检测与 JDK Manager](./R-04-jdk-manager.md)、[R-06 Spring Boot 应用发现](./R-06-spring-boot-detection.md)。

| 项 | 值 |
|---|---|
| 阶段 | Phase 1 · 构建运行闭环 |
| 优先级 | P0 |
| 状态 | ✅ 已完成 |
| 依赖 | R-09, R-04, R-06 |
| 对应源文档 | §27 Runtime 生命周期、§29 Start 流程、§33 Process Manager、§34 Process 控制 |

## 目标

实现应用启动器与进程管理：按配置组装 `java` 命令启动 Spring Boot 应用，维护 Runtime 生命周期状态机，提供 Start / Stop / Restart / Kill 全闭环进程控制与运行指标。

## 需求范围

- [x] 生命周期状态机（§27）：`Created → Preparing → Resolving → Building → Starting → Running → Stopping → Stopped`，异常 `Failed`；状态迁移全程发事件
- [x] Launcher：按 Runtime 配置组装 `java [-cp/-jar] mainClass vmOptions programArguments env`，使用绑定 JDK（R-04）与 Main Class（R-06）
- [x] Process Manager（§33）：每个运行应用一条记录——PID / Status / CPU / Memory / Ports / Start Time / Uptime，落 `runtime_processes` 表
- [x] 进程控制（§34）：Start / Stop（优雅，先发 SIGTERM 等效）/ Restart / Kill / Force Kill（**二次确认**，全局约束 §3）
- [x] 进程托管：GitWorkspace 退出/崩溃后的孤儿 java 进程检测与回收；退出码捕获；异常退出识别（`ProcessCrashed`）
- [x] 启动命令完整参数可预览、可追溯（全局约束 §3）
- [x] Windows / Unix 信号语义与进程树终止差异处理

## 架构 / 性能注意点

- 进程状态以**实际 OS 进程为准**，DB 记录只是缓存；启动时核对 PID 存活，防止状态漂移。
- 指标采样（CPU/内存）低频节流，不为采样 fork 额外进程；读取 OS 计数器即可。
- Stop 要终止整个进程树（Maven 启动的 java 子进程场景）；Force Kill 是最后手段。
- 应用启动期间的端口信息来自配置/日志探测，不做端口扫描（端口管理归 R-16）。

## 验收标准

- [x] 样例应用 Start → Running → Stop → Stopped 全闭环，状态迁移事件正确
- [x] Restart 等价于 Stop + Start 且复用最近构建产物
- [x] GitWorkspace 被强制关闭后重启，能识别并接管/清理遗留 java 进程
- [x] 应用崩溃（非零退出）状态进入 `Failed` 并带退出码
- [x] Force Kill 有确认交互；进程树无孤儿残留（测试断言）

## 进度

### 状态

- 当前状态：已完成
- 最近更新：2026-08-23 完成开发（验收全部通过）

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-23 | 🟦 开始开发 | 启动 R-10：生命周期状态机与事件、Launcher 命令组装（复用 R-09 LaunchPlan + R-04 JDK + R-06 Main Class）、Process Manager（`runtime_processes` 表 + 指标节流采样）、Stop/Kill 进程树终止、孤儿进程检测回收 |
| 2026-08-23 | ✅ 完成开发 | 新模块 `runtime/launch/`（lifecycle/launcher/store/manager）：状态机 Created→…→Stopped + Failed（含 skip-build 直达 Starting、自然退出边）；`runtime_processes` 表（SCHEMA V12，含 pid+pid_start_time 防 PID 复用）；Launcher 经 `LaunchRunner` seam 组装命令并注入 `GITWORKSPACE_PROCESS_ID`/`GITWORKSPACE_RUNTIME_NAME` 孤儿托管标记；Stop=SIGTERM→grace(默认10s)→`kill_process_tree`，Force Kill 需 `confirmed=true`；Restart=Stop+Start(skip_build 复用内存 launch_cache)；`reconcile_on_startup` 对账非终态行（活→adopted 接管，死→Failed/Stopped）；sampler 2s 读 OS 计数器不 fork，每 5 拍落 DB；端口/启动完成只读日志正则（不扫端口，R-16 归口）；R-06 mainClass 缺省时 `infer_main_class` 回退经 `BuildOptions.main_class_override` 注入（不改用户配置文件）；R-04 遗留验收「绑定 JDK 实际用于启动」由 `bound_jdk_is_used_for_launch_command` 覆盖。注意点：Resolving→Building 紧邻 `execute_build` 前置位以避免 PhaseSink 回调持 DB 锁死锁；单连接写序列化为已知取舍（reconcile 兜底）。验证：`cargo check --all-targets` 干净；`cargo clippy --all-targets --all-features` 新增文件零警告；`cargo test runtime::launch` 31 passed（含 fake 全闭环/crash/early-exit/Conflict/restart 复用/R-06 推断/classify_exit 表，unix 真实进程的 SIGTERM 优雅/忽略升级杀树/force-kill 无孤儿/孤儿接管/指标事件，及 real_maven 集成：真实 spring-boot-starter-web 3.2.5 应用 `--server.port=0` 全闭环 + 端口探测 + `--server.port=99999` 启动期崩溃→ProcessStartFailed+Failed+非零退出码）；`cargo test` 全量 354 passed / 2 ignored（`maven::settings::tests::resolve_uses_settings_when_present` 为本机 `~/.m2/settings.xml` 显式设置 localRepository 导致的既有环境性失败，与 R-10 无关）。R-08 Application Start benchmark 字段接入不在本任务验收内，留作后续。IPC 命令与前端按 R-09 先例留给 R-12/R-13（事件走内部 `RuntimeEventSink`，R-12 桥接 Tauri） |

### 子任务清单

- [x] 生命周期状态机 + 事件
- [x] Launcher 命令组装与启动
- [x] Process Manager（指标采集 + DB 记录）
- [x] Stop/Kill 进程树终止（跨平台）
- [x] 孤儿进程检测与回收
- [x] 单元/集成测试（真实启动样例应用）
