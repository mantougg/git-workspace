//! AI-04 集成测试（设计文档 §10.4 / §11.3 / §18.2）。
//!
//! 用 Gateway 的 fake transport 跑通完整链路，断言：
//! - 结果缓存只在**相同模型 / Provider / Prompt 版本 / contextHash /
//!   settingsHash** 下命中，任一维度变化即失效，且命中时零网络调用；
//! - 删除会话后不残留完整 Prompt 或 API Key；
//! - 审计记录只含 Secret 计数与类别，不含原文；
//! - 会话持久化开关关闭时不落盘消息正文。

use std::sync::Arc;
use std::time::Duration;

use rusqlite::Connection;

use super::audit;
use super::cache::{self, AiResultCache, PROMPT_VERSION};
use super::credentials::{CredentialManager, SessionStore};
use super::gateway::{AiGateway, GatewayConfig};
use super::model::{save_model, AiModelDefaults, ModelCapability, SaveAiModelRequest};
use super::provider::{save_provider, ApiType, NetworkPolicy, SaveAiProviderRequest};
use super::model::AiTaskKind;
use super::request::{
    AiMessage, AiRequest, ContextItem, ContextKind, MessageRole, ResponseFormat, ToolPolicy,
};
use super::session;

// 复用 gateway_tests 的 fake transport（同 crate 内测试模块）。
use super::gateway_tests::{FakeTransport, Step, Body, CaptureSink};
use super::events::AiEventSink;

const KEY: &str = "sk-fake-key-DO-NOT-LOG";

fn open_db() -> Connection {
    let mut conn = Connection::open_in_memory().unwrap();
    crate::db::init_db(&mut conn).unwrap();
    conn
}

fn add_provider(conn: &Connection) -> super::provider::AiProvider {
    save_provider(
        conn,
        &SaveAiProviderRequest {
            id: None,
            name: "Test Provider".into(),
            api_type: ApiType::OpenaiChatCompletions,
            base_url: "https://fake.local/v1".into(),
            enabled: true,
            network_policy: NetworkPolicy::LocalOnly,
        },
    )
    .unwrap()
}

fn add_model(conn: &Connection, provider_id: &str, model_id: &str) {
    save_model(
        conn,
        &SaveAiModelRequest {
            provider_id: provider_id.into(),
            id: model_id.into(),
            display_name: model_id.into(),
            capabilities: vec![ModelCapability::Chat, ModelCapability::StructuredOutput],
            max_context_tokens: 32000,
            defaults: AiModelDefaults::default(),
            enabled: true,
        },
    )
    .unwrap();
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

struct Harness {
    gateway: Arc<AiGateway>,
    transport: Arc<FakeTransport>,
    cache: Arc<AiResultCache>,
    db: Arc<std::sync::Mutex<Connection>>,
    credentials: Arc<CredentialManager>,
    provider_id: String,
}

fn harness(steps: Vec<Step>) -> Harness {
    let conn = open_db();
    let provider = add_provider(&conn);
    add_model(&conn, &provider.id, "test-model");
    add_model(&conn, &provider.id, "other-model");
    let provider_id = provider.id.clone();
    let credentials = Arc::new(CredentialManager::with_store(Arc::new(SessionStore::new())));
    credentials
        .set(
            provider.credential_ref.as_deref().unwrap(),
            KEY,
            true,
        )
        .unwrap();

    let db = Arc::new(std::sync::Mutex::new(conn));
    let transport = Arc::new(FakeTransport::new(steps));
    let cache = Arc::new(AiResultCache::new(8));
    let gateway = Arc::new(
        AiGateway::new(
            test_config(),
            transport.clone(),
            Arc::new(CaptureSink::default()) as Arc<dyn AiEventSink>,
        )
        .with_store(Arc::clone(&db))
        .with_cache(Arc::clone(&cache)),
    );
    Harness {
        gateway,
        transport,
        cache,
        db,
        credentials,
        provider_id,
    }
}

impl Harness {
    fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.db.lock().unwrap()
    }

    /// 提交并跑到终态（DB 锁在 submit 期间由本函数持有，与 IPC 调用一致）。
    async fn run(&self, request: AiRequest) -> super::gateway::AiRequestSnapshot {
        let id = request.request_id.clone();
        {
            let conn = self.conn();
            self.gateway.submit(&conn, request).expect("submit ok");
        }
        self.gateway
            .approve(Arc::clone(&self.credentials), &id)
            .expect("approve ok");
        self.gateway
            .wait(&id, Duration::from_secs(10))
            .await
            .expect("terminal state")
    }

    fn request(&self, request_id: &str, body: &str) -> AiRequest {
        AiRequest {
            request_id: request_id.into(),
            session_id: None,
            task_kind: AiTaskKind::RuntimeDiagnostic,
            provider_id: Some(self.provider_id.clone()),
            model_id: Some("test-model".into()),
            system_instruction: "你是排障助手".into(),
            messages: vec![AiMessage {
                role: MessageRole::User,
                content: body.into(),
            }],
            context_manifest: vec![],
            response_format: ResponseFormat::Text,
            tool_policy: ToolPolicy::Disabled,
            token_budget: 0,
            temperature: None,
            stream: false,
            secret_warn_confirmed: false,
            use_cache: true,
        }
    }
}

