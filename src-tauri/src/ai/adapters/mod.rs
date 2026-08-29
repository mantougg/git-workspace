//! Provider Adapter（设计文档 §7.2）。
//!
//! Adapter 只处理协议差异，不负责业务 prompt（§7.2）：三种协议
//! —— OpenAI Chat Completions / OpenAI Responses / Anthropic Messages
//! —— 各一个实现；同一协议的所有自定义 Endpoint 共用同一个 Adapter
//! （§21 决策 9，Provider 只区分 `apiType` 不区分厂商）。
//!
//! 统一职责：
//! - URL 端点拼接与认证头（`Authorization: Bearer` vs `x-api-key` +
//!   `anthropic-version`）；
//! - 请求/响应映射（system 字段位置、`max_tokens` 必填差异、usage 字段名）；
//! - structured output 参数映射（协议缺失时降级：不传参数、靠能力校验与
//!   system 约束前置兜底）；
//! - 流式事件归一化：三协议 → 内部 [`StreamItem`]；
//! - Provider 错误归一化（§17）；
//! - 请求取消与超时（传输层 + 泵任务块间空闲超时）。

pub mod anthropic;
pub mod openai_chat;
pub mod openai_responses;
pub mod sse;

use std::time::Duration;

use crate::error::AppResult;

use super::error::AiError;
use super::model::AiModel;
use super::provider::ApiType;
use super::provider::ANTHROPIC_API_VERSION;
use super::request::{AiMessage, AiTokenUsage};
use super::transport::{
    BoxFuture, ByteStream, CancelToken, HttpMethod, HttpTransport, TransportError,
    TransportRequest, TransportResponse,
};

/// 归一化流式输出项。
#[derive(Debug, Clone)]
pub enum StreamItem {
    /// 文本增量。
    Text { delta: String },
    /// 流结束（正常结束或上游给出终止信号）。
    End {
        finish_reason: Option<String>,
        usage: Option<AiTokenUsage>,
    },
}

/// 流式通道：正常项为 `Ok(StreamItem)`，错误终止为 `Err(AiError)`。
pub type ProviderStream = tokio::sync::mpsc::Receiver<Result<StreamItem, AiError>>;

/// 非流式完整响应（文本已从协议结构中提取）。
#[derive(Debug, Clone, Default)]
pub struct ProviderResponse {
    pub text: String,
    pub finish_reason: Option<String>,
    pub usage: Option<AiTokenUsage>,
}

/// 协议无关的请求体（Adapter 负责映射到协议 JSON）。
#[derive(Debug, Clone)]
pub struct ProviderRequest {
    pub model_id: String,
    /// 系统约束（受信，后端组装）。
    pub system: Option<String>,
    pub messages: Vec<AiMessage>,
    pub temperature: Option<f64>,
    /// 输出 token 上限（Anthropic 必填；OpenAI 系可选）。
    pub max_output_tokens: Option<i64>,
    /// structured output 请求（协议不支持时降级为不传参数）。
    pub json_mode: bool,
}

/// 目标端点（baseUrl + 凭证）。Key 只在内存流经，不进日志/URL。
#[derive(Debug, Clone)]
pub struct ProviderEndpoint {
    /// Provider 本地 ID（错误归一化的 details 用，非敏感）。
    pub provider_id: String,
    pub base_url: String,
    pub api_type: ApiType,
    pub api_key: Option<String>,
}

/// 一次 Adapter 调用的输入。
pub struct AdapterCall {
    pub endpoint: ProviderEndpoint,
    pub request: ProviderRequest,
}

/// Adapter 调用上下文（传输 + 取消 + 超时）。
pub struct AdapterContext<'a> {
    pub transport: &'a dyn HttpTransport,
    pub cancel: &'a CancelToken,
    /// 非流式 = 整请求上限；流式 = 到响应头为止 + 泵任务块间空闲上限。
    pub timeout: Duration,
}

/// Provider Adapter trait（§7.2）。
pub trait AiProviderAdapter: Send + Sync {
    fn api_type(&self) -> ApiType;

