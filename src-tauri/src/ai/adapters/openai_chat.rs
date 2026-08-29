//! OpenAI Chat Completions 协议 Adapter（§7.2）。
//!
//! 端点：`POST {base}/chat/completions`，认证 `Authorization: Bearer`。
//! system 走 messages 首条 `role: "system"`；`max_tokens` 可选；structured
//! output 用 `response_format: {"type": "json_object"}`。流式为
//! `data: {...}` delta chunk + `[DONE]` 哨兵。

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

pub struct OpenaiChatCompletionsAdapter;

impl AiProviderAdapter for OpenaiChatCompletionsAdapter {
    fn api_type(&self) -> ApiType {
        ApiType::OpenaiChatCompletions
    }

    fn validate(&self, _model: &crate::ai::model::AiModel, _request: &ProviderRequest) -> AppResult<()> {
        // json_mode：OpenAI 系支持 response_format 参数，无需拦截。
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
            let url = endpoint_url(&call.endpoint.base_url, "chat/completions")?;
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
            let url = endpoint_url(&call.endpoint.base_url, "chat/completions")?;
            let response = send_json(&ctx, url, body, &call.endpoint, &model_id).await?;
            Ok(super::spawn_sse_pump(
                response.body,
                ctx.cancel.clone(),
                ctx.timeout,
                map_chat_event,
            ))
        })
    }
}

/// 组装请求体。`stream` 决定流式开关（§16.1：流式响应经事件推送）。
fn build_body(request: &ProviderRequest, stream: bool) -> serde_json::Value {
    let mut messages = Vec::new();
    if let Some(system) = &request.system {
        messages.push(json!({"role": "system", "content": system}));
    }
    for m in &request.messages {
        messages.push(json!({"role": role_str(m.role), "content": m.content}));
    }
    let mut body = json!({
        "model": request.model_id,
        "messages": messages,
        "stream": stream,
    });
    let obj = body.as_object_mut().expect("object literal");
    if let Some(t) = request.temperature {
        obj.insert("temperature".into(), json!(t));
    }
    if let Some(max) = request.max_output_tokens {
        obj.insert("max_tokens".into(), json!(max));
    }
    if request.json_mode {
        obj.insert("response_format".into(), json!({"type": "json_object"}));
    }
    body
}

fn role_str(role: MessageRole) -> &'static str {
    match role {
        MessageRole::System => "system",
        MessageRole::User => "user",
        MessageRole::Assistant => "assistant",
    }
}

/// 解析非流式响应：choices[0].message.content + usage。
fn parse_completion(value: &serde_json::Value) -> Result<ProviderResponse, AiError> {
    let choice = value
        .get("choices")
        .and_then(|c| c.get(0))
        .ok_or_else(|| AiError::ResponseInvalid {
            message: "响应缺少 choices[0]".to_string(),
        })?;
    let text = choice
        .get("message")
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_str())
        .unwrap_or_default()
        .to_string();
    let finish_reason = choice
        .get("finish_reason")
        .and_then(|f| f.as_str())
        .map(String::from);
    Ok(ProviderResponse {
        text,
        finish_reason,
        usage: value.get("usage").and_then(parse_usage),
    })
}

/// usage 字段名（OpenAI: prompt/completion_tokens）。
fn parse_usage(u: &serde_json::Value) -> Option<AiTokenUsage> {
    Some(AiTokenUsage {
        input_tokens: u.get("prompt_tokens").and_then(|v| v.as_i64()),
        output_tokens: u.get("completion_tokens").and_then(|v| v.as_i64()),
    })
}

/// 流式事件映射：delta.content → Text；[DONE] → End。
fn map_chat_event(event: &super::sse::SseEvent) -> super::SseAction {
    if event.data.trim() == "[DONE]" {
        return SseAction::End {
            finish_reason: None,
            usage: None,
        };
    }
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&event.data) else {
        return SseAction::Invalid;
    };
    let choice = v.get("choices").and_then(|c| c.get(0));
    if let Some(delta_text) = choice
        .and_then(|c| c.get("delta"))
        .and_then(|d| d.get("content"))
        .and_then(|c| c.as_str())
        .filter(|s| !s.is_empty())
    {
        return SseAction::Emit(delta_text.to_string());
    }
    if let Some(fr) = choice.and_then(|c| c.get("finish_reason")).and_then(|f| f.as_str()) {
        // finish_reason 常在 [DONE] 前一个 chunk 出现；记录之，End 由
        // [DONE] 或流结束逻辑派发。
        return SseAction::Finish {
            finish_reason: fr.to_string(),
            usage: v.get("usage").and_then(parse_usage),
        };
    }
    SseAction::Skip
}

#[cfg(test)]
mod tests {
    use super::super::SseAction;
    use crate::ai::request::AiMessage;
    use super::*;

    fn req() -> ProviderRequest {
        ProviderRequest {
            model_id: "gpt-x".into(),
            system: Some("be brief".into()),
            messages: vec![AiMessage {
                role: MessageRole::User,
                content: "hi".into(),
            }],
            temperature: Some(0.2),
            max_output_tokens: Some(512),
            json_mode: true,
        }
    }

    #[test]
    fn body_maps_system_messages_and_params() {
        let body = build_body(&req(), true);
        assert_eq!(body["model"], "gpt-x");
        assert_eq!(body["stream"], true);
        assert_eq!(body["messages"][0]["role"], "system");
        assert_eq!(body["messages"][1]["role"], "user");
        assert_eq!(body["temperature"], 0.2);
        assert_eq!(body["max_tokens"], 512);
        assert_eq!(body["response_format"]["type"], "json_object");
    }

    #[test]
    fn parse_completion_extracts_content_and_usage() {
        let v = serde_json::json!({
            "choices": [{"message": {"content": "ok"}, "finish_reason": "stop"}],
            "usage": {"prompt_tokens": 10, "completion_tokens": 2}
        });
        let r = parse_completion(&v).unwrap();
        assert_eq!(r.text, "ok");
        assert_eq!(r.finish_reason.as_deref(), Some("stop"));
        let usage = r.usage.unwrap();
        assert_eq!(usage.input_tokens, Some(10));
        assert_eq!(usage.output_tokens, Some(2));
    }

    #[test]
    fn parse_completion_rejects_missing_choices() {
        let v = serde_json::json!({"error": {"code": "x"}});
        assert!(matches!(
            parse_completion(&v),
            Err(AiError::ResponseInvalid { .. })
        ));
    }

    #[test]
    fn stream_event_mapping_normalizes_chunks() {
        let chunk = super::super::sse::SseEvent {
            event: None,
            data: r#"{"choices":[{"delta":{"content":"he"}}]}"#.into(),
        };
        assert!(matches!(
            map_chat_event(&chunk),
            SseAction::Emit(t) if t == "he"
        ));

        let done = super::super::sse::SseEvent {
            event: None,
            data: "[DONE]".into(),
        };
        assert!(matches!(map_chat_event(&done), SseAction::End { .. }));

        let finish = super::super::sse::SseEvent {
            event: None,
            data: r#"{"choices":[{"delta":{},"finish_reason":"stop"}]}"#.into(),
        };
        assert!(matches!(
            map_chat_event(&finish),
            SseAction::Finish { .. }
        ));
    }
}
