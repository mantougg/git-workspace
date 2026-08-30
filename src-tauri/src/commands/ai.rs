//! AI IPC 命令薄适配层（设计文档 §12.1）。
//!
//! 业务逻辑在 `crate::ai::{provider, model, credentials}`，本层只做参数校验、
//! DB 锁管理与凭证实况填充。Key 永远不进日志/错误/返回值（全局约束 §4）。

use std::path::Path;

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::ai;
use crate::ai::error::AiError;
use crate::ai::tools::{self, ToolCallRequest, ToolDefinition, ToolInvocation};
use crate::error::{AppError, AppResult};

// ---------------------------------------------------------------------------
// AI-01：Provider / Model / Credential / 任务默认值 / Settings Summary
// ---------------------------------------------------------------------------

/// AI 设置总览（§12.2「用量与诊断」区块的数据来源之一）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiSettingsSummary {
    pub provider_count: i64,
    pub enabled_provider_count: i64,
    pub model_count: i64,
    pub enabled_model_count: i64,
    pub task_defaults: Vec<ai::AiTaskDefault>,
    /// OS Credential Store 是否可用（不可用时可走「仅本次会话」）。
    pub os_credential_store_available: bool,
    /// 仅保存在本次会话内存中的凭证数量（不落盘）。
    pub session_credential_count: i64,
    /// 原型遗留表的历史行数（兼容读取，不破坏性删除）。
    pub legacy_review_count: i64,
    pub legacy_task_count: i64,
}

/// 凭证实况（§12.2 凭证区块）：永远不包含 Key 本身。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiCredentialStatus {
    pub provider_id: String,
    pub has_credential: bool,
    /// 仅存在于本次会话内存（不落盘）。
    pub session_only: bool,
    pub os_store_available: bool,
}

fn lock_db<'a>(
    state: &'a tauri::State<'_, crate::state::AppState>,
) -> AppResult<std::sync::MutexGuard<'a, Connection>> {
    state
        .db
        .lock()
        .map_err(|e| AppError::Other(format!("DB lock error: {}", e)))
}

fn fill_credential_status(credentials: &ai::CredentialManager, providers: &mut [ai::AiProvider]) {
    for p in providers.iter_mut() {
        if let Some(cref) = &p.credential_ref {
            p.has_credential = credentials.has(cref);
            p.session_only_credential = credentials.is_session_only(cref);
        }
    }
}

#[tauri::command]
pub fn ai_list_providers(
    state: tauri::State<'_, crate::state::AppState>,
) -> AppResult<Vec<ai::AiProvider>> {
    let conn = lock_db(&state)?;
    let mut providers = ai::list_providers(&conn)?;
    fill_credential_status(&state.ai_credentials, &mut providers);
    Ok(providers)
}

#[tauri::command]
pub fn ai_save_provider(
    state: tauri::State<'_, crate::state::AppState>,
    input: ai::SaveAiProviderRequest,
) -> AppResult<ai::AiProvider> {
    let conn = lock_db(&state)?;
    let mut provider = ai::save_provider(&conn, &input)?;
    fill_credential_status(&state.ai_credentials, std::slice::from_mut(&mut provider));
    log::info!(
        "ai provider saved: id={} kind={} enabled={}",
        provider.id,
        provider.api_type.as_str(),
        provider.enabled
    );
    Ok(provider)
}

#[tauri::command]
pub fn ai_remove_provider(
    state: tauri::State<'_, crate::state::AppState>,
    provider_id: String,
) -> AppResult<()> {
    let provider = {
        let conn = lock_db(&state)?;
        let provider = ai::get_provider(&conn, &provider_id)?;
        ai::delete_provider(&conn, &provider_id)?;
        provider
    };
    // 凭证随 Provider 一并清除（OS 存储 + 会话内存）。
    if let Some(p) = provider {
        if let Some(cref) = &p.credential_ref {
            state.ai_credentials.delete(cref)?;
        }
    }
    log::info!("ai provider removed: id={}", provider_id);
    Ok(())
}

