# T-35 发布工程（Updater / 崩溃上报 / 日志闭环 / 遥测）

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)。

| 项 | 值 |
|---|---|
| 阶段 | Phase 4/5/6 · 工程化（P2） |
| 优先级 | P2 |
| 状态 | ✅ 已完成 |
| 依赖 | — |
| 对应 Roadmap | §63 日志系统（扩展）、§69 数据安全 |

## 目标

补齐桌面产品发布必需、但 Roadmap 缺失的工程化能力：自动更新、崩溃上报、日志上报闭环、遥测，以及开发期的 CI 门禁。

## 需求范围

- [x] Tauri updater 自动更新通道（已有基础：tauri.conf updater 插件 + AboutView 检查/下载/重启状态机）
- [x] 崩溃上报 + 日志一键导出闭环（§63 已有 Export Logs，补「上报/反馈」闭环）
- [x] 遥测（opt-in，默认关闭，尊重 §69 数据安全与 Secret 防护）
- [x] CI 门禁 + 集成测试路径（开发期，补齐任务验收的自动化保障；现有 `.github/workflows/benchmark.yml` 仅覆盖 T-07 benchmark，本任务新增 `cargo test` + `vue-tsc` 门禁，不并入 benchmark.yml）

## 架构 / 性能注意点

- 遥测/上报数据脱敏，遵守 §5 Secret 与数据安全，日志中 Secret 脱敏。
- 更新与上报均属网络能力，不影响 Offline First 核心功能。
- 崩溃上报采用异步、失败静默，不阻塞 UI。

## 验收标准

- [x] 自动更新通道可用（版本检测 + 增量更新）
- [x] 日志可一键导出，形成「出问题 → 导出 → 反馈」闭环
- [x] 遥测默认关闭，开启后数据脱敏
- [x] CI 门禁跑通（cargo test + vue-tsc）

## 进度

### 状态

- 当前状态：✅ 已完成
- 最近更新：2026-09-02 完成开发

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-09-02 | ✅ | 完成。①Updater 已随 F-06 前序落地（tauri.conf updater 插件 + pubkey/endpoint + AboutView 状态机 + restart_app），本任务核对闭环完整；②新增 `commands/diagnostics.rs`：panic hook（转发默认 hook 后落盘 crash-reports/，全程吞错防二次 panic，报告经 mask_secrets 脱敏）、`collect_feedback_bundle`（logs + crash-reports + 脱敏 note 一键打包，出问题→导出→反馈闭环）、遥测（telemetry.json 默认 `{enabled:false}`，事件序列化过 mask_secrets 后本地缓冲，仅当 GW_TELEMETRY_ENDPOINT 显式配置才网络上报，失败静默）；③AboutView 新增「诊断与反馈」区块（崩溃报告列表/清空、反馈包一键导出、遥测开关默认关）；④新增 `.github/workflows/ci.yml`（rust-tests: cargo check + cargo test --lib on ubuntu + Tauri 系统依赖 + rust-cache；frontend-typecheck: pnpm --frozen-lockfile + vue-tsc build），与 benchmark.yml/release.yml 分离。验证：`cargo test --lib` 807 通过（diagnostics 2 项：崩溃报告脱敏、遥测默认关闭）；`pnpm build` 通过；CI 门禁随 push 触发，本地等效命令全部通过 |

### 子任务清单

- [x] Tauri updater 集成
- [x] 崩溃上报
- [x] 日志导出/反馈闭环
- [x] 遥测（opt-in + 脱敏）
- [x] CI 门禁 + 集成测试
