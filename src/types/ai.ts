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

export type ProviderKind = "openaiCompatible" | "ark" | "ollama" | "custom";
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
  kind: ProviderKind;
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
  kind: ProviderKind;
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
