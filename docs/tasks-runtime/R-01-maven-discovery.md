# R-01 Maven 项目发现与 POM 解析

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)。无任务依赖（仓库清单复用 T-01 Scanner 能力）。

| 项 | 值 |
|---|---|
| 阶段 | Phase 0 · Runtime 基础设施 |
| 优先级 | P0（前置） |
| 状态 | ✅ 已完成 |
| 依赖 | —（复用 T-01） |
| 对应源文档 | §9 Maven 项目自动发现、§10 Maven 多模块识别、§52 Maven Parser、§53 Maven Model |

## 目标

在打开的 Workspace 中自动扫描全部 `pom.xml`，解析出 Maven 项目模型（GAV / packaging / parent / modules / dependencies…），识别项目类型并建立多模块 Reactor 关系，作为后续依赖图（R-02）、Closure（R-03）的数据基础。

## 需求范围

- [x] Workspace 级 `pom.xml` 扫描：基于 T-01 仓库清单逐 repo 下钻，支持 repo 内多层嵌套模块；遵守 `.gitworkspaceignore` 与默认忽略目录（`target/` 等）
- [x] XML 解析字段：`groupId / artifactId / version / packaging / parent / modules / dependencies / dependencyManagement / profiles / properties / plugins`（§52）
- [x] Effective model：parent 继承链合并 + `properties` 占位符替换 + `dependencyManagement` 版本落地，产出 **effective dependency**——禁止只解析 XML 第一层
- [x] 项目类型识别：Standalone / Parent / Multi-Module / Library（Spring Boot 判定归 R-06）
- [x] 多模块 Reactor 关系：parent → modules 树
- [x] POM Cache：`path + file hash → parsed model`，pom 未变不重新解析
- [x] `MavenProject` Rust 模型（§53），serde 序列化，入 IPC golden 快照

## 架构 / 性能注意点

- 解析模型为**纯数据**；不缓存文件句柄，解析完即释放。
- Effective model 只覆盖 Runtime 所需字段，**不追求完整 Maven Model Builder**；复杂 profile 激活、远程 parent 解析等交给 `mvn` 自身（全局约束 §1）。
- 发现 + 解析全程本地完成，**禁止任何网络请求**（远程 parent POM 缺失时降级标记，不阻塞）。
- 性能目标：Maven Project Discovery < 500ms、POM Cache Hit < 50ms（§99，以 R-08 Benchmark 实测为准）。
- 扫描与解析走 rayon 并行 + 批量的方式，但并发度受 IO 预算约束，不与 Git status 争抢（沿用 T-01 经验）。

## 验收标准

- [x] 多 repo + 多模块样例 workspace 的全部 pom 被发现并解析，GAV / parent / modules 关系正确
- [x] `properties` 占位符与 parent / dependencyManagement 继承的依赖版本能正确落地为 effective dependency
- [x] pom 未修改时二次加载命中 POM Cache < 50ms（R-01 release 探针单 POM 136µs；由 R-08 延续正式基线）
- [x] 损坏 / 非法 pom 返回 `InvalidPom` 结构化错误，不影响其他项目的发现
- [x] `cargo test` 覆盖解析、继承合并、缓存命中/失效路径

## 进度

### 状态

- 当前状态：已完成
- 最近更新：2026-08-18 完成

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2025-01-15 | 🟦 开始开发 | 启动 R-01：扫描接入 T-01 忽略规则、POM parser、effective model、cache、IPC golden |
| 2026-08-18 | ✅ 完成 | 完成 T-01 仓库清单扫描、POM 全字段解析、effective 继承/覆盖、项目分类、Reactor、结构化错误、cache 与 IPC golden；`cargo test` 191 passed / 2 ignored，`cargo check --all-targets`、`cargo clippy --all-targets --all-features`、`pnpm build` 通过；release 探针（100 POM）首次发现 148ms、整仓二次加载 43ms、单 POM cache hit 136µs（正式基线由 R-08 延续） |

### 子任务清单

- [x] Workspace pom 扫描（接入 T-01 仓库清单 + 忽略规则）
- [x] POM XML parser（字段全集）
- [x] Effective model（parent 链 + properties + dependencyManagement）
- [x] 项目类型识别 + Reactor 关系
- [x] POM Cache（path + hash）
- [x] 单元测试与 IPC 模型 golden 快照