fn chat_json(text: &str) -> String {
    serde_json::json!({
        "choices": [{"message": {"content": text}, "finish_reason": "stop"}],
        "usage": {"prompt_tokens": 5, "completion_tokens": 2}
    })
    .to_string()
}

fn ok_step(text: &str) -> Step {
    Step::Respond {
        status: 200,
        body: Body::Full(chat_json(text)),
    }
}

/// §11.3 / §18.2：相同维度命中缓存（零网络调用），任一维度变化即失效。
#[tokio::test]
async fn cache_hits_only_with_identical_dimensions() {
    let h = harness(vec![ok_step("first"), ok_step("second")]);

    let first = h.run(h.request("r1", "同一份上下文")).await;
    assert_eq!(first.phase, super::lifecycle::RequestPhase::Succeeded);
    assert!(!first.from_cache);
    assert_eq!(h.transport.call_count(), 1);

    // 同维度 → 命中缓存（不再调用 Provider）。
    let second = h.run(h.request("r2", "同一份上下文")).await;
    assert!(second.from_cache, "同维度必须命中缓存");
    assert_eq!(h.transport.call_count(), 1, "缓存命中不得发起网络请求");
    assert!(matches!(
        second.result,
        Some(super::request::AiResult::Answer { ref text }) if text == "first"
    ));
    // 缓存命中也进审计（状态 `cached`，与真实调用可区分）。
    let audit_row = audit::get_audit(&h.conn(), "r2").unwrap().expect("audit");
    assert_eq!(audit_row.status, "cached");

    // 上下文变化（diff/日志变化）→ 失效。
    let changed = h.run(h.request("r3", "上下文变了")).await;
    assert!(!changed.from_cache, "contextHash 变化必须失效");
    assert_eq!(h.transport.call_count(), 2);
}

/// §11.3：换模型必须失效，且不复用另一模型的结果。
#[tokio::test]
async fn cache_never_reused_across_models() {
    let h = harness(vec![ok_step("model-a"), ok_step("model-b")]);

    let first = h.run(h.request("r1", "上下文")).await;
    assert!(matches!(
        first.result,
        Some(super::request::AiResult::Answer { ref text }) if text == "model-a"
    ));

    let mut other_model = h.request("r2", "上下文");
    other_model.model_id = Some("other-model".into());
    let second = h.run(other_model).await;
    assert!(!second.from_cache, "跨模型不得复用缓存");
    assert!(matches!(
        second.result,
        Some(super::request::AiResult::Answer { ref text }) if text == "model-b"
    ));
    assert_eq!(h.transport.call_count(), 2);
}

/// §11.3：`useCache = false`（重新生成）强制重新调用模型。
#[tokio::test]
async fn use_cache_false_bypasses_cache() {
    let h = harness(vec![ok_step("first"), ok_step("regenerated")]);
    h.run(h.request("r1", "上下文")).await;
    assert_eq!(h.transport.call_count(), 1);

    let mut no_cache = h.request("r2", "上下文");
    no_cache.use_cache = false;
    let regenerated = h.run(no_cache).await;
    assert!(!regenerated.from_cache);
    assert_eq!(h.transport.call_count(), 2);
    assert!(matches!(
        regenerated.result,
        Some(super::request::AiResult::Answer { ref text }) if text == "regenerated"
    ));
}

/// §11.3：脱敏/排除策略变化（settingsHash）必须失效。
#[tokio::test]
async fn redaction_policy_change_invalidates_cache() {
    let h = harness(vec![ok_step("first"), ok_step("after-exclusion")]);

    let item = ContextItem {
        kind: ContextKind::Log,
        source_id: "log:app:1:tail".into(),
        display_name: "日志尾部".into(),
        char_count: 100,
        estimated_tokens: 25,
        redacted: false,
        truncated: false,
        excluded: false,
        exclusion_reason: None,
    };
    let mut first_req = h.request("r1", "上下文");
    first_req.context_manifest = vec![item.clone()];
    h.run(first_req).await;
    assert_eq!(h.transport.call_count(), 1);

    let mut excluded_req = h.request("r2", "上下文");
    excluded_req.context_manifest = vec![ContextItem {
        excluded: true,
        exclusion_reason: Some(super::request::ExclusionReason::User),
        ..item
    }];
    let second = h.run(excluded_req).await;
    assert!(!second.from_cache, "排除项变化必须失效缓存");
    assert_eq!(h.transport.call_count(), 2);
}

