//! Gateway 集成测试（AI-02 验收 / 设计文档 §18.2）。
//!
//! 用 fake transport（进程内脚本化响应）替代真实网络，对三种协议
//! 各覆盖：成功 / 流式 / 超时 / 取消 / 429 / 5xx / 非法 JSON；另覆盖
//! Preview 闸门（未确认 zero 网络调用）、自动重试与退避、v14 迁移后
//! 存量配置可用性、以及「事件/快照不含 API Key」的安全断言。

use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use rusqlite::Connection;

use super::credentials::{CredentialManager, SessionStore};
use super::events::{AiEventSink, AiRequestEvent};
use super::gateway::{AiGateway, GatewayConfig};
use super::model::AiTaskKind;
use super::model::{save_model, AiModelDefaults, ModelCapability, SaveAiModelRequest};
use super::provider::{save_provider, ApiType, NetworkPolicy, SaveAiProviderRequest};
use super::request::{AiMessage, AiRequest, AiResult, GitAssistantScenario, MessageRole, ResponseFormat, ToolPolicy};
use super::transport::{
    BoxFuture, ByteStream, CancelToken, HttpTransport, TransportError, TransportRequest, TransportResponse,
};

// ---------------------------------------------------------------------------
// Fake transport
// ---------------------------------------------------------------------------

/// 测试替身对同 crate 的其他测试模块（如 `session_tests`）开放。
pub(crate) enum Body {
    Full(String),
    /// SSE 分块（按顺序逐块送达，模拟真实流式切分）。
    Chunks(Vec<String>),
    /// 先送达给定分块，随后挂起永不产出（模拟流中卡死，用于取消测试）。
    ChunksWithStall(Vec<String>),
}

struct FakeByteStream {
    chunks: VecDeque<Vec<u8>>,
    stall: bool,
}

impl ByteStream for FakeByteStream {
    fn next_chunk<'a>(&'a mut self) -> BoxFuture<'a, std::io::Result<Option<Vec<u8>>>> {
        if self.chunks.is_empty() {
            if self.stall {
                return Box::pin(std::future::pending());
            }
            return Box::pin(async move { Ok(None) });
        }
        Box::pin(async move { Ok(self.chunks.pop_front()) })
    }
}

pub(crate) enum Step {
    /// 立即返回响应。
    Respond { status: u16, body: Body },
    /// 模拟慢响应：脚本延迟超过调用方 timeout 时直接返回 Timeout。
    SlowRespond { delay: Duration, status: u16, body: Body },
}

pub(crate) struct CapturedRequest {
    #[allow(dead_code)]
    url: String,
    #[allow(dead_code)]
    headers: Vec<(String, String)>,
    #[allow(dead_code)]
    body: Option<String>,
}

pub(crate) struct FakeTransport {
    steps: Mutex<VecDeque<Step>>,
    calls: AtomicUsize,
    requests: Mutex<Vec<CapturedRequest>>,
}

impl FakeTransport {
    pub(crate) fn new(steps: Vec<Step>) -> Self {
        Self {
            steps: Mutex::new(VecDeque::from(steps)),
            calls: AtomicUsize::new(0),
            requests: Mutex::new(Vec::new()),
        }
    }

    pub(crate) fn call_count(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

impl HttpTransport for FakeTransport {
    fn send<'a>(
        &'a self,
        request: TransportRequest,
        _cancel: &'a CancelToken,
        timeout: Duration,
    ) -> BoxFuture<'a, Result<TransportResponse, TransportError>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.requests.lock().unwrap().push(CapturedRequest {
            url: request.url.to_string(),
            headers: request.headers.clone(),
            body: request.body.as_ref().map(|b| String::from_utf8_lossy(b).into_owned()),
        });
        let step = self
            .steps
            .lock()
            .unwrap()
            .pop_front()
            .expect("fake transport: unexpected extra call");
        Box::pin(async move {
            let (delay, status, body) = match step {
                Step::Respond { status, body } => (Duration::ZERO, status, body),
                Step::SlowRespond { delay, status, body } => (delay, status, body),
            };
            if delay > timeout {
                // 与生产 reqwest 语义一致：整请求超时 → Timeout（不可重试）。
                return Err(TransportError::Timeout);
            }
            let body_stream: Box<dyn ByteStream> = match body {
                Body::Full(text) => Box::new(FakeByteStream {
                    chunks: VecDeque::from([text.into_bytes()]),
                    stall: false,
                }),
                Body::Chunks(chunks) => Box::new(FakeByteStream {
                    chunks: chunks.into_iter().map(|c| c.into_bytes()).collect(),
                    stall: false,
                }),
                Body::ChunksWithStall(chunks) => Box::new(FakeByteStream {
                    chunks: chunks.into_iter().map(|c| c.into_bytes()).collect(),
                    stall: true,
                }),
            };
            Ok(TransportResponse {
                status,
                headers: Default::default(),
                body: body_stream,
            })
        })
    }
}

/// 事件捕获（断言事件序列与安全不变量）。
#[derive(Default)]
pub(crate) struct CaptureSink {
    #[allow(dead_code)]
    events: Mutex<Vec<AiRequestEvent>>,
}

impl AiEventSink for CaptureSink {
    fn emit(&self, event: &AiRequestEvent) {
        self.events.lock().unwrap().push(event.clone());
    }
}

// ---------------------------------------------------------------------------
// 装配助手
// ---------------------------------------------------------------------------

fn open_db() -> Connection {
    let mut conn = Connection::open_in_memory().unwrap();
    crate::db::init_db(&mut conn).unwrap();
    conn
}

/// v14 迁移后的存量配置可用性（验收：存量 kind 配置经迁移后可用）：
/// 以 v13 旧 schema + `kind='ollama'` 存量行起步，跑完迁移再装配 Gateway。
fn open_db_with_migrated_legacy_provider() -> Connection {
    let mut conn = Connection::open_in_memory().unwrap();
    for sql in &crate::db::schema::MIGRATIONS[..13] {
        conn.execute_batch(sql).unwrap();
    }
    conn.execute_batch("PRAGMA user_version = 13;").unwrap();
    crate::db::apply_pragmas(&conn).unwrap();
    conn.execute(
        "INSERT INTO ai_providers (id, name, kind, base_url, credential_ref, enabled, network_policy, created_at, updated_at)
         VALUES ('p-legacy', 'Legacy', 'ollama', 'http://localhost:11434/v1', 'ai-provider:p-legacy', 1, 'localOnly', 't', 't')",
        [],
    )
    .unwrap();
    crate::db::migrate(&mut conn).unwrap();
    save_model(
        &conn,
        &SaveAiModelRequest {
            provider_id: "p-legacy".into(),
            id: "llama3".into(),
            display_name: "Llama 3".into(),
            capabilities: vec![ModelCapability::Chat, ModelCapability::StructuredOutput],
            max_context_tokens: 32000,
            defaults: AiModelDefaults::default(),
            enabled: true,
        },
    )
    .unwrap();
    conn
}

#[allow(dead_code)]
fn credentials_with_key(key: &str) -> Arc<CredentialManager> {
    credentials_for_ref("ai-provider:p1", key)
}

fn credentials_for_ref(credential_ref: &str, key: &str) -> Arc<CredentialManager> {
    let mgr = CredentialManager::with_store(Arc::new(SessionStore::new()));
    mgr.set(credential_ref, key, true).unwrap();
    Arc::new(mgr)
}

fn test_config() -> GatewayConfig {
    GatewayConfig {
        max_concurrent_requests: 2,
        request_timeout: Duration::from_secs(5),
        max_retries: 1,
        retry_backoff: Duration::from_millis(10),
        default_max_output_tokens: 512,
    }
}

fn test_gateway(config: GatewayConfig, transport: Arc<FakeTransport>) -> (Arc<AiGateway>, Arc<CaptureSink>) {
    let sink = Arc::new(CaptureSink::default());
    let gateway = Arc::new(AiGateway::new(config, transport, sink.clone()));
    (gateway, sink)
}

fn add_provider(conn: &Connection, api_type: ApiType) -> super::provider::AiProvider {
    save_provider(
        conn,
        &SaveAiProviderRequest {
            id: None,
            name: "Test Provider".into(),
            api_type,
            base_url: "https://fake.local/v1".into(),
            enabled: true,
            network_policy: NetworkPolicy::LocalOnly,
        },
    )
    .unwrap()
}

fn add_model(conn: &Connection, provider_id: &str) {
    save_model(
        conn,
        &SaveAiModelRequest {
            provider_id: provider_id.into(),
            id: "test-model".into(),
            display_name: "Test Model".into(),
            capabilities: vec![ModelCapability::Chat, ModelCapability::StructuredOutput],
            max_context_tokens: 32000,
            defaults: AiModelDefaults::default(),
            enabled: true,
        },
    )
    .unwrap();
}

