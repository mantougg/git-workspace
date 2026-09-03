//! AI-12：MCP（Model Context Protocol）映射层——JSON-RPC 2.0 ⇄ 外部调用管道。
//!
//! 本模块与传输无关（socket 在 `server.rs`），是纯函数式映射：进来一段
//! JSON-RPC 请求文本，出去一段 JSON-RPC 响应文本（Notification 返回
//! `None`，由传输层回 202）。第一阶段只实现 `initialize` / `ping` /
//! `tools/list` / `tools/call` 四个方法；工具执行一律委托注入的 executor
//! （生产环境是 [`super::run_external_call`]，测试用桩），本模块自身不
//! 触碰任何领域服务。

use std::future::Future;

use serde_json::{json, Value};

use crate::error::{AppError, AppResult};

use super::{registry_tool_name, ExternalCallRequest, ExternalSource, ExternalToolDescriptor};

pub const MCP_PROTOCOL_VERSION: &str = "2025-06-18";
/// `_meta` 键遵循 MCP 的 vendor 前缀约定。
pub const META_CONFIRMED: &str = "gitworkspace/confirmed";
pub const META_CLIENT: &str = "gitworkspace/client";

const PARSE_ERROR: i64 = -32700;
const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;

/// 处理一个 JSON-RPC 请求体。返回 `None` 表示 Notification（无需响应）。
pub async fn handle_jsonrpc<F, Fut>(body: &str, tools: &[ExternalToolDescriptor], execute: &F) -> Option<String>
where
    F: Fn(ExternalCallRequest) -> Fut,
    Fut: Future<Output = AppResult<Value>>,
{
    let request: Value = match serde_json::from_str(body) {
        Ok(value) => value,
        Err(_) => return Some(error_response(Value::Null, PARSE_ERROR, "parse error", None)),
    };
    if request.is_array() {
        return Some(error_response(
            Value::Null,
            INVALID_REQUEST,
            "batch requests are not supported",
            None,
        ));
    }
    let id = request.get("id").cloned();
    let Some(method) = request.get("method").and_then(Value::as_str) else {
        return Some(error_response(Value::Null, INVALID_REQUEST, "missing method", None));
    };
    // Notification（无 id）：按 JSON-RPC 不产生响应。
    let Some(id) = id else {
        return None;
    };

    let params = request.get("params").cloned().unwrap_or(Value::Null);
    let result = match method {
        "initialize" => Ok(initialize_result()),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(tools_list_result(tools)),
        "tools/call" => match build_call(&params) {
            Ok(call) => tool_call_result(call, execute).await,
            Err(message) => Err((INVALID_PARAMS, message, None)),
        },
        _ => Err((METHOD_NOT_FOUND, format!("method not found: {method}"), None)),
    };
    Some(match result {
        Ok(value) => json!({"jsonrpc": "2.0", "id": id, "result": value}).to_string(),
        Err((code, message, data)) => error_response(id, code, &message, data),
    })
}

fn initialize_result() -> Value {
    json!({
        "protocolVersion": MCP_PROTOCOL_VERSION,
        "capabilities": { "tools": { "listChanged": false } },
        "serverInfo": { "name": "git-workspace", "version": env!("CARGO_PKG_VERSION") },
    })
}

fn tools_list_result(tools: &[ExternalToolDescriptor]) -> Value {
    let tools: Vec<Value> = tools
        .iter()
        .map(|tool| {
            json!({
                "name": tool.name,
                "description": tool.description,
                "inputSchema": tool.input_schema,
                "annotations": { "readOnlyHint": tool.read_only },
            })
        })
        .collect();
    json!({ "tools": tools })
}

fn build_call(params: &Value) -> Result<ExternalCallRequest, String> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| "tools/call requires params.name".to_string())?;
    let arguments = params
        .get("arguments")
        .cloned()
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}));
    let meta = params.get("_meta");
    let confirmed = meta
        .and_then(|m| m.get(META_CONFIRMED))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    // CLI 经本地 MCP 端口中继时自带 client 标签，仅作审计来源标识。
    let source = match meta.and_then(|m| m.get(META_CLIENT)).and_then(Value::as_str) {
        Some("cli") => ExternalSource::Cli,
        _ => ExternalSource::Mcp,
    };
    Ok(ExternalCallRequest {
        source,
        tool_name: registry_tool_name(name),
        arguments,
        confirmed,
    })
}

async fn tool_call_result<F, Fut>(call: ExternalCallRequest, execute: &F) -> Result<Value, (i64, String, Option<Value>)>
where
    F: Fn(ExternalCallRequest) -> Fut,
    Fut: Future<Output = AppResult<Value>>,
{
    // MCP 约定：工具执行错误走 result.isError（协议错误才用 JSON-RPC error）。
    let (is_error, text) = match execute(call).await {
        Ok(value) => (
            false,
            serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string()),
        ),
        Err(error) => (true, error_payload(&error).to_string()),
    };
    Ok(json!({
        "content": [{ "type": "text", "text": text }],
        "isError": is_error,
    }))
}

fn error_payload(error: &AppError) -> Value {
    let (code, message) = match error {
        AppError::Ai(ai) => (ai.code(), ai.to_string()),
        other => ("InternalError", other.to_string()),
    };
    json!({ "error": { "code": code, "message": message } })
}

