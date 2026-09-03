//! Provider 配置模型与连接测试（设计文档 §6.1 / §12.2）。
//!
//! Provider 是 API 服务来源（不等于某个模型）。配置元数据存 SQLite
//! `ai_providers` 表；API Key 只进 OS Credential Store（见 `credentials.rs`），
//! 表中仅存 `credential_ref` 引用。

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};

use super::error::AiError;

/// Provider 接口协议类型（§6.1 / §21 决策 9）。只区分协议、不区分厂商；
/// 同一协议的所有自定义 Endpoint（OpenAI 官方、火山 Ark、DeepSeek、Ollama、
/// vLLM、企业网关等）共用同一个 Adapter（§7.2）。
///
/// baseUrl 约定包含版本段（如 `https://api.openai.com/v1`、
/// `https://api.anthropic.com/v1`），Adapter 在其后拼接协议端点
/// （`chat/completions` / `responses` / `messages`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ApiType {
    OpenaiChatCompletions,
    OpenaiResponses,
    AnthropicMessages,
}

impl ApiType {
    pub fn as_str(&self) -> &'static str {
        match self {
            ApiType::OpenaiChatCompletions => "openaiChatCompletions",
            ApiType::OpenaiResponses => "openaiResponses",
            ApiType::AnthropicMessages => "anthropicMessages",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "openaiChatCompletions" => Some(ApiType::OpenaiChatCompletions),
            "openaiResponses" => Some(ApiType::OpenaiResponses),
            "anthropicMessages" => Some(ApiType::AnthropicMessages),
            _ => None,
        }
    }
}

/// 网络策略（§6.1）：`localOnly`（如本机 Ollama）不需要凭证、不视为离线错误。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NetworkPolicy {
    OnlineOnly,
    LocalOnly,
}

impl NetworkPolicy {
    pub fn as_str(&self) -> &'static str {
        match self {
            NetworkPolicy::OnlineOnly => "onlineOnly",
            NetworkPolicy::LocalOnly => "localOnly",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "onlineOnly" => Some(NetworkPolicy::OnlineOnly),
            "localOnly" => Some(NetworkPolicy::LocalOnly),
            _ => None,
        }
    }
}

/// Provider 配置（§6.1）。`credential_ref` 是 OS Credential Store 的稳定引用
/// （`ai-provider:{id}`），永远不包含 Key 本身；`has_credential` 由 IPC 层按
/// 凭证存储实况填充。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiProvider {
    /// 本地稳定 ID（UUID），不使用 API Key 作为标识。
    pub id: String,
    pub name: String,
    /// 接口协议类型（§6.1 / §21 决策 9），决定使用哪个 Provider Adapter。
    pub api_type: ApiType,
    /// API 基础地址，不包含 Secret。
    pub base_url: String,
    pub credential_ref: Option<String>,
    /// 凭证实况（OS 存储或会话内存中是否存在 Key），非数据库字段。
    #[serde(default)]
    pub has_credential: bool,
    /// 凭证是否仅存在于本次会话内存（不落盘）。
    #[serde(default)]
    pub session_only_credential: bool,
    pub enabled: bool,
    pub network_policy: NetworkPolicy,
    pub created_at: String,
    pub updated_at: String,
}

/// 新增/编辑 Provider 的 IPC 入参（`id` 为空 = 新建）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveAiProviderRequest {
    pub id: Option<String>,
    pub name: String,
    pub api_type: ApiType,
    pub base_url: String,
    pub enabled: bool,
    pub network_policy: NetworkPolicy,
}

/// `ai_test_provider` 结果（§12.2）：只返回成功/失败原因/模型清单（模型 ID），
/// 不回显响应中的任何其他内容。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiProviderTestResult {
    pub success: bool,
    /// 用户可读结果说明（失败原因为可行动提示，不含敏感内容）。
    pub message: String,
    /// 发现的模型 ID 列表（可能为空，按字典序截断返回）。
    pub models: Vec<String>,
    pub latency_ms: i64,
}

fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339()
}

/// Provider 的凭证引用（稳定本地 ID，与 Key 本身无关）。
pub fn credential_ref_for(provider_id: &str) -> String {
    format!("ai-provider:{}", provider_id)
}

