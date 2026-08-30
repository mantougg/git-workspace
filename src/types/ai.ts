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
  line?: number | null;
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
/** AI-08 Git Assistant scenario; it shares the existing task-level model defaults. */
export type GitAssistantScenario =
  | "commitMessage"
  | "commitSummary"
  | "codeReview"
  | "securityReview"
  | "bugDetection"
  | "prDescription"
  | "commitExplanation"
  | "fileExplanation";
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

/** 条目被排除的原因（§8.2 / §10.2）；未排除为 null。 */
export type ExclusionReason = "user" | "budgetOverflow" | "secretPolicy";

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
  /** 是否为适配预算被截断（§8.2：截断必须可见）。 */
  truncated: boolean;
  /** 是否被排除（排除项不参与估算与发送）。 */
  excluded: boolean;
  /** 排除原因；未排除为 null。 */
  exclusionReason?: ExclusionReason | null;
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

/** Restricted roles and scopes exposed by the AI-05 tool registry.
 * `externalAgent`（AI-12）是外部 Agent 的独立身份，仅存在于 Adapter 层，
 * 不进入任何工具白名单——前端调用工具时使用其余内置角色。 */
export type ToolRole =
  | "workspaceAssistant"
  | "gitReviewer"
  | "commitAssistant"
  | "conflictAssistant"
  | "runtimeDiagnostician"
  | "runtimeConfigAdvisor"
  | "actionPlanner"
  | "externalAgent";
export type ToolScope = "workspace" | "repository" | "runtime" | "jdk" | "maven" | "task";

export interface AiToolDefinition {
  name: string;
  version: string;
  inputSchema: Record<string, unknown>;
  allowedRoles: ToolRole[];
  contextScope: ToolScope;
  requiresWorkspace: boolean;
  mayContainSecrets: boolean;
  timeoutMs: number;
  maxResultBytes: number;
  readOnly: boolean;
}

export interface ToolCallRequest {
  requestId: string;
  toolName: string;
  role: ToolRole;
  arguments: Record<string, unknown>;
}

export interface ToolInvocation {
  requestId: string;
  toolName: string;
  role: ToolRole;
  result: unknown;
  truncated: boolean;
  resultBytes: number;
  totalResultBytes: number;
  durationMs: number;
  parameterHash: string;
}

// AI-11：受控写操作提案。Action payload 不通过 IPC 返回，确认后由后端
// 转换为现有 Task Queue 任务。
export type ActionKind =
  | "gitCreateCommit"
  | "runtimeStart"
  | "conflictApply"
  | "runtimeUpdateConfig";
export type RiskLevel = "low" | "medium" | "high";
export type ProposalStatus = "pending" | "confirmed" | "executed" | "rejected" | "expired";

export interface ActionProposal {
  proposalId: string;
  requestId: string | null;
  actionKind: ActionKind;
  riskLevel: RiskLevel;
  targetScope: Record<string, unknown>;
  affectedRepositories: string[];
  affectedFiles: string[];
  beforeSummary: string;
  afterSummary: string;
  diff: string | null;
  commandPreview: string | null;
  reversible: boolean;
  expiresAt: string;
  status: ProposalStatus;
  confirmedAt: string | null;
  executedTaskId: string | null;
  createdAt: string;
}

/** 类型化 AI 请求（§7.1）。 */
export interface AiRequest {
  requestId: string;
  /** 会话 ID（AI-04 落地后由会话层填充）。 */
  sessionId: string | null;
  taskKind: AiTaskKind;
  /** AI-08 scene-specific prompt/result schema; null keeps legacy behavior. */
  gitScenario?: GitAssistantScenario | null;
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
  /** §10.2 Warn：用户在 Preview 中明确确认「知晓 Secret 提示仍发送」后置 true。 */
  secretWarnConfirmed?: boolean;
  /** 是否允许复用结果缓存（§11.3）；「重新生成」场景置 false。 */
  useCache?: boolean;
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
  | { type: "commitSuggestion"; payload: CommitSuggestion }
  | { type: "commitSummary"; payload: CommitSummary }
  | { type: "prDescription"; payload: PrDescription }
  | { type: "explanation"; payload: ExplanationResult }
  | { type: "conflictProposal"; payload: ConflictProposal }
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
  /** 结果是否来自缓存（§11.3：UI 需区分「过期结果」与「当前事实」）。 */
  fromCache: boolean;
}

