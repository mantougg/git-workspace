//! 模型能力目录与任务级默认模型解析（设计文档 §6.2 / §6.3）。
//!
//! 模型目录不只保存名称，还保存能力（chat / structuredOutput / toolCalling /
//! vision 等），用于任务选择与请求前校验（§6.3：能力不满足时在请求前报
//! `AiModelCapabilityMismatch`，不等 Provider 返回模糊失败）。
//!
//! 解析顺序（§6.3）：
//! 任务显式选择 > Workspace 任务配置 > 全局任务默认 > 全局聊天默认 > 首个可用模型。

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};

use super::error::AiError;
use super::provider::{get_provider, AiProvider};

/// 模型能力（§6.2）。序列化为 camelCase 字符串，与 TS 字符串联合对齐。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ModelCapability {
    Chat,
    StructuredOutput,
    ToolCalling,
    Vision,
}

impl ModelCapability {
    pub fn as_str(&self) -> &'static str {
        match self {
            ModelCapability::Chat => "chat",
            ModelCapability::StructuredOutput => "structuredOutput",
            ModelCapability::ToolCalling => "toolCalling",
            ModelCapability::Vision => "vision",
        }
    }
}

/// 模型默认参数（§6.2：`temperature` 等）。存 `ai_models.defaults_json`。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiModelDefaults {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
}

/// 模型目录条目（§6.2）。`id` 是 Provider 侧的模型 ID（如 `gpt-4o-mini`），
/// 主键为 `(provider_id, id)`。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiModel {
    pub provider_id: String,
    pub id: String,
    pub display_name: String,
    pub capabilities: Vec<ModelCapability>,
    /// 上下文预算（token）；0 = 未知，由调用方自行截断。
    pub max_context_tokens: i64,
    pub defaults: AiModelDefaults,
    pub enabled: bool,
    pub created_at: String,
    pub updated_at: String,
}

/// 新增/编辑模型的 IPC 入参（以 `(provider_id, id)` upsert）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveAiModelRequest {
    pub provider_id: String,
    pub id: String,
    pub display_name: String,
    pub capabilities: Vec<ModelCapability>,
    pub max_context_tokens: i64,
    #[serde(default)]
    pub defaults: AiModelDefaults,
    pub enabled: bool,
}

/// AI 任务种类（§6.3 的五个 `defaultXxxModel`）。序列化为 camelCase 字符串。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AiTaskKind {
    Chat,
    RuntimeDiagnostic,
    GitReview,
    CommitMessage,
    Conflict,
}

impl AiTaskKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            AiTaskKind::Chat => "chat",
            AiTaskKind::RuntimeDiagnostic => "runtimeDiagnostic",
            AiTaskKind::GitReview => "gitReview",
            AiTaskKind::CommitMessage => "commitMessage",
            AiTaskKind::Conflict => "conflict",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "chat" => Some(AiTaskKind::Chat),
            "runtimeDiagnostic" => Some(AiTaskKind::RuntimeDiagnostic),
            "gitReview" => Some(AiTaskKind::GitReview),
            "commitMessage" => Some(AiTaskKind::CommitMessage),
            "conflict" => Some(AiTaskKind::Conflict),
            _ => None,
        }
    }

    pub const ALL: [AiTaskKind; 5] = [
        AiTaskKind::Chat,
        AiTaskKind::RuntimeDiagnostic,
        AiTaskKind::GitReview,
        AiTaskKind::CommitMessage,
        AiTaskKind::Conflict,
    ];
}

/// 任务所需能力（§6.3）：Review/诊断/冲突解决需要结构化 JSON 输出。
pub fn required_capabilities(task_kind: AiTaskKind) -> &'static [ModelCapability] {
    match task_kind {
        AiTaskKind::Chat | AiTaskKind::CommitMessage => &[ModelCapability::Chat],
        AiTaskKind::RuntimeDiagnostic | AiTaskKind::GitReview | AiTaskKind::Conflict => {
            &[ModelCapability::Chat, ModelCapability::StructuredOutput]
        }
    }
}

