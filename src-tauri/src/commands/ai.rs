//! AI IPC 命令薄适配层（设计文档 §12.1）。
//!
//! 业务逻辑在 `crate::ai::{provider, model, credentials}`，本层只做参数校验、
//! DB 锁管理与凭证实况填充。Key 永远不进日志/错误/返回值（全局约束 §4）。

use std::path::Path;

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::ai;
use crate::ai::error::AiError;
use crate::core::diff;
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

fn fill_credential_status(
    credentials: &ai::CredentialManager,
    providers: &mut [ai::AiProvider],
) {
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
    let default = ai::model::set_task_default(
        &conn,
        task_kind,
        workspace_id,
        &provider_id,
        &model_id,
    )?;
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
    pub severity: String,  // "high", "medium", "low"
    pub category: String,  // "bug", "security", "optimization"
    pub file: String,
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

/// 查询仓库所属 Workspace（任务默认模型的 Workspace 覆盖解析用）。
/// 路径比较按平台规范归一化分隔符（DB 统一存正斜杠）。
fn workspace_id_for_repo(conn: &Connection, repo_path: &str) -> AppResult<Option<i64>> {
    let normalized = repo_path.replace('\\', "/");
    let mut stmt = conn.prepare("SELECT workspace_id FROM repositories WHERE path = ?1")?;
    let mut rows = stmt.query_map(rusqlite::params![normalized], |row| row.get(0))?;
    match rows.next() {
        Some(row) => Ok(Some(row?)),
        None => Ok(None),
    }
}

/// Perform an AI code review on the working directory diff.
///
/// Phase A 兼容保留（§4.2）：Provider/模型/凭证全部来自 AI 设置
/// （gitReview 任务默认链解析），不再有前端传 Key 与模型硬编码。
/// 发送前 Secret 阻断扫描保留（T-08 复用；Preview 流程由 AI-03 落地）。
#[tauri::command]
pub async fn ai_review(
    repo_path: String,
    state: tauri::State<'_, crate::state::AppState>,
) -> AppResult<ReviewResult> {
    // Get the working directory diff
    let file_diffs = diff::get_workdir_diff(Path::new(&repo_path))?;

    if file_diffs.is_empty() {
        return Ok(ReviewResult {
            summary: "No changes to review.".to_string(),
            issues: vec![],
        });
    }

    // Build the diff text for the AI
    let mut diff_text = String::new();
    for file in &file_diffs {
        diff_text.push_str(&format!("--- {} ({})\n", file.new_path, file.status));
        for hunk in &file.hunks {
            for line in &hunk.lines {
                let prefix = match line.line_type.as_str() {
                    "add" => "+",
                    "delete" => "-",
                    _ => " ",
                };
                diff_text.push_str(&format!("{}{}\n", prefix, line.content));
            }
        }
        diff_text.push('\n');
    }

    // Limit diff text size to avoid token limits
    if diff_text.len() > 10000 {
        diff_text = diff_text.chars().take(10000).collect();
        diff_text.push_str("\n... (truncated)\n");
    }

    // Refuse to send if the diff contains secrets (AWS keys, JWT, private keys, ...).
    let findings = crate::core::secret::scan_secrets(&diff_text);
    if !findings.is_empty() {
        let mut labels: Vec<&str> = findings.iter().map(|f| f.kind.label()).collect();
        labels.sort_unstable();
        labels.dedup();
        return Err(AppError::Ai(AiError::SecretDetected {
            kinds: labels.join("、"),
        }));
    }

    // 任务默认模型解析（§6.3：gitReview 链）+ 能力校验（§6.3 前置）。
    let (resolved, api_key) = {
        let conn = lock_db(&state)?;
        let workspace_id = workspace_id_for_repo(&conn, &repo_path)?;
        let resolved = ai::resolve_model(&conn, ai::AiTaskKind::GitReview, workspace_id, None)?;
        ai::ensure_task_capability(&resolved.model, ai::AiTaskKind::GitReview)?;
        let key = resolved
            .provider
            .credential_ref
            .as_deref()
            .and_then(|cref| state.ai_credentials.get(cref));
        (resolved, key)
    };

    if resolved.provider.network_policy == ai::NetworkPolicy::OnlineOnly && api_key.is_none() {
        return Err(AppError::Ai(AiError::CredentialUnavailable {
            message: format!(
                "Provider「{}」未配置 API Key：请在 AI 设置-凭证中录入",
                resolved.provider.name
            ),
        }));
    }

    // Construct the prompt
    let prompt = format!(
        "Review the following git diff. Identify bug risks, security issues, \
        and optimization suggestions. Return the result as JSON with fields: \
        \"summary\" (string), \"issues\" (array of objects with \"severity\" \
        (\"high\"/\"medium\"/\"low\"), \"category\" (\"bug\"/\"security\"/\"optimization\"), \
        \"file\" (string), \"description\" (string)).\n\nDiff:\n{}",
        diff_text
    );

    let started = std::time::Instant::now();
    let content = call_chat_completion(&resolved, api_key.as_deref(), &prompt).await.map_err(
        |e| {
            log::warn!(
                "ai review failed: task=gitReview provider={} model={} code={}",
                resolved.provider.id,
                resolved.model.id,
                e.code()
            );
            e
        },
    )?;
    log::info!(
        "ai review done: task=gitReview provider={} model={} latency_ms={}",
        resolved.provider.id,
        resolved.model.id,
        started.elapsed().as_millis()
    );

    // Parse the AI response as our ReviewResult（非法响应降级为纯文本摘要，
    // §18.1 structured output 解析与降级）。
    let result: ReviewResult = serde_json::from_str(&content).unwrap_or(ReviewResult {
        summary: content.to_string(),
        issues: vec![],
    });

    Ok(result)
}

/// 调用 Provider 的 chat completion，返回文本内容。
/// URL 结构化拼接；错误归一化，不回显响应正文（可能含敏感内容）。
///
/// 原型兼容路径（Phase A 兼容保留）：仅支持 openaiChatCompletions 协议；
/// 其余协议返回可行动错误。统一 Gateway 链路由 AI-02 落地，本函数的直连
/// HTTP 调用随 AI-03 的 Preview 流程一并下线（§2 统一调用链）。
async fn call_chat_completion(
    resolved: &ai::ResolvedModel,
    api_key: Option<&str>,
    prompt: &str,
) -> Result<String, AppError> {
    if resolved.provider.api_type != ai::provider::ApiType::OpenaiChatCompletions {
        return Err(AppError::Ai(AiError::NotConfigured {
            message: format!(
                "原型命令暂不支持协议 {}：请为该任务配置 openaiChatCompletions 协议的 Provider",
                resolved.provider.api_type.as_str()
            ),
        }));
    }

    let base = reqwest::Url::parse(&resolved.provider.base_url).map_err(|_| {
        AppError::Ai(AiError::NotConfigured {
            message: format!("Provider baseUrl 不是合法 URL: {}", resolved.provider.base_url),
        })
    })?;
    let base = if base.path().ends_with('/') {
        base
    } else {
        let mut u = base;
        u.set_path(&format!("{}/", u.path()));
        u
    };

    let url = base.join("chat/completions").map_err(|e| {
        AppError::Ai(AiError::NotConfigured {
            message: format!("baseUrl 无法拼接端点: {}", e),
        })
    })?;

    let temperature = resolved.model.defaults.temperature.unwrap_or(0.3);
    let body = serde_json::json!({
        "model": resolved.model.id,
        "messages": [
            {"role": "system", "content": "You are a code reviewer. Respond only with JSON."},
            {"role": "user", "content": prompt}
        ],
        "temperature": temperature
    });

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(|e| {
            AppError::Ai(AiError::ProviderUnavailable {
                message: format!("HTTP 客户端初始化失败: {}", e),
                transient: false,
            })
        })?;

    let mut req = client.post(url).json(&body);
    if let Some(key) = api_key {
        req = req.header("Authorization", format!("Bearer {}", key));
    }

    let response = req.send().await.map_err(|e| {
        AppError::Ai(AiError::ProviderUnavailable {
            message: if e.is_timeout() {
                "请求超时".to_string()
            } else if e.is_connect() {
                "无法建立连接（检查网络与 baseUrl）".to_string()
            } else {
                "网络错误".to_string()
            },
            transient: e.is_connect(),
        })
    })?;

    let status = response.status();
    if status.as_u16() == 401 || status.as_u16() == 403 {
        return Err(AppError::Ai(AiError::AuthenticationFailed {
            message: "Provider 认证失败（401/403）：请在 AI 设置-凭证中检查或替换 API Key"
                .to_string(),
        }));
    }
    if !status.is_success() {
        return Err(AppError::Ai(AiError::ProviderUnavailable {
            message: format!("Provider 返回 HTTP {}", status.as_u16()),
            transient: status.is_server_error(),
        }));
    }

    let response_json: serde_json::Value = response.json().await.map_err(|_| {
        AppError::Ai(AiError::ProviderUnavailable {
            message: "Provider 响应不是合法 JSON".to_string(),
            transient: false,
        })
    })?;

    let content = response_json
        .get("choices")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_str())
        .unwrap_or("{}")
        .to_string();
    Ok(content)
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

    let mut walker = WalkDir::new(repo_path)
        .into_iter();

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
            "rs", "go", "py", "js", "ts", "tsx", "jsx", "vue", "java",
            "kt", "c", "cpp", "h", "hpp", "cs", "rb", "php", "swift",
            "sql", "json", "yaml", "yml", "toml", "xml", "html", "css",
            "scss", "less", "md", "txt", "sh", "bash", "zsh", "fish",
            "lua", "r", "scala", "dart", "gradle", "dockerfile",
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

    log::info!("Code index built: {} files for {:?}", batch_count, repo_path);
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
    let sanitized = query
        .replace('"', "\"\"")
        .replace('*', "")
        .replace(':', "");

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
