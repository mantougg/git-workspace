# R-07 Runtime 配置体系

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[R-02 Maven 依赖图与 Workspace Source Mapping](./R-02-dependency-graph-source-mapping.md)（`runtime_projects` 表）。

| 项 | 值 |
|---|---|
| 阶段 | Phase 0 · Runtime 基础设施 |
| 优先级 | P0（前置） |
| 状态 | ✅ 已完成 |
| 依赖 | R-02 |
| 对应源文档 | §23 Spring Profile、§24 Runtime Configuration、§25 环境变量、§26 配置文件、§61 Runtime Configuration 存储、§62 Configuration 分层 |

## 目标

实现 Runtime Application 的完整配置模型与分层存储：SQLite 存元数据索引，`.gitworkspace/runtimes/*.json` 存用户配置（可 Git 版本化、团队共享），配置加载毫秒级。

## 需求范围

- [x] Runtime Application 配置模型（§24/§26）：`name / project / mainClass / jdk / profile / vmOptions / programArguments / environment / buildEngine`
- [x] 存储分层（§61）：SQLite（`runtime_projects` 等表，元数据/索引）+ `.gitworkspace/runtimes/<name>.json`（用户配置正文），两侧一致性同步
- [x] 配置分层优先级（§62）：**Application/Service → Runtime → Workspace → Global → System**
- [x] 环境变量三层模型（§25）：Workspace / Runtime / Application Environment，同名按优先级合并覆盖
- [x] Spring Profiles（§23）：支持 `-Dspring.profiles.active=dev`（VM Options）与 `--spring.profiles.active=dev`（Program Arguments）两种注入方式
- [x] 配置 CRUD IPC：create / update / delete / list / get
- [x] JSON schema 向后兼容：缺省字段有默认值，旧配置可安全加载

## 架构 / 性能注意点

- JSON 文件与 SQLite 的**写入顺序与失败回滚**要有明确约定（先写文件成功再更新索引，或反之；禁止双写不一致）。
- 配置加载目标 < 50ms（§99）：list 走 SQLite 索引，不逐个读 JSON。
- 敏感环境变量：存储不做加密（第一版），但 IPC 返回与 UI 展示按 key 模式掩码（与 R-14 协同；全局约束 §4）。
- JSON 可被用户手工编辑——加载时校验失败要给出文件路径 + 行号的错误，不得静默丢弃。

## 验收标准

- [x] §26 示例 JSON 可读写，CRUD 全链路可用
- [x] 同名环境变量按 Application → Runtime → Workspace 优先级正确覆盖
- [x] 配置 list 仅查询 SQLite 元数据，不逐个读取 JSON（< 50ms 以 R-08 实测为准）
- [x] 手工改坏 JSON 时给出带路径/行号的可行动错误
- [x] IPC 类型入 golden-file 快照测试

## 进度

### 状态

- 当前状态：已完成
- 最近更新：2026-08-18 完成

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-18 | 🟦 开始开发 | 启动 R-07：配置模型、`.gitworkspace/runtimes/*.json` 与 SQLite 元数据双层存储、环境变量分层合并、CRUD IPC 与 golden 快照 |
| 2026-08-18 | ✅ 完成 | 完成 Runtime 配置模型/serde 默认值、原子 JSON + SQLite 元数据双层存储、五层环境变量合并、敏感值 IPC 脱敏、Profile 两种注入识别、CRUD IPC、手工 JSON 行列错误诊断与共享 Secret key 规则；验证：`cargo test --all-targets`（276 passed / 2 ignored）、`cargo check --all-targets`、`cargo test ipc_golden`、`pnpm build` 通过；list 路径仅查询 SQLite 元数据，正式 SLA 由 R-08 Benchmark 延续 |

### 子任务清单

- [x] 配置模型 + serde（含默认值/兼容）
- [x] 双层存储与一致性策略
- [x] 配置/环境变量分层合并
- [x] CRUD IPC
- [x] 单元测试（合并优先级 / 兼容加载 / 错误路径）
