# AI-02 AI Gateway（请求生命周期 / Provider Adapter / 流式）

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[AI-01](./AI-01-provider-model-credentials.md)（Provider/模型/凭证）、[T-08](../tasks/T-08-errors-logging-secrets.md)（错误与日志）。设计约束见 [ai-assistant-design.md](../ai-assistant-design.md) §7、§16.1、§17。

| 项 | 值 |
|---|---|
| 阶段 | Phase A · AI Foundation |
| 优先级 | P0 |
| 状态 | ✅ 已完成 |
| 依赖 | AI-01, T-08 |
| 对应设计文档 | §7 AI Gateway 详细设计、§16.1 性能、§16.3 日志与审计、§17 错误模型 |

## 目标

把所有 AI 调用收敛到统一 Gateway：类型化请求模型、请求生命周期状态机、超时/取消/重试边界、三种接口协议的 Provider Adapter（OpenAI Chat Completions / OpenAI Responses / Anthropic Messages）、流式响应与事件推送。Gateway 是唯一允许访问 AI 网络的地方，且 Preview 未确认前不得联网。

## 需求范围

- [x] 请求模型（§7.1）：`AiRequest { requestId, sessionId?, taskKind, providerId?, modelId?, systemInstruction, messages, contextManifest, responseFormat, toolPolicy, tokenBudget, temperature?, stream }`；`ContextItem { kind, sourceId, displayName, charCount, estimatedTokens, redacted, excluded }`
- [x] 请求生命周期状态机（§7.3）：`Created → ContextBuilding → SecretScanning → PreviewRequired → UserApproved → Queued → Sending → Streaming/Parsing → Succeeded`，任意阶段可入 `Cancelled / Rejected / Failed / Degraded`
- [x] Preview 闸门：`UserApproved` 之前不得发起任何网络请求（测试断言）
- [x] Provider 配置模型迁移（设计修订，承接 AI-01 已交付实现）：`ProviderKind` 厂商枚举（`openaiCompatible/ark/ollama/custom`）→ `apiType` 协议枚举（`openaiChatCompletions/openaiResponses/anthropicMessages`）；新增版本化迁移重建 `ai_providers.kind` CHECK 约束（存量行一律映射为 `openaiChatCompletions`，Ollama 特判逻辑随之移除）；前端类型、AI Settings 下拉与 golden 快照同步
- [x] Provider Adapter（§7.2）：`trait AiProvider { validate_model / complete / stream }`；第一期实现三种协议 Adapter——OpenAI Chat Completions / OpenAI Responses / Anthropic Messages：URL 与认证头（`Authorization: Bearer` vs `x-api-key` + `anthropic-version`）、请求/响应格式映射（system 字段位置、`max_tokens` 必填差异、usage 字段名）、structured output 参数映射（协议缺失时靠能力校验前置拦截）、流式事件归一化（delta chunk / Responses 事件流 / `content_block_delta` → 统一内部 chunk）、tool calling 格式映射、Provider 错误归一化、取消与网络超时
- [x] 流式事件契约（设计文档缺口，本任务补齐）：定义 Tauri event（如 `ai-request://progress`）推送生命周期状态与流式 chunk，事件 payload 进 golden 快照；前端合帧渲染，不每 token 重渲染（§16.1）
- [x] 失败与重试（§7.4）：可重试 = 临时网络错误 / 429 / 5xx / 流式中断，默认最多 1 次自动重试 + 退避；不可重试 = Key 无效 / 模型不存在或能力不匹配 / Secret 未通过 / Preview 未确认 / 超上下文 / Provider 策略拒绝
- [x] 独立请求并发上限（§16.1），不占 Maven/Java 子进程预算
- [x] 结构化结果解析（§8.4）：`Answer / DiagnosticReport / ReviewReport / GeneratedText / ConflictProposal / ActionProposal`；非法 JSON 降级为纯文本 Answer
- [x] 错误码接入（§17）：`AiProviderUnavailable / AiAuthenticationFailed / AiRequestCancelled / AiRateLimited / AiContextTooLarge / AiResponseInvalid / AiPreviewRequired`
- [x] 审计日志（§16.3）：复用 `ai.log`，记录 requestId / taskKind / 状态迁移 / 耗时 / 重试 / token 估算与脱敏计数；不记 Key、完整 Prompt、Secret 原文

## 架构 / 性能注意点

- 代码落点（§5.2）：`src-tauri/src/ai/{gateway.rs, provider.rs}`；业务 prompt 不进 Adapter，Adapter 只处理协议差异。
- Gateway 全异步；请求取消必须能中断进行中的流式响应（tokio CancellationToken 或等价机制）。
- 模型能力 / token 预算校验在发送前完成，失败直接返回结构化错误（§6.3）。
- 重试不得导致重复写操作——第一期 Gateway 不执行写操作，天然满足；代码中留注释说明该不变量。
- Provider URL 结构化拼接（`url` crate），不手写字符串（全局约束 §11）。

