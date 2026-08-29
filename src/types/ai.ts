// AI Assistant 类型（AI-01，设计文档 §6 / §12.2）。
// 字段与 Rust serde 类型一一对应（golden-file 快照守卫，见
// src-tauri/src/models/ipc_golden/）。

// ---------------------------------------------------------------------------
// 原型命令（ai_review / ai_search）
// ---------------------------------------------------------------------------

export interface ReviewResult {
  summary: string;
  issues: ReviewIssue[];
}

export interface ReviewIssue {
  severity: string;
  category: string;
  file: string;
  description: string;
}

export interface SearchResult {
  repoPath: string;
  filePath: string;
  snippet: string;
  rank: number;
}

// ---------------------------------------------------------------------------
// 枚举（字符串联合，与 Rust enum 的 camelCase 序列化对齐）
// ---------------------------------------------------------------------------

export type ApiType = "openaiChatCompletions" | "openaiResponses" | "anthropicMessages";
export type NetworkPolicy = "onlineOnly" | "localOnly";
export type ModelCapability = "chat" | "structuredOutput" | "toolCalling" | "vision";
export type AiTaskKind =
  | "chat"
  | "runtimeDiagnostic"
  | "gitReview"
  | "commitMessage"
  | "conflict";
export type ModelResolutionSource =
  | "explicit"
  | "workspaceTask"
  | "globalTask"
  | "chatDefault"
  | "firstAvailable";

// ---------------------------------------------------------------------------
// Provider（§6.1）
// ---------------------------------------------------------------------------

export interface AiProvider {
  id: string;
  name: string;
  /** 接口协议类型（§6.1 / §21 决策 9），决定使用哪个 Provider Adapter。 */
  apiType: ApiType;
  baseUrl: string;
  /** OS Credential Store 引用（永不包含 Key 本身）。 */
  credentialRef: string | null;
  /** 凭证实况：Key 是否已录入（OS 存储或会话内存）。 */
  hasCredential: boolean;
  /** 凭证是否仅存在于本次会话内存（不落盘）。 */
  sessionOnlyCredential: boolean;
  enabled: boolean;
  networkPolicy: NetworkPolicy;
  createdAt: string;
  updatedAt: string;
}

export interface SaveAiProviderRequest {
  /** 为空 = 新建。 */
  id: string | null;
  name: string;
  apiType: ApiType;
  baseUrl: string;
  enabled: boolean;
  networkPolicy: NetworkPolicy;
}

export interface AiProviderTestResult {
  success: boolean;
  /** 用户可读结果说明（失败原因为可行动提示）。 */
  message: string;
  /** 发现的模型 ID 清单。 */
  models: string[];
  latencyMs: number;
}

// ---------------------------------------------------------------------------
// 模型（§6.2）
// ---------------------------------------------------------------------------

export interface AiModelDefaults {
  temperature?: number;
}

export interface AiModel {
  providerId: string;
  /** Provider 侧模型 ID（如 gpt-4o-mini）。 */
  id: string;
  displayName: string;
  capabilities: ModelCapability[];
  maxContextTokens: number;
  defaults: AiModelDefaults;
  enabled: boolean;
  createdAt: string;
  updatedAt: string;
}

export interface SaveAiModelRequest {
  providerId: string;
  id: string;
  displayName: string;
  capabilities: ModelCapability[];
  maxContextTokens: number;
  defaults: AiModelDefaults;
  enabled: boolean;
}

// ---------------------------------------------------------------------------
// 任务默认值（§6.3）
// ---------------------------------------------------------------------------

export interface AiTaskDefault {
  taskKind: AiTaskKind;
  /** 空 = 全局默认，否则为 Workspace 覆盖。 */
  workspaceId: number | null;
  providerId: string;
  modelId: string;
  updatedAt: string;
}

// ---------------------------------------------------------------------------
// Settings Summary / 凭证（§12.2）
// ---------------------------------------------------------------------------

export interface AiSettingsSummary {
  providerCount: number;
  enabledProviderCount: number;
  modelCount: number;
  enabledModelCount: number;
  taskDefaults: AiTaskDefault[];
  /** OS Credential Store 是否可用（不可用时可走「仅本次会话」）。 */
  osCredentialStoreAvailable: boolean;
  /** 仅保存在本次会话内存中的凭证数量（不落盘）。 */
  sessionCredentialCount: number;
  /** 原型遗留表历史行数（兼容读取）。 */
  legacyReviewCount: number;
  legacyTaskCount: number;
}