fn make_request(request_id: &str, stream: bool) -> AiRequest {
    AiRequest {
        request_id: request_id.into(),
        session_id: None,
        task_kind: AiTaskKind::RuntimeDiagnostic,
        git_scenario: None,
        provider_id: None,
        model_id: None,
        system_instruction: "你是构建排障助手".into(),
        messages: vec![AiMessage {
            role: MessageRole::User,
            content: "端口占用怎么办？".into(),
        }],
        context_manifest: vec![],
        response_format: ResponseFormat::Text,
        tool_policy: ToolPolicy::Disabled,
        token_budget: 0,
        temperature: None,
        stream,
        secret_warn_confirmed: false,
        use_cache: false,
    }
}

use std::sync::Arc;

// ---------------------------------------------------------------------------
// 协议响应体（OpenAI Chat Completions / OpenAI Responses / Anthropic Messages）
// ---------------------------------------------------------------------------

fn chat_json(text: &str) -> String {
    serde_json::json!({
        "choices": [{"message": {"content": text}, "finish_reason": "stop"}],
        "usage": {"prompt_tokens": 5, "completion_tokens": 2}
    })
    .to_string()
}

fn chat_sse(pieces: &[&str]) -> Vec<String> {
    let mut out: Vec<String> = pieces
        .iter()
        .map(|t| {
            format!(
                "data: {}\n\n",
                serde_json::json!({"choices": [{"delta": {"content": t}}]}).to_string()
            )
        })
        .collect();
    out.push(format!(
        "data: {}\n\n",
        serde_json::json!({"choices": [{"delta": {}, "finish_reason": "stop"}]}).to_string()
    ));
    out.push("data: [DONE]\n\n".to_string());
    out
}

fn responses_json(text: &str) -> String {
    serde_json::json!({
        "output": [{"type": "message", "content": [{"type": "output_text", "text": text}]}],
        "status": "completed",
        "usage": {"input_tokens": 5, "output_tokens": 2}
    })
    .to_string()
}

fn responses_sse(pieces: &[&str]) -> Vec<String> {
    let mut out: Vec<String> = pieces
        .iter()
        .map(|t| {
            format!(
                "data: {}\n\n",
                serde_json::json!({"type": "response.output_text.delta", "delta": t}).to_string()
            )
        })
        .collect();
    out.push(format!(
        "data: {}\n\n",
        serde_json::json!({
            "type": "response.completed",
            "response": {"status": "completed", "usage": {"input_tokens": 5, "output_tokens": 2}}
        })
        .to_string()
    ));
    out
}

fn anthropic_json(text: &str) -> String {
    serde_json::json!({
        "content": [{"type": "text", "text": text}],
        "stop_reason": "end_turn",
        "usage": {"input_tokens": 5, "output_tokens": 2}
    })
    .to_string()
}

fn anthropic_sse(pieces: &[&str]) -> Vec<String> {
    let mut out: Vec<String> = pieces
        .iter()
        .map(|t| {
            format!(
                "event: content_block_delta\ndata: {}\n\n",
                serde_json::json!({
                    "type": "content_block_delta",
                    "delta": {"type": "text_delta", "text": t}
                })
                .to_string()
            )
        })
        .collect();
    out.push(format!(
        "event: message_delta\ndata: {}\n\n",
        serde_json::json!({
            "type": "message_delta",
            "delta": {"stop_reason": "end_turn"},
            "usage": {"output_tokens": 2}
        })
        .to_string()
    ));
    out.push("event: message_stop\ndata: {\"type\": \"message_stop\"}\n\n".to_string());
    out
}

const KEY: &str = "sk-fake-key-DO-NOT-LOG";

/// 跑通 submit → approve → wait，返回终态快照。
async fn run_to_end(
    gateway: &Arc<AiGateway>,
    conn: &Connection,
    credentials: &Arc<CredentialManager>,
    request: AiRequest,
) -> super::gateway::AiRequestSnapshot {
    let id = request.request_id.clone();
    gateway.submit(conn, request).expect("submit ok");
    gateway.approve(credentials.clone(), &id).expect("approve ok");
    gateway
        .wait(&id, Duration::from_secs(10))
        .await
        .expect("request reaches terminal state")
}

// ---------------------------------------------------------------------------
// 三协议 × 成功 / 流式
// ---------------------------------------------------------------------------

#[tokio::test]
async fn three_protocols_complete_success() {
    let cases: Vec<(ApiType, String, String)> = vec![
        (
            ApiType::OpenaiChatCompletions,
            "chat/completions".into(),
            chat_json("hello"),
        ),
        (ApiType::OpenaiResponses, "responses".into(), responses_json("hello")),
        (ApiType::AnthropicMessages, "messages".into(), anthropic_json("hello")),
    ];
    for (api_type, endpoint, body) in cases {
        let conn = open_db();
        let provider = add_provider(&conn, api_type);
        add_model(&conn, &provider.id);
        let transport = Arc::new(FakeTransport::new(vec![Step::Respond {
            status: 200,
            body: Body::Full(body),
        }]));
        let (gateway, _sink) = test_gateway(test_config(), transport.clone());
        let credentials = credentials_for_ref(provider.credential_ref.as_deref().unwrap(), KEY);

        let snapshot = run_to_end(&gateway, &conn, &credentials, make_request("r", false)).await;
        assert_eq!(
            snapshot.phase,
            super::lifecycle::RequestPhase::Succeeded,
            "{:?}",
            snapshot
        );
        assert!(matches!(
            snapshot.result,
            Some(super::request::AiResult::Answer { ref text }) if text == "hello"
        ));
        assert_eq!(snapshot.usage.and_then(|u| u.output_tokens), Some(2));
        assert_eq!(transport.call_count(), 1);

        // URL 结构化拼接 + 认证头按协议差异（§7.2）
        let req = &transport.requests.lock().unwrap()[0];
        assert!(req.url.ends_with(endpoint.as_str()), "{}", req.url);
        if api_type == ApiType::AnthropicMessages {
            assert!(req.headers.iter().any(|(k, v)| k == "x-api-key" && v == KEY));
            assert!(req.headers.iter().any(|(k, _v)| k == "anthropic-version"));
            assert!(!req.headers.iter().any(|(k, _)| k == "Authorization"));
        } else {
            assert!(req
                .headers
                .iter()
                .any(|(k, v)| k == "Authorization" && v.as_str() == format!("Bearer {}", KEY)));
        }
    }
}

#[tokio::test]
async fn three_protocols_stream_success() {
    let cases: Vec<(ApiType, Vec<String>, Vec<&str>)> = vec![
        (ApiType::OpenaiChatCompletions, chat_sse(&["hel", "lo"]), vec![]),
        (ApiType::OpenaiResponses, responses_sse(&["hel", "lo"]), vec![]),
        (ApiType::AnthropicMessages, anthropic_sse(&["hel", "lo"]), vec![]),
    ];
    for (api_type, chunks, _) in cases {
        let conn = open_db();
        let provider = add_provider(&conn, api_type);
        add_model(&conn, &provider.id);
        let transport = Arc::new(FakeTransport::new(vec![Step::Respond {
            status: 200,
            body: Body::Chunks(chunks),
        }]));
        let (gateway, sink) = test_gateway(test_config(), transport.clone());
        let credentials = credentials_for_ref(provider.credential_ref.as_deref().unwrap(), KEY);

        let snapshot = run_to_end(&gateway, &conn, &credentials, make_request("rs", true)).await;
        assert_eq!(
            snapshot.phase,
            super::lifecycle::RequestPhase::Succeeded,
            "{:?}",
            snapshot
        );
        assert!(matches!(
            snapshot.result,
            Some(super::request::AiResult::Answer { ref text }) if text == "hello"
        ));
        // 流式 usage 取决于协议是否在终止事件中回传（chat finish chunk 无 usage）。

        // 流式事件：Streaming 阶段携带 textDelta chunk，且不每 token 一次
        // 事件膨胀（这里两段 delta → 至少 2 个 chunk 事件，少于 5 个字符数）。
        let events = sink.events.lock().unwrap();
        let deltas: Vec<&AiRequestEvent> = events
            .iter()
            .filter(|e| e.phase == super::lifecycle::RequestPhase::Streaming)
            .collect();
        assert!(deltas.len() >= 2, "应推送多个流式 chunk 事件");
        assert!(deltas
            .iter()
            .any(|e| matches!(e.chunk, Some(super::events::AiStreamChunk::TextDelta { .. }))));
        assert!(deltas
            .iter()
            .any(|e| matches!(e.chunk, Some(super::events::AiStreamChunk::End { .. }))));
        assert!(events
            .iter()
            .any(|e| e.phase == super::lifecycle::RequestPhase::Succeeded));
    }
}

// ---------------------------------------------------------------------------
// 429 / 5xx / 非法 JSON / 超时
// ---------------------------------------------------------------------------