fn row_to_provider(row: &rusqlite::Row) -> rusqlite::Result<AiProvider> {
    let api_type_str: String = row.get("api_type")?;
    let policy_str: String = row.get("network_policy")?;
    Ok(AiProvider {
        id: row.get("id")?,
        name: row.get("name")?,
        api_type: ApiType::parse(&api_type_str).unwrap_or(ApiType::OpenaiChatCompletions),
        base_url: row.get("base_url")?,
        credential_ref: row.get("credential_ref")?,
        has_credential: false,
        session_only_credential: false,
        enabled: row.get::<_, i64>("enabled")? != 0,
        network_policy: NetworkPolicy::parse(&policy_str).unwrap_or(NetworkPolicy::OnlineOnly),
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

const PROVIDER_COLS: &str =
    "id, name, api_type, base_url, credential_ref, enabled, network_policy, created_at, updated_at";

pub fn list_providers(conn: &Connection) -> AppResult<Vec<AiProvider>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {} FROM ai_providers ORDER BY created_at, id",
        PROVIDER_COLS
    ))?;
    let rows = stmt.query_map([], row_to_provider)?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

pub fn get_provider(conn: &Connection, id: &str) -> AppResult<Option<AiProvider>> {
    let mut stmt = conn.prepare(&format!("SELECT {} FROM ai_providers WHERE id = ?1", PROVIDER_COLS))?;
    let mut rows = stmt.query_map(params![id], row_to_provider)?;
    match rows.next() {
        Some(row) => Ok(Some(row?)),
        None => Ok(None),
    }
}

/// 校验 baseUrl：必须是可解析的 http(s) URL（§16.2 结构化 URL 处理，
/// 禁止后续环节字符串拼接出错）。本机回环地址允许 http。
fn validate_base_url(base_url: &str) -> Result<(), AiError> {
    let url = reqwest::Url::parse(base_url).map_err(|_| AiError::NotConfigured {
        message: format!("baseUrl 不是合法 URL: {}", base_url),
    })?;
    match url.scheme() {
        "https" => Ok(()),
        // 本机/回环服务（Ollama 等）允许 http；远程 http 拒绝（防 Key 明文传输）。
        "http" if url.host_str().is_some_and(is_loopback_host) => Ok(()),
        "http" => Err(AiError::NotConfigured {
            message: "远程 Provider 必须使用 https baseUrl（http 会明文传输凭证）".to_string(),
        }),
        other => Err(AiError::NotConfigured {
            message: format!("不支持的 baseUrl 协议: {}", other),
        }),
    }
}

fn is_loopback_host(host: &str) -> bool {
    host == "localhost" || host == "127.0.0.1" || host == "::1" || host == "[::1]" || host.ends_with(".local")
}

/// 新增或更新 Provider（`input.id` 为空 = 新建，返回完整行）。
pub fn save_provider(conn: &Connection, input: &SaveAiProviderRequest) -> AppResult<AiProvider> {
    let name = input.name.trim();
    if name.is_empty() {
        return Err(AppError::Ai(AiError::NotConfigured {
            message: "Provider 名称不能为空".to_string(),
        }));
    }
    let base_url = input.base_url.trim().trim_end_matches('/').to_string();
    validate_base_url(&base_url)?;

    let now = now_rfc3339();
    match &input.id {
        Some(id) => {
            let updated = conn.execute(
                "UPDATE ai_providers
                 SET name = ?2, api_type = ?3, base_url = ?4, enabled = ?5,
                     network_policy = ?6, updated_at = ?7
                 WHERE id = ?1",
                params![
                    id,
                    name,
                    input.api_type.as_str(),
                    base_url,
                    input.enabled as i64,
                    input.network_policy.as_str(),
                    now
                ],
            )?;
            if updated == 0 {
                return Err(AppError::Ai(AiError::NotConfigured {
                    message: format!("Provider 不存在: {}", id),
                }));
            }
            Ok(get_provider(conn, id)?.expect("just updated"))
        }
        None => {
            let id = uuid::Uuid::new_v4().to_string();
            conn.execute(
                "INSERT INTO ai_providers
                 (id, name, api_type, base_url, credential_ref, enabled, network_policy, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)",
                params![
                    id,
                    name,
                    input.api_type.as_str(),
                    base_url,
                    credential_ref_for(&id),
                    input.enabled as i64,
                    input.network_policy.as_str(),
                    now
                ],
            )?;
            Ok(get_provider(conn, &id)?.expect("just inserted"))
        }
    }
}

/// 删除 Provider（模型经外键级联删除；任务默认值由调用方清理）。
pub fn delete_provider(conn: &Connection, id: &str) -> AppResult<()> {
    conn.execute("DELETE FROM ai_task_defaults WHERE provider_id = ?1", params![id])?;
    conn.execute("DELETE FROM ai_providers WHERE id = ?1", params![id])?;
    Ok(())
}

/// 连接测试（§12.2）：只返回成功/失败原因/模型 ID 清单。
///
/// - `api_key` 由调用方从凭证存储取出传入；本函数不记录、不回显 Key。
/// - 统一走 `GET {base}/models`（三种协议的列表端点同形，返回 `{data:[{id}]}`；
///   Anthropic 认证用 `x-api-key` + `anthropic-version`，其余用 `Bearer`）。
/// - URL 用结构化拼接（`reqwest::Url`），不手写字符串。
pub async fn test_connection(provider: &AiProvider, api_key: Option<&str>) -> AiProviderTestResult {
    let started = std::time::Instant::now();
    let base = match reqwest::Url::parse(&provider.base_url) {
        Ok(mut u) => {
            if !u.path().ends_with('/') {
                u.set_path(&format!("{}/", u.path()));
            }
            u
        }
        Err(_) => {
            return AiProviderTestResult {
                success: false,
                message: format!("baseUrl 不是合法 URL: {}", provider.base_url),
                models: vec![],
                latency_ms: 0,
            }
        }
    };
    let url = match base.join("models") {
        Ok(u) => u,
        Err(e) => {
            return AiProviderTestResult {
                success: false,
                message: format!("baseUrl 无法拼接端点: {}", e),
                models: vec![],
                latency_ms: 0,
            }
        }
    };

    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            return AiProviderTestResult {
                success: false,
                message: format!("HTTP 客户端初始化失败: {}", e),
                models: vec![],
                latency_ms: 0,
            }
        }
    };

    let mut req = client.get(url);
    if let Some(key) = api_key {
        req = apply_auth_headers(req, provider.api_type, key);
    }

    let response = match req.send().await {
        Ok(r) => r,
        Err(e) => {
            return AiProviderTestResult {
                success: false,
                message: format!("连接失败: {}", sanitize_reqwest_error(&e)),
                models: vec![],
                latency_ms: started.elapsed().as_millis() as i64,
            }
        }
    };

    let latency_ms = started.elapsed().as_millis() as i64;
    let status = response.status();
    if status.as_u16() == 401 || status.as_u16() == 403 {
        return AiProviderTestResult {
            success: false,
            message: "认证失败（401/403）：请在凭证管理中检查或替换 API Key".to_string(),
            models: vec![],
            latency_ms,
        };
    }
    if !status.is_success() {
        return AiProviderTestResult {
            success: false,
            message: format!("Provider 返回 HTTP {}", status.as_u16()),
            models: vec![],
            latency_ms,
        };
    }

    let body: serde_json::Value = match response.json().await {
        Ok(v) => v,
        Err(_) => {
            // 连通但响应非 JSON：仍算连接成功，仅无模型清单。
            return AiProviderTestResult {
                success: true,
                message: "连接成功（响应非 JSON，未解析模型清单）".to_string(),
                models: vec![],
                latency_ms,
            };
        }
    };

    // 模型清单三种协议同形：{data: [{id, ...}]}
    let mut models: Vec<String> = body
        .get("data")
        .and_then(|d| d.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|m| m.get("id").and_then(|n| n.as_str()).map(String::from))
                .collect()
        })
        .unwrap_or_default();
    models.sort();
    models.truncate(100);

    AiProviderTestResult {
        success: true,
        message: format!("连接成功，发现 {} 个模型", models.len()),
        models,
        latency_ms,
    }
}