#[tauri::command]
pub async fn ai_test_provider(
    state: tauri::State<'_, crate::state::AppState>,
    provider_id: String,
) -> AppResult<ai::AiProviderTestResult> {
    let (provider, api_key) = {
        let conn = lock_db(&state)?;
        let provider = ai::get_provider(&conn, &provider_id)?.ok_or_else(|| {
            AppError::Ai(AiError::NotConfigured {
                message: format!("Provider 不存在: {}", provider_id),
            })
        })?;
        let key = provider
            .credential_ref
            .as_deref()
            .and_then(|cref| state.ai_credentials.get(cref));
        (provider, key)
    };

    // 在线 Provider 缺凭证：返回可行动结果而非报错（§12.2）。
    if provider.network_policy == ai::NetworkPolicy::OnlineOnly && api_key.is_none() {
        return Ok(ai::AiProviderTestResult {
            success: false,
            message: "未配置 API Key：请先在「凭证」区块为该 Provider 录入 Key".to_string(),
            models: vec![],
            latency_ms: 0,
        });
    }

    let result = ai::test_connection(&provider, api_key.as_deref()).await;
    log::info!(
        "ai provider test: id={} success={} latency_ms={} models={}",
        provider.id,
        result.success,
        result.latency_ms,
        result.models.len()
    );
    Ok(result)
}

#[tauri::command]
pub fn ai_list_models(
    state: tauri::State<'_, crate::state::AppState>,
    provider_id: Option<String>,
) -> AppResult<Vec<ai::AiModel>> {
    let conn = lock_db(&state)?;
    ai::list_models(&conn, provider_id.as_deref())
}

#[tauri::command]
pub fn ai_save_model(
    state: tauri::State<'_, crate::state::AppState>,
    input: ai::SaveAiModelRequest,
) -> AppResult<ai::AiModel> {
    let conn = lock_db(&state)?;
    let model = ai::model::save_model(&conn, &input)?;
    log::info!(
        "ai model saved: {}/{} enabled={}",
        model.provider_id,
        model.id,
        model.enabled
    );
    Ok(model)
}

#[tauri::command]
pub fn ai_remove_model(
    state: tauri::State<'_, crate::state::AppState>,
    provider_id: String,
    model_id: String,
) -> AppResult<()> {
    let conn = lock_db(&state)?;
    ai::model::delete_model(&conn, &provider_id, &model_id)?;
    log::info!("ai model removed: {}/{}", provider_id, model_id);
    Ok(())
}

#[tauri::command]
pub fn ai_set_task_default_model(
    state: tauri::State<'_, crate::state::AppState>,
    task_kind: ai::AiTaskKind,
    provider_id: String,
    model_id: String,
    workspace_id: Option<i64>,
) -> AppResult<ai::AiTaskDefault> {
    let conn = lock_db(&state)?;
    let default =
        ai::model::set_task_default(&conn, task_kind, workspace_id, &provider_id, &model_id)?;
    log::info!(
        "ai task default set: kind={} workspace={:?} model={}/{}",
        task_kind.as_str(),
        workspace_id,
        provider_id,
        model_id
    );
    Ok(default)
}

/// 清除任务默认值（Workspace 覆盖清除后回落全局链，§6.3）。
#[tauri::command]
pub fn ai_clear_task_default_model(
    state: tauri::State<'_, crate::state::AppState>,
    task_kind: ai::AiTaskKind,
    workspace_id: Option<i64>,
) -> AppResult<()> {
    let conn = lock_db(&state)?;
    ai::model::clear_task_default(&conn, task_kind, workspace_id)?;
    Ok(())
}