/// 429 后自动重试至多 1 次、退避生效（§7.4）——三协议各覆盖一遍
/// （状态码归一化在共享链路，重试语义协议无关）。
#[tokio::test]
async fn rate_limited_retries_once_then_succeeds_with_backoff() {
    for api_type in [
        ApiType::OpenaiChatCompletions,
        ApiType::OpenaiResponses,
        ApiType::AnthropicMessages,
    ] {
        let conn = open_db();
        let provider = add_provider(&conn, api_type);
        add_model(&conn, &provider.id);
        let transport = Arc::new(FakeTransport::new(vec![
            Step::Respond {
                status: 429,
                body: Body::Full(r#"{"error":{"code":"rate_limit"}}"#.into()),
            },
            Step::Respond {
                status: 200,
                body: Body::Full(match api_type {
                    ApiType::OpenaiChatCompletions => chat_json("recovered"),
                    ApiType::OpenaiResponses => responses_json("recovered"),
                    ApiType::AnthropicMessages => anthropic_json("recovered"),
                }),
            },
        ]));
        let (gateway, _sink) = test_gateway(test_config(), transport.clone());
        let credentials = credentials_for_ref(provider.credential_ref.as_deref().unwrap(), KEY);

        let started = std::time::Instant::now();
        let snapshot = run_to_end(
            &gateway,
            &conn,
            &credentials,
            make_request(&format!("r429-{:?}", api_type), false),
        )
        .await;
        assert_eq!(
            snapshot.phase,
            super::lifecycle::RequestPhase::Succeeded,
            "{:?}: {:?}",
            api_type,
            snapshot
        );
        assert_eq!(snapshot.attempts, 2, "{:?}: 429 后自动重试 1 次", api_type);
        assert_eq!(transport.call_count(), 2);
        // 退避生效：首次重试前等待 retry_backoff × 1。
        assert!(
            started.elapsed() >= test_config().retry_backoff,
            "{:?}: 退避未生效",
            api_type
        );
    }
}

/// 5xx 自动重试至多 1 次后失败（§7.4）——三协议各覆盖一遍。
#[tokio::test]
async fn server_error_retries_then_fails() {
    for api_type in [
        ApiType::OpenaiChatCompletions,
        ApiType::OpenaiResponses,
        ApiType::AnthropicMessages,
    ] {
        let conn = open_db();
        let provider = add_provider(&conn, api_type);
        add_model(&conn, &provider.id);
        let transport = Arc::new(FakeTransport::new(vec![
            Step::Respond {
                status: 503,
                body: Body::Full("boom".into()),
            },
            Step::Respond {
                status: 500,
                body: Body::Full("boom".into()),
            },
        ]));
        let (gateway, _sink) = test_gateway(test_config(), transport.clone());
        let credentials = credentials_for_ref(provider.credential_ref.as_deref().unwrap(), KEY);

        let snapshot = run_to_end(
            &gateway,
            &conn,
            &credentials,
            make_request(&format!("r5xx-{:?}", api_type), false),
        )
        .await;
        assert_eq!(snapshot.phase, super::lifecycle::RequestPhase::Failed, "{:?}", snapshot);
        assert_eq!(snapshot.error_code, Some("AiProviderUnavailable".into()));
        assert_eq!(snapshot.attempts, 2, "{:?}: 5xx 自动重试至多 1 次", api_type);
        assert_eq!(transport.call_count(), 2);
    }
}

/// 非法 JSON 响应 → AiResponseInvalid 且不重试（§18.2）——三协议各覆盖一遍。
#[tokio::test]
async fn invalid_json_fails_without_retry() {
    for api_type in [
        ApiType::OpenaiChatCompletions,
        ApiType::OpenaiResponses,
        ApiType::AnthropicMessages,
    ] {
        let conn = open_db();
        let provider = add_provider(&conn, api_type);
        add_model(&conn, &provider.id);
        let transport = Arc::new(FakeTransport::new(vec![Step::Respond {
            status: 200,
            body: Body::Full("<html>not json</html>".into()),
        }]));
        let (gateway, _sink) = test_gateway(test_config(), transport.clone());
        let credentials = credentials_for_ref(provider.credential_ref.as_deref().unwrap(), KEY);

        let snapshot = run_to_end(
            &gateway,
            &conn,
            &credentials,
            make_request(&format!("rbad-{:?}", api_type), false),
        )
        .await;
        assert_eq!(snapshot.phase, super::lifecycle::RequestPhase::Failed, "{:?}", snapshot);
        assert_eq!(snapshot.error_code, Some("AiResponseInvalid".into()));
        assert_eq!(transport.call_count(), 1, "协议违规不自动重试");
    }
}

/// 超时不自动重试（§7.4：长请求翻倍等待只会更糟）——三协议各覆盖一遍。
#[tokio::test]
async fn timeout_fails_without_retry() {
    for api_type in [
        ApiType::OpenaiChatCompletions,
        ApiType::OpenaiResponses,
        ApiType::AnthropicMessages,
    ] {
        let conn = open_db();
        let provider = add_provider(&conn, api_type);
        add_model(&conn, &provider.id);
        let transport = Arc::new(FakeTransport::new(vec![Step::SlowRespond {
            delay: Duration::from_secs(10),
            status: 200,
            body: Body::Full("too late".into()),
        }]));
        let (gateway, _sink) = test_gateway(test_config(), transport.clone());
        let credentials = credentials_for_ref(provider.credential_ref.as_deref().unwrap(), KEY);

        let snapshot = run_to_end(
            &gateway,
            &conn,
            &credentials,
            make_request(&format!("rto-{:?}", api_type), false),
        )
        .await;
        assert_eq!(snapshot.phase, super::lifecycle::RequestPhase::Failed, "{:?}", snapshot);
        assert_eq!(snapshot.error_code, Some("AiProviderUnavailable".into()));
        assert!(!snapshot.error.unwrap().is_empty());
        assert_eq!(transport.call_count(), 1, "{:?}: 超时不自动重试", api_type);
    }
}

// ---------------------------------------------------------------------------
// 取消与 Preview 闸门
// ---------------------------------------------------------------------------

#[tokio::test]
async fn cancel_before_approve_makes_zero_network_calls() {
    let conn = open_db();
    let provider = add_provider(&conn, ApiType::OpenaiChatCompletions);
    add_model(&conn, &provider.id);
    // 不给任何脚本步骤：一旦有网络调用测试立即 panic（fake transport extra call）。
    let transport = Arc::new(FakeTransport::new(vec![]));
    let (gateway, _sink) = test_gateway(test_config(), transport.clone());

    let request = make_request("rcancel", true);
    let id = request.request_id.clone();
    let snapshot = gateway.submit(&conn, request).expect("submit ok");
    assert_eq!(snapshot.phase, super::lifecycle::RequestPhase::PreviewRequired);

    // Preview 未确认 → 不允许任何网络请求（§7.3 闸门）。
    assert_eq!(transport.call_count(), 0);

    let cancelled = gateway.cancel(&id).expect("cancel ok");
    assert_eq!(cancelled.phase, super::lifecycle::RequestPhase::Cancelled);
    assert_eq!(transport.call_count(), 0, "取消后依然 zero 网络调用");
}

#[tokio::test]
async fn cancel_mid_stream_interrupts_response() {
    // 仅送达一个 delta 分块后流挂起（模拟 Provider 卡死），取消必须能中断（§7.2）。
    // 三协议各覆盖一遍（SSE 泵与归一化通道为共享链路，事件形状协议各异）。
    for api_type in [
        ApiType::OpenaiChatCompletions,
        ApiType::OpenaiResponses,
        ApiType::AnthropicMessages,
    ] {
        let conn = open_db();
        let provider = add_provider(&conn, api_type);
        add_model(&conn, &provider.id);
        let delta_only = match api_type {
            ApiType::OpenaiChatCompletions => vec![chat_sse(&["hel"]).into_iter().next().unwrap()],
            ApiType::OpenaiResponses => vec![responses_sse(&["hel"]).into_iter().next().unwrap()],
            ApiType::AnthropicMessages => {
                vec![anthropic_sse(&["hel"]).into_iter().next().unwrap()]
            }
        };
        let transport = Arc::new(FakeTransport::new(vec![Step::Respond {
            status: 200,
            body: Body::ChunksWithStall(delta_only),
        }]));
        let (gateway, sink) = test_gateway(test_config(), transport.clone());

        let request = make_request(&format!("rstall-{:?}", api_type), true);
        let id = request.request_id.clone();
        gateway.submit(&conn, request).expect("submit ok");
        gateway
            .approve(
                credentials_for_ref(provider.credential_ref.as_deref().unwrap(), KEY),
                &id,
            )
            .expect("approve ok");

        // 等 Streaming 出现后取消。
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            let phase = gateway.status(&id).unwrap().phase;
            if phase == super::lifecycle::RequestPhase::Streaming {
                break;
            }
            assert!(std::time::Instant::now() < deadline, "never reached streaming");
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        gateway.cancel(&id).expect("cancel ok");

        let ended = gateway.wait(&id, Duration::from_secs(5)).await.unwrap();
        assert_eq!(ended.phase, super::lifecycle::RequestPhase::Cancelled, "{:?}", api_type);
        assert_eq!(transport.call_count(), 1, "取消中断后不再发起新请求");

        // 已接收的输出字符数保留在快照中（诊断用），但不产生结果。
        assert!(ended.result.is_none());
        let _ = sink;
    }
}

#[tokio::test]
async fn approve_is_the_only_network_entry() {
    let conn = open_db();
    let provider = add_provider(&conn, ApiType::OpenaiChatCompletions);
    add_model(&conn, &provider.id);
    let transport = Arc::new(FakeTransport::new(vec![Step::Respond {
        status: 200,
        body: Body::Full(chat_json("ok")),
    }]));
    let (gateway, _sink) = test_gateway(test_config(), transport.clone());

    let request = make_request("rgate", false);
    let id = request.request_id.clone();
    gateway.submit(&conn, request).expect("submit ok");
    assert_eq!(transport.call_count(), 0, "submit 阶段 zero 网络调用");

    let credentials = credentials_for_ref(provider.credential_ref.as_deref().unwrap(), KEY);
    gateway.approve(credentials.clone(), &id).expect("approve ok");
    let ended = gateway.wait(&id, Duration::from_secs(5)).await.unwrap();
    assert_eq!(ended.phase, super::lifecycle::RequestPhase::Succeeded);
    assert_eq!(transport.call_count(), 1, "approve 是唯一联网入口");

    // 已终态请求再次 approve 必须被拒绝（不能重复执行）。
    let again = gateway.approve(credentials, &id);
    assert!(again.is_err(), "重复 approve 必须被拒绝");
    assert_eq!(transport.call_count(), 1, "拒绝后无新增网络调用");
}

/// AI-08：fake Provider 的 JSON 结果经既有 Gateway/Preview 闸门按场景解析，
/// 不创建第二套 HTTP 调用链。
#[tokio::test]
async fn git_scenario_uses_gateway_and_parses_structured_review() {
    let conn = open_db();
    let provider = add_provider(&conn, ApiType::OpenaiChatCompletions);
    add_model(&conn, &provider.id);
    let transport = Arc::new(FakeTransport::new(vec![Step::Respond {
        status: 200,
        body: Body::Full(chat_json(r#"{"summary":"reviewed","issues":[]}"#)),
    }]));
    let (gateway, _sink) = test_gateway(test_config(), transport.clone());
    let credentials = credentials_for_ref(provider.credential_ref.as_deref().unwrap(), KEY);
    let mut request = make_request("git-scenario", false);
    request.task_kind = AiTaskKind::GitReview;
    request.git_scenario = Some(GitAssistantScenario::SecurityReview);
    request.response_format = ResponseFormat::Json;

    let snapshot = run_to_end(&gateway, &conn, &credentials, request).await;
    assert_eq!(snapshot.phase, super::lifecycle::RequestPhase::Succeeded);
    assert!(matches!(snapshot.result, Some(AiResult::ReviewReport { .. })));
    assert_eq!(transport.call_count(), 1);
}

// ---------------------------------------------------------------------------
// 迁移存量配置可用性 + 安全断言
// ---------------------------------------------------------------------------

#[tokio::test]
async fn migrated_legacy_provider_config_works() {
    let conn = open_db_with_migrated_legacy_provider();
    let transport = Arc::new(FakeTransport::new(vec![Step::Respond {
        status: 200,
        body: Body::Full(chat_json("ok from migrated")),
    }]));
    let (gateway, _sink) = test_gateway(test_config(), transport.clone());
    let credentials = {
        let mgr = CredentialManager::with_store(Arc::new(SessionStore::new()));
        mgr.set("ai-provider:p-legacy", KEY, true).unwrap();
        Arc::new(mgr)
    };

    let snapshot = run_to_end(&gateway, &conn, &credentials, make_request("rmig", false)).await;
    assert_eq!(
        snapshot.phase,
        super::lifecycle::RequestPhase::Succeeded,
        "{:?}",
        snapshot
    );
    assert_eq!(snapshot.provider_id, "p-legacy");
}

/// 安全断言（§4 / §16.3）：事件与快照全程不携带 API Key。
#[tokio::test]
async fn api_key_never_appears_in_events_or_snapshots() {
    let conn = open_db();
    let provider = add_provider(&conn, ApiType::AnthropicMessages);
    add_model(&conn, &provider.id);
    let transport = Arc::new(FakeTransport::new(vec![Step::Respond {
        status: 200,
        body: Body::Chunks(anthropic_sse(&["secret ", "safe"])),
    }]));
    let (gateway, sink) = test_gateway(test_config(), transport.clone());
    let credentials = credentials_for_ref(provider.credential_ref.as_deref().unwrap(), KEY);

    let snapshot = run_to_end(&gateway, &conn, &credentials, make_request("rsec", true)).await;
    assert_eq!(snapshot.phase, super::lifecycle::RequestPhase::Succeeded);
    let snapshot_json = serde_json::to_string(&snapshot).unwrap();
    assert!(!snapshot_json.contains(KEY), "快照不得包含 API Key");
    for e in sink.events.lock().unwrap().iter() {
        let json = serde_json::to_string(e).unwrap();
        assert!(!json.contains(KEY), "事件不得包含 API Key: {}", json);
    }
    // 审计走查：错误/快照只含 provider/model id 与归一化网络错误（§16.3）。
    assert!(snapshot.error.is_none());
}

/// §18.2 / AI-03 验收：请求内容含 AWS Key / JWT / 私钥 / 密码 / Token 时，
/// submit（Preview 闸门前）默认阻断为 `AiSecretDetected`，零网络调用；
/// secretWarnConfirmed 仅在用户明确确认 Warn 后放行（§10.2）。
#[test]
fn submit_blocks_high_risk_secrets_by_default() {
    let conn = open_db();
    let provider = add_provider(&conn, ApiType::OpenaiChatCompletions);
    add_model(&conn, &provider.id);
    // 无响应脚本：断言期间任何网络调用都会 panic。
    let transport = Arc::new(FakeTransport::new(vec![]));
    let (gateway, _sink) = test_gateway(test_config(), transport.clone());

    let secrets = [
        ("aws", "const key = \"AKIAIOSFODNN7EXAMPLE\";"),
        (
            "jwt",
            "token=eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.dozjgNryP4J3jVmNHl0w5N_XgL0n3I9PlFUP0THsR8U",
        ),
        ("private-key", "-----BEGIN RSA PRIVATE KEY-----\nMII..."),
        ("password", "password=supersecret123"),
        ("token", "ghp_abcdefghijklmnopqrstuvwxyz0123456789"),
    ];
    for (name, content) in secrets {
        let mut req = make_request("req-secret", false);
        req.messages = vec![AiMessage {
            role: MessageRole::User,
            content: content.into(),
        }];
        let err = gateway.submit(&conn, req).unwrap_err();
        match err {
            crate::error::AppError::Ai(super::error::AiError::SecretDetected { kinds }) => {
                assert!(!kinds.is_empty(), "{name} 阻断必须携带类别");
            }
            other => panic!("{name} 应以 AiSecretDetected 阻断，实际: {other:?}"),
        }
        // 被拒绝的请求停在 Rejected 终态。
        let snapshot = gateway.status("req-secret").unwrap();
        assert_eq!(snapshot.phase, super::lifecycle::RequestPhase::Rejected, "{name}");
    }
    assert_eq!(transport.call_count(), 0, "阻断发生在任何网络调用之前");

    // Warn 显式确认后放行（进入 PreviewRequired，不联网）。
    let mut req = make_request("req-warn", false);
    req.messages = vec![AiMessage {
        role: MessageRole::User,
        content: "password=supersecret123".into(),
    }];
    req.secret_warn_confirmed = true;
    let snapshot = gateway.submit(&conn, req).unwrap();
    assert_eq!(snapshot.phase, super::lifecycle::RequestPhase::PreviewRequired);
    assert_eq!(transport.call_count(), 0);
}

// ---------------------------------------------------------------------------
// PAF-12：records 容量有界
// ---------------------------------------------------------------------------

#[tokio::test]
async fn terminal_records_are_capacity_bounded() {
    let conn = open_db();
    let provider = add_provider(&conn, ApiType::OpenaiChatCompletions);
    add_model(&conn, &provider.id);
    // 不联网：submit 后直接 cancel（PreviewRequired → Cancelled 终态）。
    let transport = Arc::new(FakeTransport::new(vec![]));
    let (gateway, _sink) = test_gateway(test_config(), transport.clone());

    // 提交并取消 141 个请求（全部终态）：容量 128，淘汰在每次插入时触发
    // （i=129..140 共 12 轮）→ 最旧 12 条（rcap0000..rcap0011）被淘汰。
    let total = 141;
    for i in 0..total {
        let request = make_request(&format!("rcap{i:04}"), false);
        let id = request.request_id.clone();
        gateway.submit(&conn, request).expect("submit ok");
        gateway.cancel(&id).expect("cancel ok");
    }

    // 最旧的 12 条已被淘汰，快照不可达。
    assert!(
        gateway.status("rcap0000").is_none(),
        "oldest terminal record must be evicted (PAF-12)"
    );
    assert!(gateway.status("rcap0011").is_none());
    // 容量内的最近记录仍可读（UI 轮询行为不回归）。
    assert!(
        gateway.status("rcap0140").is_some(),
        "recent terminal records must remain readable"
    );
    assert_eq!(transport.call_count(), 0, "全程零网络调用");
}

// ---------------------------------------------------------------------------
// 真实 API 端到端测试（需要网络 + 有效 API Key）
// ---------------------------------------------------------------------------

/// 用真实 HTTP Transport 跑通 Gateway submit → approve → wait 全链路。
/// 仅在 `RUN_REAL_API_TEST=1` 环境变量下运行，避免 CI 误触。
mod real_api {
    use super::*;
    use std::sync::Arc;
    use std::time::Duration;

    use rusqlite::Connection;

    use super::super::credentials::{CredentialManager, SessionStore};
    use super::super::gateway::{AiGateway, GatewayConfig};
    use super::super::model::{save_model, AiModelDefaults, AiTaskKind, ModelCapability, SaveAiModelRequest};
    use super::super::provider::{save_provider, ApiType, NetworkPolicy, SaveAiProviderRequest};
    use super::super::request::{AiMessage, AiRequest, AiResult, GitAssistantScenario, MessageRole, ResponseFormat, ToolPolicy};
    use super::super::transport::ReqwestTransport;

    const API_BASE: &str = "https://token-plan-cn.xiaomimimo.com/v1";
    const MODEL_ID: &str = "mimo-v2.5-pro";

    fn should_run() -> bool {
        std::env::var("RUN_REAL_API_TEST").unwrap_or_default() == "1"
    }

    fn api_key() -> String {
        std::env::var("AI_TEST_API_KEY").expect(
            "AI_TEST_API_KEY environment variable must be set when RUN_REAL_API_TEST=1",
        )
    }

    fn real_db() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        crate::db::init_db(&mut conn).unwrap();
        conn
    }

    fn real_provider(conn: &Connection) -> super::super::provider::AiProvider {
        save_provider(
            conn,
            &SaveAiProviderRequest {
                id: None,
                name: "MiMo".into(),
                api_type: ApiType::OpenaiChatCompletions,
                base_url: API_BASE.into(),
                enabled: true,
                network_policy: NetworkPolicy::OnlineOnly,
            },
        )
        .unwrap()
    }

    fn real_model(conn: &Connection, provider_id: &str) {
        save_model(
            conn,
            &SaveAiModelRequest {
                provider_id: provider_id.into(),
                id: MODEL_ID.into(),
                display_name: "MiMo v2.5 Pro".into(),
                capabilities: vec![ModelCapability::Chat, ModelCapability::StructuredOutput],
                max_context_tokens: 32000,
                defaults: AiModelDefaults::default(),
                enabled: true,
            },
        )
        .unwrap();
    }

    fn real_credentials(provider_id: &str) -> Arc<CredentialManager> {
        let mgr = CredentialManager::with_store(Arc::new(SessionStore::new()));
        let credential_ref = format!("ai-provider:{}", provider_id);
        mgr.set(&credential_ref, &api_key(), true).unwrap();
        Arc::new(mgr)
    }

    fn real_config() -> GatewayConfig {
        GatewayConfig {
            max_concurrent_requests: 2,
            request_timeout: Duration::from_secs(120),
            max_retries: 3,
            retry_backoff: Duration::from_secs(3),
            default_max_output_tokens: 1024,
        }
    }

    fn real_request(request_id: &str, stream: bool) -> AiRequest {
        AiRequest {
            request_id: request_id.into(),
            session_id: None,
            task_kind: AiTaskKind::Chat,
            git_scenario: None,
            provider_id: None,
            model_id: None,
            system_instruction: "You are a helpful assistant. Reply concisely.".into(),
            messages: vec![AiMessage {
                role: MessageRole::User,
                content: "What is 2+2? Reply with just the number.".into(),
            }],
            context_manifest: vec![],
            response_format: ResponseFormat::Text,
            tool_policy: ToolPolicy::Disabled,
            token_budget: 0,
            temperature: Some(0.1),
            stream,
            secret_warn_confirmed: false,
            use_cache: false,
        }
    }

    /// 非流式：submit → approve → wait → 断言结果包含 "4"。
    #[tokio::test]
    async fn real_non_streaming_end_to_end() {
        if !should_run() {
            eprintln!("SKIP: set RUN_REAL_API_TEST=1 to run real API tests");
            return;
        }
        let conn = real_db();
        let provider = real_provider(&conn);
        real_model(&conn, &provider.id);
        let credentials = real_credentials(&provider.id);

        let transport = Arc::new(ReqwestTransport::new().expect("reqwest transport"));
        let sink = Arc::new(CaptureSink::default());
        let gateway = Arc::new(AiGateway::new(real_config(), transport, sink.clone()));

        let request = real_request("real-e2e-001", false);
        let id = request.request_id.clone();

        // submit: 零网络，停在 PreviewRequired
        let snap = gateway.submit(&conn, request).expect("submit should succeed");
        assert_eq!(snap.phase, super::super::lifecycle::RequestPhase::PreviewRequired);
        assert_eq!(snap.request_id, id);

        // approve: 触发网络请求
        let snap = gateway.approve(credentials, &id).expect("approve should succeed");
        assert!(matches!(
            snap.phase,
            super::super::lifecycle::RequestPhase::UserApproved
                | super::super::lifecycle::RequestPhase::Queued
                | super::super::lifecycle::RequestPhase::Sending
        ));

        // wait: 等待终态
        let result = gateway
            .wait(&id, Duration::from_secs(60))
            .await
            .expect("should reach terminal state");

        assert_eq!(
            result.phase,
            super::super::lifecycle::RequestPhase::Succeeded,
            "request should succeed, error: {:?}",
            result.error
        );
        assert!(!result.from_cache, "first request should not be cached");

        // 验证结果内容
        let ai_result = result.result.expect("should have result");
        match &ai_result {
            AiResult::Answer { text } => {
                assert!(text.contains("4"), "answer should contain '4', got: {}", text);
                println!("[REAL TEST] Non-streaming result: {}", text);
            }
            AiResult::GeneratedText { text } => {
                assert!(text.contains("4"), "answer should contain '4', got: {}", text);
                println!("[REAL TEST] Non-streaming result: {}", text);
            }
            other => {
                println!("[REAL TEST] Unexpected result type (but valid): {:?}", other);
            }
        }

        // 验证 usage
        let usage = result.usage.expect("should have usage");
        assert!(
            usage.input_tokens.unwrap_or(0) > 0,
            "input_tokens should be > 0"
        );
        assert!(
            usage.output_tokens.unwrap_or(0) > 0,
            "output_tokens should be > 0"
        );
        println!(
            "[REAL TEST] Usage: input={}, output={}",
            usage.input_tokens.unwrap_or(0),
            usage.output_tokens.unwrap_or(0)
        );
    }

    /// 流式：submit → approve → wait → 断言结果非空 + 事件序列合理。
    #[tokio::test]
    async fn real_streaming_end_to_end() {
        if !should_run() {
            eprintln!("SKIP: set RUN_REAL_API_TEST=1 to run real API tests");
            return;
        }
        let conn = real_db();
        let provider = real_provider(&conn);
        real_model(&conn, &provider.id);
        let credentials = real_credentials(&provider.id);

        let transport = Arc::new(ReqwestTransport::new().expect("reqwest transport"));
        let sink = Arc::new(CaptureSink::default());
        let gateway = Arc::new(AiGateway::new(real_config(), transport, sink.clone()));

        let request = real_request("real-stream-001", true);
        let id = request.request_id.clone();

        gateway.submit(&conn, request).expect("submit should succeed");
        gateway.approve(credentials, &id).expect("approve should succeed");

        let result = gateway
            .wait(&id, Duration::from_secs(60))
            .await
            .expect("should reach terminal state");

        assert_eq!(
            result.phase,
            super::super::lifecycle::RequestPhase::Succeeded,
            "streaming request should succeed, error: {:?}",
            result.error
        );

        let ai_result = result.result.expect("should have result");
        match &ai_result {
            AiResult::Answer { text } => {
                assert!(!text.is_empty(), "streamed text should not be empty");
                assert!(text.contains("4"), "answer should contain '4', got: {}", text);
                println!("[REAL TEST] Streaming result: {}", text);
            }
            AiResult::GeneratedText { text } => {
                assert!(!text.is_empty(), "streamed text should not be empty");
                assert!(text.contains("4"), "answer should contain '4', got: {}", text);
                println!("[REAL TEST] Streaming result: {}", text);
            }
            other => {
                println!("[REAL TEST] Unexpected streaming result type: {:?}", other);
            }
        }

        // 流式事件：至少应有 TextDelta + End
        let events = sink.events.lock().unwrap();
        let has_text_delta = events.iter().any(|e| matches!(
            e.chunk,
            Some(super::super::events::AiStreamChunk::TextDelta { .. })
        ));
        let has_end = events.iter().any(|e| matches!(
            e.chunk,
            Some(super::super::events::AiStreamChunk::End { .. })
        ));
        assert!(has_text_delta, "should have received TextDelta events");
        assert!(has_end, "should have received End event");
        println!("[REAL TEST] Stream events count: {}", events.len());
    }

    /// CommitMessage 场景：模拟 Git diff 上下文，验证结构化输出。
    #[tokio::test]
    async fn real_commit_message_scenario() {
        if !should_run() {
            eprintln!("SKIP: set RUN_REAL_API_TEST=1 to run real API tests");
            return;
        }
        let conn = real_db();
        let provider = real_provider(&conn);
        real_model(&conn, &provider.id);
        let credentials = real_credentials(&provider.id);

        let transport = Arc::new(ReqwestTransport::new().expect("reqwest transport"));
        let sink = Arc::new(CaptureSink::default());
        let gateway = Arc::new(AiGateway::new(real_config(), transport, sink.clone()));

        let request = AiRequest {
            request_id: "real-commit-001".into(),
            session_id: None,
            task_kind: AiTaskKind::GitReview,
            git_scenario: Some(super::super::request::GitAssistantScenario::CommitMessage),
            provider_id: None,
            model_id: None,
            system_instruction: String::new(),
            messages: vec![AiMessage {
                role: MessageRole::User,
                content: "Generate a commit message for this diff:\n\n\
                    diff --git a/src/main.rs b/src/main.rs\n\
                    index abc1234..def5678 100644\n\
                    --- a/src/main.rs\n\
                    +++ b/src/main.rs\n\
                    @@ -10,3 +10,4 @@ fn main() {\n\
                     \x20    println!(\"Hello\");\n\
                    +    println!(\"World\");\n\
                     \x20}\n\
                    \n\
                    Reply in JSON with fields: title, body, type, scope, rationale."
                    .into(),
            }],
            context_manifest: vec![],
            response_format: ResponseFormat::Json,
            tool_policy: ToolPolicy::Disabled,
            token_budget: 0,
            temperature: Some(0.3),
            stream: false,
            secret_warn_confirmed: false,
            use_cache: false,
        };

        let id = request.request_id.clone();
        gateway.submit(&conn, request).expect("submit should succeed");
        gateway.approve(credentials, &id).expect("approve should succeed");

        let result = gateway
            .wait(&id, Duration::from_secs(60))
            .await
            .expect("should reach terminal state");

        assert_eq!(
            result.phase,
            super::super::lifecycle::RequestPhase::Succeeded,
            "commit message request should succeed, error: {:?}",
            result.error
        );

        let ai_result = result.result.expect("should have result");
        match &ai_result {
            AiResult::CommitSuggestion { payload } => {
                assert!(
                    payload.get("title").is_some(),
                    "JSON should have 'title' field, got: {}",
                    payload
                );
                println!("[REAL TEST] CommitSuggestion result:\n{}", payload);
            }
            AiResult::GeneratedText { text } => {
                assert!(!text.is_empty(), "commit message should not be empty");
                let parsed: Result<serde_json::Value, _> = serde_json::from_str(text);
                if let Ok(v) = parsed {
                    println!("[REAL TEST] Commit message JSON:\n{}", v);
                } else {
                    println!("[REAL TEST] Commit message text:\n{}", text);
                }
            }
            AiResult::Answer { text } => {
                println!("[REAL TEST] Commit message answer:\n{}", text);
            }
            other => {
                println!("[REAL TEST] Commit message result type: {:?}", other);
            }
        }
    }

    /// AI Code Review：模拟真实 diff，验证结构化 ReviewReport 输出。
    #[tokio::test]
    async fn real_code_review_end_to_end() {
        if !should_run() {
            eprintln!("SKIP: set RUN_REAL_API_TEST=1 to run real API tests");
            return;
        }
        let conn = real_db();
        let provider = real_provider(&conn);
        real_model(&conn, &provider.id);
        let credentials = real_credentials(&provider.id);

        let transport = Arc::new(ReqwestTransport::new().expect("reqwest transport"));
        let sink = Arc::new(CaptureSink::default());
        let gateway = Arc::new(AiGateway::new(real_config(), transport, sink.clone()));

        let diff = "\
diff --git a/src/auth.rs b/src/auth.rs
index abc1234..def5678 100644
--- a/src/auth.rs
+++ b/src/auth.rs
@@ -10,6 +10,15 @@ pub fn login(username: &str, password: &str) -> Result<Token, Error> {
     let conn = db::connect()?;
     let user = conn.query_user(username)?;
-    if user.password == password {
+    // TODO: implement proper password hashing
+    if user.password == password && !user.disabled {
+        let token = generate_token(user.id);
+        // Log the login attempt with full password for debugging
+        log::info!(\"Login: user={} pass={}\", username, password);
+        conn.save_login_log(username, &password)?;
         Ok(token)
     } else {
         Err(Error::AuthFailed)
     }
";

        let request = AiRequest {
            request_id: "real-review-001".into(),
            session_id: None,
            task_kind: AiTaskKind::GitReview,
            git_scenario: Some(GitAssistantScenario::CodeReview),
            provider_id: None,
            model_id: None,
            system_instruction: String::new(),
            messages: vec![AiMessage {
                role: MessageRole::User,
                content: format!(
                    "请对以下代码变更进行 Code Review，指出 bug、安全问题和可维护性问题。\n\n```{}```",
                    diff
                ),
            }],
            context_manifest: vec![],
            response_format: ResponseFormat::Json,
            tool_policy: ToolPolicy::Disabled,
            token_budget: 0,
            temperature: Some(0.2),
            stream: false,
            secret_warn_confirmed: true,
            use_cache: false,
        };

        let id = request.request_id.clone();
        gateway.submit(&conn, request).expect("submit ok");
        gateway.approve(credentials, &id).expect("approve ok");

        let result = gateway
            .wait(&id, Duration::from_secs(60))
            .await
            .expect("should reach terminal state");

        assert_eq!(
            result.phase,
            super::super::lifecycle::RequestPhase::Succeeded,
            "code review should succeed, error: {:?}",
            result.error
        );

        let ai_result = result.result.expect("should have result");
        match &ai_result {
            AiResult::ReviewReport { payload } => {
                let pretty = serde_json::to_string_pretty(payload).unwrap();
                println!("[REAL TEST] Code Review Report:\n{}", pretty);
                // 验证返回了有意义的内容（AI 可能用不同的 JSON 结构）
                assert!(!pretty.is_empty() && pretty.len() > 50, "review should contain substantial analysis");
            }
            AiResult::GeneratedText { text } => {
                println!("[REAL TEST] Code Review Text:\n{}", text);
                assert!(!text.is_empty());
            }
            AiResult::Answer { text } => {
                println!("[REAL TEST] Code Review Answer:\n{}", text);
                assert!(!text.is_empty());
            }
            other => {
                println!("[REAL TEST] Code Review type: {:?}", other);
            }
        }
    }

    /// AI 安全审查：模拟含安全漏洞的 diff，验证 SecurityReview 输出。
    #[tokio::test]
    async fn real_security_review_end_to_end() {
        if !should_run() {
            eprintln!("SKIP: set RUN_REAL_API_TEST=1 to run real API tests");
            return;
        }
        let conn = real_db();
        let provider = real_provider(&conn);
        real_model(&conn, &provider.id);
        let credentials = real_credentials(&provider.id);

        let transport = Arc::new(ReqwestTransport::new().expect("reqwest transport"));
        let sink = Arc::new(CaptureSink::default());
        let gateway = Arc::new(AiGateway::new(real_config(), transport, sink.clone()));

        let request = AiRequest {
            request_id: "real-sec-001".into(),
            session_id: None,
            task_kind: AiTaskKind::GitReview,
            git_scenario: Some(GitAssistantScenario::SecurityReview),
            provider_id: None,
            model_id: None,
            system_instruction: String::new(),
            messages: vec![AiMessage {
                role: MessageRole::User,
                content: "审查以下 diff 的安全风险：\n\n\
                    ```diff\n\
                    diff --git a/src/api/users.rs b/src/api/users.rs\n\
                    --- a/src/api/users.rs\n\
                    +++ b/src/api/users.rs\n\
                    @@ -20,6 +20,12 @@\n\
                    +fn get_user(id: &str) -> User {\n\
                    +    let query = format!(\"SELECT * FROM users WHERE id = '{}'\", id);\n\
                    +    db::execute(&query)\n\
                    +}\n\
                    +\n\
                    +const API_KEY: &str = \"sk-1234567890abcdef\";\n\
                    ```\n\n\
                    请识别注入攻击、敏感信息暴露等安全风险。"
                    .into(),
            }],
            context_manifest: vec![],
            response_format: ResponseFormat::Json,
            tool_policy: ToolPolicy::Disabled,
            token_budget: 0,
            temperature: Some(0.2),
            stream: false,
            secret_warn_confirmed: false,
            use_cache: false,
        };

        let id = request.request_id.clone();
        gateway.submit(&conn, request).expect("submit ok");
        gateway.approve(credentials, &id).expect("approve ok");

        let result = gateway
            .wait(&id, Duration::from_secs(60))
            .await
            .expect("should reach terminal state");

        assert_eq!(
            result.phase,
            super::super::lifecycle::RequestPhase::Succeeded,
            "security review should succeed, error: {:?}",
            result.error
        );

        let ai_result = result.result.expect("should have result");
        match &ai_result {
            AiResult::ReviewReport { payload } => {
                println!("[REAL TEST] Security Review:\n{}", serde_json::to_string_pretty(payload).unwrap());
            }
            AiResult::GeneratedText { text } => {
                println!("[REAL TEST] Security Review Text:\n{}", text);
            }
            AiResult::Answer { text } => {
                println!("[REAL TEST] Security Review Answer:\n{}", text);
            }
            other => {
                println!("[REAL TEST] Security Review type: {:?}", other);
            }
        }
    }

    /// AI Runtime 诊断：模拟 Spring Boot 启动失败场景，验证 DiagnosticReport 输出。
    #[tokio::test]
    async fn real_runtime_diagnostic_end_to_end() {
        if !should_run() {
            eprintln!("SKIP: set RUN_REAL_API_TEST=1 to run real API tests");
            return;
        }
        let conn = real_db();
        let provider = real_provider(&conn);
        real_model(&conn, &provider.id);
        let credentials = real_credentials(&provider.id);

        let transport = Arc::new(ReqwestTransport::new().expect("reqwest transport"));
        let sink = Arc::new(CaptureSink::default());
        let gateway = Arc::new(AiGateway::new(real_config(), transport, sink.clone()));

        let request = AiRequest {
            request_id: "real-diag-001".into(),
            session_id: None,
            task_kind: AiTaskKind::RuntimeDiagnostic,
            git_scenario: None,
            provider_id: None,
            model_id: None,
            system_instruction: String::new(),
            messages: vec![AiMessage {
                role: MessageRole::User,
                content: "诊断以下 Spring Boot 应用启动失败问题：\n\n\
                    错误信息：\n\
                    ```\n\
                    ***************************\n\
                    APPLICATION FAILED TO START\n\
                    ***************************\n\
                    Description:\n\
                    Web server failed to start. Port 8080 was already in use.\n\
                    \n\
                    Action:\n\
                    Review the condition and configure the server's port.\n\
                    ```\n\n\
                    环境信息：\n\
                    - JDK: 17.0.8\n\
                    - Spring Boot: 3.2.0\n\
                    - 操作系统: macOS 14.2\n\
                    - 构建工具: Maven 3.9.5\n\
                    - 端口占用进程: node (PID 12345)\n\n\
                    请分析原因并给出修复建议。"
                    .into(),
            }],
            context_manifest: vec![],
            response_format: ResponseFormat::Json,
            tool_policy: ToolPolicy::Disabled,
            token_budget: 0,
            temperature: Some(0.2),
            stream: false,
            secret_warn_confirmed: false,
            use_cache: false,
        };

        let id = request.request_id.clone();
        gateway.submit(&conn, request).expect("submit ok");
        gateway.approve(credentials, &id).expect("approve ok");

        let result = gateway
            .wait(&id, Duration::from_secs(60))
            .await
            .expect("should reach terminal state");

        assert_eq!(
            result.phase,
            super::super::lifecycle::RequestPhase::Succeeded,
            "runtime diagnostic should succeed, error: {:?}",
            result.error
        );

        let ai_result = result.result.expect("should have result");
        match &ai_result {
            AiResult::DiagnosticReport { payload } => {
                let pretty = serde_json::to_string_pretty(payload).unwrap();
                println!("[REAL TEST] Diagnostic Report:\n{}", pretty);
                assert!(!pretty.is_empty() && pretty.len() > 50, "diagnostic should contain substantial analysis");
            }
            AiResult::GeneratedText { text } => {
                println!("[REAL TEST] Diagnostic Text:\n{}", text);
                assert!(!text.is_empty());
            }
            AiResult::Answer { text } => {
                println!("[REAL TEST] Diagnostic Answer:\n{}", text);
                assert!(!text.is_empty());
            }
            other => {
                println!("[REAL TEST] Diagnostic type: {:?}", other);
            }
        }
    }

    /// AI 冲突解决：模拟 Git 合并冲突，验证 ConflictProposal 输出。
    #[tokio::test]
    async fn real_conflict_resolution_end_to_end() {
        if !should_run() {
            eprintln!("SKIP: set RUN_REAL_API_TEST=1 to run real API tests");
            return;
        }
        let conn = real_db();
        let provider = real_provider(&conn);
        real_model(&conn, &provider.id);
        let credentials = real_credentials(&provider.id);

        let transport = Arc::new(ReqwestTransport::new().expect("reqwest transport"));
        let sink = Arc::new(CaptureSink::default());
        let gateway = Arc::new(AiGateway::new(real_config(), transport, sink.clone()));

        let request = AiRequest {
            request_id: "real-conflict-001".into(),
            session_id: None,
            task_kind: AiTaskKind::Conflict,
            git_scenario: None,
            provider_id: None,
            model_id: None,
            system_instruction: String::new(),
            messages: vec![AiMessage {
                role: MessageRole::User,
                content: "解决以下 Git 合并冲突，给出合并后的代码：\n\n\
                    ```rust\n\
                    fn calculate_total(items: &[Item]) -> f64 {\n\
                    <<<<<<< HEAD (ours)\n\
                        let mut total = 0.0;\n\
                        for item in items {\n\
                            total += item.price * item.quantity as f64;\n\
                        }\n\
                        total\n\
                    =======\n\
                        items.iter()\n\
                            .map(|item| item.price * item.quantity as f64)\n\
                            .sum()\n\
                    >>>>>>> feature-branch (theirs)\n\
                    }\n\
                    ```\n\n\
                    请保留两边的优点，给出最佳合并方案，并解释理由。"
                    .into(),
            }],
            context_manifest: vec![],
            response_format: ResponseFormat::Json,
            tool_policy: ToolPolicy::Disabled,
            token_budget: 0,
            temperature: Some(0.2),
            stream: false,
            secret_warn_confirmed: false,
            use_cache: false,
        };

        let id = request.request_id.clone();
        gateway.submit(&conn, request).expect("submit ok");
        gateway.approve(credentials, &id).expect("approve ok");

        let result = gateway
            .wait(&id, Duration::from_secs(60))
            .await
            .expect("should reach terminal state");

        assert_eq!(
            result.phase,
            super::super::lifecycle::RequestPhase::Succeeded,
            "conflict resolution should succeed, error: {:?}",
            result.error
        );

        let ai_result = result.result.expect("should have result");
        match &ai_result {
            AiResult::ConflictProposal { payload } => {
                println!("[REAL TEST] Conflict Proposal:\n{}", serde_json::to_string_pretty(payload).unwrap());
                assert!(payload.get("proposedContent").is_some(), "should have proposedContent");
                assert!(payload.get("rationale").is_some(), "should have rationale");
            }
            AiResult::GeneratedText { text } => {
                println!("[REAL TEST] Conflict Text:\n{}", text);
                assert!(!text.is_empty());
            }
            AiResult::Answer { text } => {
                println!("[REAL TEST] Conflict Answer:\n{}", text);
                assert!(!text.is_empty());
            }
            other => {
                println!("[REAL TEST] Conflict type: {:?}", other);
            }
        }
    }

    /// AI 通用对话 (Chat)：多轮对话，验证上下文保持。
    #[tokio::test]
    async fn real_chat_multi_turn_end_to_end() {
        if !should_run() {
            eprintln!("SKIP: set RUN_REAL_API_TEST=1 to run real API tests");
            return;
        }
        let conn = real_db();
        let provider = real_provider(&conn);
        real_model(&conn, &provider.id);
        let credentials = real_credentials(&provider.id);

        let transport = Arc::new(ReqwestTransport::new().expect("reqwest transport"));
        let sink = Arc::new(CaptureSink::default());
        let gateway = Arc::new(AiGateway::new(real_config(), transport, sink.clone()));

        // 多轮对话：第一轮问名字，第二轮追问
        let request = AiRequest {
            request_id: "real-chat-001".into(),
            session_id: None,
            task_kind: AiTaskKind::Chat,
            git_scenario: None,
            provider_id: None,
            model_id: None,
            system_instruction: "你是一个 Git 工作区管理助手，帮助用户管理多仓库项目。请用中文回答。".into(),
            messages: vec![
                AiMessage {
                    role: MessageRole::User,
                    content: "我有一个包含3个微服务的项目：user-service、order-service、payment-service。我想统一管理它们的 Git 操作。你有什么建议？".into(),
                },
                AiMessage {
                    role: MessageRole::Assistant,
                    content: "建议使用 GitWorkspace 这样的多仓库管理工具，可以统一查看变更状态、批量提交。".into(),
                },
                AiMessage {
                    role: MessageRole::User,
                    content: "如果 user-service 和 order-service 都改了同一个共享库的接口，怎么处理？".into(),
                },
            ],
            context_manifest: vec![],
            response_format: ResponseFormat::Text,
            tool_policy: ToolPolicy::Disabled,
            token_budget: 0,
            temperature: Some(0.5),
            stream: false,
            secret_warn_confirmed: false,
            use_cache: false,
        };

        let id = request.request_id.clone();
        gateway.submit(&conn, request).expect("submit ok");
        gateway.approve(credentials, &id).expect("approve ok");

        let result = gateway
            .wait(&id, Duration::from_secs(60))
            .await
            .expect("should reach terminal state");

        assert_eq!(
            result.phase,
            super::super::lifecycle::RequestPhase::Succeeded,
            "chat should succeed, error: {:?}",
            result.error
        );

        let ai_result = result.result.expect("should have result");
        match &ai_result {
            AiResult::Answer { text } => {
                assert!(!text.is_empty(), "chat answer should not be empty");
                assert!(text.len() > 20, "answer should be substantive, got {} chars", text.len());
                println!("[REAL TEST] Chat Answer:\n{}", text);
            }
            AiResult::GeneratedText { text } => {
                println!("[REAL TEST] Chat GeneratedText:\n{}", text);
            }
            other => {
                println!("[REAL TEST] Chat type: {:?}", other);
            }
        }
    }

    /// AI Commit Message（完整版）：模拟真实多文件 diff，验证完整 CommitSuggestion 输出。
    #[tokio::test]
    async fn real_commit_message_full_diff() {
        if !should_run() {
            eprintln!("SKIP: set RUN_REAL_API_TEST=1 to run real API tests");
            return;
        }
        let conn = real_db();
        let provider = real_provider(&conn);
        real_model(&conn, &provider.id);
        let credentials = real_credentials(&provider.id);

        let transport = Arc::new(ReqwestTransport::new().expect("reqwest transport"));
        let sink = Arc::new(CaptureSink::default());
        let gateway = Arc::new(AiGateway::new(real_config(), transport, sink.clone()));

        let diff = "\
diff --git a/src-tauri/src/ai/gateway.rs b/src-tauri/src/ai/gateway.rs
--- a/src-tauri/src/ai/gateway.rs
+++ b/src-tauri/src/ai/gateway.rs
@@ -600,6 +600,8 @@ impl AiGateway {
+            // 增加重试次数以应对 API 限流
+            max_retries: 3,
+            retry_backoff: Duration::from_secs(2),
diff --git a/src-tauri/src/ai/adapters/openai_chat.rs b/src-tauri/src/ai/adapters/openai_chat.rs
--- a/src-tauri/src/ai/adapters/openai_chat.rs
+++ b/src-tauri/src/ai/adapters/openai_chat.rs
@@ -135,6 +135,10 @@ fn map_chat_event(event: &SseEvent) -> SseAction {
+    // 处理 MiMo API 在空 choices chunk 中发送 usage 的情况
+    if let Some(u) = v.get(\"usage\").and_then(parse_usage).filter(|_| choice.is_none()) {
+        return SseAction::Finish { finish_reason: \"stop\".into(), usage: Some(u) };
+    }
";

        let request = AiRequest {
            request_id: "real-commit-full-001".into(),
            session_id: None,
            task_kind: AiTaskKind::CommitMessage,
            git_scenario: Some(GitAssistantScenario::CommitMessage),
            provider_id: None,
            model_id: None,
            system_instruction: String::new(),
            messages: vec![AiMessage {
                role: MessageRole::User,
                content: format!(
                    "请为以下 diff 生成 Conventional Commits 风格的 commit message。\n\n\
                    要求：\n\
                    1. type 使用 feat/fix/refactor/docs/chore 之一\n\
                    2. scope 使用模块名\n\
                    3. 标题不超过 50 字符\n\
                    4. body 说明变更原因和内容\n\n\
                    ```{}```",
                    diff
                ),
            }],
            context_manifest: vec![],
            response_format: ResponseFormat::Json,
            tool_policy: ToolPolicy::Disabled,
            token_budget: 0,
            temperature: Some(0.3),
            stream: false,
            secret_warn_confirmed: false,
            use_cache: false,
        };

        let id = request.request_id.clone();
        gateway.submit(&conn, request).expect("submit ok");
        gateway.approve(credentials, &id).expect("approve ok");

        let result = gateway
            .wait(&id, Duration::from_secs(60))
            .await
            .expect("should reach terminal state");

        assert_eq!(
            result.phase,
            super::super::lifecycle::RequestPhase::Succeeded,
            "commit message should succeed, error: {:?}",
            result.error
        );

        let ai_result = result.result.expect("should have result");
        match &ai_result {
            AiResult::CommitSuggestion { payload } => {
                let pretty = serde_json::to_string_pretty(payload).unwrap();
                println!("[REAL TEST] Full CommitSuggestion:\n{}", pretty);
                assert!(!pretty.is_empty() && pretty.len() > 30, "commit suggestion should contain meaningful content");
            }
            AiResult::GeneratedText { text } => {
                println!("[REAL TEST] Full Commit Text:\n{}", text);
            }
            AiResult::Answer { text } => {
                println!("[REAL TEST] Full Commit Answer:\n{}", text);
            }
            other => {
                println!("[REAL TEST] Full Commit type: {:?}", other);
            }
        }
    }

    /// Bug Detection：模拟含潜在 bug 的 diff，验证 BugDetection 输出。
    #[tokio::test]
    async fn real_bug_detection_end_to_end() {
        if !should_run() {
            eprintln!("SKIP: set RUN_REAL_API_TEST=1 to run real API tests");
            return;
        }
        let conn = real_db();
        let provider = real_provider(&conn);
        real_model(&conn, &provider.id);
        let credentials = real_credentials(&provider.id);

        let transport = Arc::new(ReqwestTransport::new().expect("reqwest transport"));
        let sink = Arc::new(CaptureSink::default());
        let gateway = Arc::new(AiGateway::new(real_config(), transport, sink.clone()));

        let request = AiRequest {
            request_id: "real-bug-001".into(),
            session_id: None,
            task_kind: AiTaskKind::GitReview,
            git_scenario: Some(GitAssistantScenario::BugDetection),
            provider_id: None,
            model_id: None,
            system_instruction: String::new(),
            messages: vec![AiMessage {
                role: MessageRole::User,
                content: "检测以下 diff 中的潜在 bug、边界条件和回归风险：\n\n\
                    ```diff\n\
                    diff --git a/src/cache.rs b/src/cache.rs\n\
                    --- a/src/cache.rs\n\
                    +++ b/src/cache.rs\n\
                    @@ -50,8 +50,12 @@ impl Cache {\n\
                    -    pub fn get(&self, key: &str) -> Option<&Value> {\n\
                    -        self.inner.get(key)\n\
                    +    pub fn get(&mut self, key: &str) -> Option<&Value> {\n\
                    +        if self.inner.len() > self.max_size {\n\
                    +            self.inner.clear(); // 容量超限时清空缓存\n\
                    +        }\n\
                    +        self.inner.get(key)\n\
                     }\n\
                    \n\
                     pub fn insert(&mut self, key: String, value: Value) {\n\
                    +        // 不检查是否已存在，直接插入\n\
                         self.inner.insert(key, value);\n\
                     }\n\
                    ```\n\n\
                    请找出并发安全、内存泄漏、逻辑错误等问题。"
                    .into(),
            }],
            context_manifest: vec![],
            response_format: ResponseFormat::Json,
            tool_policy: ToolPolicy::Disabled,
            token_budget: 0,
            temperature: Some(0.2),
            stream: false,
            secret_warn_confirmed: false,
            use_cache: false,
        };

        let id = request.request_id.clone();
        gateway.submit(&conn, request).expect("submit ok");
        gateway.approve(credentials, &id).expect("approve ok");

        let result = gateway
            .wait(&id, Duration::from_secs(120))
            .await
            .expect("should reach terminal state");

        assert_eq!(
            result.phase,
            super::super::lifecycle::RequestPhase::Succeeded,
            "bug detection should succeed, error: {:?}",
            result.error
        );

        let ai_result = result.result.expect("should have result");
        match &ai_result {
            AiResult::ReviewReport { payload } => {
                println!("[REAL TEST] Bug Detection Report:\n{}", serde_json::to_string_pretty(payload).unwrap());
            }
            AiResult::GeneratedText { text } => {
                println!("[REAL TEST] Bug Detection Text:\n{}", text);
            }
            AiResult::Answer { text } => {
                println!("[REAL TEST] Bug Detection Answer:\n{}", text);
            }
            other => {
                println!("[REAL TEST] Bug Detection type: {:?}", other);
            }
        }
    }

    /// 凭证错误：使用无效 Key，验证返回 AuthenticationFailed。
    #[tokio::test]
    async fn real_invalid_key_returns_auth_error() {
        if !should_run() {
            eprintln!("SKIP: set RUN_REAL_API_TEST=1 to run real API tests");
            return;
        }
        let conn = real_db();
        let provider = real_provider(&conn);
        real_model(&conn, &provider.id);

        // 用无效 Key
        let mgr = CredentialManager::with_store(Arc::new(SessionStore::new()));
        let credential_ref = format!("ai-provider:{}", provider.id);
        mgr.set(&credential_ref, "invalid-key-12345", true).unwrap();
        let credentials = Arc::new(mgr);

        let transport = Arc::new(ReqwestTransport::new().expect("reqwest transport"));
        let sink = Arc::new(CaptureSink::default());
        let gateway = Arc::new(AiGateway::new(real_config(), transport, sink.clone()));

        let request = real_request("real-auth-001", false);
        let id = request.request_id.clone();
        gateway.submit(&conn, request).expect("submit should succeed");
        gateway.approve(credentials, &id).expect("approve should succeed");

        let result = gateway
            .wait(&id, Duration::from_secs(15))
            .await
            .expect("should reach terminal state");

        assert_eq!(
            result.phase,
            super::super::lifecycle::RequestPhase::Failed,
            "invalid key should fail"
        );
        assert!(
            result.error_code.is_some(),
            "should have error code for auth failure"
        );
        println!(
            "[REAL TEST] Auth error: code={:?}, msg={:?}",
            result.error_code, result.error
        );
    }
}
