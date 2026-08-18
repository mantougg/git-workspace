# R-02 Maven 依赖图与 Workspace Source Mapping

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[R-01 Maven 项目发现与 POM 解析](./R-01-maven-discovery.md)；数据层复用 T-03。

| 项 | 值 |
|---|---|
| 阶段 | Phase 0 · Runtime 基础设施 |
| 优先级 | P0（前置） |
| 状态 | ✅ 已完成 |
| 依赖 | R-01, T-03 |
| 对应源文档 | §11 跨 Git Repository Maven 模块、§12 Dependency Mapping、§54 Dependency Model、§55 Source Mapping、§56 Dependency Resolver、§57 Workspace Maven Index、§58 ~ §60 表结构 |

## 目标

建立 Workspace 级 Maven 坐标索引（`groupId:artifactId:version` → 本地项目路径），把每个项目的依赖解析为 **Workspace Source / Local Maven Repository / Remote Maven Repository** 三类（§12），形成跨 Git Repository 的依赖图——这是本产品最核心的差异化能力。

## 需求范围

- [x] SQLite schema（版本化迁移，叠加 T-03 数据层）：`maven_projects / maven_dependencies / maven_modules / maven_artifacts / maven_source_mappings / runtime_projects / runtime_dependencies`（§57 ~ §60）
- [x] Workspace Maven Index：GAV → project path 映射持久化，扫描后增量更新
- [x] Dependency Resolver（§56）：依赖按优先级归类 `Workspace Source → ~/.m2 → Remote`，默认 `Auto` 模式
- [x] 跨 Git Repository 模块映射（§11）：`com.example:common` → `repo-common/common`
- [x] 依赖图数据模型与查询接口（供 R-03 Closure、R-13 UI、R-20 可视化消费）
- [x] Dependency Graph Cache：`POM hash → dependency graph / source mapping`，POM 未变直接复用（§69）

## 架构 / 性能注意点

- 数据层遵守 T-03 全局约束：WAL、**单写者模型**、批量事务 + Prepared Statement，禁止逐条 INSERT。
- `~/.m2` 检测只看本地文件存在性；**Remote 只标记不下载**——下载发生在 Build 阶段（全局约束 §10 网络边界）。
- 版本匹配遵循 Maven 语义的最小子集：精确版本命中本地源码才映射 Source；范围/快照版本优先落 Local/Remote 并记录原因。
- 缓存失效触发源 = pom 文件变化（watcher / 扫描发现 hash 变化），禁止只靠 TTL。
- 性能目标：Dependency Graph Cache Hit < 100ms（§99，以 R-08 实测为准）。

## 验收标准

- [x] 跨 repo 依赖样例（boot → common/core/auth）全部映射到本地源码路径，UI/IPC 可查
- [x] 无本地源码的依赖（如 `org.springframework.boot:spring-boot`）正确归类 Local/Remote
- [x] pom 变化后索引增量更新，未变化项目不重算（Graph Cache 命中路径有测试）
- [x] 版本不匹配时不错误映射到本地源码
- [x] `cargo test` 覆盖 resolver 优先级、mapping、cache 失效

## 进度

### 状态

- 当前状态：已完成
- 最近更新：2026-08-18 完成

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-18 | 🟦 开始开发 | 启动 R-02：七表迁移、Workspace Maven Index、依赖来源解析、依赖图查询与 POM hash 缓存 |
| 2026-08-18 | ✅ 完成 | 完成 v8 七表迁移、事务批量增量索引、跨 repo Source Mapping、Source/Local/Remote resolver、图查询与有界缓存；补齐 `.m2` 变化刷新、Windows 路径规范化、Runtime 错误码、Rust↔TS golden。验证：`cargo test` 205 passed / 2 ignored，`cargo check --all-targets`、`cargo clippy --all-targets --all-features`、`pnpm build` 通过；独立代码复核 APPROVE |

### 子任务清单

- [x] SQLite schema 迁移（7 张表）
- [x] Workspace Maven Index 构建与增量更新
- [x] Dependency Resolver（Source → Local → Remote）
- [x] 依赖图模型与查询
- [x] Dependency Graph Cache
- [x] 单元测试
