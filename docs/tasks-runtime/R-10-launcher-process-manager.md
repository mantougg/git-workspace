# R-10 Runtime Launcher 与 Process Manager

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[R-09 Build Engine](./R-09-build-engine.md)、[R-04 JDK 检测与 JDK Manager](./R-04-jdk-manager.md)、[R-06 Spring Boot 应用发现](./R-06-spring-boot-detection.md)。

| 项 | 值 |
|---|---|
| 阶段 | Phase 1 · 构建运行闭环 |
| 优先级 | P0 |
| 状态 | ⬜ 未开始 |
| 依赖 | R-09, R-04, R-06 |
| 对应源文档 | §27 Runtime 生命周期、§29 Start 流程、§33 Process Manager、§34 Process 控制 |

## 目标

实现应用启动器与进程管理：按配置组装 `java` 命令启动 Spring Boot 应用，维护 Runtime 生命周期状态机，提供 Start / Stop / Restart / Kill 全闭环进程控制与运行指标。

## 需求范围

- [ ] 生命周期状态机（§27）：`Created → Preparing → Resolving → Building → Starting → Running → Stopping → Stopped`，异常 `Failed`；状态迁移全程发事件
- [ ] Launcher：按 Runtime 配置组装 `java [-cp/-jar] mainClass vmOptions programArguments env`，使用绑定 JDK（R-04）与 Main Class（R-06）
- [ ] Process Manager（§33）：每个运行应用一条记录——PID / Status / CPU / Memory / Ports / Start Time / Uptime，落 `runtime_processes` 表
- [ ] 进程控制（§34）：Start / Stop（优雅，先发 SIGTERM 等效）/ Restart / Kill / Force Kill（**二次确认**，全局约束 §3）
- [ ] 进程托管：GitWorkspace 退出/崩溃后的孤儿 java 进程检测与回收；退出码捕获；异常退出识别（`ProcessCrashed`）
- [ ] 启动命令完整参数可预览、可追溯（全局约束 §3）
- [ ] Windows / Unix 信号语义与进程树终止差异处理

## 架构 / 性能注意点

- 进程状态以**实际 OS 进程为准**，DB 记录只是缓存；启动时核对 PID 存活，防止状态漂移。
- 指标采样（CPU/内存）低频节流，不为采样 fork 额外进程；读取 OS 计数器即可。
- Stop 要终止整个进程树（Maven 启动的 java 子进程场景）；Force Kill 是最后手段。
- 应用启动期间的端口信息来自配置/日志探测，不做端口扫描（端口管理归 R-16）。

## 验收标准

- [ ] 样例应用 Start → Running → Stop → Stopped 全闭环，状态迁移事件正确
- [ ] Restart 等价于 Stop + Start 且复用最近构建产物
- [ ] GitWorkspace 被强制关闭后重启，能识别并接管/清理遗留 java 进程
- [ ] 应用崩溃（非零退出）状态进入 `Failed` 并带退出码
- [ ] Force Kill 有确认交互；进程树无孤儿残留（测试断言）

## 进度

### 状态

- 当前状态：未开始
- 最近更新：—

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|

### 子任务清单

- [ ] 生命周期状态机 + 事件
- [ ] Launcher 命令组装与启动
- [ ] Process Manager（指标采集 + DB 记录）
- [ ] Stop/Kill 进程树终止（跨平台）
- [ ] 孤儿进程检测与回收
- [ ] 单元/集成测试（真实启动样例应用）
