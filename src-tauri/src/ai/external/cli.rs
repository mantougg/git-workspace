//! AI-12：CLI Adapter——`git-workspace ai-tools ...`，供脚本与外部 Agent
//! 以命令行方式使用同一 Tool Registry。
//!
//! - `list` 离线可用：注册表是静态定义，直接映射输出（与 MCP Adapter 共用
//!   [`super::external_tool_manifest`]，保证清单一致）；
//! - `call` 经本机 MCP 端口中继到运行中的应用实例——执行与安全校验只在
//!   应用内管道发生（不维护第二套能力与安全边界，§15）；应用未运行时
//!   返回可行动错误，不做任何本地降级执行。

use serde_json::{json, Value};

use super::mcp::{META_CLIENT, META_CONFIRMED};
use super::server::{discovery_path, read_discovery};
use super::tools::registry;
use super::{external_tool_manifest, mcp_tool_name};

const EXIT_OK: i32 = 0;
/// 工具调用被拒绝 / 执行出错（含越权、缺确认标记）。
const EXIT_TOOL_ERROR: i32 = 1;
/// 用法错误 / 端点不可达等传输层问题。
const EXIT_USAGE: i32 = 2;

/// main 入口钩子：参数不是 `ai-tools` 子命令时返回 `None`（继续 GUI 启动）；
/// 否则完成 CLI 工作并返回进程退出码。
pub fn run_cli(args: Vec<String>) -> Option<i32> {
    if args.first().map(String::as_str) != Some("ai-tools") {
        return None;
    }
    // Windows release 是 windows_subsystem 二进制：先挂回父控制台，
    // 否则脚本看不到 stdout/stderr（仅 CLI 路径需要，GUI 不受影响）。
    #[cfg(windows)]
    attach_parent_console();

    Some(match args.get(1).map(String::as_str) {
        Some("list") => cmd_list(),
        Some("endpoint") => cmd_endpoint(),
        Some("call") => cmd_call(&args[2..]),
        _ => usage(),
    })
}

#[cfg(windows)]
fn attach_parent_console() {
    use windows_sys::Win32::System::Console::{AttachConsole, ATTACH_PARENT_PROCESS};
    unsafe {
        AttachConsole(ATTACH_PARENT_PROCESS);
    }
}

fn usage() -> i32 {
    eprintln!(
        "用法:\n  \
         git-workspace ai-tools list\n  \
         git-workspace ai-tools endpoint\n  \
         git-workspace ai-tools call <tool> [--args '<json>'] [--confirm] [--endpoint <url>]"
    );
    EXIT_USAGE
}

fn cmd_list() -> i32 {
    let manifest = external_tool_manifest(registry());
    println!(
        "{}",
        serde_json::to_string_pretty(&manifest).unwrap_or_else(|_| "[]".into())
    );
    EXIT_OK
}

fn cmd_endpoint() -> i32 {
    match read_discovery(&crate::get_app_data_dir()) {
        Some(info) => {
            println!(
                "{}",
                serde_json::to_string_pretty(&info).unwrap_or_else(|_| "{}".into())
            );
            EXIT_OK
        }
        None => {
            eprintln!(
                "未发现运行中的 GitWorkspace 实例（{} 不存在）。请先启动应用。",
                discovery_path().display()
            );
            EXIT_USAGE
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
struct CallOptions {
    tool_name: String,
    arguments: Value,
    confirmed: bool,
    endpoint: Option<String>,
}

fn parse_call_args(args: &[String]) -> Result<CallOptions, String> {
    let mut tool_name: Option<String> = None;
    let mut arguments = json!({});
    let mut confirmed = false;
    let mut endpoint = None;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--confirm" => confirmed = true,
            "--args" => {
                let raw = iter
                    .next()
                    .ok_or_else(|| "--args 需要一个 JSON 参数".to_string())?;
                arguments = serde_json::from_str(raw)
                    .map_err(|error| format!("--args 不是合法 JSON: {error}"))?;
                if !arguments.is_object() {
                    return Err("--args 必须是 JSON object".into());
                }
            }
            "--endpoint" => {
                endpoint = Some(
                    iter.next()
                        .ok_or_else(|| "--endpoint 需要一个 URL".to_string())?
                        .clone(),
                );
            }
            other if other.starts_with("--") => {
                return Err(format!("未知参数: {other}"));
            }
            other => {
                if tool_name.is_some() {
                    return Err(format!("多余的参数: {other}"));
                }
                tool_name = Some(other.to_string());
            }
        }
    }
    let tool_name = tool_name.ok_or_else(|| "缺少工具名（git-workspace ai-tools call <tool> ...）".to_string())?;
    Ok(CallOptions {
        tool_name,
        arguments,
        confirmed,
        endpoint,
    })
}

fn build_call_request(options: &CallOptions) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": {
            "name": mcp_tool_name(&options.tool_name),
            "arguments": options.arguments,
            "_meta": {
                META_CLIENT: "cli",
                META_CONFIRMED: options.confirmed,
            },
        },
    })
}