export interface AiCredentialStatus {
  providerId: string;
  hasCredential: boolean;
  /** 仅存在于本次会话内存（不落盘）。 */
  sessionOnly: boolean;
  osStoreAvailable: boolean;
}

// ---------------------------------------------------------------------------
// AI-02 Gateway：请求模型 / 结果模型 / 流式事件契约（§7 / §8.4 / §16.1）
// ---------------------------------------------------------------------------

export type ContextKind = "diff" | "log" | "error" | "repository" | "runtime" | "dependency" | "file";

/** 上下文清单条目（§7.1）：只描述来源与计量，不含正文。 */
export interface ContextItem {
  kind: ContextKind;
  /** 来源标识（如文件路径、运行时名、diff 范围）。 */
  sourceId: string;
  displayName: string;
  charCount: number;
  estimatedTokens: number;
  /** 是否经过脱敏（T-08 Mask）。 */
  redacted: boolean;
  /** 是否被用户排除（排除项不参与估算与发送）。 */
  excluded: boolean;
}

export type MessageRole = "system" | "user" | "assistant";

export interface AiMessage {
  role: MessageRole;
  content: string;
}

/** 期望的响应形态；Json 要求模型具备 structuredOutput 能力。 */
export type ResponseFormat = "text" | "json";

/** 工具策略（§9）：第一期只读。 */
export type ToolPolicy = "disabled" | "readOnlyWhitelist";

/** 类型化 AI 请求（§7.1）。 */
export interface AiRequest {
  requestId: string;
  /** 会话 ID（AI-04 落地后由会话层填充）。 */
  sessionId: string | null;
  taskKind: AiTaskKind;
  /** 显式指定 Provider/模型；为空时走任务默认模型解析链。 */
  providerId: string | null;
  modelId: string | null;
  systemInstruction: string;
  messages: AiMessage[];
  contextManifest: ContextItem[];
  responseFormat: ResponseFormat;
  toolPolicy: ToolPolicy;
  /** 请求总 token 预算（prompt + completion 估算上限；0 = 不限制）。 */
  tokenBudget: number;
  temperature?: number | null;
  stream: boolean;
}

export interface AiTokenUsage {
  inputTokens: number | null;
  outputTokens: number | null;
}

/** 结构化结果类别（§8.4）；非法 JSON 降级为纯文本 Answer。 */
export type AiResult =
  | { type: "answer"; text: string }
  | { type: "diagnosticReport"; payload: Record<string, unknown> }
  | { type: "reviewReport"; payload: Record<string, unknown> }
  | { type: "generatedText"; text: string }
  | { type: "conflictProposal"; payload: Record<string, unknown> }
  | { type: "actionProposal"; payload: Record<string, unknown> };

/** 请求生命周期阶段（§7.3）。 */
export type RequestPhase =
  | "created"
  | "contextBuilding"
  | "secretScanning"
  | "previewRequired"
  | "userApproved"
  | "queued"
  | "sending"
  | "streaming"
  | "parsing"
  | "succeeded"
  | "cancelled"
  | "rejected"
  | "failed"
  | "degraded";

/** 归一化流式 chunk（各协议事件统一映射）。 */
export type AiStreamChunk =
  | { type: "textDelta"; text: string }
  | { type: "end"; finishReason: string | null };

/** `ai-request://progress` 事件 payload。 */
export interface AiRequestEvent {
  requestId: string;
  phase: RequestPhase;
  /** 流式 chunk（仅 Streaming 阶段携带）。 */
  chunk?: AiStreamChunk;
  /** 已累计输出的字符数（诊断用，不含内容本身）。 */
  outputChars: number;
}

/** 请求状态快照（不含 Prompt 内容）。 */
export interface AiRequestSnapshot {
  requestId: string;
  sessionId: string | null;
  taskKind: AiTaskKind;
  providerId: string;
  modelId: string;
  phase: RequestPhase;
  stream: boolean;
  estimatedPromptTokens: number;
  outputChars: number;
  attempts: number;
  usage: AiTokenUsage | null;
  result: AiResult | null;
  error: string | null;
  errorCode: string | null;
}
