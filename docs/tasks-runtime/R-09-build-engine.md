# R-09 Build Engine

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[R-03 Runtime Closure 与 Synthetic Reactor](./R-03-runtime-closure-reactor.md)、[R-05 Maven 检测与执行策略](./R-05-maven-detection.md)、[R-07 Runtime 配置体系](./R-07-runtime-config.md)。

| 项 | 值 |
|---|---|
| 阶段 | Phase 1 · 构建运行闭环 |
| 优先级 | P0 |
| 状态 | ✅ 已完成 |
| 依赖 | R-03, R-05, R-07 |
| 对应源文档 | §28 Build 流程、§29 Start 流程抽象、§30 Spring Boot Run Strategy、§73 Build Cache Strategy |

## 目标

实现 Build Engine：把「校验 → 解析 → 闭包 → Reactor → Maven 构建 → Classpath」串成可观测的流水线，并抽象 `Build Engine → Artifact/Classpath → Runtime Launcher → java` 的启动路径（§29），不把 `mvn spring-boot:run` 作为唯一实现。

## 需求范围

- [x] Build 流水线（§28）：Validate JDK → Validate Maven → Parse POM → Resolve Dependency → Build Runtime Closure → Generate Synthetic Reactor → Maven Build → Generate Classpath
- [x] 三种 Run Strategy（§30）：`Maven Run`（mvn spring-boot:run）/ `Package Run`（mvn package + java -jar）/ `Classpath Run`（mvn compile + 解析 classpath + java -cp）
- [x] 默认策略：Development → Classpath / Maven Run，Production-like → Package Run；最终默认以 R-08 Benchmark 数据校准
- [x] Classpath 解析：`dependency:build-classpath` 或等效手段，产物交给 R-10 Launcher
- [x] Build Engine 接口抽象：Maven 先行实现，为 mvnd（R-18）/ Gradle（R-22）预留
- [x] 构建输出实时流式转发（stdout/stderr → R-11 日志引擎 + R-12 进度事件）
- [x] 构建范围只含 Runtime Closure 模块；利用 Maven 原生 `~/.m2` 缓存，**不自行实现 Java 编译缓存**（§73 第一阶段）

## 架构 / 性能注意点

- 构建全程走 Runtime Task Scheduler 限流（全局约束 §6），禁止多应用同时全量 Build。
- Maven 子进程：超时策略、取消传播（用户取消构建要能杀掉进程树）、输出无上限缓冲（直接管道转发）。
- Classpath Run 的 classpath 生成结果按模块缓存，输入未变时复用。
- `BuildFailed` 错误必须携带：失败模块、Maven 退出码、日志尾部上下文（供 R-14 可行动提示）。

## 验收标准

- [x] 样例 Spring Boot 应用三种 Run Strategy 均可构建成功
- [x] 二次构建耗时不高于首次（Maven 原生缓存生效），数据由 R-08 采集
- [x] 构建中取消，Maven 进程树被完整终止，无残留
- [x] 构建失败返回结构化 `BuildFailed`（模块 + 退出码 + 日志尾部）
- [x] 单仓与跨 repo（Synthetic Reactor）两种拓扑均可构建

## 进度

### 状态

- 当前状态：已完成
- 最近更新：2026-08-21 完成

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-21 | 🟦 开始开发 | 启动 R-09：Build 流水线编排、三种 Run Strategy、Classpath 解析与缓存、输出流转发与取消传播、BuildFailed 结构化错误 |
| 2026-08-21 | ✅ 完成 | 新增 `process/`（streaming 流式转发 + kill_tree 进程树终止）、`runtime/build/`（pipeline 九步编排、三种 Run Strategy 与 LaunchPlan、classpath 按模块磁盘缓存（pom_hash+图指纹+本地仓库路径）、BuildScheduler 并发闸默认 2、MavenRunner seam）、`java/resolve.rs`（配置字符串→JDK）、`AppError::BuildFailed`（module/exitCode/logTail 经 details JSON 穿透 IPC + golden 样例）；输出脱敏复用 T-08 共享规则；benchmark 新增 `build` 阶段实测二次构建 3621ms→1529ms（-57.8%）。验证：`cargo test` 321 passed / 2 ignored（含 5 个真实 mvn 集成测试：三策略构建、跨仓 Synthetic Reactor、取消杀树无残留；`maven::settings` 1 个存量失败系本机 `~/.m2/settings.xml` 显式固定 localRepository 的环境问题，与 R-09 无关）、`cargo clippy --all-targets --all-features` 新文件零警告、`pnpm build` 通过。IPC 命令与前端由 R-12/R-13 接入（BuildOutputSink / LaunchPlan / 取消令牌已预留挂接点） |

### 子任务清单

- [x] Build 流水线编排（校验 → 构建 → classpath）
- [x] 三种 Run Strategy 实现
- [x] Classpath 解析与缓存
- [x] 输出流转发与取消传播
- [x] BuildFailed 结构化错误
- [x] 单元/集成测试（含真实样例工程构建）
