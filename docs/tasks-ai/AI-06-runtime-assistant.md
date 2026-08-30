# AI-06 Runtime Assistant（失败诊断 / 日志异常解释）

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[AI-03](./AI-03-context-builder-preview.md)（上下文/Preview）、[AI-05](./AI-05-tool-registry.md)（只读工具）、[R-11](../tasks-runtime/R-11-log-engine.md)（日志）、[R-14](../tasks-runtime/R-14-security-errors.md)（结构化错误）。设计约束见 [ai-assistant-design.md](../ai-assistant-design.md) §13。本任务是 [R-26](../tasks-runtime/R-26-ai-runtime-assistant.md) 的实现载体。

| 项 | 值 |
|---|---|
| 阶段 | Phase B · 工具注册表与 Runtime Assistant |
| 优先级 | P0 |
| 状态 | ✅ 已完成 |
| 依赖 | AI-03, AI-05, R-11, R-14 |
| 对应设计文档 | §13 Runtime Assistant 详细方案、§3.2 场景 A/B、§12.4 未配置和离线状态 |

## 目标

交付第一个完整 AI 场景（§21 决策 4）：Runtime 启动失败诊断与日志异常解释。AI 只读排障——给原因、证据、排查路径和建议，不修改配置、不重启进程。

## 需求范围

- [x] 失败诊断输入（§13.1）：优先发送 R-14 结构化错误（`error.code / message / details(module,pid,port,processName,runtime,reason)`）+ runtime config 摘要 + JDK/Maven 摘要 + 构建命令摘要 + 日志尾部；**不发送**未选择的完整日志、敏感环境变量的值、与诊断无关的项目源码
- [x] 诊断结果（§13.2）：`DiagnosticReport { headline, confidence, facts[], likelyCauses[], suggestedActions[], needsUserCheck[], sourceContext[] }`；`facts` 只能来自确定性上下文，`likelyCauses` / `suggestedActions` 必须标记为 AI 建议；禁止输出「已重启」「已修复」等未执行事实
- [x] 入口接入（§13.3）：`RuntimeErrorAlert` 对 BuildFailed / ProcessStartFailed / PortOccupied / ProcessCrashed 增加「AI 分析」；`RuntimeLogsView` 支持选中日志片段分析；`RuntimeDashboard` 支持对当前应用和最近一次失败请求诊断
- [x] 诊断请求/结果与具体 `processId`、`runtimeName`、错误发生时间关联，可追溯
- [x] 结果可复制、可重试、可查看上下文来源（§4.2 Phase B）
- [x] 未配置/离线降级（§12.4）：未配置时隐藏入口或显示「配置 AI」引导（跳 AI Settings）；离线保留 Preview 与上下文允许重试
- [x] 配置建议（§4.1 P2，本任务仅做只读建议的最小版）：根据模块数 / Spring 版本 / JDK 给出 VM Options / Profile 建议，**不落盘**

## 架构 / 性能注意点

- 复用统一调用链：Context Builder（AI-03）→ Preview → Gateway（AI-02）；诊断专用 prompt 与输出 Schema 是本任务的主要新增物。
- 日志入 prompt 前摘要与截断（尾部优先），遵循 AI-03 的错误诊断预算策略。
- 与 Runtime 边界（§13.4）：不修改 `runtimes/*.json`、不修改 Maven 项目或源码、不直接运行 Maven/Java/脚本、不绕过 R-14 错误分类与 Command Safety、不阻塞 Runtime 主链路。
- 诊断结果卡片组件落点 `src/components/ai/AiSuggestionCard.vue`，区分事实与 AI 推断（全局约束 §12）。

## 验收标准

- [ ] 典型失败场景（端口占用 / 依赖缺失 / JDK 或 Maven 不可用 / 配置错误 / 进程崩溃）均能生成正确上下文并给出有效诊断（§18.2）
- [ ] AI 未配置时 Runtime 核心功能完全不受影响，入口优雅隐藏/降级（§18.2）
- [ ] AI 请求前 Secret 检测生效（测试断言）
- [ ] 无任何自动修改行为（代码走查确认，§18.4）
- [ ] DiagnosticReport 中 facts 与 AI 建议在 UI 上可区分；结果可复制/重试/查看来源

## 进度

### 状态

- 当前状态：已完成
- 最近更新：2026-08-30

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-30 | 🟦 | 开始开发：复用 AI-03 Preview 的 Runtime 收集器与统一调用链，新增诊断编排（diagnose.rs）、§13.2 DiagnosticReport Schema 与三处前端入口 |
| 2026-08-30 | ✅ | 完成：接入结构化错误/Runtime 上下文/日志尾部与选中日志 Preview，补齐 Secret 检测、用户确认发送、轮询结果、事实/建议区分、复制/重试/来源查看及未配置/离线降级；配置建议仅作为不落盘的文本建议 |
| 2026-08-30 | ✅ | 验证：`cargo test --manifest-path src-tauri/Cargo.toml --lib ai::diagnose::tests -- --test-threads=1`（10 passed）、`cargo test --manifest-path src-tauri/Cargo.toml --lib ipc_golden`（2 passed）、`cargo check --manifest-path src-tauri/Cargo.toml`、`pnpm build`；安全走查确认无自动修改、无 shell/Runtime 子进程调用、凭证不落盘 |

### 子任务清单

- [x] 诊断 prompt 与 DiagnosticReport Schema
- [x] 失败诊断上下文组装（R-14 错误 + 日志尾部 + 环境摘要）
- [x] `RuntimeErrorAlert` / `RuntimeLogsView` / `RuntimeDashboard` 三处入口
- [x] 日志选段分析
- [x] 结果卡片（事实/推断区分、复制/重试/来源）
- [x] 未配置与离线降级
- [x] 单元/集成测试
