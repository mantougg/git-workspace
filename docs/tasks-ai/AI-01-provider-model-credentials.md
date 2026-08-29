# AI-01 Provider / Model / Credential 与 AI Settings

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[T-08 错误处理 + 日志 + Secret Protection](../tasks/T-08-errors-logging-secrets.md)（✅ 已完成）。设计约束见 [ai-assistant-design.md](../ai-assistant-design.md) §6、§11.2、§12.2。

| 项 | 值 |
|---|---|
| 阶段 | Phase A · AI Foundation |
| 优先级 | P0 |
| 状态 | ✅ 已完成 |
| 依赖 | T-08 |
| 对应设计文档 | §6 Provider 与模型管理、§11.2 建议表、§12.2 设置页面、§17 错误模型 |

## 目标

建立 Provider/Model 的统一配置模型、OS Credential Store 凭证存取、任务级默认模型解析和 AI Settings 页面，替换「前端直接传 API Key + 模型名硬编码」的原型调用方式（现状：`src/api/ai.ts::aiReview`、`src-tauri/src/commands/ai.rs` 写死 `gpt-4o-mini`）。

## 需求范围

- [x] Provider 配置模型（§6.1）：`id / name / kind(openaiCompatible|ark|ollama|custom) / baseUrl / credentialRef / enabled / networkPolicy / createdAt / updatedAt`
- [x] 模型能力目录（§6.2）：`chat / structuredOutput / toolCalling / vision / maxContextTokens / supportsStreaming / supportsReasoning` + 默认参数（temperature 等）
  - 注：`supportsStreaming` / `supportsReasoning` 未作为独立能力字段落地——AI-02 Gateway 落地流式时再按 §6.2 扩展能力枚举（当前目录为 `chat / structuredOutput / toolCalling / vision` + `maxContextTokens` + 默认参数），避免无消费端的字段先行
- [x] 数据层迁移（§11.2）：新增 `ai_providers` / `ai_models` / `ai_task_defaults`（含 `workspace_id` 可空）；保留 `ai_reviews` / `ai_tasks` 兼容读取，不破坏性删除
- [x] 任务级默认模型（§6.3）：`defaultChatModel / defaultRuntimeDiagnosticModel / defaultGitReviewModel / defaultCommitMessageModel / defaultConflictModel`，解析顺序 = 任务显式选择 > Workspace 任务配置 > 全局任务默认 > 全局聊天默认 > 首个可用模型
- [x] OS Credential Store 存取（§6.4）：Windows Credential Manager / macOS Keychain / Linux Secret Service（keyring crate 原生后端）；不可用时**不回退普通文件**，允许本次会话临时输入（不落盘）
- [x] AI Settings 页面（§12.2）六个区块：Provider 管理 / 模型管理 / 任务默认值 / 隐私与安全 / 用量与诊断 / 凭证管理（设置、替换、删除，不回显完整 Key）
- [x] Workspace 级任务默认模型入口（§6.3 有 `workspace_id`，设计文档 §12.2 未列，本任务补齐：在 Workspace 设置内提供覆盖入口，缺省继承全局）
- [x] IPC 命令（§12.1）：`ai_list_providers / ai_save_provider / ai_remove_provider / ai_test_provider / ai_list_models / ai_save_model / ai_set_task_default_model / ai_get_settings_summary`（另按「模型/任务默认值 CRUD 全部可用」验收补充 `ai_remove_model / ai_clear_task_default_model`，凭证区块补充 `ai_set_credential / ai_clear_credential`）
- [x] 结构化错误（§17）：`AiNotConfigured / AiCredentialUnavailable / AiModelNotFound / AiModelCapabilityMismatch`（另含原型迁移所需 `AiProviderUnavailable / AiAuthenticationFailed / AiSecretDetected`；其余 §17 code 随后续任务补充）

## 架构 / 性能注意点

