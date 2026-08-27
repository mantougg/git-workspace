---
name: gitworkspace-runtime-dev
description: GitWorkspace Runtime 任务开发流程：如何读 docs/tasks-runtime/ 文档（总索引/全局约束/任务spec）开始与继续 Runtime（R-XX）任务开发、并同步进度。
---

# GitWorkspace Runtime 任务开发流程

本 skill 教你在 **GitWorkspace** 项目中，如何基于 `docs/tasks-runtime/` 的任务文档**开始开发**或**继续开发**某个 Runtime（R-XX）任务。

Runtime Workspace 是与 Git Workspace（`docs/tasks/` 的 T-XX）同库共生的第二个引擎：解决「不打开 IDEA 也能构建运行大型 Java / Spring Boot 多仓库工程」。两个引擎**共享基础设施**（Scanner/SQLite/Task Queue/File Watcher/Benchmark/错误日志 Secret，即 T-01/03/05/06/07/08），不重建。

## 文档地图

Runtime 需求已拆解在 `docs/tasks-runtime/` 下，共三类文档：

| 文件 | 作用 | 何时读 |
|---|---|---|
| `docs/tasks-runtime/README.md` | 总索引：26 个任务的阶段/优先级/状态/依赖总表 + 依赖链 + MVP 口径 + 维护规范 | 选任务、核对状态、判断是否 MVP 时 |
| `docs/tasks-runtime/00-全局开发约束.md` | Runtime 横切约束（不替代 IDEA / 不重实现 Maven / 用户项目只读 / 命令执行安全 / Secret / 性能 / 并发调度 / 配置分层 / 错误 / 网络边界 / Git 联动安全） | 任何 Runtime 任务开发前**必读** |
| `docs/tasks-runtime/R-XX-*.md` | 任务 spec：目标 / 需求范围 / 架构性能注意点 / 验收标准 / 进度 | 开发目标任务时 |

涉及 Git 联动（R-21）或改动共享基础设施时，`docs/tasks/00-全局开发约束.md` 一并生效（任务 spec 顶部「开发前必读」会标注）。

## 关键边界（贯穿所有 Runtime 任务）

1. **不替代 IDEA**：放弃代码编辑/补全/重构/索引；不为 Runtime 引入 Java AST / 代码索引设施。
2. **不重新实现 Maven / Gradle**：构建一律驱动外部 CLI（mvn / mvnw / mvnd / gradlew）；自研只做「发现、解析、映射、编排」。
3. **用户项目只读**：绝不修改用户 `pom.xml` / 源码 / Git 分支；所有运行时生成物只写 `.gitworkspace/`（默认加入 `.gitignore`，`runtimes/` 等可共享配置例外）。
4. **AI as Assistant**：Runtime AI（R-26）只建议/解释/分析，不自动改配置或代码。

## MVP 口径

- MVP = **Phase 0 + Phase 1 全部**（R-01 ~ R-14，共 14 个 P0 任务）：发现 → 解析 → Spring Boot 检测 → 选 JDK/Profile → 依赖分析 → 本地源码映射 → Closure → Build → Start → Logs → Stop。
- MVP 暂不实现：Gradle / Docker / Kubernetes / Debug / JMX / 复杂 Hot Reload / AI / 复杂 Maven Resolver。

## 开始开发一个新任务

1. 确定任务编号（用户指定，或从 README 总表选一个「无依赖」或「依赖均已就绪」的任务）。
2. 读 `README.md` 总表，确认该任务的状态、优先级、直接依赖；若依赖里含 T-XX，确认对应 T 任务状态。
3. 读 `00-全局开发约束.md`（必读，贯穿所有 Runtime 任务）。
4. 读目标任务文档顶部的「**开发前必读**」指针，按它列出的「直接依赖」加载依赖任务文档——**只读这几份，不要全读 26 份**。
5. 通读目标任务文档，明确：目标、需求范围（checklist）、验收标准。
6. 把任务状态 `⬜ → 🟦`（**同步**更新 README 总表 + 任务文档「进度」章节），并在时间线追加一行「开始开发」。
7. 开始实现。

## 继续开发（恢复一个进行中的任务）

1. 读目标任务文档「**进度**」章节：当前状态 + 时间线最后一条 + 子任务清单勾选情况。
2. 读 `README.md` 总表该任务行，核对两处状态一致（不一致时以任务文档为准，并修正 README）。
3. 从时间线最后一条记录恢复上下文，继续**未勾选的子任务**。

## 完成一个任务

1. 逐条核对「验收标准」，**全部满足**才算完成；性能类验收以 R-08 Benchmark 实测为准。
2. 运行相关测试/构建验证（`cargo test`、`cargo check`、`pnpm build` 等，按改动范围选择）；涉及 Synthetic Reactor 的任务需含真实 `mvn validate` 集成测试（找不到 mvn 时跳过并标注）。
3. 更新任务文档「进度」：状态 `→ ✅`，时间线追加一行（日期 + 结果 + 验证命令）。
4. 同步更新 README 总表该任务状态 `→ ✅`。
5. 若存在依赖此任务的下游任务，提示用户可开始下游；若该任务在 R-01~R-14 内，提示距 MVP 又近一步。

## 必须遵守

- **全局约束优先**：`00-全局开发约束.md`（Runtime 特有）是硬约束，任务文档「架构/性能注意点」是叠加的特有约束；若冲突，在任务文档显式说明原因与边界。
- **进度与状态规则以 README 为准**：进度两处同步、状态流转的权威定义在 `README.md` 末尾「维护规范」；本 skill 各步骤按它执行，不另立规则。
- **代码落点**（源文档 §51）：Rust 后端在 `src-tauri/src/{runtime,maven,java,process}/`（按任务归属落对应模块），Vue 前端在 `src/{api,components,composables,stores,views}/`。
- **基础设施复用边界**：仓库扫描复用 T-01 Scanner、SQLite 数据层复用 T-03、任务队列复用 T-05/T-24 DAG、文件监听复用 T-06、Benchmark 复用 T-07、错误/日志/Secret 复用 T-08——**禁止另起一套**（全局约束 §7 有对照表）。
