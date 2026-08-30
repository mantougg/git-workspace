import { invoke } from "@tauri-apps/api/core";
import type {
  AiContextPreview,
  AiCredentialStatus,
  AiModel,
  AiProvider,
  AiProviderTestResult,
  AiRequest,
  AiRequestSnapshot,
  AiSettingsSummary,
  AiTaskDefault,
  AiTaskKind,
  ContextPreviewRequest,
  ReviewResult,
  SaveAiModelRequest,
  SaveAiProviderRequest,
  SearchResult,
  AiToolDefinition,
  ToolCallRequest,
  ToolInvocation,
  RuntimeDiagnosticRequest,
  GitDiffSelection,
  AiSession,
  AiSessionDetail,
  AiSessionList,
  AiSessionListQuery,
  CreateAiSessionRequest,
  AiSessionPersistence,
  AiSessionExport,
} from "@/types/ai";

// ---------------------------------------------------------------------------
// 原型命令（Phase A 兼容保留）：Provider/模型/凭证由 AI 设置解析，
// 不再前端传 Key。
// ---------------------------------------------------------------------------

export function aiReview(repoPath: string, diffSelection?: GitDiffSelection): Promise<ReviewResult> {
  return invoke<ReviewResult>("ai_review", {
    repoPath,
    diffSelection: diffSelection ?? null,
  });
}

export function buildCodeIndex(repoPath: string): Promise<void> {
  return invoke<void>("build_code_index", { repoPath });
}

export function aiSearch(query: string): Promise<SearchResult[]> {
  return invoke<SearchResult[]>("ai_search", { query });
}

export function clearCodeIndex(repoPath: string): Promise<void> {
  return invoke<void>("clear_code_index", { repoPath });
}

// ---------------------------------------------------------------------------
// AI-01：Provider 管理（§6.1 / §12.2）
// ---------------------------------------------------------------------------

export function aiListProviders(): Promise<AiProvider[]> {
  return invoke<AiProvider[]>("ai_list_providers");
}

export function aiSaveProvider(input: SaveAiProviderRequest): Promise<AiProvider> {
  return invoke<AiProvider>("ai_save_provider", { input });
}

export function aiRemoveProvider(providerId: string): Promise<void> {
  return invoke<void>("ai_remove_provider", { providerId });
}

export function aiTestProvider(providerId: string): Promise<AiProviderTestResult> {
  return invoke<AiProviderTestResult>("ai_test_provider", { providerId });
}

// ---------------------------------------------------------------------------
// AI-01：模型管理（§6.2）
// ---------------------------------------------------------------------------

export function aiListModels(providerId?: string): Promise<AiModel[]> {
  return invoke<AiModel[]>("ai_list_models", { providerId: providerId ?? null });
}

export function aiSaveModel(input: SaveAiModelRequest): Promise<AiModel> {
  return invoke<AiModel>("ai_save_model", { input });
}

export function aiRemoveModel(providerId: string, modelId: string): Promise<void> {
  return invoke<void>("ai_remove_model", { providerId, modelId });
}

// ---------------------------------------------------------------------------
// AI-01：任务级默认模型（§6.3）
// ---------------------------------------------------------------------------

export function aiSetTaskDefaultModel(
  taskKind: AiTaskKind,
  providerId: string,
  modelId: string,
  workspaceId?: number,
): Promise<AiTaskDefault> {
  return invoke<AiTaskDefault>("ai_set_task_default_model", {
    taskKind,
    providerId,
    modelId,
    workspaceId: workspaceId ?? null,
  });
}

/** 清除任务默认值（Workspace 覆盖清除后回落全局链）。 */
export function aiClearTaskDefaultModel(
  taskKind: AiTaskKind,
  workspaceId?: number,
): Promise<void> {
  return invoke<void>("ai_clear_task_default_model", {
    taskKind,
    workspaceId: workspaceId ?? null,
  });
}

// ---------------------------------------------------------------------------
// AI-01：Settings Summary / 凭证（§6.4 / §12.2）
// ---------------------------------------------------------------------------

export function aiGetSettingsSummary(): Promise<AiSettingsSummary> {
  return invoke<AiSettingsSummary>("ai_get_settings_summary");
}

/**
 * 设置/替换 Provider 的 API Key。
 * persist=true 写入 OS Credential Store（不可用时返回 AiCredentialUnavailable，
 * 不回退普通文件）；persist=false 仅本次会话内存保存（不落盘）。
 * Key 只在本函数调用期内存在于前端内存，调用方不得持久化。
 */
export function aiSetCredential(
  providerId: string,
  apiKey: string,
  persist: boolean,
): Promise<AiCredentialStatus> {
  return invoke<AiCredentialStatus>("ai_set_credential", { providerId, apiKey, persist });
}

