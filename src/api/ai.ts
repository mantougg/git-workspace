import { invoke } from "@tauri-apps/api/core";
import type {
  AiCredentialStatus,
  AiModel,
  AiProvider,
  AiProviderTestResult,
  AiSettingsSummary,
  AiTaskDefault,
  AiTaskKind,
  ReviewResult,
  SaveAiModelRequest,
  SaveAiProviderRequest,
  SearchResult,
} from "@/types/ai";

// ---------------------------------------------------------------------------
// 原型命令（Phase A 兼容保留）：Provider/模型/凭证由 AI 设置解析，
// 不再前端传 Key。
// ---------------------------------------------------------------------------

export function aiReview(repoPath: string): Promise<ReviewResult> {
  return invoke<ReviewResult>("ai_review", { repoPath });
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
