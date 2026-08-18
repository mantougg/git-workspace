# R-05 Maven 检测与执行策略

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)。无任务依赖。

| 项 | 值 |
|---|---|
| 阶段 | Phase 0 · Runtime 基础设施 |
| 优先级 | P0（前置） |
| 状态 | ✅ 已完成 |
| 依赖 | — |
| 对应源文档 | §18 Maven 执行策略、§19 Maven Wrapper（§20 mvnd 归 R-18） |

## 目标

按「Project Maven Wrapper → Configured Maven → System Maven」优先级确定每个项目的 Maven 可执行体，抽象出统一的 Maven 执行接口（命令构造 / 工作目录 / 环境注入），供 Build Engine（R-09）调用。

## 需求范围

- [x] 优先级链检测：项目内 `mvnw` / `mvnw.cmd` 存在即优先使用（§19）→ 用户配置的 Maven → 系统 `PATH` 中的 `mvn`
- [x] 版本解析（`mvn -v`）与最低版本校验，结果缓存
- [x] Maven Executor 抽象：可执行路径、参数构造、工作目录、环境变量注入、输出流转发（对接 R-11 日志）
- [x] 本地仓库路径探测：默认 `~/.m2/repository` + 解析 `settings.xml` 的 `localRepository` 覆盖
- [x] Settings UI：Maven 路径配置 + 当前生效来源展示
- [x] `MavenNotFound` / 版本过低的可行动错误（指向设置页）

## 架构 / 性能注意点

- **不重新实现 Maven**（全局约束 §1）：本任务只做发现、选择、执行封装。
- Windows 下 `mvnw.cmd` 需经 `cmd /c` 调用；Unix 下 `mvnw` 需可执行位检查（缺失时给提示而非失败）。
- `mvn -v` 等探测进程要有超时与输出上限，避免卡死启动流程。
- 执行接口设计时预留 mvnd（R-18）、Gradle（R-22）的扩展位，但不提前实现。

## 验收标准

- [x] 有 wrapper 的项目优先使用 wrapper（构造命令可断言）
- [x] 无 wrapper 时按 配置 → 系统 顺序回退
- [x] 三者皆缺时返回 `MavenNotFound` 可行动错误
- [x] `settings.xml` 自定义 localRepository 被正确识别
- [x] 单元测试覆盖优先级链与命令构造（含 Windows 分支）

## 进度

### 状态

- 当前状态：已完成
- 最近更新：2026-08-18

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-18 | 🟦 开始开发 | 启动 R-05：优先级链检测（wrapper/configured/system）、`mvn -v` 解析与缓存、Maven Executor 抽象、settings.xml localRepository 探测、MavenNotFound 错误、Settings UI；预留 mvnd（R-18）/ Gradle（R-22）扩展位 |
| 2026-08-18 | ✅ 完成 | 实现 `src-tauri/src/maven/{exec_model,detect_exec,settings,executor,registry}.rs` + `commands/maven.rs`（10 个 IPC 命令）+ schema v10 `maven_executables` 表 + `src/{api/maven.ts,types/maven.ts,views/MavenSettingsView.vue}` + `/maven-settings` 路由；`MavenNotFound` 错误（error.rs）；golden 快照守卫。验证：`cargo test` 265 passed（含 79 个 maven 测试，Windows `cmd /c` 分支覆盖）、`cargo clippy` 无新警告、`vue-tsc --noEmit` + `vite build` 通过 |

### 子任务清单

- [x] 优先级链检测（wrapper / configured / system）
- [x] 版本解析与缓存
- [x] Maven Executor 抽象（命令构造 + env 注入 + 输出转发）
- [x] settings.xml / localRepository 探测
- [x] Settings UI
- [x] 单元测试