    /// 请求前校验（§6.3）：协议与模型/请求参数不匹配时在发送前报错，
    /// 不等 Provider 返回模糊失败。
    fn validate(&self, _model: &AiModel, _request: &ProviderRequest) -> AppResult<()> {
        Ok(())
    }

    fn complete<'a>(
        &'a self,
        call: AdapterCall,
        ctx: AdapterContext<'a>,
    ) -> BoxFuture<'a, Result<ProviderResponse, AiError>>;

    fn stream<'a>(
        &'a self,
        call: AdapterCall,
        ctx: AdapterContext<'a>,
    ) -> BoxFuture<'a, Result<ProviderStream, AiError>>;
}

/// 按 `apiType` 返回单例式 Adapter（无状态，共享实例即可）。
pub fn adapter_for(api_type: ApiType) -> &'static dyn AiProviderAdapter {
    match api_type {
        ApiType::OpenaiChatCompletions => &openai_chat::OpenaiChatCompletionsAdapter,
        ApiType::OpenaiResponses => &openai_responses::OpenaiResponsesAdapter,
        ApiType::AnthropicMessages => &anthropic::AnthropicMessagesAdapter,
    }
}

// ---------------------------------------------------------------------------
// Adapter 共享助手
// ---------------------------------------------------------------------------

/// baseUrl + 相对端点路径的结构化拼接（全局约束 §11：不手写字符串拼 URL）。
/// baseUrl 约定含版本段（如 `https://api.openai.com/v1`），补齐尾斜杠后 join。
pub(super) fn endpoint_url(base_url: &str, path: &str) -> Result<reqwest::Url, AiError> {
    let mut url = reqwest::Url::parse(base_url)
        .map_err(|_| AiError::NotConfigured {
            message: format!("baseUrl 不是合法 URL: {}", base_url),
        })?;
    if !url.path().ends_with('/') {
        url.set_path(&format!("{}/", url.path()));
    }
    url.join(path).map_err(|e| AiError::NotConfigured {
        message: format!("baseUrl 无法拼接端点: {}", e),
    })
}

/// 发送 JSON 请求并做状态码归一化（§17）。成功返回响应（body 未读）。
pub(super) async fn send_json(
    ctx: &AdapterContext<'_>,
    url: reqwest::Url,
    body: serde_json::Value,
    endpoint: &ProviderEndpoint,
    model_id: &str,
) -> Result<TransportResponse, AiError> {
    let response =
        send_request(ctx, HttpMethod::Post, url, Some(body.to_string().into_bytes()), endpoint)
            .await
            .map_err(transport_error)?;
    classify_status(response, &endpoint.provider_id, model_id).await
}

/// 组装并发送请求：认证头按协议差异附加，取消/超时交给传输层。
async fn send_request(
    ctx: &AdapterContext<'_>,
    method: HttpMethod,
    url: reqwest::Url,
    body: Option<Vec<u8>>,
    endpoint: &ProviderEndpoint,
) -> Result<TransportResponse, TransportError> {
    let mut headers: Vec<(String, String)> =
        vec![("Content-Type".into(), "application/json".into())];
    if let Some(key) = &endpoint.api_key {
        match endpoint.api_type {
            ApiType::AnthropicMessages => {
                headers.push(("x-api-key".into(), key.clone()));
                headers.push(("anthropic-version".into(), ANTHROPIC_API_VERSION.into()));
            }
            _ => headers.push(("Authorization".into(), format!("Bearer {}", key))),
        }
    }
    let request = TransportRequest {
        method,
        url,
        headers,
        body,
    };
    ctx.transport.send(request, ctx.cancel, ctx.timeout).await
}

/// 传输错误 → AiError（§7.4：临时网络错误可重试；超时不可自动重试）。
pub(super) fn transport_error(e: TransportError) -> AiError {
    match e {
        TransportError::Transient(m) => AiError::ProviderUnavailable {
            message: m,
            transient: true,
        },
        TransportError::Timeout => AiError::ProviderUnavailable {
            message: "请求超时".to_string(),
            transient: false,
        },
        TransportError::Cancelled => AiError::RequestCancelled {
            request_id: String::new(),
        },
        TransportError::Invalid(m) => AiError::ProviderUnavailable {
            message: m,
            transient: false,
        },
    }
}

