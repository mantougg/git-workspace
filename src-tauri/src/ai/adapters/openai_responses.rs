//! OpenAI Responses 协议 Adapter（§7.2）。
//!
//! 端点：`POST {base}/responses`，认证 `Authorization: Bearer`。
//! system 走顶层 `instructions`；输入为 `input` 数组（内容项
//! `input_text`）；structured output 用 `text.format: {"type": "json_object"}`。
//! 流式为 Responses 事件流（`response.output_text.delta` /
//! `response.completed` 等，事件类型在 data JSON 的 `type` 字段）。

use serde_json::json;

use super::super::error::AiError;
use super::super::request::{AiTokenUsage, MessageRole};
use super::super::transport::BoxFuture;
use super::SseAction;
use super::{
    endpoint_url, parse_json_body, read_body_limited, send_json, AdapterCall, AdapterContext, AiProviderAdapter,
    ProviderRequest, ProviderResponse, ProviderStream, MAX_RESPONSE_BODY_BYTES,
};
use crate::ai::provider::ApiType;
use crate::error::AppResult;

pub struct OpenaiResponsesAdapter;

impl AiProviderAdapter for OpenaiResponsesAdapter {
    fn api_type(&self) -> ApiType {
        ApiType::OpenaiResponses
    }

    fn validate(&self, _model: &crate::ai::model::AiModel, _request: &ProviderRequest) -> AppResult<()> {
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
            let url = endpoint_url(&call.endpoint.base_url, "responses")?;
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
            let url = endpoint_url(&call.endpoint.base_url, "responses")?;
            let response = send_json(&ctx, url, body, &call.endpoint, &model_id).await?;
            Ok(super::spawn_sse_pump(
                response.body,
                ctx.cancel.clone(),
                ctx.timeout,
                map_responses_event,
            ))
        })
    }
}