#[tauri::command]
pub fn ai_get_settings_summary(
    state: tauri::State<'_, crate::state::AppState>,
) -> AppResult<AiSettingsSummary> {
    let conn = lock_db(&state)?;
    let providers = ai::list_providers(&conn)?;
    let models = ai::list_models(&conn, None)?;
    let task_defaults = ai::list_task_defaults(&conn, None)?;
    let legacy_review_count: i64 =
        conn.query_row("SELECT COUNT(*) FROM ai_reviews", [], |r| r.get(0))?;
    let legacy_task_count: i64 =
        conn.query_row("SELECT COUNT(*) FROM ai_tasks", [], |r| r.get(0))?;
    Ok(AiSettingsSummary {
        provider_count: providers.len() as i64,
        enabled_provider_count: providers.iter().filter(|p| p.enabled).count() as i64,
        model_count: models.len() as i64,
        enabled_model_count: models.iter().filter(|m| m.enabled).count() as i64,
        task_defaults,
        os_credential_store_available: state.ai_credentials.os_store_available(),
        session_credential_count: state.ai_credentials.session_count() as i64,
        legacy_review_count,
        legacy_task_count,
    })
}

/// 设置/替换 Provider 的 API Key（§6.4）。
///
/// - `persist = true`：写入 OS Credential Store（不可用时返回
///   `AiCredentialUnavailable`，**不回退普通文件**）；
/// - `persist = false`：仅本次会话内存保存（不落盘）。
/// - Key 只在内存中流经本函数，不进日志、不持久化到 SQLite/文件。
#[tauri::command]
pub fn ai_set_credential(
    state: tauri::State<'_, crate::state::AppState>,
    provider_id: String,
    api_key: String,
    persist: bool,
) -> AppResult<AiCredentialStatus> {
    let key = api_key.trim();
    if key.is_empty() {
        return Err(AppError::Ai(AiError::CredentialUnavailable {
            message: "API Key 不能为空".to_string(),
        }));
    }
    let conn = lock_db(&state)?;
    let provider = ai::get_provider(&conn, &provider_id)?.ok_or_else(|| {
        AppError::Ai(AiError::NotConfigured {
            message: format!("Provider 不存在: {}", provider_id),
        })
    })?;
    let cref = provider
        .credential_ref
        .clone()
        .unwrap_or_else(|| ai::provider::credential_ref_for(&provider_id));
    state.ai_credentials.set(&cref, key, persist)?;
    log::info!(
        "ai credential set: provider={} persist={}",
        provider_id,
        persist
    );
    Ok(AiCredentialStatus {
        provider_id: provider_id.clone(),
        has_credential: state.ai_credentials.has(&cref),
        session_only: state.ai_credentials.is_session_only(&cref),
        os_store_available: state.ai_credentials.os_store_available(),
    })
}

#[tauri::command]
pub fn ai_clear_credential(
    state: tauri::State<'_, crate::state::AppState>,
    provider_id: String,
) -> AppResult<AiCredentialStatus> {
    let conn = lock_db(&state)?;
    let provider = ai::get_provider(&conn, &provider_id)?;
    if let Some(p) = provider {
        if let Some(cref) = &p.credential_ref {
            state.ai_credentials.delete(cref)?;
        }
    }
    log::info!("ai credential cleared: provider={}", provider_id);
    let cref = ai::provider::credential_ref_for(&provider_id);
    Ok(AiCredentialStatus {
        provider_id,
        has_credential: state.ai_credentials.has(&cref),
        session_only: state.ai_credentials.is_session_only(&cref),
        os_store_available: state.ai_credentials.os_store_available(),
    })
}

// ---------------------------------------------------------------------------
// AI-02：AI Gateway 请求生命周期（§7.3 / §12.1）
// ---------------------------------------------------------------------------

/// 提交 AI 请求：模型解析 + 能力/Secret/预算前置校验，停在 PreviewRequired。
/// **本命令不发起任何网络请求**；网络访问只能经 `ai_approve_request`
/// （§7.3 Preview 闸门）。请求内容不落盘，仅驻留 Gateway 内存。
#[tauri::command]
pub fn ai_submit_request(
    state: tauri::State<'_, crate::state::AppState>,
    request: ai::AiRequest,
) -> AppResult<ai::AiRequestSnapshot> {
    let conn = lock_db(&state)?;
    state.ai_gateway.submit(&conn, request)
}