fn cmd_call(args: &[String]) -> i32 {
    let options = match parse_call_args(args) {
        Ok(options) => options,
        Err(message) => {
            eprintln!("{message}");
            return usage();
        }
    };
    let endpoint = match options
        .endpoint
        .clone()
        .or_else(|| read_discovery(&crate::get_app_data_dir()).map(|info| info.base_url))
    {
        Some(endpoint) => endpoint,
        None => {
            eprintln!(
                "未发现运行中的 GitWorkspace 实例。请先启动应用，或用 --endpoint <url> 指定外部端点。"
            );
            return EXIT_USAGE;
        }
    };
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("无法创建异步运行时: {error}");
            return EXIT_USAGE;
        }
    };
    runtime.block_on(post_call(&endpoint, &options))
}

async fn post_call(endpoint: &str, options: &CallOptions) -> i32 {
    let client = reqwest::Client::new();
    let response = match client
        .post(endpoint)
        .header("content-type", "application/json")
        .json(&build_call_request(options))
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await
    {
        Ok(response) => response,
        Err(error) => {
            eprintln!(
                "无法连接 GitWorkspace 外部端点 {endpoint}（{error}）。请确认应用正在运行。"
            );
            return EXIT_USAGE;
        }
    };
    if !response.status().is_success() {
        eprintln!("外部端点返回 HTTP {}", response.status());
        return EXIT_USAGE;
    }
    let body: Value = match response.json().await {
        Ok(body) => body,
        Err(error) => {
            eprintln!("外部端点响应无法解析: {error}");
            return EXIT_USAGE;
        }
    };
    if let Some(error) = body.get("error") {
        eprintln!(
            "{}",
            serde_json::to_string_pretty(error).unwrap_or_else(|_| error.to_string())
        );
        return EXIT_TOOL_ERROR;
    }
    let result = &body["result"];
    let text = result["content"][0]["text"]
        .as_str()
        .unwrap_or_default();
    if result["isError"].as_bool().unwrap_or(false) {
        eprintln!("{text}");
        EXIT_TOOL_ERROR
    } else {
        println!("{text}");
        EXIT_OK
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn non_ai_tools_args_pass_through_to_gui() {
        assert_eq!(run_cli(vec![]), None);
        assert_eq!(run_cli(args(&["--some-gui-flag"])), None);
    }

    #[test]
    fn parse_call_args_defaults_and_flags() {
        let options = parse_call_args(&args(&["workspace.list"])).unwrap();
        assert_eq!(options.tool_name, "workspace.list");
        assert_eq!(options.arguments, json!({}));
        assert!(!options.confirmed);
        assert_eq!(options.endpoint, None);

        let options = parse_call_args(&args(&[
            "repository.status",
            "--args",
            r#"{"workspaceId":1,"repoPath":"."}"#,
            "--confirm",
            "--endpoint",
            "http://127.0.0.1:39117",
        ]))
        .unwrap();
        assert!(options.confirmed);
        assert_eq!(options.arguments["workspaceId"], 1);
        assert_eq!(options.endpoint.as_deref(), Some("http://127.0.0.1:39117"));
    }

    #[test]
    fn parse_call_args_rejects_bad_input() {
        assert!(parse_call_args(&args(&[])).is_err());
        assert!(parse_call_args(&args(&["a", "b"])).is_err());
        assert!(parse_call_args(&args(&["a", "--args", "not-json"])).is_err());
        assert!(parse_call_args(&args(&["a", "--args", "[1]"])).is_err());
        assert!(parse_call_args(&args(&["a", "--unknown"])).is_err());
        assert!(parse_call_args(&args(&["a", "--args"])).is_err());
    }

    /// CLI 发往 MCP 端点的请求：工具名做适配层映射，确认标记与来源标识
    /// 走 `_meta`（与 MCP 客户端同一协议面）。
    #[test]
    fn call_request_uses_adapter_name_and_meta() {
        let options = CallOptions {
            tool_name: "runtime.startProposal".into(),
            arguments: json!({"workspaceId": 1, "runtimeName": "app"}),
            confirmed: true,
            endpoint: None,
        };
        let request = build_call_request(&options);
        assert_eq!(request["method"], "tools/call");
        assert_eq!(request["params"]["name"], "runtime_startProposal");
        assert_eq!(request["params"]["_meta"][META_CONFIRMED], true);
        assert_eq!(request["params"]["_meta"][META_CLIENT], "cli");
        assert_eq!(request["params"]["arguments"]["runtimeName"], "app");
    }
}
