//! AI-12：MCP Adapter 的本地 HTTP 传输层。
//!
//! 仅监听 127.0.0.1，生命周期随应用启停（`spawn` 于应用 setup，
//! `shutdown` 于 RunEvent::Exit）。刻意不引入 HTTP 框架：第一阶段只有
//! 「POST JSON → JSON-RPC 响应」一条通路，用已有 tokio 直接实现，
//! 映射逻辑全部在 [`super::mcp`]，本模块只管字节收发。
//!
//! 端口：默认 [`DEFAULT_PORT`]，被占用时回退临时端口（不依赖 shell 探测，
//! 遵守平台规范）；实际端口写入应用数据目录的 discovery 文件，供 CLI
//! Adapter 与外部 MCP 客户端发现。

use std::future::Future;
use std::io;
use std::net::Ipv4Addr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use crate::error::AppResult;

use super::tools::{self, ToolContext};
use super::{
    external_tool_manifest, mcp, run_external_call, ExternalCallRequest, ExternalToolDescriptor,
};

pub const DEFAULT_PORT: u16 = 39117;
const DISCOVERY_FILE: &str = "ai-external-endpoint.json";
const MAX_HEAD_BYTES: usize = 16 * 1024;
const MAX_BODY_BYTES: usize = 1024 * 1024;
const READ_TIMEOUT: Duration = Duration::from_secs(10);

/// 写入 discovery 文件的端点信息（CLI 凭此找到运行中的应用实例）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalEndpointInfo {
    pub base_url: String,
    pub port: u16,
    pub pid: u32,
    pub started_at: String,
}

struct ServerGuard {
    task: tauri::async_runtime::JoinHandle<()>,
}

static SERVER: OnceLock<Mutex<Option<ServerGuard>>> = OnceLock::new();

/// 应用 setup 中调用：绑定端口并后台服务。失败只记日志，不影响应用启动
/// （Offline First：外部接入不可用不阻塞任何核心功能）。
pub fn spawn(context: ToolContext) {
    let task = tauri::async_runtime::spawn(async move {
        if let Err(error) = try_serve(context).await {
            log::warn!("ai external endpoint stopped: {error}");
        }
    });
    SERVER
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap()
        .replace(ServerGuard { task });
}

/// 应用退出时调用：停止服务并清理 discovery 文件，不留残留。
pub fn shutdown() {
    if let Some(guard) = SERVER.get().and_then(|slot| slot.lock().unwrap().take()) {
        guard.task.abort();
    }
    let path = discovery_path();
    if path.exists() {
        let _ = std::fs::remove_file(path);
    }
}

async fn try_serve(context: ToolContext) -> io::Result<()> {
    let listener = match TcpListener::bind((Ipv4Addr::LOCALHOST, DEFAULT_PORT)).await {
        Ok(listener) => listener,
        Err(error) => {
            log::warn!(
                "ai external endpoint: port {DEFAULT_PORT} unavailable ({error}), \
                 falling back to an ephemeral port"
            );
            TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?
        }
    };
    let port = listener.local_addr()?.port();
    let info = ExternalEndpointInfo {
        base_url: format!("http://127.0.0.1:{port}"),
        port,
        pid: std::process::id(),
        started_at: chrono::Utc::now().to_rfc3339(),
    };
    if let Err(error) = write_discovery(&crate::get_app_data_dir(), &info) {
        log::warn!("ai external endpoint: failed to write discovery file: {error}");
    }
    log::info!("ai external endpoint listening on {}", info.base_url);
    serve(listener, context).await
}