/// 确认 Preview 并开始执行（Gateway 唯一联网入口）。返回提交时快照，
/// 后续状态经 `ai_get_request_status` 轮询或监听 `ai-request://progress`。
#[tauri::command]
pub fn ai_approve_request(
    state: tauri::State<'_, crate::state::AppState>,
    request_id: String,
) -> AppResult<ai::AiRequestSnapshot> {
    state
        .ai_gateway
        .approve(state.ai_credentials.clone(), &request_id)
}

/// 取消请求（幂等）：排队/执行中触发协作取消，中断进行中的流式响应。
#[tauri::command]
pub fn ai_cancel_request(
    state: tauri::State<'_, crate::state::AppState>,
    request_id: String,
) -> AppResult<ai::AiRequestSnapshot> {
    state.ai_gateway.cancel(&request_id)
}

/// 查询请求状态快照（不存在返回 None；不含 Prompt 内容）。
#[tauri::command]
pub fn ai_get_request_status(
    state: tauri::State<'_, crate::state::AppState>,
    request_id: String,
) -> AppResult<Option<ai::AiRequestSnapshot>> {
    Ok(state.ai_gateway.status(&request_id))
}

// ---------------------------------------------------------------------------
// AI-04：会话 / 消息 / 请求审计 / 结果缓存（§10.4 / §11.2 / §11.3 / §12.1）
// ---------------------------------------------------------------------------

/// 创建会话（§11.2）。
#[tauri::command]
pub fn ai_create_session(
    state: tauri::State<'_, crate::state::AppState>,
    input: ai::CreateAiSessionRequest,
) -> AppResult<ai::AiSession> {
    let conn = lock_db(&state)?;
    let session = ai::session::create_session(&conn, &input)?;
    log::info!(
        "ai session created: id={} role={}",
        session.id,
        session.role.as_str()
    );
    Ok(session)
}

/// 会话列表（分页，默认不含已归档；§16.1）。
#[tauri::command]
pub fn ai_list_sessions(
    state: tauri::State<'_, crate::state::AppState>,
    query: ai::AiSessionListQuery,
) -> AppResult<ai::AiSessionList> {
    let conn = lock_db(&state)?;
    ai::session::list_sessions(&conn, &query)
}

/// 读取会话与消息窗口（`beforeSequence` 游标向前翻页；§16.1 按需加载）。
#[tauri::command]
pub fn ai_get_session(
    state: tauri::State<'_, crate::state::AppState>,
    session_id: String,
    message_limit: Option<i64>,
    before_sequence: Option<i64>,
) -> AppResult<Option<ai::AiSessionDetail>> {
    let conn = lock_db(&state)?;
    ai::session::get_session_detail(
        &conn,
        &session_id,
        message_limit.unwrap_or(50),
        before_sequence,
    )
}

#[tauri::command]
pub fn ai_rename_session(
    state: tauri::State<'_, crate::state::AppState>,
    session_id: String,
    title: String,
) -> AppResult<ai::AiSession> {
    let conn = lock_db(&state)?;
    ai::session::rename_session(&conn, &session_id, &title)
}

/// 归档 / 取消归档（`archived = false` 为恢复）。
#[tauri::command]
pub fn ai_archive_session(
    state: tauri::State<'_, crate::state::AppState>,
    session_id: String,
    archived: bool,
) -> AppResult<ai::AiSession> {
    let conn = lock_db(&state)?;
    ai::session::set_archived(&conn, &session_id, archived)
}

/// 删除会话：级联删除消息内容与相关本地缓存（§10.4）。
///
/// DB 侧由外键与显式删除清理；内存 LRU 需显式失效——否则会话已删仍可能
/// 命中内存里的旧结果。
#[tauri::command]
pub fn ai_delete_session(
    state: tauri::State<'_, crate::state::AppState>,
    session_id: String,
) -> AppResult<()> {
    let conn = lock_db(&state)?;
    ai::session::delete_session(&conn, &session_id)?;
    state.ai_result_cache.invalidate_session(&conn, &session_id);
    log::info!("ai session deleted: id={}", session_id);
    Ok(())
}