/// 按协议附加认证头（§7.2 协议差异）：Anthropic Messages 用
/// `x-api-key` + `anthropic-version`，其余协议用 `Authorization: Bearer`。
/// Key 只经内存进入请求头，不进日志/URL/进程命令行。
pub(crate) fn apply_auth_headers(
    mut req: reqwest::RequestBuilder,
    api_type: ApiType,
    api_key: &str,
) -> reqwest::RequestBuilder {
    match api_type {
        ApiType::AnthropicMessages => {
            req = req.header("x-api-key", api_key);
            req.header("anthropic-version", ANTHROPIC_API_VERSION)
        }
        _ => req.header("Authorization", format!("Bearer {}", api_key)),
    }
}

/// Anthropic Messages API 的版本头（§7.2）。
pub(crate) const ANTHROPIC_API_VERSION: &str = "2023-06-01";

/// 网络错误归一化（§16.2）：只保留错误类别，不带 URL/头信息（可能含 Key）。
fn sanitize_reqwest_error(e: &reqwest::Error) -> String {
    if e.is_timeout() {
        "请求超时".to_string()
    } else if e.is_connect() {
        "无法建立连接（检查网络与 baseUrl）".to_string()
    } else if e.is_request() {
        "请求构造失败".to_string()
    } else {
        "网络错误".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open_memory() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        crate::db::init_db(&mut conn).unwrap();
        conn
    }

    fn provider_input() -> SaveAiProviderRequest {
        SaveAiProviderRequest {
            id: None,
            name: "Team OpenAI".into(),
            api_type: ApiType::OpenaiChatCompletions,
            base_url: "https://api.openai.com/v1".into(),
            enabled: true,
            network_policy: NetworkPolicy::OnlineOnly,
        }
    }

    #[test]
    fn provider_crud_roundtrip() {
        let conn = open_memory();
        let saved = save_provider(&conn, &provider_input()).unwrap();
        assert!(!saved.id.is_empty());
        assert_eq!(saved.api_type, ApiType::OpenaiChatCompletions);
        assert_eq!(
            saved.credential_ref.as_deref(),
            Some(credential_ref_for(&saved.id).as_str())
        );
        assert!(saved.enabled);

        // 更新：改名 + 禁用
        let updated = save_provider(
            &conn,
            &SaveAiProviderRequest {
                id: Some(saved.id.clone()),
                enabled: false,
                ..provider_input()
            },
        )
        .unwrap();
        assert_eq!(updated.id, saved.id, "id 必须保持稳定");
        assert!(!updated.enabled);
        assert_eq!(updated.created_at, saved.created_at);

        let listed = list_providers(&conn).unwrap();
        assert_eq!(listed.len(), 1);

        delete_provider(&conn, &saved.id).unwrap();
        assert!(list_providers(&conn).unwrap().is_empty());
    }

    #[test]
    fn base_url_validation_rejects_remote_http() {
        assert!(validate_base_url("https://api.openai.com/v1").is_ok());
        assert!(validate_base_url("http://localhost:11434").is_ok());
        assert!(validate_base_url("http://127.0.0.1:11434").is_ok());
        assert!(validate_base_url("http://192.168.1.10:11434").is_err());
        assert!(validate_base_url("not-a-url").is_err());
        assert!(validate_base_url("ftp://example.com").is_err());
    }

    #[test]
    fn base_url_trailing_slash_is_normalized() {
        let conn = open_memory();
        let mut input = provider_input();
        input.base_url = "https://api.openai.com/v1/".into();
        let saved = save_provider(&conn, &input).unwrap();
        assert_eq!(saved.base_url, "https://api.openai.com/v1");
    }

    #[test]
    fn empty_name_is_rejected() {
        let conn = open_memory();
        let mut input = provider_input();
        input.name = "   ".into();
        let err = save_provider(&conn, &input).unwrap_err();
        assert_eq!(err.code(), "AiNotConfigured");
    }

    #[test]
    fn api_type_and_policy_serde_names_match_design() {
        assert_eq!(
            serde_json::to_value(ApiType::OpenaiChatCompletions).unwrap(),
            "openaiChatCompletions"
        );
        assert_eq!(
            serde_json::to_value(ApiType::OpenaiResponses).unwrap(),
            "openaiResponses"
        );
        assert_eq!(
            serde_json::to_value(ApiType::AnthropicMessages).unwrap(),
            "anthropicMessages"
        );
        assert_eq!(serde_json::to_value(NetworkPolicy::OnlineOnly).unwrap(), "onlineOnly");
        assert_eq!(serde_json::to_value(NetworkPolicy::LocalOnly).unwrap(), "localOnly");
    }
}