## 验收标准

- [x] fake Provider 集成测试对三种协议各覆盖：成功 / 流式 / 超时 / 取消 / 429 / 5xx / 非法 JSON（§18.2）；存量 `kind` 配置经迁移后可用
- [x] 生命周期状态迁移单元测试（含所有终止态）
- [x] Preview 未确认时无网络请求（mock 断言 zero calls）
- [x] 429/5xx 自动重试至多 1 次且退避生效；Key 无效等不可重试错误直接失败
- [x] 流式事件契约进 golden 快照，前后端类型一致
- [x] `ai.log` 无 Key / Prompt 原文 / Secret（走查 + 测试）

## 进度

### 状态

- 当前状态：已完成
- 最近更新：2026-08-30

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-30 | 🟦 | 开始开发。承接 AI-01 时间线的设计修订（§6.1 / §21 决策 9）：`kind` → `apiType` 迁移随本任务落地。 |
| 2026-08-30 | ✅ | 完成。①Provider 迁移：v14 迁移重建 `ai_providers`（`api_type` CHECK，存量行一律映射 `openaiChatCompletions`，外键敏感迁移在事务外关 FK + `foreign_key_check` 复核）；Rust `ProviderKind`→`ApiType`，`test_connection`/原型 `ai_review` 移除 Ollama 特判（Anthropic 认证头 `x-api-key`+`anthropic-version`）；TS 类型/`AiProvidersSection.vue`/`AiCredentialsSection.vue` 下拉同步。②Gateway：`ai/{request,lifecycle,transport,events,gateway,adapters/*}.rs`；`AiGateway` 挂 AppState，事件出口 Tauri `ai-request://progress`；三协议 Adapter（complete+stream，SSE 泵任务归一化 chunk，取消/空闲超时可中断）；Preview 闸门（submit 停在 PreviewRequired，approve 是唯一联网入口，重复 approve 拒绝）；重试（临时网络/429/5xx/流式启动中断至多 1 次 + 退避；超时/Key 无效/策略拒绝/协议违规/已有输出不重试）；独立信号量并发上限（默认 3）；§8.4 结果模型（非法 JSON 降级 Answer）；§17 新增 6 个错误 code。③前端：`api/ai.ts` 4 个新命令封装 + `api/aiStream.ts`（事件订阅 + rAF 合帧缓冲）。④测试：fake transport 脚本化响应，三协议 × 成功/流式/超时/取消/429/5xx/非法 JSON + Preview 闸门 zero calls + 重复 approve 拒绝 + v14 迁移存量配置可用 + 事件/快照不含 API Key；生命周期状态机单测（含终止态吸收性）。验证：`cargo test --lib`（AI 域 63 项全过；全量 554 过，4 个失败均为存量/环境问题——maven settings ×2、runtime logs flood ×1 在干净 master 上复现，benchmark_smoke 为时序 flake 单跑通过）；golden 经 `GW_UPDATE_GOLDEN=1` 再生成（AiProvider/SaveAiProviderRequest 的 apiType + 8 个新类型）；`pnpm build`（vue-tsc + vite）通过；`detect_changes` 复核（critical 广度来自 migrate/init_db 被全部 DB 流程引用，属迁移任务必然，受影响流程已被全量测试覆盖）。安全走查：Preview 未确认 Gateway 无任何联网路径；Key 只在内存经 Adapter 进入请求头，事件/快照/审计日志/错误均无 Key 与 Prompt 原文（测试断言 + T-08 logger mask_secrets 兜底）；重试不产生重复写操作（第一期无写操作，流式已有输出后不再重试）。注：原型 `ai_review` 的直连 HTTP 与 Preview 完整流程（策略化 Mask/Exclude/二次扫描）分别随 AI-03 落地下线/补齐。 |

### 子任务清单

- [x] `AiRequest` / `ContextItem` / 结果模型类型 + golden 快照
- [x] 生命周期状态机
- [x] Provider 模型迁移（`kind` → `apiType`，含 DB 迁移 / 前端类型与下拉 / golden 快照）
- [x] 三个协议 Adapter（complete + stream）：OpenAI Chat Completions / OpenAI Responses / Anthropic Messages
- [x] 流式事件契约与前端合帧
- [x] 重试 / 取消 / 超时 / 并发上限
- [x] 错误码与 `ai.log` 审计接入
- [x] fake Provider 测试套件