export function aiClearCredential(providerId: string): Promise<AiCredentialStatus> {
  return invoke<AiCredentialStatus>("ai_clear_credential", { providerId });
}

// ---------------------------------------------------------------------------
// AI-02：Gateway 请求生命周期（§7.3 / §12.1）
// ---------------------------------------------------------------------------

/**
 * 提交 AI 请求：模型解析 + 能力/Secret/预算前置校验，停在 PreviewRequired。
 * 本命令不发起任何网络请求；联网必须经 aiApproveRequest（Preview 闸门）。
 */
export function aiSubmitRequest(request: AiRequest): Promise<AiRequestSnapshot> {
  return invoke<AiRequestSnapshot>("ai_submit_request", { request });
}

/** 确认 Preview 并开始执行（Gateway 唯一联网入口）。 */
export function aiApproveRequest(requestId: string): Promise<AiRequestSnapshot> {
  return invoke<AiRequestSnapshot>("ai_approve_request", { requestId });
}

/** 取消请求（幂等）：中断进行中的流式响应。 */
export function aiCancelRequest(requestId: string): Promise<AiRequestSnapshot> {
  return invoke<AiRequestSnapshot>("ai_cancel_request", { requestId });
}

/** 查询请求状态快照（不存在返回 null；不含 Prompt 内容）。 */
export function aiGetRequestStatus(requestId: string): Promise<AiRequestSnapshot | null> {
  return invoke<AiRequestSnapshot | null>("ai_get_request_status", { requestId });
}

// ---------------------------------------------------------------------------
// AI-03：发送前 Preview（§10.1）
// ---------------------------------------------------------------------------

/**
 * 构建发送前 Preview：收集上下文（只调现有领域服务）→ Secret 管道 →
 * 预算策略 → Prompt 分层 → 内容 hash。本命令不发起任何网络请求；
 * 用户确认后把返回的 `request` 交给 aiSubmitRequest。
 * 排除项变更 = 用新 `exclusions` 重新调用（重算扫描/估算/hash）。
 */
export function aiBuildContextPreview(req: ContextPreviewRequest): Promise<AiContextPreview> {
  return invoke<AiContextPreview>("ai_build_context_preview", { req });
}

/** AI-06 Runtime 失败诊断/日志选段 Preview；构建阶段零网络。 */
export function aiRuntimeDiagnosticPreview(
  req: RuntimeDiagnosticRequest,
): Promise<AiContextPreview> {
  return invoke<AiContextPreview>("ai_runtime_diagnostic_preview", { req });
}

// ---------------------------------------------------------------------------
// AI-05：只读工具注册表
// ---------------------------------------------------------------------------

export function aiListTools(): Promise<AiToolDefinition[]> {
  return invoke<AiToolDefinition[]>("ai_list_tools");
}

export function aiExecuteTool(request: ToolCallRequest): Promise<ToolInvocation> {
  return invoke<ToolInvocation>("ai_execute_tool", { request });
}

// ---------------------------------------------------------------------------
// AI-04：统一 Assistant 会话
// ---------------------------------------------------------------------------

export function aiCreateSession(input: CreateAiSessionRequest): Promise<AiSession> {
  return invoke<AiSession>("ai_create_session", { input });
}

export function aiListSessions(query: AiSessionListQuery): Promise<AiSessionList> {
  return invoke<AiSessionList>("ai_list_sessions", { query });
}

export function aiGetSession(
  sessionId: string,
  messageLimit = 50,
  beforeSequence?: number,
): Promise<AiSessionDetail | null> {
  return invoke<AiSessionDetail | null>("ai_get_session", {
    sessionId,
    messageLimit,
    beforeSequence: beforeSequence ?? null,
  });
}

export function aiRenameSession(sessionId: string, title: string): Promise<AiSession> {
  return invoke<AiSession>("ai_rename_session", { sessionId, title });
}

export function aiArchiveSession(sessionId: string, archived: boolean): Promise<AiSession> {
  return invoke<AiSession>("ai_archive_session", { sessionId, archived });
}

export function aiDeleteSession(sessionId: string): Promise<void> {
  return invoke<void>("ai_delete_session", { sessionId });
}

/** 导出会话为 Markdown 文件（内容由后端从结构化消息渲染，不含 Secret 原文）。 */
export function aiExportSession(sessionId: string, destPath: string): Promise<AiSessionExport> {
  return invoke<AiSessionExport>("ai_export_session", { sessionId, destPath });
}

export function aiGetSessionPersistence(): Promise<AiSessionPersistence> {
  return invoke<AiSessionPersistence>("ai_get_session_persistence");
}

export function aiSetSessionPersistence(persist: boolean): Promise<AiSessionPersistence> {
  return invoke<AiSessionPersistence>("ai_set_session_persistence", { persist });
}