// ---------------------------------------------------------------------------
// AI-03 Context Builder / Preview（§8 / §10.1 / §10.2）
// ---------------------------------------------------------------------------

/** 上下文角色（预算策略 §8.2 的优先级依据）。 */
export type ContextRole =
  | "structuredError"
  | "errorLog"
  | "logTail"
  | "selectedLogRange"
  | "exceptionStack"
  | "environmentSummary"
  | "runtimeConfig"
  | "processInfo"
  | "fileList"
  | "hunkStructure"
  | "fullDiff"
  | "changeSummary"
  | "repoSummary"
  | "history"
  | "conflictState"
  | "conflictContent"
  | "dependency"
  | "userNote";

/** 五类预算策略（§8.2）。 */
export type BudgetStrategy =
  | "errorDiagnosis"
  | "logAnalysis"
  | "codeReview"
  | "commitMessage"
  | "multiRepoSummary";

/** Secret 处理策略（§10.2；Exclude 走条目级 exclusions）。 */
export type SecretStrategyKind = "block" | "mask" | "warn";

/** 请求的 Secret 策略（默认 Block）。 */
export interface SecretPolicyChoice {
  strategy: SecretStrategyKind;
  /** Warn 策略下用户已明确确认「知晓风险仍发送」。 */
  warnConfirmed?: boolean;
}

/** diff 范围。 */
export type DiffScope = "workdir" | "staged" | "unstaged";

/** 一个 Repository 内的文件/目录选择；路径相对仓库根目录。 */
export interface DiffRepositorySelection {
  repoPath: string;
  /** 为空表示该仓库的全部变更文件。 */
  includePaths: string[];
  /** 文件或目录路径，排除优先于 includePaths。 */
  excludePaths: string[];
}

/** Commit/Review/Conflict 共用的多仓库 Diff 选择。 */
export interface GitDiffSelection {
  repositories: DiffRepositorySelection[];
}

/** 调用方注入的补充上下文（结构化错误、UI 选中的日志范围等）。 */
export interface SupplementaryContext {
  role: ContextRole;
  kind: ContextKind;
  sourceId: string;
  displayName: string;
  content: string;
  /** 来源侧已脱敏。 */
  redacted?: boolean;
}

/** A single conflict-marker hunk used for a bounded AI-09 Preview request. */
export interface ConflictPreviewTarget {
  path: string;
  hunkIndex: number;
  hunkTotal: number;
}

/** Preview 构建请求（`ai_build_context_preview` 入参）。 */
export interface ContextPreviewRequest {
  taskKind: AiTaskKind;
  gitScenario?: GitAssistantScenario | null;
  /** 显式 Provider/模型；为空走任务默认解析链（§6.3）。 */
  providerId: string | null;
  modelId: string | null;
  /** 目标范围（按任务种类取用）。 */
  workspaceId: number | null;
  repoPath: string | null;
  /** Optional bounded conflict hunk. Null preserves the legacy all-files context. */
  conflict?: ConflictPreviewTarget | null;
  runtimeName: string | null;
  processId: number | null;
  /** 依赖上下文的目标项目（R-02/R-03）。 */
  project: string | null;
  /** 用户补充指令（作为 user 消息，不进系统约束，§8.3）。 */
  userInstruction?: string;
  /** diff 范围（默认：commitMessage → staged，其余 → workdir）。 */
  diffScope?: DiffScope | null;
  /** 多仓库 / 目录 / 文件选择；为空时兼容使用 repoPath。 */
  diffSelection?: GitDiffSelection | null;
  supplementary?: SupplementaryContext[];
  /** 用户排除的 sourceId 列表（§10.2 Exclude；变更后整体重建）。 */
  exclusions?: string[];
  /** Secret 策略（默认 Block）。 */
  secretPolicy?: SecretPolicyChoice;
  /** 预算策略覆盖（默认按任务种类）。 */
  budgetStrategy?: BudgetStrategy | null;
  stream?: boolean;
  /** token 估算校准系数（默认 1.0 = chars/4 基准）。 */
  tokenEstimateFactor?: number | null;
  /** 日志尾部行数覆盖（默认 200）。 */
  logTailLines?: number | null;
  /** token 预算覆盖（默认 = 模型上下文上限的 3/4）。 */
  tokenBudget?: number | null;
  /** 选中日志场景关闭 Runtime 自动日志收集，避免发送未选中的日志。 */
  includeRuntimeLogs?: boolean;
}