/// 导出会话为 Markdown 文件（AI-10 §4.2 Phase D）。
///
/// 内容由 `session::export_markdown` 从结构化消息渲染：只含用户指令与
/// 结构化结果字段，不含 Secret 原文（入库前已经过 Secret 管道）。
#[tauri::command]
pub fn ai_export_session(
    state: tauri::State<'_, crate::state::AppState>,
    session_id: String,
    dest_path: String,
) -> AppResult<ai::session::AiSessionExport> {
    let conn = lock_db(&state)?;
    let Some((session, markdown)) = ai::session::export_markdown(&conn, &session_id)? else {
        return Err(AppError::Ai(AiError::NotConfigured {
            message: format!("会话不存在: {}", session_id),
        }));
    };
    std::fs::write(&dest_path, markdown).map_err(|e| AppError::Ai(AiError::NotConfigured {
        message: format!("导出会话失败（{}）: {}", dest_path, e),
    }))?;
    log::info!(
        "ai session exported: id={} messages={} dest={}",
        session_id,
        session.message_count,
        dest_path
    );
    Ok(ai::session::AiSessionExport {
        session_id,
        title: session.title,
        path: dest_path,
        message_count: session.message_count,
    })
}

/// 会话持久化开关（§10.4：完整会话是否保存由用户设置决定）。
#[tauri::command]
pub fn ai_get_session_persistence(
    state: tauri::State<'_, crate::state::AppState>,
) -> AppResult<ai::session::AiSessionPersistence> {
    let conn = lock_db(&state)?;
    ai::session::persistence_settings(&conn)
}

#[tauri::command]
pub fn ai_set_session_persistence(
    state: tauri::State<'_, crate::state::AppState>,
    persist: bool,
) -> AppResult<ai::session::AiSessionPersistence> {
    let conn = lock_db(&state)?;
    ai::session::set_persistence(&conn, persist)?;
    log::info!("ai session persistence set: persist={}", persist);
    ai::session::persistence_settings(&conn)
}

/// 单条请求审计（§10.4 / §16.3：只含元数据，不含 Prompt 原文）。
#[tauri::command]
pub fn ai_get_request_audit(
    state: tauri::State<'_, crate::state::AppState>,
    request_id: String,
) -> AppResult<Option<ai::audit::AiRequestAudit>> {
    let conn = lock_db(&state)?;
    ai::audit::get_audit(&conn, &request_id)
}

/// 会话维度的请求审计列表（最近的在前）。
#[tauri::command]
pub fn ai_list_session_audits(
    state: tauri::State<'_, crate::state::AppState>,
    session_id: String,
    limit: Option<i64>,
) -> AppResult<Vec<ai::audit::AiRequestAudit>> {
    let conn = lock_db(&state)?;
    ai::audit::list_session_audits(&conn, &session_id, limit.unwrap_or(50))
}

/// 清除 AI 结果缓存（§12.2「用量与诊断」）。
#[tauri::command]
pub fn ai_clear_result_cache(state: tauri::State<'_, crate::state::AppState>) -> AppResult<i64> {
    let conn = lock_db(&state)?;
    let removed = state.ai_result_cache.clear(&conn)?;
    log::info!("ai result cache cleared: removed={}", removed);
    Ok(removed as i64)
}

