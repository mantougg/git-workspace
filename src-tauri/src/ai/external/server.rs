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
use super::{external_tool_manifest, mcp, run_external_call, ExternalCallRequest, ExternalToolDescriptor};

pub const DEFAULT_PORT: u16 = 39117;
const DISCOVERY_FILE: &str = "ai-external-endpoint.json";
const MAX_HEAD_BYTES: usize = 16 * 1024;
const MAX_BODY_BYTES: usize = 1024 * 1024;
const READ_TIMEOUT: Duration = Duration::from_secs(10);

/// 写入 discovery 文件的端点信息（CLI 凭此找到运行中的应用实例）。
/// `token`（PAF-20）：per-boot 随机 Bearer token，调用方必须携带
/// `Authorization: Bearer <token>`；浏览器无法读取本地文件，凭此阻断
/// CSRF 简单请求。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalEndpointInfo {
    pub base_url: String,
    pub port: u16,
    pub pid: u32,
    pub started_at: String,
    #[serde(default)]
    pub token: String,
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
    // PAF-20：per-boot 随机 token——无 token / 错 token 的请求一律 401。
    let token = crate::crypto::secret::random_secret(32).hex;
    let info = ExternalEndpointInfo {
        base_url: format!("http://127.0.0.1:{port}"),
        port,
        pid: std::process::id(),
        started_at: chrono::Utc::now().to_rfc3339(),
        token: token.clone(),
    };
    if let Err(error) = write_discovery(&crate::get_app_data_dir(), &info) {
        log::warn!("ai external endpoint: failed to write discovery file: {error}");
    }
    log::info!("ai external endpoint listening on {}", info.base_url);
    serve(listener, context, token).await
}

async fn serve(listener: TcpListener, context: ToolContext, token: String) -> io::Result<()> {
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
        let token = token.clone();
        tokio::spawn(async move {
            let executor = move |request: ExternalCallRequest| {
                let context = context.clone();
                async move {
                    run_external_call(tools::registry(), context, request)
                        .await
                        .map(|invocation| invocation.result)
                }
            };
            if let Err(error) = handle_connection(stream, &token, tools, executor).await {
                log::warn!("ai external endpoint: connection failed: {error}");
            }
        });
    }
}

async fn handle_connection<F, Fut>(
    mut stream: TcpStream,
    token: &str,
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
    if let Some(status) = validate_request(&request, token) {
        return write_response(&mut stream, status, None).await;
    }
    match mcp::handle_jsonrpc(&request.body, &tools, &execute).await {
        Some(payload) => write_response(&mut stream, 200, Some(("application/json", payload.as_bytes()))).await,
        // Notification：202 Accepted，无响应体。
        None => write_response(&mut stream, 202, None).await,
    }
}

/// PAF-20 请求校验：全部通过返回 `None`，否则返回拒绝状态码。
/// - `Authorization: Bearer <token>` 必须匹配（401）；
/// - `Content-Type` 必须是 `application/json`（415）——浏览器 `text/plain`
///   简单请求免 preflight，靠它阻断 CSRF 副作用；
/// - `Host` 只允许本机（403，防 DNS rebinding）；带 `Origin` 的请求（浏览器）
///   只接受本机源（403）。
fn validate_request(request: &HttpRequest, token: &str) -> Option<u16> {
    let content_type = request.header("content-type").unwrap_or("");
    let media_type = content_type.split(';').next().unwrap_or("").trim().to_ascii_lowercase();
    if media_type != "application/json" {
        return Some(415);
    }

    let provided = request.header("authorization").unwrap_or("");
    let scheme_ok = provided.len() > 7 && provided[..7].eq_ignore_ascii_case("Bearer ");
    if !scheme_ok || !constant_time_eq(provided[7..].trim(), token) {
        return Some(401);
    }

    let host = request.header("host").unwrap_or("");
    let host_name = host.rsplit_once(':').map(|(h, _)| h).unwrap_or(host);
    if !host_name.eq_ignore_ascii_case("127.0.0.1") && !host_name.eq_ignore_ascii_case("localhost") {
        return Some(403);
    }

    if let Some(origin) = request.header("origin") {
        let origin = origin.trim_end_matches('/');
        let origin_host = origin
            .strip_prefix("http://")
            .or_else(|| origin.strip_prefix("https://"))
            .unwrap_or("");
        let origin_name = origin_host.rsplit_once(':').map(|(h, _)| h).unwrap_or(origin_host);
        if !origin_name.eq_ignore_ascii_case("127.0.0.1") && !origin_name.eq_ignore_ascii_case("localhost") {
            return Some(403);
        }
    }
    None
}