export interface DiagnosticErrorInput {
  code: string;
  message: string;
  details?: Record<string, unknown> | null;
  occurredAt?: string | null;
}

export interface RuntimeDiagnosticRequest {
  workspaceId: number;
  runtimeName: string;
  processId?: number | null;
  error?: DiagnosticErrorInput | null;
  project?: string | null;
  wantConfigAdvice?: boolean;
  userInstruction?: string;
  exclusions?: string[];
  secretPolicy?: SecretPolicyChoice;
  logTailLines?: number | null;
  selectedLog?: string | null;
  tokenBudget?: number | null;
  stream?: boolean;
  tokenEstimateFactor?: number | null;
}

export interface DiagnosticReport {
  headline: string;
  confidence: "high" | "medium" | "low";
  facts: string[];
  likelyCauses: string[];
  suggestedActions: string[];
  needsUserCheck: string[];
  sourceContext: string[];
}

/** AI-08 structured Commit Message suggestion; shown in an editable existing Commit input. */
export interface CommitSuggestion {
  title: string;
  body: string[];
  type?: string | null;
  scope?: string | null;
  changedRepositories: string[];
  rationale: string;
}

export interface CommitSummaryRepository {
  path: string;
  summary: string;
  risk: string;
}

export interface CommitSummary {
  summary: string;
  repositories: CommitSummaryRepository[];
  risks: string[];
}

export interface PrDescription {
  title: string;
  description: string;
  summary: string[];
  testing: string[];
  risks: string[];
}

export interface ExplanationResult {
  summary: string;
  details: string[];
  riskNotes: string[];
}

/** AI-09 Conflict Assistant suggestion. It is display-only until the user
 * explicitly confirms the existing T-16 Apply / Mark Resolved action. */
export interface ConflictProposal {
  proposedContent: string;
  /** Unified diff supplied for Preview; the RESULT editor remains authoritative. */
  diff: string;
  rationale: string;
  confidence: "high" | "medium" | "low";
}

/** 目标范围（§10.1「目标 Workspace/Repository/Runtime」）。 */
export interface PreviewTarget {
  workspaceId: number | null;
  workspaceName: string | null;
  repoPath: string | null;
  /** 参与本次 Git 上下文的仓库清单。 */
  repositoryPaths: string[];
  runtimeName: string | null;
  processId: number | null;
}

/** 单条目的 Secret 命中摘要（类别 + 次数；不含原文/位置）。 */
export interface SecretFindingSummary {
  sourceId: string;
  displayName: string;
  kinds: string[];
  count: number;
}

/** Secret 管道结果（§10.2）。 */
export interface SecretReport {
  findings: SecretFindingSummary[];
  /** 被自动脱敏的条目 sourceId（§10.1「自动脱敏项」）。 */
  maskedSources: string[];
  /** 是否阻断发送（Block 命中 / Mask 二次扫描仍命中 / Warn 未确认）。 */
  blocked: boolean;
  /** 阻断原因涉及的 Secret 类别。 */
  blockKinds: string[];
  /** Warn 策略存在命中且用户尚未确认。 */
  warnPending: boolean;
}

/** 发送前 Preview（§10.1 全字段）；`request` 可直接提交 `ai_submit_request`。 */
export interface AiContextPreview {
  requestId: string;
  taskKind: AiTaskKind;
  gitScenario?: GitAssistantScenario | null;
  providerId: string;
  providerName: string;
  modelId: string;
  modelName: string;
  target: PreviewTarget;
  /** Context Manifest（每项字符数、估算 token、脱敏/截断/排除标记）。 */
  items: ContextItem[];
  /** 参与发送的合计（排除项不计）。 */
  totalChars: number;
  totalEstimatedTokens: number;
  budgetTokens: number;
  budgetStrategy: BudgetStrategy;
  /** Secret 检测结果。 */
  secret: SecretReport;
  /** 预算截断的条目（§8.2 可见性）。 */
  truncatedSources: string[];
  /** 预算排除的条目（§8.2 可见性）。 */
  budgetExcludedSources: string[];
  /** 预计请求次数（第一期单请求 = 1）。 */
  estimatedRequests: number;
  /** 成本估算（无定价数据源，恒为 null）。 */
  costEstimate: string | null;
  /** 是否会使用网络。 */
  usesNetwork: boolean;
  /** 是否阻断发送。 */
  blocked: boolean;
  /** 阻断原因（用户可读）。 */
  blockReasons: string[];
  /** 最终内容 hash（§7.3；排除项变更后重建即变）。 */
  contentHash: string;
  /** 可直接提交 `ai_submit_request` 的请求。 */
  request: AiRequest;
}

