# GitWorkspace Runtime Workspace 任务拆解总览

> 来源：`docs/大型企业项目轻量级开发运行工作台.md`（V1.0，下称「源文档」，各任务表格中的 `§` 号指其章节号）。
> 拆分原则：**按功能模块拆分**，每个任务一个独立文档（同目录下 `R-XX-<slug>.md`），可独立跟踪进度与验收。
> 本文件是唯一的总进度索引；每个任务文档内另有自己的「进度」章节。
>
> 编号用 **R-XX**（Runtime），与 Git Workspace 任务（`docs/tasks/` 的 T-XX）区分。
> Runtime Workspace 与 Git Workspace 同属 GitWorkspace 产品，**基础设施复用而非重建**：Scanner（T-01）、SQLite 数据层（T-03）、Task Queue（T-05）、File Watcher（T-06）、Benchmark（T-07）、错误/日志/Secret（T-08）。
>
> 横切约束分两层叠加：本目录 [00-全局开发约束.md](./00-全局开发约束.md)（Runtime 特有）+ 必要时 `../tasks/00-全局开发约束.md`（Git 联动类任务）；各任务文档顶部标注了各自的最小加载集（全局约束 + 直接依赖）。

---

## 状态图例

| 图标 | 状态 |
|---|---|
| ⬜ | 未开始 |
| 🟦 | 进行中 |
| ✅ | 已完成 |
| ⏸️ | 暂停 / 阻塞 |

## 总体进度

- 任务总数：**26**
- 已完成：**19** · 进行中：**0** · 未开始：**7**
- 完成度：**19 / 26（73%）**

## MVP 口径（源文档 §91 / §92）

- MVP = **Phase 0 + Phase 1 全部**（发现 → 解析 → Spring Boot 检测 → 选 JDK/Profile → 依赖分析 → 本地源码映射 → Closure → Build → Start → Logs → Stop）。
- MVP 暂不实现：Gradle / Docker / Kubernetes / Debug / JMX / 复杂 Hot Reload / AI / 复杂 Maven Resolver。
- **不要自己重新实现 Maven**（贯穿所有构建类任务）。

---

## 阶段与任务索引

### Phase 0 · Runtime 基础设施（前置，P0，8 个）

> 对应源文档 Phase 1（§93）的「发现/解析/映射/闭包/检测/配置」部分；Benchmark 提前到本阶段以校准全部性能目标（与 T-07 同策略）。

| 编号 | 任务 | 优先级 | 状态 | 依赖 | 文档 |
|---|---|---|---|---|---|
| R-01 | Maven 项目发现与 POM 解析 | P0 | ✅ | —（复用 T-01） | [R-01-maven-discovery.md](./R-01-maven-discovery.md) |
| R-02 | Maven 依赖图与 Workspace Source Mapping（含 SQLite 索引） | P0 | ✅ | R-01, T-03 | [R-02-dependency-graph-source-mapping.md](./R-02-dependency-graph-source-mapping.md) |
| R-03 | Runtime Closure 与 Synthetic Reactor | P0 | ✅ | R-02 | [R-03-runtime-closure-reactor.md](./R-03-runtime-closure-reactor.md) |
| R-04 | JDK 检测与 JDK Manager | P0 | ✅ | — | [R-04-jdk-manager.md](./R-04-jdk-manager.md) |
| R-05 | Maven 检测与执行策略（mvn / mvnw） | P0 | ✅ | — | [R-05-maven-detection.md](./R-05-maven-detection.md) |
| R-06 | Spring Boot 应用发现与 Main Class 推断 | P0 | ✅ | R-01 | [R-06-spring-boot-detection.md](./R-06-spring-boot-detection.md) |
| R-07 | Runtime 配置体系（Config / 环境变量 / 配置分层） | P0 | ✅ | R-02 | [R-07-runtime-config.md](./R-07-runtime-config.md) |
| R-08 | Runtime Benchmark 与性能基线 | P0 | ✅ | R-01, T-07 | [R-08-runtime-benchmark.md](./R-08-runtime-benchmark.md) |

### Phase 1 · 构建运行闭环（P0，6 个）

| 编号 | 任务 | 优先级 | 状态 | 依赖 | 文档 |
|---|---|---|---|---|---|
| R-09 | Build Engine（Build 流程 / Run Strategy） | P0 | ✅ | R-03, R-05, R-07 | [R-09-build-engine.md](./R-09-build-engine.md) |
| R-10 | Runtime Launcher 与 Process Manager | P0 | ✅ | R-09, R-04, R-06 | [R-10-launcher-process-manager.md](./R-10-launcher-process-manager.md) |
| R-11 | Runtime 日志引擎 | P0 | ✅ | R-10 | [R-11-log-engine.md](./R-11-log-engine.md) |
| R-12 | Runtime IPC / Event API 与 Task Engine 集成 | P0 | ✅ | R-09, R-10, R-11, T-05 | [R-12-ipc-task-integration.md](./R-12-ipc-task-integration.md) |
| R-13 | Runtime UI（Dashboard / 依赖映射 / Scope / 配置 / 日志） | P0 | ✅ | R-12, R-02, R-03 | [R-13-runtime-ui.md](./R-13-runtime-ui.md) |
| R-14 | Runtime 安全与错误处理 | P0 | ✅ | R-10, R-11, T-08 | [R-14-security-errors.md](./R-14-security-errors.md) |