/// 长度无关的逐字节比较（防本机时序侧信道；token 泄露即鉴权失效）。
fn constant_time_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

struct HttpRequest {
    method: String,
    headers: Vec<(String, String)>,
    body: String,
}

impl HttpRequest {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }
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
    let (method, headers, content_length) = parse_head(&head)?;
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
        headers,
        body: String::from_utf8_lossy(&body).into_owned(),
    })
}

fn parse_head(head: &str) -> Result<(String, Vec<(String, String)>, usize), u16> {
    let mut lines = head.lines();
    let request_line = lines.next().ok_or(400u16)?;
    let method = request_line.split_whitespace().next().ok_or(400u16)?.to_string();
    let mut headers = Vec::new();
    let mut content_length = 0usize;
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            let name = name.trim().to_ascii_lowercase();
            let value = value.trim().to_string();
            if name == "content-length" {
                content_length = value.parse().map_err(|_| 400u16)?;
            }
            headers.push((name, value));
        }
    }
    Ok((method, headers, content_length))
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|window| window == needle)
}

async fn write_response(stream: &mut TcpStream, status: u16, body: Option<(&str, &[u8])>) -> io::Result<()> {
    let reason = match status {
        200 => "OK",
        202 => "Accepted",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        405 => "Method Not Allowed",
        408 => "Request Timeout",
        411 => "Length Required",
        413 => "Content Too Large",
        415 => "Unsupported Media Type",
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
    let payload = serde_json::to_string_pretty(info).map_err(|error| io::Error::new(io::ErrorKind::Other, error))?;
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
        let (method, _, len) =
            parse_head("POST /mcp HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: 42\r\n").unwrap();
        assert_eq!(method, "POST");
        assert_eq!(len, 42);
        let (_, _, len) = parse_head("POST / HTTP/1.1\r\ncontent-length: 7").unwrap();
        assert_eq!(len, 7);
        assert!(parse_head("").is_err());
        assert!(parse_head("POST / HTTP/1.1\r\nContent-Length: abc").is_err());
    }

    /// PAF-20：请求头被完整收集（小写键），供鉴权校验读取。
    #[test]
    fn parse_head_collects_headers_with_lowercase_names() {
        let (_, headers, _) =
            parse_head("POST / HTTP/1.1\r\nAuthorization: Bearer xyz\r\nContent-Type: application/json\r\n").unwrap();
        assert_eq!(headers.iter().find(|(k, _)| k == "authorization").unwrap().1, "Bearer xyz");
        assert_eq!(
            headers.iter().find(|(k, _)| k == "content-type").unwrap().1,
            "application/json"
        );
    }

    #[test]
    fn discovery_file_round_trips() {
        let dir = crate::test_support::temp_root("ai12", "discovery");
        let info = ExternalEndpointInfo {
            base_url: "http://127.0.0.1:39117".into(),
            port: 39117,
            pid: 1234,
            started_at: "2026-08-31T00:00:00Z".into(),
            token: "t".repeat(64),
        };
        write_discovery(&dir, &info).unwrap();
        let loaded = read_discovery(&dir).unwrap();
        assert_eq!(loaded.base_url, info.base_url);
        assert_eq!(loaded.port, 39117);
        assert_eq!(loaded.token, info.token);
        remove_discovery(&dir);
        assert!(read_discovery(&dir).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn http_transport_round_trips_jsonrpc_over_localhost() {
        let token = "a".repeat(64);
        let tools = Arc::new(external_tool_manifest(&tools::ToolRegistry::default()));
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let token_for_server = token.clone();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            handle_connection(stream, &token_for_server, tools, |call| {
                std::future::ready(Ok(json!({"echo": call.tool_name})))
            })
            .await
        });
        let response = reqwest::Client::new()
            .post(format!("http://127.0.0.1:{port}/"))
            .header("content-type", "application/json")
            .header("authorization", format!("Bearer {token}"))
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
        let token = "b".repeat(64);
        let tools = Arc::new(external_tool_manifest(&tools::ToolRegistry::default()));
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let token_for_server = token.clone();
        let server = tokio::spawn(async move {
            for _ in 0..2 {
                let (stream, _) = listener.accept().await.unwrap();
                let tools = Arc::clone(&tools);
                let token = token_for_server.clone();
                tokio::spawn(async move {
                    let _ = handle_connection(stream, &token, tools, |_| {
                        std::future::ready(Ok(json!({})))
                    })
                    .await;
                });
            }
        });
        let client = reqwest::Client::new();
        let notification = client
            .post(format!("http://127.0.0.1:{port}/"))
            .header("content-type", "application/json")
            .header("authorization", format!("Bearer {token}"))
            .body(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#)
            .send()
            .await
            .unwrap();
        assert_eq!(notification.status(), 202);
        let get = client.get(format!("http://127.0.0.1:{port}/")).send().await.unwrap();
        assert_eq!(get.status(), 405);
        let _ = server.await;
    }

    /// PAF-20：无 token / 错 token → 401；text/plain 简单请求（浏览器 CSRF）
    /// → 415；跨源 Origin → 403。合法请求不受影响。
    #[tokio::test]
    async fn auth_rejects_missing_wrong_token_and_cross_origin() {
        let token = "c".repeat(64);
        let tools = Arc::new(external_tool_manifest(&tools::ToolRegistry::default()));
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let token_for_server = token.clone();
        let server = tokio::spawn(async move {
            for _ in 0..4 {
                let (stream, _) = listener.accept().await.unwrap();
                let tools = Arc::clone(&tools);
                let token = token_for_server.clone();
                tokio::spawn(async move {
                    let _ = handle_connection(stream, &token, tools, |_| {
                        std::future::ready(Ok(json!({})))
                    })
                    .await;
                });
            }
        });
        let client = reqwest::Client::new();
        let url = format!("http://127.0.0.1:{port}/");
        let body = r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#;

        // 无 token → 401。
        let no_auth = client.post(&url).header("content-type", "application/json").body(body).send().await.unwrap();
        assert_eq!(no_auth.status(), 401);
        // 错 token → 401。
        let wrong = client
            .post(&url)
            .header("content-type", "application/json")
            .header("authorization", "Bearer deadbeef")
            .body(body)
            .send()
            .await
            .unwrap();
        assert_eq!(wrong.status(), 401);
        // text/plain（浏览器简单请求）→ 415。
        let plain = client
            .post(&url)
            .header("content-type", "text/plain")
            .header("authorization", format!("Bearer {token}"))
            .body(body)
            .send()
            .await
            .unwrap();
        assert_eq!(plain.status(), 415);
        // 跨源 Origin（恶意网页）→ 403。
        let evil = client
            .post(&url)
            .header("content-type", "application/json")
            .header("authorization", format!("Bearer {token}"))
            .header("origin", "https://evil.example")
            .body(body)
            .send()
            .await
            .unwrap();
        assert_eq!(evil.status(), 403);
        let _ = server.await;
    }

    /// PAF-20：per-boot token 是 64 位 hex（32 字节 OsRng），两次生成不同。
    #[test]
    fn generated_token_is_hex_and_unique() {
        let first = crate::crypto::secret::random_secret(32).hex;
        let second = crate::crypto::secret::random_secret(32).hex;
        assert_eq!(first.len(), 64);
        assert_ne!(first, second);
    }

    /// PAF-20：token 比较是长度无关的常量时间路径。
    #[test]
    fn constant_time_eq_matches_only_equal_input() {
        assert!(constant_time_eq("abc", "abc"));
        assert!(!constant_time_eq("abc", "abd"));
        assert!(!constant_time_eq("abc", "abcd"));
        assert!(!constant_time_eq("", "a"));
    }
}
