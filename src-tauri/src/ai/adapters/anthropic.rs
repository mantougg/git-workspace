//! Anthropic Messages 协议 Adapter（§7.2）。
//!
//! 端点：`POST {base}/messages`；认证 `x-api-key` + `anthropic-version`
//! （版本头由 `send_json` 统一附加）。差异点：
//! - system 是顶层 `system` 字段，不进 messages；
//! - `max_tokens` **必填**（由 Gateway 按预算/模型上下文推导）；
//! - messages 只允许 user/assistant 交替；
//! - 无原生 structured output 参数 → 降级不传（能力校验 + system 约束
//!   前置兜底，§7.2）；
//! - 流式为 `content_block_delta`（`text_delta`）→ `message_delta`
//!   （stop_reason/usage）→ `message_stop` 事件流。

use serde_json::json;

use super::super::error::AiError;
use super::SseAction;
use super::super::request::{AiTokenUsage, MessageRole};
use super::super::transport::BoxFuture;
use super::{
    endpoint_url, parse_json_body, read_body_limited, send_json, AdapterCall, AdapterContext,
    AiProviderAdapter, MAX_RESPONSE_BODY_BYTES, ProviderRequest, ProviderResponse, ProviderStream,
};
use crate::ai::provider::ApiType;
use crate::error::AppResult;

pub struct AnthropicMessagesAdapter;

impl AiProviderAdapter for AnthropicMessagesAdapter {
    fn api_type(&self) -> ApiType {
        ApiType::AnthropicMessages
    }

    fn validate(
        &self,
        _model: &crate::ai::model::AiModel,
        request: &ProviderRequest,
    ) -> AppResult<()> {
        // max_tokens 必填：Gateway 负责推导（默认值兜底），这里防御校验。
        if request.max_output_tokens.is_none() || request.max_output_tokens <= Some(0) {
            return Err(crate::error::AppError::Ai(AiError::NotConfigured {
                message: "Anthropic Messages 协议要求 max_output_tokens（token 预算或模型上下文推导）"
                    .to_string(),
            }));
        }
        Ok(())
    }

    fn complete<'a>(
        &'a self,
        call: AdapterCall,
        ctx: AdapterContext<'a>,
    ) -> BoxFuture<'a, Result<ProviderResponse, AiError>> {
        Box::pin(async move {
            let model_id = call.request.model_id.clone();
            let body = build_body(&call.request, false);
            let url = endpoint_url(&call.endpoint.base_url, "messages")?;
            let response = send_json(&ctx, url, body, &call.endpoint, &model_id).await?;
            let raw = read_body_limited(response.body, MAX_RESPONSE_BODY_BYTES).await;
            let value = parse_json_body(&raw)?;
            parse_completion(&value)
        })
    }

    fn stream<'a>(
        &'a self,
        call: AdapterCall,
        ctx: AdapterContext<'a>,
    ) -> BoxFuture<'a, Result<ProviderStream, AiError>> {
        Box::pin(async move {
            let model_id = call.request.model_id.clone();
            let body = build_body(&call.request, true);
            let url = endpoint_url(&call.endpoint.base_url, "messages")?;
            let response = send_json(&ctx, url, body, &call.endpoint, &model_id).await?;
            Ok(super::spawn_sse_pump(
                response.body,
                ctx.cancel.clone(),
                ctx.timeout,
                map_anthropic_event,
            ))
        })
    }
}

fn build_body(request: &ProviderRequest, stream: bool) -> serde_json::Value {
    // Anthropic messages 只接受 user/assistant；system 消息并入顶层 system。
    let mut system_parts: Vec<String> = Vec::new();
    if let Some(system) = &request.system {
        system_parts.push(system.clone());
    }
    let mut messages = Vec::new();
    for m in &request.messages {
        match m.role {
            MessageRole::System => system_parts.push(m.content.clone()),
            MessageRole::User => messages.push(json!({"role": "user", "content": m.content})),
            MessageRole::Assistant => {
                messages.push(json!({"role": "assistant", "content": m.content}))
            }
        }
    }
    let mut body = json!({
        "model": request.model_id,
        // 必填（§7.2 协议差异）：由 Gateway 按预算推导，防御兜底 4096。
        "max_tokens": request.max_output_tokens.unwrap_or(4096),
        "messages": messages,
        "stream": stream,
    });
    let obj = body.as_object_mut().expect("object literal");
    if !system_parts.is_empty() {
        obj.insert("system".into(), json!(system_parts.join("\n\n")));
    }
    if let Some(t) = request.temperature {
        obj.insert("temperature".into(), json!(t));
    }
    // 无原生 structured output 参数：json_mode 降级为不传（§7.2）。
    body
}

/// 解析非流式响应：content[] 中 text 项拼接 + usage(input/output_tokens) +
/// stop_reason。
fn parse_completion(value: &serde_json::Value) -> Result<ProviderResponse, AiError> {
    let Some(content) = value.get("content").and_then(|c| c.as_array()) else {
        return Err(AiError::ResponseInvalid {
            message: "响应缺少 content 数组".to_string(),
        });
    };
    let mut text = String::new();
    for item in content {
        if item.get("type").and_then(|t| t.as_str()) == Some("text") {
            if let Some(t) = item.get("text").and_then(|t| t.as_str()) {
                text.push_str(t);
            }
        }
    }
    Ok(ProviderResponse {
        text,
        finish_reason: value
            .get("stop_reason")
            .and_then(|s| s.as_str())
            .map(String::from),
        usage: value.get("usage").and_then(parse_usage),
    })
}