/// HTTP 状态码归一化（§17）：
/// - 401/403 → `AiAuthenticationFailed`（不可重试）
/// - 429 → `AiRateLimited`（可重试）
/// - 404 + 模型不存在特征 → `AiModelNotFound`
/// - 其余 4xx → `AiPolicyRejected`（Provider 拒绝，含 413 载荷过大）
/// - 5xx → `AiProviderUnavailable{transient: true}`（可重试）
/// 失败时读取并截断错误体，仅提取 provider 错误类型/码，不透传正文。
async fn classify_status(
    response: TransportResponse,
    provider_id: &str,
    model_id: &str,
) -> Result<TransportResponse, AiError> {
    let status = response.status;
    if response.is_success() {
        return Ok(response);
    }
    let body = read_body_limited(response.body, MAX_ERROR_BODY_BYTES).await;
    let body_text = String::from_utf8_lossy(&body).into_owned();

    if status == 401 || status == 403 {
        return Err(AiError::AuthenticationFailed {
            message: format!(
                "Provider 认证失败（HTTP {}）：请在 AI 设置-凭证中检查或替换 API Key",
                status
            ),
        });
    }
    if status == 429 {
        return Err(AiError::RateLimited {
            message: format!("Provider 返回 429（请求过于频繁）"),
        });
    }
    if status == 404 {
        let label = api_error_label(&body_text).unwrap_or_default();
        if label.contains("model_not_found") || label.contains("does not exist") {
            return Err(AiError::ModelNotFound {
                provider_id: provider_id.to_string(),
                model_id: model_id.to_string(),
            });
        }
    }
    if (500..600).contains(&status) {
        return Err(AiError::ProviderUnavailable {
            message: format!("Provider 返回 HTTP {}", status),
            transient: true,
        });
    }
    // 其余 4xx：Provider 侧拒绝（参数/权限/策略/载荷过大），不自动重试。
    Err(AiError::PolicyRejected {
        message: format!(
            "Provider 返回 HTTP {}{}",
            status,
            api_error_label(&body_text)
                .map(|l| format!("（{}）", l))
                .unwrap_or_default()
        ),
    })
}

/// 错误体最大读取量（错误正文不进错误消息，只用于提取类型/码）。
pub(super) const MAX_ERROR_BODY_BYTES: usize = 64 * 1024;

/// 响应体最大读取量上限（§16.1 payload 上限；正常响应超限报错）。
pub(super) const MAX_RESPONSE_BODY_BYTES: usize = 16 * 1024 * 1024;

/// 读取 body 至多 `cap` 字节；超出上限报错（防 Provider 异常响应撑爆内存）。
pub(super) async fn read_body_limited(
    mut body: Box<dyn ByteStream>,
    cap: usize,
) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        match body.next_chunk().await {
            Ok(Some(bytes)) => {
                if out.len() + bytes.len() > cap {
                    return out; // 截断（错误体路径只提取类型/码，够用）
                }
                out.extend_from_slice(&bytes);
            }
            Ok(None) => return out,
            Err(_) => return out, // 读取中断：返回已读部分（调用方按解析失败处理）
        }
    }
}

/// 解析 JSON 响应体；非法 JSON → `AiResponseInvalid`（§18.2 非法 JSON 场景）。
pub(super) fn parse_json_body(raw: &[u8]) -> Result<serde_json::Value, AiError> {
    serde_json::from_slice(raw).map_err(|e| AiError::ResponseInvalid {
        message: format!("Provider 响应不是合法 JSON（{}）", truncate_label(&e.to_string())),
    })
}

fn truncate_label(s: &str) -> String {
    s.chars().take(80).collect()
}