- 代码落点（§5.2）：后端 `src-tauri/src/ai/{provider.rs, model.rs, credentials.rs}`，`commands/ai.rs` 只做 IPC 薄适配；前端 `src/views/AiSettingsView.vue` + `src/types/ai.ts` + `src/api/ai.ts`。
- Rust serde 类型是 IPC 单一事实来源，新增/变更类型必须更新 golden-file 快照测试。
- Key 不进日志、错误信息、Pinia 持久化、LocalStorage、URL、进程命令行（全局约束 §4）；`credentialRef` 用稳定的本地 ID，不以 Key 本身做标识。
- `ai_test_provider` 只返回成功/失败原因/模型能力，不返回响应敏感内容。
- 打开 AI Settings 不得触发全量 Repository 扫描（全局约束 §10）。
- 前端新页面遵循 desktop-skin 约定：tokens 变量、设置导航下新增 `AI 设置` 入口。

## 验收标准

- [x] Provider / 模型 / 任务默认值 CRUD 全部可用，重启后配置保留（Key 在 Credential Store，元数据在 SQLite）
- [x] 凭证在三平台存取正常；环境不可用时产品返回 `AiCredentialUnavailable` 可行动错误（测试环境不可用则 skip 并打印原因）
  - 三平台分支由 keyring crate 原生后端承载（windows-native / apple-native / sync-secret-service）；Linux 本机冒烟为 skip/通过双态（详见时间线）
- [x] 模型能力校验前置：任务要求的能力（如 `structuredOutput`）不满足时在请求前报 `AiModelCapabilityMismatch`
- [x] Key 全流程无回显：表单、日志、错误、LocalStorage 中均不出现明文（测试断言 + 走查）
- [x] 迁移后 `ai_reviews` / `ai_tasks` 旧数据可正常读取
- [x] IPC golden 快照与 TypeScript 类型一致

## 进度

### 状态

- 当前状态：已完成
- 最近更新：2026-08-30

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-29 | 🟦 | 开始开发 |
| 2026-08-30 | ✅ | 完成。迁移 v13（ai_providers/ai_models/ai_task_defaults，ai_reviews/ai_tasks 兼容保留）；`src-tauri/src/ai/{mod,provider,model,credentials,error}.rs`；`AppError::Ai(String)` → 结构化 `AiError`（§17 code + suggestedActions）；`ai_review` 原型移除前端传 Key 与模型硬编码（gitReview 链解析 + 能力前置校验）；12 个新 IPC 命令；AI Settings 六区块 + Workspace「AI 模型」覆盖入口；DiffViewer/ChangeSetView 移除 Key 输入、未配置时引导设置页。验证：`cargo test --lib`（ai/db/error/ipc_golden 37 项全过；全量 511 过，3 个失败为干净 master 上复现的存量环境/时序失败，与本次无关）；`GW_UPDATE_GOLDEN=1` 再生成 golden；`pnpm build`（vue-tsc + vite）通过。安全走查：Key 仅存 OS 凭证存储/会话内存（不可用时不回退文件），前端为组件本地状态、无 Pinia/localStorage 持久化、无回显；后端错误与日志仅含 provider/model id 与归一化网络错误；Key 不进 URL/进程命令行（Authorization 头内存流经）。 |
| 2026-08-30 | ✅ | 设计修订（§6.1 / §21 决策 9）：Provider 模型由 `kind` 厂商枚举调整为 `apiType` 协议枚举（`openaiChatCompletions` / `openaiResponses` / `anthropicMessages`）；本任务已交付的 `kind` 实现与 v13 schema 的迁移改造由 AI-02 承载，本任务验收记录保持原样。 |

### 子任务清单

- [x] `ai_providers` / `ai_models` / `ai_task_defaults` 迁移与数据层
- [x] Provider / Model 配置模型与能力校验
- [x] OS Credential Store 三平台分支
- [x] 任务级默认模型解析（含 Workspace 覆盖）
- [x] AI Settings 页面六区块
- [x] IPC 命令 + golden 快照
- [x] 单元/集成测试