/// 构建发送前 Preview（AI-03，§10.1）：收集上下文（只调现有领域服务）
/// → Secret 管道 → 预算策略 → Prompt 分层 → 内容 hash。**零网络访问**；
/// 用户确认后把返回的 `request` 交给 `ai_submit_request`（Gateway 仍有
/// 自己的 Secret/预算闸门）。排除项变更 = 用新 `exclusions` 重新调用。
#[tauri::command]
pub async fn ai_build_context_preview(
    state: tauri::State<'_, crate::state::AppState>,
    req: ai::preview::ContextPreviewRequest,
) -> AppResult<ai::preview::AiContextPreview> {
    let db = state.db.clone();
    let runtime = state.runtime.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let conn = db
            .lock()
            .map_err(|e| AppError::Other(format!("DB lock error: {}", e)))?;
        ai::preview::build(&conn, Some(runtime.as_ref()), req)
    })
    .await
    .map_err(|e| AppError::Other(format!("preview task join error: {}", e)))?
}

/// AI-06：构建 Runtime 失败诊断/日志选段 Preview。业务编排在 `ai::diagnose`
/// 中完成，本命令只负责 DB/Runtime 句柄转移到 blocking 线程；零网络访问。
#[tauri::command]
pub async fn ai_runtime_diagnostic_preview(
    state: tauri::State<'_, crate::state::AppState>,
    req: ai::RuntimeDiagnosticRequest,
) -> AppResult<ai::preview::AiContextPreview> {
    let db = state.db.clone();
    let runtime = state.runtime.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let conn = db
            .lock()
            .map_err(|e| AppError::Other(format!("DB lock error: {}", e)))?;
        ai::build_diagnostic_preview(&conn, Some(runtime.as_ref()), req)
    })
    .await
    .map_err(|e| AppError::Other(format!("diagnostic preview task join error: {}", e)))?
}

// ---------------------------------------------------------------------------
// 原型命令（Phase A 兼容保留）：ai_review 移除「前端直接传 Key + 模型硬编码」
// （§4.2 Phase A），改走任务默认模型解析 + 凭证存储。
// ---------------------------------------------------------------------------

/// AI code review result.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewResult {
    pub summary: String,
    pub issues: Vec<ReviewIssue>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewIssue {
    pub severity: String, // "high", "medium", "low"
    pub category: String, // "bug", "security", "optimization"
    pub file: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<i64>,
    pub description: String,
}

/// AI search result entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResult {
    pub repo_path: String,
    pub file_path: String,
    pub snippet: String,
    pub rank: f64,
}

/// Perform an AI code review on the working directory diff.
///
/// Phase A 兼容保留（§4.2）：Provider/模型/凭证全部来自 AI 设置
/// （gitReview 任务默认链解析），不再有前端传 Key 与模型硬编码。
/// 兼容调用内部转发到 AI-03 Preview + AI-02 Gateway；不再保留第二套
/// Diff、Secret 或 HTTP 实现。
#[tauri::command]
pub async fn ai_review(
    repo_path: String,
    diff_selection: Option<ai::GitDiffSelection>,
    state: tauri::State<'_, crate::state::AppState>,
) -> AppResult<ReviewResult> {
    let request = ai::preview::ContextPreviewRequest {
        task_kind: ai::AiTaskKind::GitReview,
        git_scenario: None,
        provider_id: None,
        model_id: None,
        workspace_id: None,
        repo_path: Some(repo_path),
        conflict: None,
        runtime_name: None,
        process_id: None,
        project: None,
        user_instruction: String::new(),
        diff_scope: Some(ai::context::DiffScope::Workdir),
        diff_selection,
        supplementary: vec![],
        exclusions: vec![],
        secret_policy: Default::default(),
        budget_strategy: None,
        stream: false,
        token_estimate_factor: None,
        log_tail_lines: None,
        token_budget: None,
        include_runtime_logs: true,
    };
    let db = state.db.clone();
    let runtime = state.runtime.clone();
    let preview = tauri::async_runtime::spawn_blocking(move || {
        let conn = db
            .lock()
            .map_err(|e| AppError::Other(format!("DB lock error: {}", e)))?;
        ai::preview::build(&conn, Some(runtime.as_ref()), request)
    })
    .await
    .map_err(|e| AppError::Other(format!("review preview task join error: {}", e)))??;

    // Preserve the old empty-diff response without creating a provider call.
    if !preview
        .items
        .iter()
        .any(|item| item.kind == ai::ContextKind::Diff && item.display_name.starts_with("diff ["))
    {
        return Ok(ReviewResult {
            summary: "No changes to review.".to_string(),
            issues: vec![],
        });
    }
    if preview.blocked {
        return Err(AppError::Ai(AiError::SecretDetected {
            kinds: preview.secret.block_kinds.join("、"),
        }));
    }

    let request_id = preview.request.request_id.clone();
    {
        let conn = lock_db(&state)?;
        state.ai_gateway.submit(&conn, preview.request)?;
    }
    state
        .ai_gateway
        .approve(state.ai_credentials.clone(), &request_id)?;
    let snapshot = state
        .ai_gateway
        .wait(&request_id, std::time::Duration::from_secs(130))
        .await
        .ok_or_else(|| {
            AppError::Ai(AiError::ProviderUnavailable {
                message: "AI Review 请求状态不可用".into(),
                transient: false,
            })
        })?;
    if let Some(result) = snapshot.result {
        return Ok(legacy_review_result(result));
    }
    Err(AppError::Ai(AiError::ProviderUnavailable {
        message: snapshot
            .error
            .unwrap_or_else(|| "AI Review 未返回结果".to_string()),
        transient: false,
    }))
}