fn build_body(request: &ProviderRequest, stream: bool) -> serde_json::Value {
    let mut input = Vec::new();
    for m in &request.messages {
        let type_field = match m.role {
            MessageRole::User => "input_text",
            MessageRole::Assistant => "output_text",
            MessageRole::System => "input_text", // system 已并入 instructions，防御兜底
        };
        input.push(json!({
            "role": role_str(m.role),
            "content": [{"type": type_field, "text": m.content}],
        }));
    }
    let mut body = json!({
        "model": request.model_id,
        "input": input,
        "stream": stream,
    });
    if let Some(system) = &request.system {
        body.as_object_mut()
            .expect("object literal")
            .insert("instructions".into(), json!(system));
    }
    let obj = body.as_object_mut().expect("object literal");
    if let Some(t) = request.temperature {
        obj.insert("temperature".into(), json!(t));
    }
    if let Some(max) = request.max_output_tokens {
        obj.insert("max_output_tokens".into(), json!(max));
    }
    if request.json_mode {
        obj.insert("text".into(), json!({"format": {"type": "json_object"}}));
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

/// 解析非流式响应：`output[].content[]` 中 `output_text` 项拼接 +
/// `usage.input_tokens/output_tokens` + `status/incomplete_details`。
fn parse_completion(value: &serde_json::Value) -> Result<ProviderResponse, AiError> {
    let mut text = String::new();
    let Some(output) = value.get("output").and_then(|o| o.as_array()) else {
        return Err(AiError::ResponseInvalid {
            message: "响应缺少 output 数组".to_string(),
        });
    };
    for item in output {
        if let Some(content) = item.get("content").and_then(|c| c.as_array()) {
            for part in content {
                if part.get("type").and_then(|t| t.as_str()) == Some("output_text") {
                    if let Some(t) = part.get("text").and_then(|t| t.as_str()) {
                        text.push_str(t);
                    }
                }
            }
        }
    }
    let finish_reason = value.get("status").and_then(|s| s.as_str()).map(String::from);
    Ok(ProviderResponse {
        text,
        finish_reason,
        usage: value.get("usage").and_then(parse_usage),
    })
}

/// usage 字段名（Responses: input_tokens/output_tokens）。
fn parse_usage(u: &serde_json::Value) -> Option<AiTokenUsage> {
    Some(AiTokenUsage {
        input_tokens: u.get("input_tokens").and_then(|v| v.as_i64()),
        output_tokens: u.get("output_tokens").and_then(|v| v.as_i64()),
    })
}

/// 事件类型在 data JSON 的 `type` 字段（与 SSE `event:` 行等价，取其一即可）：
/// - `response.output_text.delta` → `delta` → Text；
/// - `response.completed` / `response.incomplete` → End（带 usage）；
/// - `response.failed` / `error` → 协议错误。
fn map_responses_event(event: &super::sse::SseEvent) -> super::SseAction {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&event.data) else {
        return SseAction::Invalid;
    };
    match v.get("type").and_then(|t| t.as_str()).unwrap_or("") {
        "response.output_text.delta" => {
            let delta = v.get("delta").and_then(|d| d.as_str()).unwrap_or_default();
            if delta.is_empty() {
                SseAction::Skip
            } else {
                SseAction::Emit(delta.to_string())
            }
        }
        "response.completed" | "response.incomplete" => SseAction::End {
            finish_reason: v.pointer("/response/status").and_then(|s| s.as_str()).map(String::from),
            usage: v.pointer("/response/usage").and_then(parse_usage),
        },
        "response.failed" => SseAction::Invalid,
        "error" => SseAction::Invalid,
        // response.created / output_item.added / content_part.added / ...
        _ => SseAction::Skip,
    }
}

#[cfg(test)]
mod tests {
    use super::super::SseAction;
    use super::*;
    use crate::ai::request::AiMessage;

    #[test]
    fn body_uses_instructions_and_input_text() {
        let req = ProviderRequest {
            model_id: "gpt-5".into(),
            system: Some("sys".into()),
            messages: vec![AiMessage {
                role: MessageRole::User,
                content: "hi".into(),
            }],
            temperature: None,
            max_output_tokens: Some(256),
            json_mode: true,
        };
        let body = build_body(&req, false);
        assert_eq!(body["instructions"], "sys");
        assert_eq!(body["input"][0]["content"][0]["type"], "input_text");
        assert_eq!(body["max_output_tokens"], 256);
        assert_eq!(body["text"]["format"]["type"], "json_object");
        assert_eq!(body["stream"], false);
    }

    #[test]
    fn parse_completion_walks_output_items() {
        let v = serde_json::json!({
            "output": [
                {"type": "reasoning", "summary": []},
                {"type": "message", "content": [
                    {"type": "output_text", "text": "hel"},
                    {"type": "output_text", "text": "lo"}
                ]}
            ],
            "status": "completed",
            "usage": {"input_tokens": 7, "output_tokens": 3}
        });
        let r = parse_completion(&v).unwrap();
        assert_eq!(r.text, "hello");
        assert_eq!(r.finish_reason.as_deref(), Some("completed"));
        assert_eq!(r.usage.unwrap().output_tokens, Some(3));
    }

    #[test]
    fn parse_completion_rejects_missing_output() {
        assert!(matches!(
            parse_completion(&serde_json::json!({"error": {}})),
            Err(AiError::ResponseInvalid { .. })
        ));
    }

    #[test]
    fn stream_events_map_to_chunks() {
        let delta = super::super::sse::SseEvent {
            event: Some("response.output_text.delta".into()),
            data: r#"{"type":"response.output_text.delta","delta":"he"}"#.into(),
        };
        assert!(matches!(
            map_responses_event(&delta),
            SseAction::Emit(t) if t == "he"
        ));

        let completed = super::super::sse::SseEvent {
            event: Some("response.completed".into()),
            data: r#"{"type":"response.completed","response":{"status":"completed","usage":{"input_tokens":5,"output_tokens":2}}}"#.into(),
        };
        assert!(matches!(map_responses_event(&completed), SseAction::End { .. }));

        let failed = super::super::sse::SseEvent {
            event: Some("response.failed".into()),
            data: r#"{"type":"response.failed"}"#.into(),
        };
        assert!(matches!(map_responses_event(&failed), SseAction::Invalid));
    }
}
