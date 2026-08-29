# AI-01 Provider / Model / Credential 与 AI Settings

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[T-08 错误处理 + 日志 + Secret Protection](../tasks/T-08-errors-logging-secrets.md)（✅ 已完成）。设计约束见 [ai-assistant-design.md](../ai-assistant-design.md) §6、§11.2、§12.2。

| 项 | 值 |
|---|---|
| 阶段 | Phase A · AI Foundation |
| 优先级 | P0 |
| 状态 | ⬜ 未开始 |
| 依赖 | T-08 |
| 对应设计文档 | §6 Provider 与模型管理、§11.2 建议表、§12.2 设置页面、§17 错误模型 |

## 目标

建立 Provider/Model 的统一配置模型、OS Credential Store 凭证存取、任务级默认模型解析和 AI Settings 页面，替换「前端直接传 API Key + 模型名硬编码」的原型调用方式（现状：`src/api/ai.ts::aiReview`、`src-tauri/src/commands/ai.rs` 写死 `gpt-4o-mini`）。

## 需求范围

- [ ] Provider 配置模型（§6.1）：`id / name / kind(openaiCompatible|ark|ollama|custom) / baseUrl / credentialRef / enabled / networkPolicy / createdAt / updatedAt`
- [ ] 模型能力目录（§6.2）：`chat / structuredOutput / toolCalling / vision / maxContextTokens / supportsStreaming / supportsReasoning` + 默认参数（temperature 等）
- [ ] 数据层迁移（§11.2）：新增 `ai_providers` / `ai_models` / `ai_task_defaults`（含 `workspace_id` 可空）；保留 `ai_reviews` / `ai_tasks` 兼容读取，不破坏性删除
- [ ] 任务级默认模型（§6.3）：`defaultChatModel / defaultRuntimeDiagnosticModel / defaultGitReviewModel / defaultCommitMessageModel / defaultConflictModel`，解析顺序 = 任务显式选择 > Workspace 任务配置 > 全局任务默认 > 全局聊天默认 > 首个可用模型
- [ ] OS Credential Store 存取（§6.4）：Windows Credential Manager / macOS Keychain / Linux Secret Service；不可用时**不回退普通文件**，允许本次会话临时输入（不落盘）
- [ ] AI Settings 页面（§12.2）六个区块：Provider 管理 / 模型管理 / 任务默认值 / 隐私与安全 / 用量与诊断 / 凭证管理（设置、替换、删除，不回显完整 Key）
- [ ] Workspace 级任务默认模型入口（§6.3 有 `workspace_id`，设计文档 §12.2 未列，本任务补齐：在 Workspace 设置内提供覆盖入口，缺省继承全局）
- [ ] IPC 命令（§12.1）：`ai_list_providers / ai_save_provider / ai_remove_provider / ai_test_provider / ai_list_models / ai_save_model / ai_set_task_default_model / ai_get_settings_summary`
- [ ] 结构化错误（§17）：`AiNotConfigured / AiCredentialUnavailable / AiModelNotFound / AiModelCapabilityMismatch`

## 架构 / 性能注意点

- 代码落点（§5.2）：后端 `src-tauri/src/ai/{provider.rs, model.rs, credentials.rs}`，`commands/ai.rs` 只做 IPC 薄适配；前端 `src/views/AiSettingsView.vue` + `src/types/ai.ts` + `src/api/ai.ts`。
- Rust serde 类型是 IPC 单一事实来源，新增/变更类型必须更新 golden-file 快照测试。
- Key 不进日志、错误信息、Pinia 持久化、LocalStorage、URL、进程命令行（全局约束 §4）；`credentialRef` 用稳定的本地 ID，不以 Key 本身做标识。
- `ai_test_provider` 只返回成功/失败原因/模型能力，不返回响应敏感内容。
- 打开 AI Settings 不得触发全量 Repository 扫描（全局约束 §10）。
- 前端新页面遵循 desktop-skin 约定：tokens 变量、设置导航下新增 `AI 设置` 入口。

## 验收标准

- [ ] Provider / 模型 / 任务默认值 CRUD 全部可用，重启后配置保留（Key 在 Credential Store，元数据在 SQLite）
- [ ] 凭证在三平台存取正常；环境不可用时产品返回 `AiCredentialUnavailable` 可行动错误（测试环境不可用则 skip 并打印原因）
- [ ] 模型能力校验前置：任务要求的能力（如 `structuredOutput`）不满足时在请求前报 `AiModelCapabilityMismatch`
- [ ] Key 全流程无回显：表单、日志、错误、LocalStorage 中均不出现明文（测试断言 + 走查）
- [ ] 迁移后 `ai_reviews` / `ai_tasks` 旧数据可正常读取
- [ ] IPC golden 快照与 TypeScript 类型一致

## 进度

### 状态

- 当前状态：未开始
- 最近更新：—

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|

### 子任务清单

- [ ] `ai_providers` / `ai_models` / `ai_task_defaults` 迁移与数据层
- [ ] Provider / Model 配置模型与能力校验
- [ ] OS Credential Store 三平台分支
- [ ] 任务级默认模型解析（含 Workspace 覆盖）
- [ ] AI Settings 页面六区块
- [ ] IPC 命令 + golden 快照
- [ ] 单元/集成测试
