# AI-02 AI Gateway（请求生命周期 / Provider Adapter / 流式）

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[AI-01](./AI-01-provider-model-credentials.md)（Provider/模型/凭证）、[T-08](../tasks/T-08-errors-logging-secrets.md)（错误与日志）。设计约束见 [ai-assistant-design.md](../ai-assistant-design.md) §7、§16.1、§17。

| 项 | 值 |
|---|---|
| 阶段 | Phase A · AI Foundation |
| 优先级 | P0 |
| 状态 | ⬜ 未开始 |
| 依赖 | AI-01, T-08 |
| 对应设计文档 | §7 AI Gateway 详细设计、§16.1 性能、§16.3 日志与审计、§17 错误模型 |

## 目标

把所有 AI 调用收敛到统一 Gateway：类型化请求模型、请求生命周期状态机、超时/取消/重试边界、三种接口协议的 Provider Adapter（OpenAI Chat Completions / OpenAI Responses / Anthropic Messages）、流式响应与事件推送。Gateway 是唯一允许访问 AI 网络的地方，且 Preview 未确认前不得联网。

## 需求范围

- [ ] 请求模型（§7.1）：`AiRequest { requestId, sessionId?, taskKind, providerId?, modelId?, systemInstruction, messages, contextManifest, responseFormat, toolPolicy, tokenBudget, temperature?, stream }`；`ContextItem { kind, sourceId, displayName, charCount, estimatedTokens, redacted, excluded }`
- [ ] 请求生命周期状态机（§7.3）：`Created → ContextBuilding → SecretScanning → PreviewRequired → UserApproved → Queued → Sending → Streaming/Parsing → Succeeded`，任意阶段可入 `Cancelled / Rejected / Failed / Degraded`
- [ ] Preview 闸门：`UserApproved` 之前不得发起任何网络请求（测试断言）
- [ ] Provider 配置模型迁移（设计修订，承接 AI-01 已交付实现）：`ProviderKind` 厂商枚举（`openaiCompatible/ark/ollama/custom`）→ `apiType` 协议枚举（`openaiChatCompletions/openaiResponses/anthropicMessages`）；新增版本化迁移重建 `ai_providers.kind` CHECK 约束（存量行一律映射为 `openaiChatCompletions`，Ollama 特判逻辑随之移除）；前端类型、AI Settings 下拉与 golden 快照同步
- [ ] Provider Adapter（§7.2）：`trait AiProvider { validate_model / complete / stream }`；第一期实现三种协议 Adapter——OpenAI Chat Completions / OpenAI Responses / Anthropic Messages：URL 与认证头（`Authorization: Bearer` vs `x-api-key` + `anthropic-version`）、请求/响应格式映射（system 字段位置、`max_tokens` 必填差异、usage 字段名）、structured output 参数映射（协议缺失时靠能力校验前置拦截）、流式事件归一化（delta chunk / Responses 事件流 / `content_block_delta` → 统一内部 chunk）、tool calling 格式映射、Provider 错误归一化、取消与网络超时
- [ ] 流式事件契约（设计文档缺口，本任务补齐）：定义 Tauri event（如 `ai-request://progress`）推送生命周期状态与流式 chunk，事件 payload 进 golden 快照；前端合帧渲染，不每 token 重渲染（§16.1）
- [ ] 失败与重试（§7.4）：可重试 = 临时网络错误 / 429 / 5xx / 流式中断，默认最多 1 次自动重试 + 退避；不可重试 = Key 无效 / 模型不存在或能力不匹配 / Secret 未通过 / Preview 未确认 / 超上下文 / Provider 策略拒绝
- [ ] 独立请求并发上限（§16.1），不占 Maven/Java 子进程并发预算
- [ ] 结构化结果解析（§8.4）：`Answer / DiagnosticReport / ReviewReport / GeneratedText / ConflictProposal / ActionProposal`；非法 JSON 降级为纯文本 Answer
- [ ] 错误码接入（§17）：`AiProviderUnavailable / AiAuthenticationFailed / AiRequestCancelled / AiRateLimited / AiContextTooLarge / AiResponseInvalid / AiPreviewRequired`
- [ ] 审计日志（§16.3）：复用 `ai.log`，记录 requestId / taskKind / 状态迁移 / 耗时 / 重试 / token 估算与脱敏计数；不记 Key、完整 Prompt、Secret 原文

## 架构 / 性能注意点

- 代码落点（§5.2）：`src-tauri/src/ai/{gateway.rs, provider.rs}`；业务 prompt 不进 Adapter，Adapter 只处理协议差异。
- Gateway 全异步；请求取消必须能中断进行中的流式响应（tokio CancellationToken 或等价机制）。
- 模型能力 / token 预算校验在发送前完成，失败直接返回结构化错误（§6.3）。
- 重试不得导致重复写操作——第一期 Gateway 不执行写操作，天然满足；代码中留注释说明该不变量。
- Provider URL 结构化拼接（`url` crate），不手写字符串（全局约束 §11）。

## 验收标准

- [ ] fake Provider 集成测试对三种协议各覆盖：成功 / 流式 / 超时 / 取消 / 429 / 5xx / 非法 JSON（§18.2）；存量 `kind` 配置经迁移后可用
- [ ] 生命周期状态迁移单元测试（含所有终止态）
- [ ] Preview 未确认时无网络请求（mock 断言 zero calls）
- [ ] 429/5xx 自动重试至多 1 次且退避生效；Key 无效等不可重试错误直接失败
- [ ] 流式事件契约进 golden 快照，前后端类型一致
- [ ] `ai.log` 无 Key / Prompt 原文 / Secret（走查 + 测试）

## 进度

### 状态

- 当前状态：未开始
- 最近更新：—

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|

### 子任务清单

- [ ] `AiRequest` / `ContextItem` / 结果模型类型 + golden 快照
- [ ] 生命周期状态机
- [ ] Provider 模型迁移（`kind` → `apiType`，含 DB 迁移 / 前端类型与下拉 / golden 快照）
- [ ] 三个协议 Adapter（complete + stream）：OpenAI Chat Completions / OpenAI Responses / Anthropic Messages
- [ ] 流式事件契约与前端合帧
- [ ] 重试 / 取消 / 超时 / 并发上限
- [ ] 错误码与 `ai.log` 审计接入
- [ ] fake Provider 测试套件