fn legacy_review_result(result: ai::AiResult) -> ReviewResult {
    match result {
        ai::AiResult::ReviewReport { payload } => serde_json::from_value(payload.clone())
            .unwrap_or_else(|_| ReviewResult {
                summary: payload
                    .get("summary")
                    .and_then(|v| v.as_str())
                    .unwrap_or("AI Review 返回了无法转换的结构化结果")
                    .to_string(),
                issues: vec![],
            }),
        ai::AiResult::Answer { text } | ai::AiResult::GeneratedText { text } => ReviewResult {
            summary: text,
            issues: vec![],
        },
        other => ReviewResult {
            summary: serde_json::to_string_pretty(&other)
                .unwrap_or_else(|_| "AI Review 完成".into()),
            issues: vec![],
        },
    }
}

/// Build the code search index for a repository.
/// Scans all non-binary files and writes their content to the FTS5 index.
#[tauri::command]
pub fn build_code_index(
    repo_path: String,
    state: tauri::State<'_, crate::state::AppState>,
) -> AppResult<()> {
    use rusqlite::params;
    use std::fs;
    use walkdir::WalkDir;

    let repo_path = Path::new(&repo_path);

    // Delete existing index entries for this repo
    {
        let conn = state
            .db
            .lock()
            .map_err(|e| AppError::Other(format!("DB lock error: {}", e)))?;
        conn.execute(
            "DELETE FROM code_index WHERE repo_path = ?1",
            params![repo_path.to_string_lossy().to_string()],
        )?;
    }

    // Scan files
    let skip_dirs = [
        "node_modules",
        "target",
        "dist",
        "build",
        ".git",
        "__pycache__",
        ".next",
        ".nuxt",
        "vendor",
        ".venv",
    ];

    let mut walker = WalkDir::new(repo_path).into_iter();

    let mut batch_count = 0;
    let conn = state
        .db
        .lock()
        .map_err(|e| AppError::Other(format!("DB lock error: {}", e)))?;

    while let Some(Ok(entry)) = walker.next() {
        if entry.file_type().is_dir() {
            let name = entry.file_name().to_string_lossy();
            if skip_dirs.contains(&name.as_ref()) {
                walker.skip_current_dir();
            }
            continue;
        }

        let path = entry.path();
        let relative = match path.strip_prefix(repo_path) {
            Ok(r) => r.to_string_lossy().to_string(),
            Err(_) => continue,
        };

        // Skip binary file extensions
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        let text_exts = [
            "rs",
            "go",
            "py",
            "js",
            "ts",
            "tsx",
            "jsx",
            "vue",
            "java",
            "kt",
            "c",
            "cpp",
            "h",
            "hpp",
            "cs",
            "rb",
            "php",
            "swift",
            "sql",
            "json",
            "yaml",
            "yml",
            "toml",
            "xml",
            "html",
            "css",
            "scss",
            "less",
            "md",
            "txt",
            "sh",
            "bash",
            "zsh",
            "fish",
            "lua",
            "r",
            "scala",
            "dart",
            "gradle",
            "dockerfile",
        ];
        if !text_exts.contains(&ext) && ext != "" {
            continue;
        }

        // Read file content (limit size)
        if let Ok(metadata) = fs::metadata(path) {
            if metadata.len() > 100_000 {
                continue; // Skip large files
            }
        }

        let content = match fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => continue, // Skip binary files
        };

        // Insert into FTS5 index
        let repo_path_str = repo_path.to_string_lossy().to_string();
        conn.execute(
            "INSERT INTO code_index (content, repo_path, file_path) VALUES (?1, ?2, ?3)",
            params![content, repo_path_str, relative],
        )?;

        batch_count += 1;
        if batch_count % 100 == 0 {
            log::debug!("Indexed {} files for {:?}", batch_count, repo_path);
        }
    }

    log::info!(
        "Code index built: {} files for {:?}",
        batch_count,
        repo_path
    );
    Ok(())
}

