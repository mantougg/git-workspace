# T-08 错误处理 + 日志 + Secret Protection

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)。

| 项 | 值 |
|---|---|
| 阶段 | Phase 0 · 基础稳定化 |
| 优先级 | P0（前置） |
| 状态 | ✅ 已完成 |
| 依赖 | 无 |
| 对应 Roadmap | §44 错误处理、§63 日志系统、§70/71/72 Secret Protection、§69 Security |

## 目标

补齐横切工程底座：统一错误类型、分级日志、Secret 检测与 Commit 安全检查。这三项是 AI 类任务（T-25/26/27）与安全操作的前置。

## 需求范围

### 错误处理（§44）

- [x] 统一 `GitWorkspaceError`：分类 `RepositoryError / GitError / NetworkError / ConflictError / TaskError / IndexError / AIError / PermissionError / IOError`（`AppError::code()` 映射）
- [x] 结构化字段：`code / message / repository / operation / details / recoverable`（`ErrorResponse`）

### 日志（§63）

- [x] 分级日志 Debug/Info/Warn/Error/Trace（`core/logger.rs` 按 level 输出）
- [x] 分类文件：`app.log` / `git.log` / `task.log` / `ai.log` / `performance.log`（按 target 分流）
- [x] 支持 Open Logs / Export Logs / Clear Logs（`commands/logs.rs` + `LogManager` UI）

### Secret Protection（§69/70/71/72）

- [x] Secret 检测规则：AWS Key / GitHub Token / JWT / Private Key / Password / Database URL（`core/secret.rs`）
- [x] 检测点：AI 请求发送前（`ai.rs`）、Commit 前（`git_ops.rs`）
- [x] 处理策略：拦截 + `mask_secrets` 脱敏（Mask/Exclude 完整 UI 待 T-25/26/27）
- [x] Commit 安全检查：Forbidden File 拦截（`.env` / `*.pem` / `*.key` / `credentials.json` / 私钥）
- [x] API Key 不落盘（保持现状），后续可升级 OS Credential Store（§69，P2）

## 架构 / 性能注意点

- Secret 检测采用规则 + 正则，运行在离用户数据最近的 Rust 侧，检测是轻量级、可增量缓存（按文件 mtime）。
- AI 发送前的「Preview + 可排除文件/目录」属于产品硬要求，禁止无预览直接发送。
- 错误要能穿透 IPC 到达 UI 且不泄漏敏感信息（日志中 Secret 必须脱敏）。

## 验收标准

- [x] 各错误分类在 IPC 层正确映射为结构化错误，UI 可读且含 recoverable 提示（`errMsg` 展示可重试/需手动处理）
- [x] 五类日志文件按模块正确分流，可导出/清空（Open/Export/Clear 命令）
- [x] 含 AWS Key / JWT 的 diff 在 AI 请求前被检测并提示 Mask/Exclude
- [x] 含 `.env` / `*.pem` 的提交被 Commit 安全检查拦截
- [x] 日志中不出现明文 Secret（脱敏）

## 进度

### 状态

- 当前状态：已完成
- 最近更新：2026-08-13 完成

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-13 | 🟦 | 开始开发：结构化错误（ErrorResponse）+ Secret 检测引擎（core/secret.rs） |
| 2026-08-13 | 🟦 | 核心完成：ErrorResponse 结构化 + logger 五文件分流脱敏 + Secret 引擎（7 测试）+ AI/Commit 检测点 + 前端 errMsg 适配；`cargo test` 13 passed、`vue-tsc` 通过。剩余 UI：日志导出/清空命令、recoverable 提示展示
| 2026-08-13 | ✅ | 完成剩余：Open/Export/Clear Logs 命令（commands/logs.rs + LogManager UI）+ recoverable 提示展示（errMsg）；`cargo test` 33 passed、`npm run build` 通过 |

### 子任务清单

- [x] 定义错误分类与结构化字段
- [x] 实现分级日志与分类文件
- [x] 实现 Secret 检测规则引擎
- [x] 接入 AI 发送前与 Commit 前两个检测点
- [x] 实现日志脱敏
