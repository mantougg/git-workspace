# R-09 Build Engine

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[R-03 Runtime Closure 与 Synthetic Reactor](./R-03-runtime-closure-reactor.md)、[R-05 Maven 检测与执行策略](./R-05-maven-detection.md)、[R-07 Runtime 配置体系](./R-07-runtime-config.md)。

| 项 | 值 |
|---|---|
| 阶段 | Phase 1 · 构建运行闭环 |
| 优先级 | P0 |
| 状态 | ⬜ 未开始 |
| 依赖 | R-03, R-05, R-07 |
| 对应源文档 | §28 Build 流程、§29 Start 流程抽象、§30 Spring Boot Run Strategy、§73 Build Cache Strategy |

## 目标

实现 Build Engine：把「校验 → 解析 → 闭包 → Reactor → Maven 构建 → Classpath」串成可观测的流水线，并抽象 `Build Engine → Artifact/Classpath → Runtime Launcher → java` 的启动路径（§29），不把 `mvn spring-boot:run` 作为唯一实现。

## 需求范围

- [ ] Build 流水线（§28）：Validate JDK → Validate Maven → Parse POM → Resolve Dependency → Build Runtime Closure → Generate Synthetic Reactor → Maven Build → Generate Classpath
- [ ] 三种 Run Strategy（§30）：`Maven Run`（mvn spring-boot:run）/ `Package Run`（mvn package + java -jar）/ `Classpath Run`（mvn compile + 解析 classpath + java -cp）
- [ ] 默认策略：Development → Classpath / Maven Run，Production-like → Package Run；最终默认以 R-08 Benchmark 数据校准
- [ ] Classpath 解析：`dependency:build-classpath` 或等效手段，产物交给 R-10 Launcher
- [ ] Build Engine 接口抽象：Maven 先行实现，为 mvnd（R-18）/ Gradle（R-22）预留
- [ ] 构建输出实时流式转发（stdout/stderr → R-11 日志引擎 + R-12 进度事件）
- [ ] 构建范围只含 Runtime Closure 模块；利用 Maven 原生 `~/.m2` 缓存，**不自行实现 Java 编译缓存**（§73 第一阶段）

## 架构 / 性能注意点

- 构建全程走 Runtime Task Scheduler 限流（全局约束 §6），禁止多应用同时全量 Build。
- Maven 子进程：超时策略、取消传播（用户取消构建要能杀掉进程树）、输出无上限缓冲（直接管道转发）。
- Classpath Run 的 classpath 生成结果按模块缓存，输入未变时复用。
- `BuildFailed` 错误必须携带：失败模块、Maven 退出码、日志尾部上下文（供 R-14 可行动提示）。

## 验收标准

- [ ] 样例 Spring Boot 应用三种 Run Strategy 均可构建成功
- [ ] 二次构建耗时不高于首次（Maven 原生缓存生效），数据由 R-08 采集
- [ ] 构建中取消，Maven 进程树被完整终止，无残留
- [ ] 构建失败返回结构化 `BuildFailed`（模块 + 退出码 + 日志尾部）
- [ ] 单仓与跨 repo（Synthetic Reactor）两种拓扑均可构建

## 进度

### 状态

- 当前状态：未开始
- 最近更新：—

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|

### 子任务清单

- [ ] Build 流水线编排（校验 → 构建 → classpath）
- [ ] 三种 Run Strategy 实现
- [ ] Classpath 解析与缓存
- [ ] 输出流转发与取消传播
- [ ] BuildFailed 结构化错误
- [ ] 单元/集成测试（含真实样例工程构建）
