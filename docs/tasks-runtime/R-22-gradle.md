# R-22 Gradle 支持

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[R-09 Build Engine](./R-09-build-engine.md)（Build Engine 抽象）。

| 项 | 值 |
|---|---|
| 阶段 | Phase 3 · 扩展运行时 |
| 优先级 | P2 |
| 状态 | ⬜ 未开始 |
| 依赖 | R-09 |
| 对应源文档 | §6 P1/P2 功能范围（Gradle）、§50 架构总览（Maven / Gradle）、§92 MVP 暂不实现 |

## 目标

在 Build Engine 抽象上实现 Gradle 支持：项目发现识别 `build.gradle(.kts)`，构建/运行走 Gradle 自身能力，与 Maven 路径并列。

## 需求范围

- [ ] 项目发现扩展：识别 `build.gradle` / `build.gradle.kts` / `settings.gradle(.kts)` 与多项目结构
- [ ] Gradle Wrapper 优先（`gradlew` / `gradlew.bat`），其次配置/系统 Gradle
- [ ] 依赖与模块信息获取：驱动 Gradle 自身任务输出（如 `dependencies` / `projects`），**不自行解析 Gradle 脚本语义**
- [ ] Spring Boot 检测扩展：`bootJar` / `bootRun` 任务与 `@SpringBootApplication` 扫描复用 R-06
- [ ] Build / Run 接入：`gradlew build` / `bootRun` / jar 产物运行
- [ ] Source Mapping 策略：Gradle 复合构建（composite build）或源码替换方案调研后落地，设计决策记入时间线

## 架构 / 性能注意点

- **不解析 Groovy/Kotlin DSL**（全局约束 §1：不重新实现构建工具）；一切语义信息经 Gradle CLI 输出获得。
- Gradle 输出解析要有版本兼容性测试（不同 Gradle 版本输出格式差异）。
- 与 Maven 共用 Closure / Reactor / Launcher 概念时，差异点在任务文档显式说明。

## 验收标准

- [ ] 样例 Gradle Spring Boot 项目发现 → 构建 → 启动 → 停止闭环可用
- [ ] wrapper 优先级正确
- [ ] Gradle 失败输出归类为结构化 `BuildFailed`

## 进度

### 状态

- 当前状态：未开始
- 最近更新：—

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|

### 子任务清单

- [ ] Gradle 项目发现
- [ ] Wrapper / 版本检测
- [ ] 依赖与模块信息获取（CLI 驱动）
- [ ] Build / Run 接入 Build Engine
- [ ] Source Mapping 方案落地
- [ ] 单元/集成测试