/// §10.4：删除会话后不残留完整 Prompt 与 API Key（缓存随会话级联清理）。
#[tokio::test]
async fn deleting_session_leaves_no_prompt_or_key() {
    let h = harness(vec![ok_step("诊断结论")]);
    let session = {
        let conn = h.conn();
        let session = session::create_session(
            &conn,
            &session::CreateAiSessionRequest {
                title: "排障会话".into(),
                role: None,
                workspace_id: None,
                repository_scope: vec![],
                runtime_scope: None,
            },
        )
        .unwrap();
        session::set_persistence(&conn, true).unwrap();
        session
    };

    const PROMPT_BODY: &str = "这段内容只应存在于请求里，持久化后才进会话";
    let mut request = h.request("r1", PROMPT_BODY);
    request.session_id = Some(session.id.clone());
    h.run(request).await;

    // 持久化开启 → 消息正文落库（会话语义），缓存也随会话记录。
    {
        let conn = h.conn();
        let messages: i64 = conn
            .query_row("SELECT COUNT(*) FROM ai_messages", [], |r| r.get(0))
            .unwrap();
        assert!(messages >= 2, "开启持久化后应写入用户/助手消息");
        assert_eq!(h.cache.persisted_count(&conn).unwrap(), 1);
    }

    // 删除会话：消息、缓存、以及任何 Prompt/Key 痕迹都必须消失。
    {
        let conn = h.conn();
        session::delete_session(&conn, &session.id).unwrap();
        // 与 IPC 一致：内存 LRU 同步失效。
        h.cache.invalidate_session(&conn, &session.id);

        let messages: i64 = conn
            .query_row("SELECT COUNT(*) FROM ai_messages", [], |r| r.get(0))
            .unwrap();
        let cached: i64 = conn
            .query_row("SELECT COUNT(*) FROM ai_result_cache", [], |r| r.get(0))
            .unwrap();
        assert_eq!(messages, 0, "消息内容必须随会话删除");
        assert_eq!(cached, 0, "关联缓存必须随会话删除");

        // 全库扫描：不得残留完整 Prompt 或 API Key。
        let dumped = dump_ai_tables(&conn);
        assert!(
            !dumped.contains(PROMPT_BODY),
            "删除会话后不得残留完整 Prompt"
        );
        assert!(!dumped.contains(KEY), "任何表中都不得出现 API Key");
    }

    // 缓存已清理：重复同维度请求必须重新调用模型。
    let again = h.run(h.request("r2", PROMPT_BODY)).await;
    assert!(!again.from_cache, "缓存已随会话删除，必须重新请求");
}

/// §10.4：持久化开关关闭时，会话不落盘任何消息正文（只留审计元数据）。
#[tokio::test]
async fn persistence_off_keeps_only_audit_metadata() {
    let h = harness(vec![ok_step("结论")]);
    let session = {
        let conn = h.conn();
        session::create_session(
            &conn,
            &session::CreateAiSessionRequest {
                title: "会话".into(),
                role: None,
                workspace_id: None,
                repository_scope: vec![],
                runtime_scope: None,
            },
        )
        .unwrap()
    };
    // 开关保持默认关闭。

    const PROMPT_BODY: &str = "不落库的正文";
    let mut request = h.request("r1", PROMPT_BODY);
    request.session_id = Some(session.id.clone());
    h.run(request).await;

    let conn = h.conn();
    let messages: i64 = conn
        .query_row("SELECT COUNT(*) FROM ai_messages", [], |r| r.get(0))
        .unwrap();
    assert_eq!(messages, 0, "开关关闭时不得写入消息正文");

    // 审计仍记录元数据（hash / manifest / 计数），但不含正文。
    let row = audit::get_audit(&conn, "r1").unwrap().expect("审计必须存在");
    assert_eq!(row.status, "succeeded");
    assert!(!row.input_hash.is_empty());
    let dumped = dump_ai_tables(&conn);
    assert!(!dumped.contains(PROMPT_BODY), "审计不得含 Prompt 原文");
    assert!(!dumped.contains(KEY));
}