async fn serve(listener: TcpListener, context: ToolContext) -> io::Result<()> {
    let tools = Arc::new(external_tool_manifest(tools::registry()));
    loop {
        let (stream, _) = match listener.accept().await {
            Ok(pair) => pair,
            Err(error) => {
                log::warn!("ai external endpoint: accept failed: {error}");
                continue;
            }
        };
        let tools = Arc::clone(&tools);
        let context = context.clone();
        tokio::spawn(async move {
            let executor = move |request: ExternalCallRequest| {
                let context = context.clone();
                async move {
                    run_external_call(tools::registry(), context, request)
                        .await
                        .map(|invocation| invocation.result)
                }
            };
            if let Err(error) = handle_connection(stream, tools, executor).await {
                log::warn!("ai external endpoint: connection failed: {error}");
            }
        });
    }
}

async fn handle_connection<F, Fut>(
    mut stream: TcpStream,
    tools: Arc<Vec<ExternalToolDescriptor>>,
    execute: F,
) -> io::Result<()>
where
    F: Fn(ExternalCallRequest) -> Fut,
    Fut: Future<Output = AppResult<Value>>,
{
    let request = match tokio::time::timeout(READ_TIMEOUT, read_request(&mut stream)).await {
        Ok(Ok(request)) => request,
        Ok(Err(status)) => return write_response(&mut stream, status, None).await,
        Err(_) => return write_response(&mut stream, 408, None).await,
    };
    if request.method != "POST" {
        return write_response(&mut stream, 405, None).await;
    }
    match mcp::handle_jsonrpc(&request.body, &tools, &execute).await {
        Some(payload) => {
            write_response(&mut stream, 200, Some(("application/json", payload.as_bytes()))).await
        }
        // Notification：202 Accepted，无响应体。
        None => write_response(&mut stream, 202, None).await,
    }
}

struct HttpRequest {
    method: String,
    body: String,
}

async fn read_request(stream: &mut TcpStream) -> Result<HttpRequest, u16> {
    let mut buf = Vec::with_capacity(4096);
    let mut chunk = [0u8; 8192];
    let head_end = loop {
        if let Some(pos) = find_subslice(&buf, b"\r\n\r\n") {
            break pos;
        }
        if buf.len() > MAX_HEAD_BYTES {
            return Err(431);
        }
        let n = stream.read(&mut chunk).await.map_err(|_| 400u16)?;
        if n == 0 {
            return Err(400);
        }
        buf.extend_from_slice(&chunk[..n]);
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
    let (method, content_length) = parse_head(&head)?;
    if content_length > MAX_BODY_BYTES {
        return Err(413);
    }
    let mut body = buf.split_off(head_end + 4);
    while body.len() < content_length {
        let n = stream.read(&mut chunk).await.map_err(|_| 400u16)?;
        if n == 0 {
            return Err(400);
        }
        body.extend_from_slice(&chunk[..n]);
    }
    body.truncate(content_length);
    Ok(HttpRequest {
        method,
        body: String::from_utf8_lossy(&body).into_owned(),
    })
}

fn parse_head(head: &str) -> Result<(String, usize), u16> {
    let mut lines = head.lines();
    let request_line = lines.next().ok_or(400u16)?;
    let method = request_line
        .split_whitespace()
        .next()
        .ok_or(400u16)?
        .to_string();
    let mut content_length = 0usize;
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            if name.trim().eq_ignore_ascii_case("content-length") {
                content_length = value.trim().parse().map_err(|_| 400u16)?;
            }
        }
    }
    Ok((method, content_length))
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

async fn write_response(
    stream: &mut TcpStream,
    status: u16,
    body: Option<(&str, &[u8])>,
) -> io::Result<()> {
    let reason = match status {
        200 => "OK",
        202 => "Accepted",
        400 => "Bad Request",
        405 => "Method Not Allowed",
        408 => "Request Timeout",
        411 => "Length Required",
        413 => "Content Too Large",
        431 => "Request Header Fields Too Large",
        _ => "Error",
    };
    let mut head = format!("HTTP/1.1 {status} {reason}\r\nconnection: close\r\n");
    match &body {
        Some((content_type, bytes)) => {
            head.push_str(&format!(
                "content-type: {content_type}\r\ncontent-length: {}\r\n",
                bytes.len()
            ));
        }
        None => head.push_str("content-length: 0\r\n"),
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes()).await?;
    if let Some((_, bytes)) = body {
        stream.write_all(bytes).await?;
    }
    stream.flush().await
}

