//! OpenAI Chat Completions 协议 Adapter（§7.2）。
//!
//! 端点：`POST {base}/chat/completions`，认证 `Authorization: Bearer`。
//! system 走 messages 首条 `role: "system"`；`max_tokens` 可选；structured
//! output 用 `response_format: {"type": "json_object"}`。流式为
//! `data: {...}` delta chunk + `[DONE]` 哨兵。

use serde_json::json;

use super::super::error::AiError;
use super::super::model::ReasoningEffort;
use super::super::request::{AiTokenUsage, MessageRole};
use super::super::transport::BoxFuture;
use super::SseAction;
use super::{
    endpoint_url, parse_json_body, read_body_limited, send_json, AdapterCall, AdapterContext, AiProviderAdapter,
    ProviderRequest, ProviderResponse, ProviderStream, MAX_RESPONSE_BODY_BYTES,
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
                ctx.stream_idle_timeout,
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
    // F-46 思考程度方言：off 同时带国产方言与 OpenAI 系参数（不识别的字段
    // 被 Provider 忽略）；其余档发 OpenAI 系 reasoning_effort。
    match request.reasoning_effort {
        Some(ReasoningEffort::Off) => {
            obj.insert("enable_thinking".into(), json!(false)); // 通义 Qwen3
            obj.insert("thinking".into(), json!({"type": "disabled"})); // 火山 Doubao / 智谱 GLM
            obj.insert("reasoning_effort".into(), json!("minimal")); // OpenAI 系
        }
        Some(level) => {
            obj.insert("reasoning_effort".into(), json!(level.as_str()));
        }
        None => {}
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
    let finish_reason = choice.get("finish_reason").and_then(|f| f.as_str()).map(String::from);
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
    // F-48：思考增量（深度求索/通义等 reasoning_content）透传，前端实时展示。
    if let Some(reasoning) = choice
        .and_then(|c| c.get("delta"))
        .and_then(|d| d.get("reasoning_content"))
        .and_then(|c| c.as_str())
        .filter(|s| !s.is_empty())
    {
        return SseAction::EmitReasoning(reasoning.to_string());
    }
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
    // MiMo API 等兼容端点在 choices 为空的独立 chunk 中发送 usage，
    // 此时 choice 为 None，需单独提取 usage 作为 Finish 信号。
    if choice.is_none() {
        if let Some(u) = v.get("usage").and_then(parse_usage) {
            return SseAction::Finish {
                finish_reason: "stop".into(),
                usage: Some(u),
            };
        }
    }
    SseAction::Skip
}

#[cfg(test)]
mod tests {
    use super::super::SseAction;
    use super::*;
    use crate::ai::request::AiMessage;

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
            reasoning_effort: None,
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
        // F-46：缺省不传任何思考参数（零回归）
        assert!(body.get("reasoning_effort").is_none());
        assert!(body.get("enable_thinking").is_none());
        assert!(body.get("thinking").is_none());
    }

    /// F-46：off 同时覆盖国产方言（enable_thinking / thinking.disabled）
    /// 与 OpenAI 系（reasoning_effort=minimal）。
    #[test]
    fn reasoning_off_emits_all_dialects() {
        let mut r = req();
        r.reasoning_effort = Some(ReasoningEffort::Off);
        let body = build_body(&r, false);
        assert_eq!(body["enable_thinking"], false);
        assert_eq!(body["thinking"]["type"], "disabled");
        assert_eq!(body["reasoning_effort"], "minimal");
    }

    /// F-46：其余档只发 OpenAI 系 reasoning_effort。
    #[test]
    fn reasoning_levels_emit_reasoning_effort_only() {
        for (level, expect) in [
            (ReasoningEffort::Low, "low"),
            (ReasoningEffort::Medium, "medium"),
            (ReasoningEffort::High, "high"),
        ] {
            let mut r = req();
            r.reasoning_effort = Some(level);
            let body = build_body(&r, false);
            assert_eq!(body["reasoning_effort"], expect);
            assert!(body.get("enable_thinking").is_none());
            assert!(body.get("thinking").is_none());
        }
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
        assert!(matches!(parse_completion(&v), Err(AiError::ResponseInvalid { .. })));
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
        assert!(matches!(map_chat_event(&finish), SseAction::Finish { .. }));
    }

    /// F-48：reasoning_content 增量映射为 EmitReasoning。
    #[test]
    fn stream_event_maps_reasoning_content() {
        let chunk = super::super::sse::SseEvent {
            event: None,
            data: r#"{"choices":[{"delta":{"reasoning_content":"想"}}]}"#.into(),
        };
        assert!(matches!(
            map_chat_event(&chunk),
            SseAction::EmitReasoning(t) if t == "想"
        ));
    }

    #[test]
    fn stream_event_extracts_usage_from_empty_choices() {
        // MiMo API 在 choices 为空的独立 chunk 中发送 usage
        let chunk = super::super::sse::SseEvent {
            event: None,
            data: r#"{"choices":[],"usage":{"prompt_tokens":100,"completion_tokens":50}}"#.into(),
        };
        match map_chat_event(&chunk) {
            SseAction::Finish { finish_reason, usage } => {
                assert_eq!(finish_reason, "stop");
                let u = usage.unwrap();
                assert_eq!(u.input_tokens, Some(100));
                assert_eq!(u.output_tokens, Some(50));
            }
            other => panic!("expected Finish with usage, got: {:?}", other),
        }
    }
}