// ---------------------------------------------------------------------------
// AI-04 会话 / 消息 / 请求审计（§10.4 / §11.2 / §16.1）
// ---------------------------------------------------------------------------

/** 会话角色（§9.2 七个受限角色；§12.3 Drawer 顶部「当前角色」）。
 * 序列化值与 ToolRole 对齐，角色即工具白名单准入身份。 */
export type AiSessionRole =
  | "workspaceAssistant"
  | "gitReviewer"
  | "commitAssistant"
  | "conflictAssistant"
  | "runtimeDiagnostician"
  | "runtimeConfigAdvisor"
  | "actionPlanner";

/** 会话（§11.2 `ai_sessions`）。 */
export interface AiSession {
  id: string;
  title: string;
  role: AiSessionRole;
  workspaceId: number | null;
  /** 作用域内的仓库路径清单（归一化正斜杠）。 */
  repositoryScope: string[];
  /** Runtime 作用域（runtime 名 / 进程 id 等）。 */
  runtimeScope: unknown;
  createdAt: string;
  updatedAt: string;
  /** 归档时间；null = 未归档。 */
  archivedAt: string | null;
  /** 消息条数（列表用）。 */
  messageCount: number;
}

export interface CreateAiSessionRequest {
  title: string;
  role?: AiSessionRole | null;
  workspaceId: number | null;
  repositoryScope?: string[];
  runtimeScope?: unknown;
}

/** 会话列表查询（分页 + 归档过滤）。 */
export interface AiSessionListQuery {
  workspaceId: number | null;
  /** 是否包含已归档会话（默认 false）。 */
  includeArchived?: boolean;
  /** 每页条数（默认 20，上限 100）。 */
  limit?: number | null;
  offset?: number | null;
}

export interface AiSessionList {
  items: AiSession[];
  /** 满足过滤条件的总条数。 */
  total: number;
}

/** 会话消息（§11.2 `ai_messages`）。 */
export interface AiSessionMessage {
  id: number;
  sessionId: string;
  role: MessageRole;
  /** 结构化内容（Secret 原文永不入库）。 */
  content: unknown;
  sequence: number;
  createdAt: string;
}

/** 会话详情（会话 + 按需加载的消息窗口）。 */
export interface AiSessionDetail {
  session: AiSession;
  messages: AiSessionMessage[];
  /** 消息总条数；大于 messages.length 表示还有更早的历史。 */
  totalMessages: number;
}

/** 会话持久化设置（§10.4：完整会话是否保存由用户设置决定）。 */
export interface AiSessionPersistence {
  persistSessions: boolean;
  sessionCount: number;
}

/** 会话导出结果（`ai_export_session` 返回）。 */
export interface AiSessionExport {
  sessionId: string;
  title: string;
  /** 实际写入的文件路径。 */
  path: string;
  /** 导出的消息条数。 */
  messageCount: number;
}

/**
 * 请求审计（§10.4 / §16.3）：只含元数据与 Secret 计数，不含 Prompt 原文。
 * status 取生命周期阶段名；缓存命中为 `cached`。
 */
export interface AiRequestAudit {
  id: string;
  sessionId: string | null;
  taskKind: AiTaskKind;
  providerId: string;
  modelId: string;
  /** 最终内容 hash（与缓存 contextHash 同口径）。 */
  inputHash: string;
  contextManifest: ContextItem[];
  status: string;
  errorCode: string | null;
  /** Secret 类别 → 命中次数（不含原文）。 */
  secretCounts: Record<string, number>;
  inputTokens: number | null;
  outputTokens: number | null;
  latencyMs: number | null;
  createdAt: string;
  finishedAt: string | null;
}