// ---- discovery 文件 ----

pub fn discovery_path() -> PathBuf {
    crate::get_app_data_dir().join(DISCOVERY_FILE)
}

pub fn write_discovery(dir: &Path, info: &ExternalEndpointInfo) -> io::Result<()> {
    if !dir.exists() {
        std::fs::create_dir_all(dir)?;
    }
    let payload = serde_json::to_string_pretty(info)
        .map_err(|error| io::Error::new(io::ErrorKind::Other, error))?;
    std::fs::write(dir.join(DISCOVERY_FILE), payload)
}

pub fn read_discovery(dir: &Path) -> Option<ExternalEndpointInfo> {
    let payload = std::fs::read_to_string(dir.join(DISCOVERY_FILE)).ok()?;
    serde_json::from_str(&payload).ok()
}

pub fn remove_discovery(dir: &Path) {
    let _ = std::fs::remove_file(dir.join(DISCOVERY_FILE));
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_head_extracts_method_and_content_length_case_insensitively() {
        let (method, len) =
            parse_head("POST /mcp HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: 42\r\n").unwrap();
        assert_eq!(method, "POST");
        assert_eq!(len, 42);
        let (_, len) = parse_head("POST / HTTP/1.1\r\ncontent-length: 7").unwrap();
        assert_eq!(len, 7);
        assert!(parse_head("").is_err());
        assert!(parse_head("POST / HTTP/1.1\r\nContent-Length: abc").is_err());
    }

    #[test]
    fn discovery_file_round_trips() {
        let dir = crate::test_support::temp_root("ai12", "discovery");
        let info = ExternalEndpointInfo {
            base_url: "http://127.0.0.1:39117".into(),
            port: 39117,
            pid: 1234,
            started_at: "2026-08-31T00:00:00Z".into(),
        };
        write_discovery(&dir, &info).unwrap();
        let loaded = read_discovery(&dir).unwrap();
        assert_eq!(loaded.base_url, info.base_url);
        assert_eq!(loaded.port, 39117);
        remove_discovery(&dir);
        assert!(read_discovery(&dir).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn http_transport_round_trips_jsonrpc_over_localhost() {
        let tools = Arc::new(external_tool_manifest(&tools::ToolRegistry::default()));
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            handle_connection(stream, tools, |call| {
                std::future::ready(Ok(json!({"echo": call.tool_name})))
            })
            .await
        });
        let response = reqwest::Client::new()
            .post(format!("http://127.0.0.1:{port}/"))
            .header("content-type", "application/json")
            .body(r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"workspace_list"}}"#)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        let body: Value = response.json().await.unwrap();
        assert_eq!(body["result"]["isError"], false);
        assert!(body["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("workspace.list"));
        server.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn notification_gets_202_and_get_gets_405() {
        let tools = Arc::new(external_tool_manifest(&tools::ToolRegistry::default()));
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move {
            for _ in 0..2 {
                let (stream, _) = listener.accept().await.unwrap();
                let tools = Arc::clone(&tools);
                tokio::spawn(async move {
                    let _ = handle_connection(stream, tools, |_| {
                        std::future::ready(Ok(json!({})))
                    })
                    .await;
                });
            }
        });
        let client = reqwest::Client::new();
        let notification = client
            .post(format!("http://127.0.0.1:{port}/"))
            .body(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#)
            .send()
            .await
            .unwrap();
        assert_eq!(notification.status(), 202);
        let get = client
            .get(format!("http://127.0.0.1:{port}/"))
            .send()
            .await
            .unwrap();
        assert_eq!(get.status(), 405);
        let _ = server.await;
    }
}