/// §10.4 / §18.2：Secret 命中进审计只记类别与计数（Warn 确认放行的场景）。
#[tokio::test]
async fn audit_records_secret_counts_without_raw_values() {
    let h = harness(vec![ok_step("结论")]);
    // 命中 Secret 但用户已明确确认放行（Warn 路径）。
    let mut request = h.request("r1", "上下文正常");
    request.secret_warn_confirmed = true;
    request.messages.push(AiMessage {
        role: MessageRole::User,
        content: "aws key = AKIAIOSFODNN7EXAMPLE".into(),
    });
    h.run(request).await;

    let conn = h.conn();
    let row = audit::get_audit(&conn, "r1").unwrap().expect("审计");
    assert!(
        !row.secret_counts.is_empty(),
        "Secret 命中必须记入审计（§10.4）"
    );
    let serialized = serde_json::to_string(&row.secret_counts).unwrap();
    assert!(
        !serialized.contains("AKIAIOSFODNN7EXAMPLE"),
        "审计不得含 Secret 原文: {serialized}"
    );
}

/// Secret 默认阻断路径：请求被拒绝且进审计（状态 rejected + 错误 code）。
#[tokio::test]
async fn blocked_secret_request_is_audited_as_rejected() {
    let h = harness(vec![]);
    let conn = h.conn();
    let mut request = h.request("r1", "上下文正常");
    request.messages.push(AiMessage {
        role: MessageRole::User,
        content: "aws key = AKIAIOSFODNN7EXAMPLE".into(),
    });
    let err = h.gateway.submit(&conn, request).unwrap_err();
    assert_eq!(err.code(), "AiSecretDetected");
    drop(conn);

    let conn = h.conn();
    let row = audit::get_audit(&conn, "r1").unwrap().expect("审计");
    assert_eq!(row.status, "rejected");
    assert_eq!(row.error_code.as_deref(), Some("AiSecretDetected"));
    assert!(!row.secret_counts.is_empty());
    assert_eq!(h.transport.call_count(), 0, "拒绝请求不得联网");
}

/// Prompt 版本维度：缓存条目写入时的版本与当前不一致时不命中。
#[tokio::test]
async fn prompt_version_change_invalidates_cache() {
    let h = harness(vec![ok_step("first"), ok_step("after-bump")]);
    h.run(h.request("r1", "上下文")).await;
    assert_eq!(h.transport.call_count(), 1);

    // 构造一条「Key 相同但 promptVersion 列被改成旧值」的条目（模拟版本
    // 漂移或数据被外部改写）：读取时的维度校验必须拒绝它（§11.3）。
    let request = h.request("r1", "上下文");
    let parts = cache::CacheKeyParts::for_request(&request, &h.provider_id, "test-model");
    assert_eq!(parts.prompt_version, PROMPT_VERSION);
    let key = parts.key();
    {
        let conn = h.conn();
        h.cache.invalidate(&conn, &key);
        conn.execute(
            "INSERT INTO ai_result_cache
             (cache_key, task_kind, provider_id, model_id, prompt_version,
              context_hash, settings_hash, result_json, created_at)
             VALUES (?1, 'runtimeDiagnostic', ?2, 'test-model', '0', ?3, ?4, ?5, 't')",
            rusqlite::params![
                key,
                h.provider_id,
                parts.context_hash,
                parts.settings_hash,
                serde_json::json!({"type": "answer", "text": "stale"}).to_string(),
            ],
        )
        .unwrap();
    }

    let again = h.run(h.request("r2", "上下文")).await;
    assert!(
        !again.from_cache,
        "Prompt 版本不一致的缓存条目不得被命中（当前版本 {}）",
        PROMPT_VERSION
    );
    assert_eq!(h.transport.call_count(), 2);
}

/// 拼接 AI 相关表的全部文本（用于「无残留」扫描断言）。
fn dump_ai_tables(conn: &Connection) -> String {
    let mut out = String::new();
    for table in [
        "ai_sessions",
        "ai_messages",
        "ai_requests",
        "ai_result_cache",
        "ai_providers",
        "ai_proposals",
        "ai_settings",
    ] {
        let sql = format!("SELECT * FROM {table}");
        let mut stmt = conn.prepare(&sql).unwrap();
        let column_count = stmt.column_count();
        let mut rows = stmt.query([]).unwrap();
        while let Some(row) = rows.next().unwrap() {
            for i in 0..column_count {
                let value: rusqlite::Result<String> = row.get(i);
                if let Ok(v) = value {
                    out.push_str(&v);
                    out.push('\n');
                }
            }
        }
    }
    out
}