/// 任务级默认模型配置行。`workspace_id` 为空 = 全局默认。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiTaskDefault {
    pub task_kind: AiTaskKind,
    pub workspace_id: Option<i64>,
    pub provider_id: String,
    pub model_id: String,
    pub updated_at: String,
}

/// 默认模型解析来源（§6.3 解析链的审计信息）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ModelResolutionSource {
    Explicit,
    WorkspaceTask,
    GlobalTask,
    ChatDefault,
    FirstAvailable,
}

/// 解析结果：模型 + 所属 Provider + 来源。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedModel {
    pub provider: AiProvider,
    pub model: AiModel,
    pub source: ModelResolutionSource,
}

fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339()
}

fn row_to_model(row: &rusqlite::Row) -> rusqlite::Result<AiModel> {
    let capabilities_json: String = row.get("capabilities_json")?;
    let defaults_json: String = row.get("defaults_json")?;
    Ok(AiModel {
        provider_id: row.get("provider_id")?,
        id: row.get("id")?,
        display_name: row.get("display_name")?,
        capabilities: serde_json::from_str(&capabilities_json).unwrap_or_default(),
        max_context_tokens: row.get("max_context_tokens")?,
        defaults: serde_json::from_str(&defaults_json).unwrap_or_default(),
        enabled: row.get::<_, i64>("enabled")? != 0,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

const MODEL_COLS: &str =
    "provider_id, id, display_name, capabilities_json, max_context_tokens, defaults_json, enabled, created_at, updated_at";

pub fn list_models(conn: &Connection, provider_id: Option<&str>) -> AppResult<Vec<AiModel>> {
    let sql = match provider_id {
        Some(_) => format!(
            "SELECT {} FROM ai_models WHERE provider_id = ?1 ORDER BY created_at, id",
            MODEL_COLS
        ),
        None => format!(
            "SELECT {} FROM ai_models ORDER BY provider_id, created_at, id",
            MODEL_COLS
        ),
    };
    let mut stmt = conn.prepare(&sql)?;
    let rows = match provider_id {
        Some(pid) => stmt.query_map(params![pid], row_to_model)?,
        None => stmt.query_map([], row_to_model)?,
    };
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

pub fn get_model(conn: &Connection, provider_id: &str, model_id: &str) -> AppResult<Option<AiModel>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {} FROM ai_models WHERE provider_id = ?1 AND id = ?2",
        MODEL_COLS
    ))?;
    let mut rows = stmt.query_map(params![provider_id, model_id], row_to_model)?;
    match rows.next() {
        Some(row) => Ok(Some(row?)),
        None => Ok(None),
    }
}

/// 新增或更新模型（按 `(provider_id, id)` upsert，保持 created_at）。
pub fn save_model(conn: &Connection, input: &SaveAiModelRequest) -> AppResult<AiModel> {
    if get_provider(conn, &input.provider_id)?.is_none() {
        return Err(AppError::Ai(AiError::ModelNotFound {
            provider_id: input.provider_id.clone(),
            model_id: input.id.clone(),
        }));
    }
    let model_id = input.id.trim();
    if model_id.is_empty() {
        return Err(AppError::Ai(AiError::NotConfigured {
            message: "模型 ID 不能为空".to_string(),
        }));
    }
    if input.max_context_tokens < 0 {
        return Err(AppError::Ai(AiError::NotConfigured {
            message: "maxContextTokens 不能为负数".to_string(),
        }));
    }
    if let Some(t) = input.defaults.temperature {
        if !(0.0..=2.0).contains(&t) {
            return Err(AppError::Ai(AiError::NotConfigured {
                message: "temperature 必须在 0.0 ~ 2.0 之间".to_string(),
            }));
        }
    }

    let now = now_rfc3339();
    let capabilities_json = serde_json::to_string(&input.capabilities)?;
    let defaults_json = serde_json::to_string(&input.defaults)?;
    conn.execute(
        "INSERT INTO ai_models
         (provider_id, id, display_name, capabilities_json, max_context_tokens, defaults_json, enabled, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)
         ON CONFLICT(provider_id, id) DO UPDATE SET
            display_name = excluded.display_name,
            capabilities_json = excluded.capabilities_json,
            max_context_tokens = excluded.max_context_tokens,
            defaults_json = excluded.defaults_json,
            enabled = excluded.enabled,
            updated_at = excluded.updated_at",
        params![
            input.provider_id,
            model_id,
            input.display_name.trim(),
            capabilities_json,
            input.max_context_tokens,
            defaults_json,
            input.enabled as i64,
            now
        ],
    )?;
    Ok(get_model(conn, &input.provider_id, model_id)?.expect("just upserted"))
}

/// 删除模型；引用它的任务默认值一并清理（调用方负责在删除前确认）。
pub fn delete_model(conn: &Connection, provider_id: &str, model_id: &str) -> AppResult<()> {
    conn.execute(
        "DELETE FROM ai_task_defaults WHERE provider_id = ?1 AND model_id = ?2",
        params![provider_id, model_id],
    )?;
    conn.execute(
        "DELETE FROM ai_models WHERE provider_id = ?1 AND id = ?2",
        params![provider_id, model_id],
    )?;
    Ok(())
}

fn row_to_task_default(row: &rusqlite::Row) -> rusqlite::Result<AiTaskDefault> {
    let kind_str: String = row.get("task_kind")?;
    Ok(AiTaskDefault {
        task_kind: AiTaskKind::parse(&kind_str).unwrap_or(AiTaskKind::Chat),
        workspace_id: row.get("workspace_id")?,
        provider_id: row.get("provider_id")?,
        model_id: row.get("model_id")?,
        updated_at: row.get("updated_at")?,
    })
}

/// 列出任务默认值；`workspace_id = None` 返回全部（含全局与各 Workspace 覆盖）。
pub fn list_task_defaults(
    conn: &Connection,
    workspace_id: Option<i64>,
) -> AppResult<Vec<AiTaskDefault>> {
    let mut stmt = conn.prepare(
        "SELECT task_kind, workspace_id, provider_id, model_id, updated_at
         FROM ai_task_defaults ORDER BY task_kind, workspace_id",
    )?;
    let rows = stmt.query_map([], row_to_task_default)?;
    let all = rows.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(match workspace_id {
        Some(ws) => all
            .into_iter()
            .filter(|d| d.workspace_id.is_none() || d.workspace_id == Some(ws))
            .collect(),
        None => all,
    })
}

fn get_task_default(
    conn: &Connection,
    task_kind: AiTaskKind,
    workspace_id: Option<i64>,
) -> AppResult<Option<AiTaskDefault>> {
    let mut stmt = conn.prepare(
        "SELECT task_kind, workspace_id, provider_id, model_id, updated_at
         FROM ai_task_defaults
         WHERE task_kind = ?1 AND (workspace_id IS ?2 OR workspace_id = ?2)",
    )?;
    let mut rows = stmt.query_map(params![task_kind.as_str(), workspace_id], row_to_task_default)?;
    match rows.next() {
        Some(row) => Ok(Some(row?)),
        None => Ok(None),
    }
}

/// 设置任务默认模型（`workspace_id` 为空 = 全局默认）。设置时即做能力校验：
/// 不具备任务所需能力的模型不能被设为该任务默认（§6.3 前置校验）。
pub fn set_task_default(
    conn: &Connection,
    task_kind: AiTaskKind,
    workspace_id: Option<i64>,
    provider_id: &str,
    model_id: &str,
) -> AppResult<AiTaskDefault> {
    let model = get_model(conn, provider_id, model_id)?.ok_or_else(|| {
        AppError::Ai(AiError::ModelNotFound {
            provider_id: provider_id.to_string(),
            model_id: model_id.to_string(),
        })
    })?;
    ensure_task_capability(&model, task_kind)?;

    let now = now_rfc3339();
    // workspace_id 可空导致无法直接用 ON CONFLICT upsert（NULL ≠ NULL），
    // 唯一性由 COALESCE 表达式索引保证，这里先删后插。
    conn.execute(
        "DELETE FROM ai_task_defaults
         WHERE task_kind = ?1 AND (workspace_id IS ?2 OR workspace_id = ?2)",
        params![task_kind.as_str(), workspace_id],
    )?;
    conn.execute(
        "INSERT INTO ai_task_defaults (task_kind, workspace_id, provider_id, model_id, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![task_kind.as_str(), workspace_id, provider_id, model_id, now],
    )?;
    Ok(get_task_default(conn, task_kind, workspace_id)?.expect("just inserted"))
}

/// 清除任务默认值（Workspace 覆盖清除后回落到全局链）。
pub fn clear_task_default(
    conn: &Connection,
    task_kind: AiTaskKind,
    workspace_id: Option<i64>,
) -> AppResult<()> {
    conn.execute(
        "DELETE FROM ai_task_defaults
         WHERE task_kind = ?1 AND (workspace_id IS ?2 OR workspace_id = ?2)",
        params![task_kind.as_str(), workspace_id],
    )?;
    Ok(())
}

/// 模型能力校验（§6.3）：不满足任务所需能力时报 `AiModelCapabilityMismatch`。
pub fn ensure_task_capability(model: &AiModel, task_kind: AiTaskKind) -> AppResult<()> {
    for cap in required_capabilities(task_kind) {
        if !model.capabilities.contains(cap) {
            return Err(AppError::Ai(AiError::ModelCapabilityMismatch {
                provider_id: model.provider_id.clone(),
                model_id: model.id.clone(),
                capability: cap.as_str().to_string(),
            }));
        }
    }
    Ok(())
}

/// 把一个「默认配置行」解析为可用模型：模型存在且启用、Provider 存在且启用
/// 才可用，否则返回 None（调用方继续沿解析链下落）。
fn resolve_candidate(
    conn: &Connection,
    provider_id: &str,
    model_id: &str,
    source: ModelResolutionSource,
) -> AppResult<Option<ResolvedModel>> {
    let Some(model) = get_model(conn, provider_id, model_id)? else {
        return Ok(None);
    };
    if !model.enabled {
        return Ok(None);
    }
    let Some(provider) = get_provider(conn, provider_id)? else {
        return Ok(None);
    };
    if !provider.enabled {
        return Ok(None);
    }
    Ok(Some(ResolvedModel {
        provider,
        model,
        source,
    }))
}

/// 任务级默认模型解析（§6.3）：
/// 任务显式选择 > Workspace 任务配置 > 全局任务默认 > 全局聊天默认 > 首个可用模型。
///
/// - `explicit`：任务显式选择的 `(provider_id, model_id)`，必须可用，否则报
///   `AiModelNotFound`（显式选择不沿链下落——用户明确指定了模型）。
/// - 配置行指向已删除/禁用的模型时打警告并沿链下落（配置漂移自愈）。
/// - 全部落空时报 `AiNotConfigured`。
pub fn resolve_model(
    conn: &Connection,
    task_kind: AiTaskKind,
    workspace_id: Option<i64>,
    explicit: Option<(&str, &str)>,
) -> AppResult<ResolvedModel> {
    if let Some((pid, mid)) = explicit {
        // 显式选择不沿链下落——用户明确指定了模型，报错必须精确。
        let Some(model) = get_model(conn, pid, mid)? else {
            return Err(AppError::Ai(AiError::ModelNotFound {
                provider_id: pid.to_string(),
                model_id: mid.to_string(),
            }));
        };
        let provider = get_provider(conn, pid)?.ok_or_else(|| {
            AppError::Ai(AiError::ModelNotFound {
                provider_id: pid.to_string(),
                model_id: mid.to_string(),
            })
        })?;
        if !model.enabled || !provider.enabled {
            return Err(AppError::Ai(AiError::NotConfigured {
                message: format!("模型 {} 或其 Provider 已禁用", mid),
            }));
        }
        return Ok(ResolvedModel {
            provider,
            model,
            source: ModelResolutionSource::Explicit,
        });
    }

    let chain: [(Option<i64>, AiTaskKind, ModelResolutionSource); 4] = [
        (workspace_id, task_kind, ModelResolutionSource::WorkspaceTask),
        (None, task_kind, ModelResolutionSource::GlobalTask),
        (None, AiTaskKind::Chat, ModelResolutionSource::ChatDefault),
        // FirstAvailable 不走配置表，占位项（下方单独处理）。
        (None, AiTaskKind::Chat, ModelResolutionSource::FirstAvailable),
    ];
    for (ws, kind, source) in chain {
        if source == ModelResolutionSource::FirstAvailable {
            break;
        }
        if ws.is_none() && source == ModelResolutionSource::WorkspaceTask {
            continue;
        }
        if let Some(default) = get_task_default(conn, kind, ws)? {
            match resolve_candidate(conn, &default.provider_id, &default.model_id, source)? {
                Some(resolved) => return Ok(resolved),
                None => {
                    log::warn!(
                        "task default {:?} ({:?}) points at missing/disabled model {}/{}; falling through",
                        kind, source, default.provider_id, default.model_id
                    );
                }
            }
        }
    }

    // 首个可用模型：启用 Provider 下的首个启用模型（稳定顺序）。
    let providers = super::provider::list_providers(conn)?;
    for provider in providers.into_iter().filter(|p| p.enabled) {
        let models = list_models(conn, Some(&provider.id))?;
        if let Some(model) = models.into_iter().find(|m| m.enabled) {
            return Ok(ResolvedModel {
                provider,
                model,
                source: ModelResolutionSource::FirstAvailable,
            });
        }
    }

    Err(AppError::Ai(AiError::NotConfigured {
        message: "没有可用的 AI 模型：请在 AI 设置中添加 Provider 与模型".to_string(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::provider::{
        save_provider, ApiType, NetworkPolicy, SaveAiProviderRequest,
    };

    fn open_memory() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        crate::db::init_db(&mut conn).unwrap();
        conn
    }

    fn add_workspace(conn: &Connection) -> i64 {
        add_workspace_with_path(conn, "D:/w")
    }

    fn add_workspace_with_path(conn: &Connection, path: &str) -> i64 {
        conn.execute(
            "INSERT INTO workspaces (name, path, created_at, updated_at) VALUES ('w', ?1, 't', 't')",
            [path],
        )
        .unwrap();
        conn.last_insert_rowid()
    }

    fn add_provider(conn: &Connection, name: &str) -> AiProvider {
        save_provider(
            conn,
            &SaveAiProviderRequest {
                id: None,
                name: name.into(),
                api_type: ApiType::OpenaiChatCompletions,
                base_url: "https://api.example.com/v1".into(),
                enabled: true,
                network_policy: NetworkPolicy::OnlineOnly,
            },
        )
        .unwrap()
    }

    fn model_input(provider_id: &str, id: &str, caps: Vec<ModelCapability>) -> SaveAiModelRequest {
        SaveAiModelRequest {
            provider_id: provider_id.into(),
            id: id.into(),
            display_name: id.into(),
            capabilities: caps,
            max_context_tokens: 128000,
            defaults: AiModelDefaults {
                temperature: Some(0.2),
            },
            enabled: true,
        }
    }

    fn full_caps() -> Vec<ModelCapability> {
        vec![
            ModelCapability::Chat,
            ModelCapability::StructuredOutput,
            ModelCapability::ToolCalling,
        ]
    }

    #[test]
    fn model_crud_and_capability_check() {
        let conn = open_memory();
        let p = add_provider(&conn, "p");
        let m = save_model(&conn, &model_input(&p.id, "gpt-x", full_caps())).unwrap();
        assert_eq!(m.id, "gpt-x");
        assert_eq!(m.defaults.temperature, Some(0.2));
        assert_eq!(m.capabilities.len(), 3);

        // upsert 更新能力
        let m2 = save_model(
            &conn,
            &model_input(&p.id, "gpt-x", vec![ModelCapability::Chat]),
        )
        .unwrap();
        assert_eq!(m2.capabilities, vec![ModelCapability::Chat]);
        assert_eq!(m2.created_at, m.created_at, "upsert 保持 created_at");

        // structuredOutput 任务要求校验
        assert!(ensure_task_capability(&m2, AiTaskKind::Chat).is_ok());
        let err = ensure_task_capability(&m2, AiTaskKind::GitReview).unwrap_err();
        assert_eq!(err.code(), "AiModelCapabilityMismatch");

        delete_model(&conn, &p.id, "gpt-x").unwrap();
        assert!(list_models(&conn, None).unwrap().is_empty());
    }

    #[test]
    fn save_model_rejects_invalid_input() {
        let conn = open_memory();
        let p = add_provider(&conn, "p");

        let mut bad = model_input(&p.id, "  ", full_caps());
        let err = save_model(&conn, &bad).unwrap_err();
        assert_eq!(err.code(), "AiNotConfigured");

        bad = model_input(&p.id, "m", full_caps());
        bad.max_context_tokens = -1;
        assert!(save_model(&conn, &bad).is_err());

        bad = model_input(&p.id, "m", full_caps());
        bad.defaults.temperature = Some(3.0);
        assert!(save_model(&conn, &bad).is_err());

        // Provider 不存在
        bad = model_input("no-such-provider", "m", full_caps());
        assert_eq!(
            save_model(&conn, &bad).unwrap_err().code(),
            "AiModelNotFound"
        );
    }

    #[test]
    fn resolution_follows_design_chain() {
        let conn = open_memory();
        let ws = add_workspace(&conn);
        let p = add_provider(&conn, "p");
        save_model(&conn, &model_input(&p.id, "chat-model", vec![ModelCapability::Chat])).unwrap();
        save_model(&conn, &model_input(&p.id, "review-model", full_caps())).unwrap();
        save_model(&conn, &model_input(&p.id, "ws-model", full_caps())).unwrap();

        // 无配置 → 首个可用模型
        let r = resolve_model(&conn, AiTaskKind::GitReview, Some(ws), None).unwrap();
        assert_eq!(r.source, ModelResolutionSource::FirstAvailable);
        assert_eq!(r.model.id, "chat-model");

        // 全局聊天默认
        set_task_default(&conn, AiTaskKind::Chat, None, &p.id, "chat-model").unwrap();
        let r = resolve_model(&conn, AiTaskKind::GitReview, Some(ws), None).unwrap();
        assert_eq!(r.source, ModelResolutionSource::ChatDefault);

        // 全局任务默认 > 全局聊天默认
        set_task_default(&conn, AiTaskKind::GitReview, None, &p.id, "review-model").unwrap();
        let r = resolve_model(&conn, AiTaskKind::GitReview, Some(ws), None).unwrap();
        assert_eq!(r.source, ModelResolutionSource::GlobalTask);
        assert_eq!(r.model.id, "review-model");

        // Workspace 任务配置 > 全局任务默认
        set_task_default(&conn, AiTaskKind::GitReview, Some(ws), &p.id, "ws-model").unwrap();
        let r = resolve_model(&conn, AiTaskKind::GitReview, Some(ws), None).unwrap();
        assert_eq!(r.source, ModelResolutionSource::WorkspaceTask);
        assert_eq!(r.model.id, "ws-model");

        // 其他 Workspace 不受覆盖影响
        let other_ws = add_workspace_with_path(&conn, "D:/w2");
        let r = resolve_model(&conn, AiTaskKind::GitReview, Some(other_ws), None).unwrap();
        assert_eq!(r.source, ModelResolutionSource::GlobalTask);

        // 显式选择 > Workspace 任务配置
        let r = resolve_model(
            &conn,
            AiTaskKind::GitReview,
            Some(ws),
            Some((p.id.as_str(), "review-model")),
        )
        .unwrap();
        assert_eq!(r.source, ModelResolutionSource::Explicit);

        // 显式选择不存在的模型 → AiModelNotFound（不下落）
        let err = resolve_model(&conn, AiTaskKind::GitReview, Some(ws), Some((p.id.as_str(), "gone")))
            .unwrap_err();
        assert_eq!(err.code(), "AiModelNotFound");

        // 清除 Workspace 覆盖 → 回落全局任务默认
        clear_task_default(&conn, AiTaskKind::GitReview, Some(ws)).unwrap();
        let r = resolve_model(&conn, AiTaskKind::GitReview, Some(ws), None).unwrap();
        assert_eq!(r.source, ModelResolutionSource::GlobalTask);
    }

    #[test]
    fn resolution_skips_disabled_and_falls_through() {
        let conn = open_memory();
        let ws = add_workspace(&conn);
        let p = add_provider(&conn, "p");
        save_model(&conn, &model_input(&p.id, "a", full_caps())).unwrap();
        save_model(&conn, &model_input(&p.id, "b", full_caps())).unwrap();

        // 全局默认指向 a；禁用 a 后沿链下落到首个可用（b）
        set_task_default(&conn, AiTaskKind::GitReview, None, &p.id, "a").unwrap();
        let mut disabled_a = model_input(&p.id, "a", full_caps());
        disabled_a.enabled = false;
        save_model(&conn, &disabled_a).unwrap();

        let r = resolve_model(&conn, AiTaskKind::GitReview, Some(ws), None).unwrap();
        assert_eq!(r.source, ModelResolutionSource::FirstAvailable);
        assert_eq!(r.model.id, "b");

        // Provider 禁用后：默认链与首个可用都落空 → AiNotConfigured
        let mut off = crate::ai::provider::SaveAiProviderRequest {
            id: Some(p.id.clone()),
            name: "p".into(),
            api_type: ApiType::OpenaiChatCompletions,
            base_url: "https://api.example.com/v1".into(),
            enabled: false,
            network_policy: NetworkPolicy::OnlineOnly,
        };
        off.enabled = false;
        crate::ai::provider::save_provider(&conn, &off).unwrap();
        let err = resolve_model(&conn, AiTaskKind::GitReview, Some(ws), None).unwrap_err();
        assert_eq!(err.code(), "AiNotConfigured");
    }

    #[test]
    fn set_task_default_validates_capability() {
        let conn = open_memory();
        let p = add_provider(&conn, "p");
        save_model(&conn, &model_input(&p.id, "chat-only", vec![ModelCapability::Chat])).unwrap();

        // chat-only 模型不能设为 GitReview（需要 structuredOutput）默认
        let err = set_task_default(&conn, AiTaskKind::GitReview, None, &p.id, "chat-only")
            .unwrap_err();
        assert_eq!(err.code(), "AiModelCapabilityMismatch");

        // 但可以作为 chat 默认
        assert!(set_task_default(&conn, AiTaskKind::Chat, None, &p.id, "chat-only").is_ok());
    }

    #[test]
    fn task_defaults_upsert_keeps_single_row_per_scope() {
        let conn = open_memory();
        let ws = add_workspace(&conn);
        let p = add_provider(&conn, "p");
        save_model(&conn, &model_input(&p.id, "a", vec![ModelCapability::Chat])).unwrap();
        save_model(&conn, &model_input(&p.id, "b", vec![ModelCapability::Chat])).unwrap();

        set_task_default(&conn, AiTaskKind::Chat, None, &p.id, "a").unwrap();
        set_task_default(&conn, AiTaskKind::Chat, None, &p.id, "b").unwrap();
        let globals: Vec<_> = list_task_defaults(&conn, None)
            .unwrap()
            .into_iter()
            .filter(|d| d.workspace_id.is_none())
            .collect();
        assert_eq!(globals.len(), 1, "全局每任务仅一行");
        assert_eq!(globals[0].model_id, "b");

        set_task_default(&conn, AiTaskKind::Chat, Some(ws), &p.id, "a").unwrap();
        set_task_default(&conn, AiTaskKind::Chat, Some(ws), &p.id, "b").unwrap();
        let all = list_task_defaults(&conn, None).unwrap();
        assert_eq!(all.len(), 2, "全局一行 + Workspace 覆盖一行");
    }

    #[test]
    fn capability_and_task_kind_serde_names_match_design() {
        assert_eq!(
            serde_json::to_value(ModelCapability::StructuredOutput).unwrap(),
            "structuredOutput"
        );
        assert_eq!(
            serde_json::to_value(ModelCapability::ToolCalling).unwrap(),
            "toolCalling"
        );
        assert_eq!(
            serde_json::to_value(AiTaskKind::RuntimeDiagnostic).unwrap(),
            "runtimeDiagnostic"
        );
        assert_eq!(
            serde_json::to_value(AiTaskKind::CommitMessage).unwrap(),
            "commitMessage"
        );
        assert_eq!(
            serde_json::to_value(ModelResolutionSource::WorkspaceTask).unwrap(),
            "workspaceTask"
        );
    }
}