fn error_response(id: Value, code: i64, message: &str, data: Option<Value>) -> String {
    let mut error = json!({ "code": code, "message": message });
    if let Some(data) = data {
        error["data"] = data;
    }
    json!({ "jsonrpc": "2.0", "id": id, "error": error }).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::error::AiError;
    use crate::ai::tools::ToolRegistry;

    fn tools() -> Vec<ExternalToolDescriptor> {
        super::super::external_tool_manifest(&ToolRegistry::default())
    }

    fn ok_executor(_: ExternalCallRequest) -> std::future::Ready<AppResult<Value>> {
        std::future::ready(Ok(json!({"workspaces": []})))
    }

    fn parse(response: Option<String>) -> Value {
        serde_json::from_str(&response.expect("request must produce a response")).unwrap()
    }

    #[tokio::test]
    async fn initialize_advertises_protocol_and_server_info() {
        let response = parse(
            handle_jsonrpc(
                r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
                &tools(),
                &ok_executor,
            )
            .await,
        );
        let result = &response["result"];
        assert_eq!(result["protocolVersion"], MCP_PROTOCOL_VERSION);
        assert_eq!(result["serverInfo"]["name"], "git-workspace");
        assert!(result["capabilities"]["tools"].is_object());
    }

    /// 验收：MCP Adapter 工具清单与 Registry 一致。
    #[tokio::test]
    async fn tools_list_exposes_every_registry_tool() {
        let registry = ToolRegistry::default();
        let response = parse(
            handle_jsonrpc(
                r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
                &tools(),
                &ok_executor,
            )
            .await,
        );
        let listed = response["result"]["tools"].as_array().unwrap();
        assert_eq!(listed.len(), registry.definitions().len());
        for tool in listed {
            let name = tool["name"].as_str().unwrap();
            assert!(registry.get(&registry_tool_name(name)).is_some());
            assert!(tool["inputSchema"].is_object());
            assert!(tool["annotations"]["readOnlyHint"].is_boolean());
        }
    }

    #[tokio::test]
    async fn tools_call_returns_text_content() {
        let response = parse(
            handle_jsonrpc(
                r#"{"jsonrpc":"2.0","id":"abc","method":"tools/call",
                   "params":{"name":"workspace_list","arguments":{}}}"#,
                &tools(),
                &ok_executor,
            )
            .await,
        );
        assert_eq!(response["id"], "abc");
        assert_eq!(response["result"]["isError"], false);
        assert!(response["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("workspaces"));
    }

    #[tokio::test]
    async fn tools_call_maps_dotted_names_and_meta_flags() {
        let executor = |call: ExternalCallRequest| {
            assert_eq!(call.tool_name, "runtime.startProposal");
            assert!(call.confirmed, "confirmation marker must be parsed from _meta");
            assert_eq!(call.source, ExternalSource::Cli);
            std::future::ready(Ok(json!({"proposalId": "p1"})))
        };
        let response = parse(
            handle_jsonrpc(
                r#"{"jsonrpc":"2.0","id":3,"method":"tools/call",
                   "params":{"name":"runtime_startProposal","arguments":{"workspaceId":1},
                             "_meta":{"gitworkspace/confirmed":true,"gitworkspace/client":"cli"}}}"#,
                &tools(),
                &executor,
            )
            .await,
        );
        assert_eq!(response["result"]["isError"], false);
    }

    /// 验收：越权 / 缺确认标记等拒绝以 isError 结果返回，带稳定错误码。
    #[tokio::test]
    async fn tools_call_domain_error_is_is_error_result_with_code() {
        let executor = |_: ExternalCallRequest| {
            std::future::ready(Err(AppError::Ai(AiError::ExternalConfirmationRequired {
                tool: "runtime.startProposal".into(),
            })))
        };
        let response = parse(
            handle_jsonrpc(
                r#"{"jsonrpc":"2.0","id":4,"method":"tools/call",
                   "params":{"name":"runtime_startProposal"}}"#,
                &tools(),
                &executor,
            )
            .await,
        );
        assert_eq!(response["result"]["isError"], true);
        let text = response["result"]["content"][0]["text"].as_str().unwrap();
        let payload: Value = serde_json::from_str(text).unwrap();
        assert_eq!(payload["error"]["code"], "AiActionConfirmationRequired");
    }

    #[tokio::test]
    async fn unknown_method_and_bad_json_are_protocol_errors() {
        let response = parse(
            handle_jsonrpc(
                r#"{"jsonrpc":"2.0","id":5,"method":"resources/list"}"#,
                &tools(),
                &ok_executor,
            )
            .await,
        );
        assert_eq!(response["error"]["code"], METHOD_NOT_FOUND);

        let response = parse(handle_jsonrpc("not json", &tools(), &ok_executor).await);
        assert_eq!(response["error"]["code"], PARSE_ERROR);
        assert!(response["id"].is_null());

        let response = parse(
            handle_jsonrpc(
                r#"{"jsonrpc":"2.0","id":6,"method":"tools/call","params":{}}"#,
                &tools(),
                &ok_executor,
            )
            .await,
        );
        assert_eq!(response["error"]["code"], INVALID_PARAMS);
    }

    #[tokio::test]
    async fn notifications_produce_no_response() {
        assert!(handle_jsonrpc(
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
            &tools(),
            &ok_executor
        )
        .await
        .is_none());
    }
}