/// 流式事件映射动作（协议映射函数的返回值）。
pub(super) enum SseAction {
    /// 文本增量。
    Emit(String),
    /// 记录终止原因/用量，但不结束流（等 [DONE]/message_stop）。
    Finish {
        finish_reason: String,
        usage: Option<AiTokenUsage>,
    },
    /// 结束流。
    End {
        finish_reason: Option<String>,
        usage: Option<AiTokenUsage>,
    },
    /// 协议违规（非法 JSON 事件）。
    Invalid,
    /// 忽略（ping / 其他事件类型）。
    Skip,
}

/// 启动 SSE 泵任务：从响应体读字节 → SSE 解码 → 协议映射 → 归一化通道。
///
/// - 取消：select 在 `cancel.cancelled()` 上，取消立即终止读取并退出；
/// - 空闲超时：块间超过 `idle_timeout` 视为连接死亡（§16.1 异步不阻塞）；
/// - 流在正常结束信号前断开：`流式连接中断`（可重试，§7.4）。
pub(super) fn spawn_sse_pump<F>(
    mut body: Box<dyn ByteStream>,
    cancel: CancelToken,
    idle_timeout: Duration,
    map_event: F,
) -> ProviderStream
where
    F: Fn(&sse::SseEvent) -> SseAction + Send + 'static,
{
    let (tx, rx) = tokio::sync::mpsc::channel::<Result<StreamItem, AiError>>(64);
    tokio::spawn(async move {
        let mut decoder = sse::SseDecoder::new();
        let mut finish_reason: Option<String> = None;
        let mut usage: Option<AiTokenUsage> = None;
        let mut done = false;
        loop {
            let chunk = tokio::select! {
                c = body.next_chunk() => c,
                _ = cancel.cancelled() => {
                    let _ = tx.send(Err(AiError::RequestCancelled { request_id: String::new() })).await;
                    return;
                }
                _ = tokio::time::sleep(idle_timeout) => {
                    let _ = tx
                        .send(Err(AiError::ProviderUnavailable {
                            message: "流式响应空闲超时".to_string(),
                            transient: false,
                        }))
                        .await;
                    return;
                }
            };
            let Ok(Some(bytes)) = chunk else {
                // Ok(None) = 服务端提前断流；Err = 读中断。均为连接中断。
                let _ = tx
                    .send(Err(AiError::ProviderUnavailable {
                        message: "流式连接中断".to_string(),
                        transient: true,
                    }))
                    .await;
                return;
            };
            for event in decoder.push(&bytes) {
                match map_event(&event) {
                    SseAction::Emit(delta) => {
                        if tx.send(Ok(StreamItem::Text { delta })).await.is_err() {
                            return; // 接收端已放弃（取消/超时）
                        }
                    }
                    SseAction::Finish {
                        finish_reason: fr,
                        usage: u,
                    } => {
                        finish_reason = Some(fr);
                        if u.is_some() {
                            usage = u;
                        }
                    }
                    SseAction::End {
                        finish_reason: fr,
                        usage: u,
                    } => {
                        done = true;
                        if fr.is_some() {
                            finish_reason = fr;
                        }
                        if u.is_some() {
                            usage = u;
                        }
                    }
                    SseAction::Invalid => {
                        let _ = tx
                            .send(Err(AiError::ResponseInvalid {
                                message: "流式响应包含非法 JSON 事件".to_string(),
                            }))
                            .await;
                        return;
                    }
                    SseAction::Skip => {}
                }
                if done {
                    break;
                }
            }
            if done {
                let _ = tx
                    .send(Ok(StreamItem::End {
                        finish_reason,
                        usage,
                    }))
                    .await;
                return;
            }
        }
    });
    rx
}

/// 从错误体提取 provider 错误类型/码标签（OpenAI `error.code|type`、
/// Anthropic `error.type`）。只取类型字段，不取 message 正文（避免把
/// 请求内容回显进错误）。
fn api_error_label(body: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    let error = v.get("error")?;
    for key in ["code", "type"] {
        if let Some(s) = error.get(key).and_then(|x| x.as_str()) {
            return Some(s.to_string());
        }
    }
    None
}
