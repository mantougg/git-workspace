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