fn parse_usage(u: &serde_json::Value) -> Option<AiTokenUsage> {
    Some(AiTokenUsage {
        input_tokens: u.get("input_tokens").and_then(|v| v.as_i64()),
        output_tokens: u.get("output_tokens").and_then(|v| v.as_i64()),
    })
}

/// 流式事件映射（data JSON 的 `type` 字段）：
/// - `content_block_delta` + `text_delta` → Text；
/// - `message_delta` → 记录 stop_reason/usage；
/// - `message_stop` → End；
/// - `error` → 协议错误；`ping` / block 生命周期事件 → Skip。
fn map_anthropic_event(event: &super::sse::SseEvent) -> super::SseAction {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&event.data) else {
        return SseAction::Invalid;
    };
    match v.get("type").and_then(|t| t.as_str()).unwrap_or("") {
        "content_block_delta" => {
            let Some(delta) = v.get("delta") else {
                return SseAction::Skip;
            };
            if delta.get("type").and_then(|t| t.as_str()) == Some("text_delta") {
                let text = delta.get("text").and_then(|t| t.as_str()).unwrap_or_default();
                if text.is_empty() {
                    SseAction::Skip
                } else {
                    SseAction::Emit(text.to_string())
                }
            } else {
                SseAction::Skip // thinking_delta 等忽略
            }
        }
        "message_delta" => SseAction::Finish {
            finish_reason: v
                .pointer("/delta/stop_reason")
                .and_then(|s| s.as_str())
                .unwrap_or_default()
                .to_string(),
            usage: v.get("usage").and_then(parse_usage),
        },
        "message_stop" => SseAction::End {
            finish_reason: None,
            usage: None,
        },
        "error" => SseAction::Invalid,
        _ => SseAction::Skip,
    }
}

#[cfg(test)]
mod tests {
    use super::super::SseAction;
    use crate::ai::request::AiMessage;
    use super::*;

    #[test]
    fn body_moves_system_to_top_level_and_sets_max_tokens() {
        let req = ProviderRequest {
            model_id: "claude-x".into(),
            system: Some("sys".into()),
            messages: vec![
                AiMessage {
                    role: MessageRole::System,
                    content: "extra sys".into(),
                },
                AiMessage {
                    role: MessageRole::User,
                    content: "hi".into(),
                },
            ],
            temperature: Some(0.1),
            max_output_tokens: Some(1024),
            json_mode: true,
        };
        let body = build_body(&req, true);
        assert_eq!(body["system"], "sys\n\nextra sys");
        assert_eq!(body["max_tokens"], 1024);
        assert_eq!(body["messages"][0]["role"], "user");
        assert!(body.get("response_format").is_none(), "无原生 json 参数");
        assert_eq!(body["stream"], true);
    }

    #[test]
    fn validate_requires_max_output_tokens() {
        let req = ProviderRequest {
            model_id: "claude-x".into(),
            system: None,
            messages: vec![],
            temperature: None,
            max_output_tokens: None,
            json_mode: false,
        };
        assert!(AnthropicMessagesAdapter.validate(&test_model(), &req).is_err());
    }

    fn test_model() -> crate::ai::model::AiModel {
        crate::ai::model::AiModel {
            provider_id: "p".into(),
            id: "claude-x".into(),
            display_name: "claude-x".into(),
            capabilities: vec![],
            max_context_tokens: 200000,
            defaults: Default::default(),
            enabled: true,
            created_at: "t".into(),
            updated_at: "t".into(),
        }
    }

    #[test]
    fn parse_completion_joins_text_blocks() {
        let v = serde_json::json!({
            "content": [
                {"type": "text", "text": "hel"},
                {"type": "tool_use", "id": "x"},
                {"type": "text", "text": "lo"}
            ],
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 9, "output_tokens": 4}
        });
        let r = parse_completion(&v).unwrap();
        assert_eq!(r.text, "hello");
        assert_eq!(r.finish_reason.as_deref(), Some("end_turn"));
        assert_eq!(r.usage.unwrap().input_tokens, Some(9));
    }

    #[test]
    fn stream_events_map_to_chunks() {
        let delta = super::super::sse::SseEvent {
            event: Some("content_block_delta".into()),
            data: r#"{"type":"content_block_delta","delta":{"type":"text_delta","text":"he"}}"#
                .into(),
        };
        assert!(matches!(
            map_anthropic_event(&delta),
            SseAction::Emit(t) if t == "he"
        ));

        let stop = super::super::sse::SseEvent {
            event: Some("message_stop".into()),
            data: r#"{"type":"message_stop"}"#.into(),
        };
        assert!(matches!(
            map_anthropic_event(&stop),
            SseAction::End { .. }
        ));

        let ping = super::super::sse::SseEvent {
            event: None,
            data: r#"{"type":"ping"}"#.into(),
        };
        assert!(matches!(map_anthropic_event(&ping), SseAction::Skip));
    }
}