### Phase 2 · 多服务与效率（P1，7 个）

| 编号 | 任务 | 优先级 | 状态 | 依赖 | 文档 |
|---|---|---|---|---|---|
| R-15 | Multi-Service Runtime 与 Runtime Environment | P1 | ✅ | R-10, R-13 | [R-15-multi-service.md](./R-15-multi-service.md) |
| R-16 | Health Check 与 Port Manager | P1 | ✅ | R-10 | [R-16-health-port.md](./R-16-health-port.md) |
| R-17 | File Watch / 增量构建 / 自动重启 | P1 | ✅ | R-09, R-02, T-06 | [R-17-watch-incremental-restart.md](./R-17-watch-incremental-restart.md) |
| R-18 | 构建加速：mvnd 与构建缓存分级 | P1 | ✅ | R-09, R-05 | [R-18-mvnd-build-cache.md](./R-18-mvnd-build-cache.md) |
| R-19 | Runtime Templates | P1 | ✅ | R-07 | [R-19-runtime-templates.md](./R-19-runtime-templates.md) |
| R-20 | Runtime 依赖图可视化 | P1 | ⬜ | R-02, R-13 | [R-20-dependency-visualization.md](./R-20-dependency-visualization.md) |
| R-21 | Git 联动（Status 提示 / Branch 联动 / 操作保护） | P1 | ⬜ | R-10, T-02, T-09 | [R-21-git-integration.md](./R-21-git-integration.md) |

### Phase 3 · 扩展运行时（P2，5 个）

| 编号 | 任务 | 优先级 | 状态 | 依赖 | 文档 |
|---|---|---|---|---|---|
| R-22 | Gradle 支持 | P2 | ⬜ | R-09 | [R-22-gradle.md](./R-22-gradle.md) |
| R-23 | Debug 与 IDE 协同（JDWP / Attach） | P2 | ⬜ | R-10 | [R-23-debug.md](./R-23-debug.md) |
| R-24 | Docker / Kubernetes Runtime | P2 | ⬜ | R-10 | [R-24-docker-k8s.md](./R-24-docker-k8s.md) |
| R-25 | JVM 监控 / JMX / Runtime Metrics | P2 | ⬜ | R-10 | [R-25-jvm-monitoring.md](./R-25-jvm-monitoring.md) |
| R-26 | AI Runtime Assistant | P2 | ⬜ | R-11, T-08 | [R-26-ai-runtime-assistant.md](./R-26-ai-runtime-assistant.md) |

---

## 关键依赖链

```text
R-01 发现/解析 ──► R-02 依赖图/源码映射 ──► R-03 Closure/Reactor ──► R-09 Build Engine ──► R-10 Launcher/Process
                     │                            │                     ▲                    │        │
                     │                            └──► R-20 可视化       │                    │        ├──► R-11 日志 ──► R-26 AI
R-04 JDK ────────────┼──────────────────────────────────────────────────┤                    │        ├──► R-15 多服务
R-05 Maven 检测 ─────┴──────────────────────────────────────────────────┘                    │        ├──► R-16 健康/端口
R-06 Spring Boot 发现 ──────────────────────────────────────────────► R-10                  │        ├──► R-17 监听/增量/重启（+R-09）
R-07 配置 ──────────────────────────────────────────────────► R-09 / R-10                   │        └──► R-21 Git 联动
                                                                                            ▼
                                                                   R-12 IPC/Task 集成 ──► R-13 Runtime UI
                                                                   R-14 安全与错误（P0 收尾，横切 R-10/R-11）
R-08 Benchmark（贯穿，校准所有性能目标）
```

- **Phase 0 全部完成后**才进入构建运行闭环（发现/映射/闭包不稳，上层返工成本高）。
- **R-08 Benchmark** 贯穿始终：每个性能相关验收标准都必须以真实 Benchmark 数据为准。
- MVP 完成的判定 = Phase 0 + Phase 1（R-01 ~ R-14）全部 ✅。

---

## 维护规范

1. 更新任务状态时，**同时更新**本 README 总表与对应任务文档「进度」章节，二者保持一致。
2. 完成任务需满足该文档的「验收标准」，并在其进度时间线追加一行记录。
3. 新增/调整任务时，重新编号并同步依赖字段。
4. 状态只允许在 ⬜ → 🟦 → ✅（或 ⏸️）之间流转，回退需在时间线注明原因。
5. 全局横切约束统一记录在 `00-全局开发约束.md`；各任务文档的「架构/性能注意点」只写该任务特有内容，与全局约束叠加，不重复。