/// Search the code index for matching files.
#[tauri::command]
pub fn ai_search(
    query: String,
    state: tauri::State<'_, crate::state::AppState>,
) -> AppResult<Vec<SearchResult>> {
    use rusqlite::params;

    let conn = state
        .db
        .lock()
        .map_err(|e| AppError::Other(format!("DB lock error: {}", e)))?;

    // FTS5 MATCH query
    // Sanitize the query for FTS5 (escape special characters)
    let sanitized = query.replace('"', "\"\"").replace('*', "").replace(':', "");

    let fts_query = format!("\"{}\"", sanitized);

    let mut stmt = conn.prepare(
        "SELECT repo_path, file_path, content, rank \
         FROM code_index \
         WHERE code_index MATCH ?1 \
         ORDER BY rank \
         LIMIT 50",
    )?;

    let results = stmt
        .query_map(params![fts_query], |row| {
            let repo_path: String = row.get(0)?;
            let file_path: String = row.get(1)?;
            let content: String = row.get(2)?;
            let rank: f64 = row.get(3)?;

            // Extract a snippet around the first match
            let snippet = if let Some(pos) = content.to_lowercase().find(&query.to_lowercase()) {
                let start = pos.saturating_sub(50);
                let end = (pos + query.len() + 50).min(content.len());
                let snip = &content[start..end];
                format!("...{}...", snip.trim())
            } else {
                content.chars().take(100).collect()
            };

            Ok(SearchResult {
                repo_path,
                file_path,
                snippet,
                rank,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();

    Ok(results)
}

/// Clear the code index for a specific repository.
#[tauri::command]
pub fn clear_code_index(
    repo_path: String,
    state: tauri::State<'_, crate::state::AppState>,
) -> AppResult<()> {
    use rusqlite::params;

    let conn = state
        .db
        .lock()
        .map_err(|e| AppError::Other(format!("DB lock error: {}", e)))?;

    conn.execute(
        "DELETE FROM code_index WHERE repo_path = ?1",
        params![repo_path],
    )?;

    Ok(())
}

/// AI-05: list the typed, read-only tool definitions. The schema is the
/// backend source of truth for the UI and future external adapters.
#[tauri::command]
pub fn ai_list_tools() -> Vec<ToolDefinition> {
    tools::registry().definitions()
}

/// AI-05: execute one bounded, read-only tool call through the registry.
#[tauri::command]
pub async fn ai_execute_tool(
    request: ToolCallRequest,
    state: tauri::State<'_, crate::state::AppState>,
) -> AppResult<ToolInvocation> {
    tools::registry()
        .invoke(request, ai::ToolContext::from_state(state.inner()))
        .await
}
