//! AI Gateway：统一请求生命周期（设计文档 §7.3 / §7.4 / §16.1 / §16.3）。
//!
//! Gateway 是**唯一允许访问 AI 网络的地方**（§2 统一调用链）：
//!
//! 1. `submit()`：解析任务默认模型（§6.3）、能力前置校验、T-08 Secret
//!    阻断扫描（策略化 Mask/Exclude 由 AI-03 扩展）、token 预算校验，
//!    停在 `PreviewRequired`——**不发起任何网络请求**（§7.3 Preview 闸门）；
//! 2. `approve()`：用户确认 Preview 后才会进入执行（UserApproved → Queued
//!    → Sending → Streaming/Parsing → Succeeded），支持流式事件推送；
//! 3. `cancel()`：任意阶段协作式取消（中断进行中的流式响应）；
//! 4. 失败重试（§7.4）：临时网络错误 / 429 / 5xx / 流式启动中断自动重试
//!    至多 `max_retries`（默认 1）次并退避；Key 无效、超时、策略拒绝等
//!    直接失败。**重试不会导致重复写操作**——第一期 Gateway 与 Adapter
//!    都不执行写操作（§7.4 不变量），重试仅重新发起只读的推理请求，且
//!    流式已产生输出后不再重试（避免向 UI 重复推送）。
//! 5. 并发上限：独立信号量（§16.1，不占 Maven/Java 子进程预算）。
//!
//! 审计（§16.3）：经 `log`（模块路径 `::ai` 路由进 T-08 的 `ai.log`）记录
//! requestId / taskKind / provider/model ID / 状态迁移 / 耗时 / 重试次数 /
//! token 估算与脱敏计数 / 错误 code；不记 Key、Prompt 原文、Secret 原文。

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use tokio::sync::Semaphore;

use crate::error::{AppError, AppResult};

use super::adapters::{
    adapter_for, AdapterCall, AdapterContext, ProviderEndpoint, ProviderRequest, StreamItem,
};
use super::audit::{self, AuditFinish, AuditStart};
use super::cache::{self, AiResultCache, CacheEntryInput, CacheKeyParts};
use super::credentials::CredentialManager;
use super::error::AiError;
use super::events::{AiEventSink, AiRequestEvent, AiStreamChunk};
use super::lifecycle::{invalid_transition_error, Lifecycle, RequestPhase};
use super::model::{ensure_task_capability, resolve_model, AiModel};
use super::provider::AiProvider;
use super::request::{
    estimate_tokens, parse_result, AiMessage, AiRequest, AiResult, AiTokenUsage, MessageRole,
    ResponseFormat,
};
use super::session;
use super::transport::HttpTransport;

/// 缓存命中的审计状态值（§11.3：与真实调用区分，UI 不得当当前事实展示）。
const AUDIT_STATUS_CACHED: &str = "cached";

// ---------------------------------------------------------------------------
// 配置与快照
// ---------------------------------------------------------------------------

/// Gateway 运行配置（测试可注入更小的超时/退避）。
#[derive(Debug, Clone)]
pub struct GatewayConfig {
    /// 独立请求并发上限（§16.1；不占 Maven/Java 子进程预算）。
    pub max_concurrent_requests: usize,
    /// 非流式整请求上限；流式 = 响应头等待 + 泵任务块间空闲上限。
    pub request_timeout: Duration,
    /// 自动重试次数上限（默认 1，§7.4）。
    pub max_retries: u32,
    /// 重试退避基数（第 n 次重试等待 backoff × n）。
    pub retry_backoff: Duration,
    /// 未配置预算时的默认输出 token 上限（Anthropic 必填 max_tokens）。
    pub default_max_output_tokens: i64,
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            max_concurrent_requests: 3,
            request_timeout: Duration::from_secs(120),
            max_retries: 1,
            retry_backoff: Duration::from_millis(750),
            default_max_output_tokens: 4096,
        }
    }
}

/// 请求状态快照（IPC `ai_get_request_status` 的返回；不含 Prompt 内容）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiRequestSnapshot {
    pub request_id: String,
    pub session_id: Option<String>,
    pub task_kind: super::model::AiTaskKind,
    pub provider_id: String,
    pub model_id: String,
    pub phase: RequestPhase,
    pub stream: bool,
    /// 发送前估算的 prompt token（含上下文清单，§16.3 审计口径）。
    pub estimated_prompt_tokens: i64,
    /// 已累计输出的字符数。
    pub output_chars: i64,
    /// 实际发起的请求次数（含首次，不含被取消的排队）。
    pub attempts: u32,
    pub usage: Option<AiTokenUsage>,
    pub result: Option<AiResult>,
    /// 失败/拒绝原因（用户可读，不含敏感内容）。
    pub error: Option<String>,
    pub error_code: Option<String>,
    /// 结果是否来自缓存（§11.3：UI 需区分「过期结果」与「当前事实」）。
    pub from_cache: bool,
}

// ---------------------------------------------------------------------------
// 请求记录
// ---------------------------------------------------------------------------

struct RequestRecord {
    request: AiRequest,
    provider: AiProvider,
    model: AiModel,
    lifecycle: Lifecycle,
    cancel: super::transport::CancelToken,
    estimated_prompt_tokens: i64,
    output_chars: i64,
    attempts: u32,
    usage: Option<AiTokenUsage>,
    result: Option<AiResult>,
    error: Option<(String, String)>, // (code, user-readable message)
    /// 缓存维度（提交时算定，执行期只读）：含 contextHash / settingsHash。
    cache_key: Option<CacheKeyParts>,
    /// 结果是否来自缓存（§11.3）。
    from_cache: bool,
}

impl RequestRecord {
    fn snapshot(&self) -> AiRequestSnapshot {
        AiRequestSnapshot {
            request_id: self.request.request_id.clone(),
            session_id: self.request.session_id.clone(),
            task_kind: self.request.task_kind,
            provider_id: self.provider.id.clone(),
            model_id: self.model.id.clone(),
            phase: self.lifecycle.phase(),
            stream: self.request.stream,
            estimated_prompt_tokens: self.estimated_prompt_tokens,
            output_chars: self.output_chars,
            attempts: self.attempts,
            usage: self.usage,
            result: self.result.clone(),
            error: self.error.as_ref().map(|(_, m)| m.clone()),
            error_code: self.error.as_ref().map(|(c, _)| c.clone()),
            from_cache: self.from_cache,
        }
    }
}

// ---------------------------------------------------------------------------
// Gateway
// ---------------------------------------------------------------------------

/// AI Gateway 服务。以 `Arc<AiGateway>` 挂在 AppState 上（approve 需要
/// `Arc<Self>` 以派生执行任务）。
pub struct AiGateway {
    config: GatewayConfig,
    transport: Arc<dyn HttpTransport>,
    sink: Arc<dyn AiEventSink>,
    records: Mutex<HashMap<String, RequestRecord>>,
    semaphore: Arc<Semaphore>,
    /// AI-04：审计与会话持久化的写入口（`AppState.db` 的共享句柄）。
    /// 未装配（测试/早期引导）时审计与缓存自动降级为 no-op。
    store: Option<Arc<Mutex<Connection>>>,
    /// AI-04：结果缓存（内存 LRU + SQLite，§11.3）。
    cache: Option<Arc<AiResultCache>>,
}

impl AiGateway {
    pub fn new(
        config: GatewayConfig,
        transport: Arc<dyn HttpTransport>,
        sink: Arc<dyn AiEventSink>,
    ) -> Self {
        let max_concurrent_requests = config.max_concurrent_requests.max(1);
        Self {
            semaphore: Arc::new(Semaphore::new(max_concurrent_requests)),
            config,
            transport,
            sink,
            records: Mutex::new(HashMap::new()),
            store: None,
            cache: None,
        }
    }

    /// 装配 SQLite 句柄（AI-04 审计与会话持久化）。Builder 形式，返回 Self。
    pub fn with_store(mut self, db: Arc<Mutex<Connection>>) -> Self {
        self.store = Some(db);
        self
    }

    /// 装配结果缓存（AI-04 §11.3）。Builder 形式，返回 Self。
    pub fn with_cache(mut self, cache: Arc<AiResultCache>) -> Self {
        self.cache = Some(cache);
        self
    }

    /// 在持锁连接上执行 AI 侧的 DB 写入（审计 / 缓存 / 会话）。
    ///
    /// 失败只告警不抛出：辅助设施不得拖垮 AI 主链路（§16.1）；未装配 store
    /// 时静默跳过。
    fn with_db<T>(&self, what: &str, f: impl FnOnce(&Connection) -> AppResult<T>) -> Option<T> {
        let store = self.store.as_ref()?;
        // 锁中毒时仍复用内部连接（AI 侧只做单条短事务写入）。
        let conn = store.lock().unwrap_or_else(|e| {
            log::warn!("ai {what}: db lock poisoned, reusing connection");
            e.into_inner()
        });
        match f(&conn) {
            Ok(value) => Some(value),
            Err(e) => {
                log::warn!(
                    "ai {what} failed: code={} message={}",
                    e.code(),
                    e
                );
                None
            }
        }
    }

    // -----------------------------------------------------------------
    // submit：解析 + 前置校验，停在 PreviewRequired（零网络访问）
    // -----------------------------------------------------------------

    /// 提交请求（§7.1 / §7.3）：模型解析 → 能力校验 → Secret 阻断扫描 →
    /// token 预算校验 → `PreviewRequired`。
    ///
    /// Preview 内容的构建（上下文清单展示、排除项编辑、二次扫描）由
    /// AI-03 的 Context Builder/Preview 流程承载；本方法是该流程的闸门。
    pub fn submit(
        &self,
        conn: &Connection,
        request: AiRequest,
    ) -> AppResult<AiRequestSnapshot> {
        let mut request = request;
        if request.request_id.trim().is_empty() {
            request.request_id = uuid::Uuid::new_v4().to_string();
        }
        let request_id = request.request_id.clone();

        {
            let records = self.lock_records();
            if let Some(rec) = records.get(&request_id) {
                if !rec.lifecycle.is_terminal() {
                    return Err(AppError::Ai(AiError::NotConfigured {
                        message: format!("请求 {} 仍在进行中，不能重复提交", request_id),
                    }));
                }
            }
        }

        // 1. 模型解析（§6.3）：显式 (provider, model) 成对提供，否则走任务链。
        let explicit = match (&request.provider_id, &request.model_id) {
            (Some(p), Some(m)) => Some((p.as_str(), m.as_str())),
            (None, None) => None,
            _ => {
                return Err(AppError::Ai(AiError::NotConfigured {
                    message: "providerId 与 modelId 必须成对提供，或都为空（走任务默认链）"
                        .to_string(),
                }))
            }
        };
        let resolved = resolve_model(conn, request.task_kind, None, explicit)?;
        ensure_task_capability(&resolved.model, request.task_kind)?;

        // 2. 生命周期：Created → ContextBuilding → SecretScanning。
        //    上下文正文已由调用方组装进 messages（清单见 context_manifest）；
        //    此处复用 T-08 的 Secret 扫描做阻断（策略化处理随 AI-03 落地）。
        self.transition(&request_id, &request, &resolved, RequestPhase::ContextBuilding, None);
        let content = compose_request_text(&request);
        let findings = crate::core::secret::scan_secrets(&content);
        // §10.4：审计只留「类别 → 计数」，不保留 Secret 原文/位置。
        let secret_counts = audit::secret_counts(&findings);
        if !findings.is_empty() && !request.secret_warn_confirmed {
            let mut labels: Vec<&str> = findings.iter().map(|f| f.kind.label()).collect();
            labels.sort_unstable();
            labels.dedup();
            let kinds = labels.join("、");
            let error = AiError::SecretDetected { kinds };
            self.reject(conn, &request_id, &request, &resolved, &error, &secret_counts);
            return Err(AppError::Ai(error));
        }
        if !findings.is_empty() {
            // §10.2 Warn：用户在 Preview 中明确确认后放行（命中仍进审计，
            // 不含原文）。默认路径（未确认）已在上方阻断。
            let mut labels: Vec<&str> = findings.iter().map(|f| f.kind.label()).collect();
            labels.sort_unstable();
            labels.dedup();
            log::warn!(
                "ai request secret warn confirmed: id={} kinds={}",
                request_id,
                labels.join("、")
            );
        }

        // 3. token 预算校验（§6.3：请求前报错，不等 Provider 模糊失败）。
        let mut estimated = estimate_tokens(&content);
        for item in request.context_manifest.iter().filter(|i| !i.excluded) {
            estimated += item.estimated_tokens.max(0);
        }
        let ctx_item_count = request
            .context_manifest
            .iter()
            .filter(|i| !i.excluded)
            .count();
        let redacted_count = request.context_manifest.iter().filter(|i| i.redacted).count();
        let budget = if request.token_budget > 0 {
            request.token_budget
        } else {
            resolved.model.max_context_tokens
        };
        if budget > 0 && estimated > budget {
            let error = AiError::ContextTooLarge {
                estimated_tokens: estimated,
                budget_tokens: budget,
            };
            self.reject(conn, &request_id, &request, &resolved, &error, &secret_counts);
            return Err(AppError::Ai(error));
        }

        // 4. AI-04：缓存维度（§11.3）与审计起始行（§10.4）。
        //    注意：本方法在调用方持 DB 锁的上下文执行，所有 DB 写入必须走
        //    传入的 `conn`，不得再取 `self.store`（std::sync::Mutex 不可重入）。
        let cache_key =
            CacheKeyParts::for_request(&request, &resolved.provider.id, &resolved.model.id);
        self.audit_start(
            conn,
            AuditStart {
                request_id: &request_id,
                session_id: request.session_id.as_deref(),
                task_kind: request.task_kind,
                provider_id: &resolved.provider.id,
                model_id: &resolved.model.id,
                input_hash: &cache_key.context_hash,
                context_manifest: &request.context_manifest,
                status: RequestPhase::PreviewRequired.as_str(),
                secret_counts: &secret_counts,
            },
        );

        // 5. PreviewRequired：等待 approve（Preview 展示由 AI-03 承载）。
        self.transition(&request_id, &request, &resolved, RequestPhase::PreviewRequired, None);
        let stream = request.stream;
        let task_kind = request.task_kind;

        let mut records = self.lock_records();
        records.insert(
            request_id.clone(),
            RequestRecord {
                request,
                provider: resolved.provider.clone(),
                model: resolved.model.clone(),
                lifecycle: {
                    let mut lc = Lifecycle::new();
                    lc.transition(RequestPhase::ContextBuilding).expect("validated");
                    lc.transition(RequestPhase::SecretScanning).expect("validated");
                    lc.transition(RequestPhase::PreviewRequired).expect("validated");
                    lc
                },
                cancel: super::transport::CancelToken::new(),
                estimated_prompt_tokens: estimated,
                output_chars: 0,
                attempts: 0,
                usage: None,
                result: None,
                error: None,
                cache_key: Some(cache_key),
                from_cache: false,
            },
        );
        let snapshot = records.get(&request_id).expect("just inserted").snapshot();
        drop(records);

        log::info!(
            "ai request submitted: id={} task={} provider={} model={} ctx_items={} redacted={} est_tokens={} stream={}",
            request_id,
            task_kind.as_str(),
            snapshot.provider_id,
            snapshot.model_id,
            ctx_item_count,
            redacted_count,
            estimated,
            stream
        );
        Ok(snapshot)
    }

    // -----------------------------------------------------------------
    // approve：Preview 闸门之后的唯一联网入口
    // -----------------------------------------------------------------

    /// 确认 Preview 并开始执行（§7.3）。只有 `PreviewRequired` 状态可批准；
    /// 这是 Gateway 唯一会发起网络请求的路径。
    pub fn approve(
        self: &Arc<Self>,
        credentials: Arc<CredentialManager>,
        request_id: &str,
    ) -> AppResult<AiRequestSnapshot> {
        let (request, provider, model) = {
            let mut records = self.lock_records();
            let rec = records.get_mut(request_id).ok_or_else(|| {
                AppError::Ai(AiError::NotConfigured {
                    message: format!("请求不存在: {}", request_id),
                })
            })?;
            if rec.lifecycle.phase() != RequestPhase::PreviewRequired {
                return Err(AppError::Ai(AiError::PreviewRequired {
                    request_id: request_id.to_string(),
                }));
            }
            rec.lifecycle
                .transition(RequestPhase::UserApproved)
                .map_err(|e| AppError::Ai(invalid_transition_error(e)))?;
            (rec.request.clone(), rec.provider.clone(), rec.model.clone())
        };
        self.emit_event(request_id, RequestPhase::UserApproved, None, 0);
        log::info!(
            "ai request approved: id={} task={} provider={} model={}",
            request_id,
            request.task_kind.as_str(),
            provider.id,
            model.id
        );

        // 凭证在执行开始时读取（Key 只经内存流经，不进日志/快照）。
        tokio::spawn(self.clone().execute(
            request_id.to_string(),
            request,
            provider,
            model,
            credentials,
        ));
        Ok(self.status(request_id).expect("record exists"))
    }

    /// 执行任务：Queued（并发闸）→ 重试循环 { Sending → Streaming/Parsing
    /// → Succeeded | Failed }。
    async fn execute(
        self: Arc<Self>,
        request_id: String,
        request: AiRequest,
        provider: AiProvider,
        model: AiModel,
        credentials: Arc<CredentialManager>,
    ) {
        let cancel = self.cancel_token(&request_id);
        let started = Instant::now();

        // 并发闸（§16.1）：取消感知的排队。
        let _permit = tokio::select! {
            p = self.semaphore.clone().acquire_owned() => p.ok(),
            _ = cancel.cancelled() => None,
        };
        let Some(_permit) = _permit else {
            self.finish_cancelled(&request_id, started);
            return;
        };
        if cancel.is_cancelled() {
            self.finish_cancelled(&request_id, started);
            return;
        }
        self.transition_by_id(&request_id, RequestPhase::Queued, None);

        // AI-04：结果缓存（§11.3）——命中则直接复用，**不发起任何网络请求**。
        // 维度不匹配（模型 / Provider / Prompt 版本 / contextHash / settingsHash）
        // 不会命中；`use_cache = false`（重新生成）跳过缓存。
        if request.use_cache {
            if let Some(parts) = self.cache_key(&request_id) {
                if let Some(hit) = self
                    .cache
                    .as_ref()
                    .and_then(|cache| {
                        self.with_db("cache get", |conn| {
                            Ok(cache.get(conn, &parts))
                        })
                        .flatten()
                    })
                {
                    self.transition_by_id(&request_id, RequestPhase::Sending, None);
                    self.transition_by_id(&request_id, RequestPhase::Parsing, None);
                    self.record_cache_hit(&request_id);
                    self.record_result(&request_id, hit.result.clone());
                    self.finalize(&request_id, RequestPhase::Succeeded, None);
                    self.audit_finish_via_store(
                        &request_id,
                        &AuditFinish {
                            status: AUDIT_STATUS_CACHED,
                            error_code: None,
                            usage: None,
                            latency_ms: Some(started.elapsed().as_millis() as i64),
                            finished_at: &chrono::Utc::now().to_rfc3339(),
                        },
                    );
                    self.log_finished(&request_id, 0, started, true);
                    return;
                }
            }
        }

        // 凭证（§6.4）：在线策略缺少 Key 直接失败，不重试。
        let api_key = provider
            .credential_ref
            .as_deref()
            .and_then(|cref| credentials.get(cref));
        if provider.network_policy == super::provider::NetworkPolicy::OnlineOnly && api_key.is_none()
        {
            let error = AiError::CredentialUnavailable {
                message: format!(
                    "Provider「{}」未配置 API Key：请在 AI 设置-凭证中录入",
                    provider.name
                ),
            };
            self.fail_with_audit(&request_id, &error, started);
            return;
        }

        let adapter = adapter_for(provider.api_type);
        let endpoint = ProviderEndpoint {
            provider_id: provider.id.clone(),
            base_url: provider.base_url.clone(),
            api_type: provider.api_type,
            api_key,
        };
        let provider_request = self.build_provider_request(&request, &model);
        if let Err(e) = adapter.validate(&model, &provider_request) {
            match e {
                AppError::Ai(ai_error) => self.fail_with_audit(&request_id, &ai_error, started),
                other => self.fail_with_audit(
                    &request_id,
                    &AiError::ProviderUnavailable {
                        message: other.to_string(),
                        transient: false,
                    },
                    started,
                ),
            }
            return;
        }

        let max_retries = self.config.max_retries;
        let mut attempt: u32 = 0;
        loop {
            attempt += 1;
            self.record_attempts(&request_id, attempt);
            self.transition_by_id(&request_id, RequestPhase::Sending, None);

            let ctx = AdapterContext {
                transport: self.transport.as_ref(),
                cancel: &cancel,
                timeout: self.config.request_timeout,
            };
            let call = AdapterCall {
                endpoint: endpoint.clone(),
                request: provider_request.clone(),
            };

            let outcome = if request.stream {
                self.run_stream(&request_id, adapter, call, ctx).await
            } else {
                adapter
                    .complete(call, ctx)
                    .await
                    .map(|r| (r.text, r.usage))
            };

            match outcome {
                Ok((text, usage)) => {
                    // Parsing → Succeeded（§8.4：非法 JSON 降级 Answer）。
                    self.transition_by_id(&request_id, RequestPhase::Parsing, None);
                    let result =
                        parse_result(request.task_kind, request.response_format, &text);
                    if let Some(u) = usage {
                        self.record_usage(&request_id, u);
                    }
                    self.record_result(&request_id, result.clone());
                    self.finalize(&request_id, RequestPhase::Succeeded, None);
                    // AI-04：审计收尾 + 结果缓存 + 会话持久化（都经 store 写入）。
                    let status = RequestPhase::Succeeded.as_str().to_string();
                    let latency_ms = started.elapsed().as_millis() as i64;
                    let finished_at = chrono::Utc::now().to_rfc3339();
                    self.audit_finish_via_store(
                        &request_id,
                        &AuditFinish {
                            status: &status,
                            error_code: None,
                            usage,
                            latency_ms: Some(latency_ms),
                            finished_at: &finished_at,
                        },
                    );
                    let parts = self.cache_key(&request_id);
                    let session_id = request.session_id.clone();
                    let use_cache = request.use_cache;
                    let outer = self.clone();
                    let inner = outer.clone();
                    outer.with_db("cache put", move |conn| {
                        if use_cache {
                            if let Some(parts) = &parts {
                                inner.cache_put(conn, parts, &result, session_id.as_deref());
                            }
                        }
                        inner.persist_session_exchange(conn, &request, &result);
                        Ok(())
                    });
                    self.log_finished(&request_id, attempt, started, true);
                    return;
                }
                Err(e) => {
                    if cancel.is_cancelled() {
                        self.finish_cancelled(&request_id, started);
                        return;
                    }
                    let output_empty = self.output_chars(&request_id) == 0;
                    let can_retry = e.is_retryable() && attempt <= max_retries && output_empty;
                    if can_retry {
                        let backoff = self.config.retry_backoff * attempt;
                        log::warn!(
                            "ai request retry: id={} attempt={} code={} backoff_ms={}",
                            request_id,
                            attempt,
                            e.code(),
                            backoff.as_millis()
                        );
                        tokio::time::sleep(backoff).await;
                        if cancel.is_cancelled() {
                            self.finish_cancelled(&request_id, started);
                            return;
                        }
                        continue;
                    }
                    // AI-04：失败进审计（状态 + 错误 code + 耗时）。
                    let status = RequestPhase::Failed.as_str().to_string();
                    let code = e.code().to_string();
                    self.audit_finish_via_store(
                        &request_id,
                        &AuditFinish {
                            status: &status,
                            error_code: Some(&code),
                            usage: None,
                            latency_ms: Some(started.elapsed().as_millis() as i64),
                            finished_at: &chrono::Utc::now().to_rfc3339(),
                        },
                    );
                    self.fail(&request_id, &e);
                    self.log_finished(&request_id, attempt, started, false);
                    return;
                }
            }
        }
    }

    /// 流式执行：Sending → Streaming（逐 chunk 推事件）→ 文本回传给
    /// 重试循环统一解析。取消随时中断（§7.2 取消语义）。
    async fn run_stream(
        &self,
        request_id: &str,
        adapter: &dyn super::adapters::AiProviderAdapter,
        call: AdapterCall,
        ctx: AdapterContext<'_>,
    ) -> Result<(String, Option<AiTokenUsage>), AiError> {
        let mut rx = adapter.stream(call, ctx).await?;
        self.transition_by_id(request_id, RequestPhase::Streaming, None);

        let cancel = self.cancel_token(request_id);
        let mut text = String::new();
        // 只在 End 分支赋值后 break，因此循环外必然已初始化。
        let usage: Option<AiTokenUsage>;
        loop {
            let item = tokio::select! {
                item = rx.recv() => item,
                _ = cancel.cancelled() => {
                    return Err(AiError::RequestCancelled {
                        request_id: request_id.to_string(),
                    });
                }
            };
            match item {
                Some(Ok(StreamItem::Text { delta })) => {
                    text.push_str(&delta);
                    self.record_output(request_id, text.chars().count() as i64);
                    self.emit_event(
                        request_id,
                        RequestPhase::Streaming,
                        Some(AiStreamChunk::TextDelta { text: delta }),
                        text.chars().count() as i64,
                    );
                }
                Some(Ok(StreamItem::End { usage: u, .. })) => {
                    usage = u;
                    self.emit_event(
                        request_id,
                        RequestPhase::Streaming,
                        Some(AiStreamChunk::End {
                            finish_reason: None,
                        }),
                        text.chars().count() as i64,
                    );
                    break;
                }
                Some(Err(e)) => return Err(e),
                None => {
                    // 泵任务结束但未发 End：视作流中断（泵已发错误或收尾）。
                    return Err(AiError::ProviderUnavailable {
                        message: "流式连接中断".to_string(),
                        transient: true,
                    });
                }
            }
        }
        Ok((text, usage))
    }

    // -----------------------------------------------------------------
    // cancel / status / wait
    // -----------------------------------------------------------------

    /// 取消请求（幂等）：已终态直接返回快照；排队/执行中则触发取消令牌，
    /// 执行任务在下一个取消点收尾为 Cancelled。PreviewRequired（尚未执行）
    /// 直接迁移 Cancelled。
    pub fn cancel(&self, request_id: &str) -> AppResult<AiRequestSnapshot> {
        // 直接迁移标志：PreviewRequired 阶段尚无执行任务，需在此收尾。
        let direct_transition: Option<i64> = {
            let mut records = self.lock_records();
            let rec = records.get_mut(request_id).ok_or_else(|| {
                AppError::Ai(AiError::NotConfigured {
                    message: format!("请求不存在: {}", request_id),
                })
            })?;
            if rec.lifecycle.is_terminal() {
                return Ok(rec.snapshot());
            }
            rec.cancel.cancel();
            if rec.lifecycle.phase() == RequestPhase::PreviewRequired {
                rec.lifecycle
                    .transition(RequestPhase::Cancelled)
                    .map_err(|e| AppError::Ai(invalid_transition_error(e)))?;
                Some(rec.output_chars)
            } else {
                None
            }
        };
        if let Some(output_chars) = direct_transition {
            // AI-04：Preview 阶段取消也要落审计终态（用户已看到 Preview 后放弃）。
            self.audit_finish_via_store(
                request_id,
                &AuditFinish {
                    status: RequestPhase::Cancelled.as_str(),
                    error_code: None,
                    usage: None,
                    latency_ms: None,
                    finished_at: &chrono::Utc::now().to_rfc3339(),
                },
            );
            self.emit_event(request_id, RequestPhase::Cancelled, None, output_chars);
        }
        Ok(self.status(request_id).expect("record exists"))
    }

    pub fn status(&self, request_id: &str) -> Option<AiRequestSnapshot> {
        self.lock_records().get(request_id).map(|r| r.snapshot())
    }

    /// 等待请求到终态（测试与后续同步场景用）。
    pub async fn wait(&self, request_id: &str, deadline: Duration) -> Option<AiRequestSnapshot> {
        let started = Instant::now();
        while started.elapsed() < deadline {
            if let Some(s) = self.status(request_id) {
                if s.phase.is_terminal() {
                    return Some(s);
                }
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        self.status(request_id)
    }

    /// 清理已终态的请求记录（防长期驻留内存；调用方在会话清理时触发）。
    pub fn prune_terminal(&self) -> usize {
        let mut records = self.lock_records();
        let before = records.len();
        records.retain(|_, r| !r.lifecycle.is_terminal());
        before - records.len()
    }

    // -----------------------------------------------------------------
    // 内部助手
    // -----------------------------------------------------------------

    fn build_provider_request(&self, request: &AiRequest, model: &AiModel) -> ProviderRequest {
        // system_instruction + messages 中的 System 角色合并为协议 system
        // （Adapter 再按协议位置放置）。
        let mut system_parts: Vec<String> = Vec::new();
        if !request.system_instruction.is_empty() {
            system_parts.push(request.system_instruction.clone());
        }
        let messages: Vec<AiMessage> = request
            .messages
            .iter()
            .filter(|m| {
                if m.role == MessageRole::System {
                    system_parts.push(m.content.clone());
                    false
                } else {
                    true
                }
            })
            .cloned()
            .collect();

        let temperature = request
            .temperature
            .or(model.defaults.temperature)
            .filter(|t| (0.0..=2.0).contains(t));

        // 输出 token 上限：预算的一部分；Anthropic 必填，OpenAI 系可选。
        let max_output_tokens = if request.token_budget > 0 {
            Some(request.token_budget)
        } else if model.max_context_tokens > 0 {
            Some(model.max_context_tokens / 2)
        } else {
            Some(self.config.default_max_output_tokens)
        };

        ProviderRequest {
            model_id: model.id.clone(),
            system: if system_parts.is_empty() {
                None
            } else {
                Some(system_parts.join("\n\n"))
            },
            messages,
            temperature,
            max_output_tokens,
            json_mode: request.response_format == ResponseFormat::Json,
        }
    }

    fn lock_records(&self) -> std::sync::MutexGuard<'_, HashMap<String, RequestRecord>> {
        self.records.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn cancel_token(&self, request_id: &str) -> super::transport::CancelToken {
        self.lock_records()
            .get(request_id)
            .map(|r| r.cancel.clone())
            .unwrap_or_default()
    }

    fn output_chars(&self, request_id: &str) -> i64 {
        self.lock_records()
            .get(request_id)
            .map(|r| r.output_chars)
            .unwrap_or(0)
    }

    fn record_attempts(&self, request_id: &str, attempts: u32) {
        if let Some(rec) = self.lock_records().get_mut(request_id) {
            rec.attempts = attempts;
        }
    }

    fn record_output(&self, request_id: &str, chars: i64) {
        if let Some(rec) = self.lock_records().get_mut(request_id) {
            rec.output_chars = chars;
        }
    }

    fn record_usage(&self, request_id: &str, usage: AiTokenUsage) {
        if let Some(rec) = self.lock_records().get_mut(request_id) {
            rec.usage = Some(usage);
        }
    }

    fn record_result(&self, request_id: &str, result: AiResult) {
        if let Some(rec) = self.lock_records().get_mut(request_id) {
            rec.result = Some(result);
        }
    }

    fn record_cache_hit(&self, request_id: &str) {
        if let Some(rec) = self.lock_records().get_mut(request_id) {
            rec.from_cache = true;
        }
    }

    fn cache_key(&self, request_id: &str) -> Option<CacheKeyParts> {
        self.lock_records()
            .get(request_id)
            .and_then(|r| r.cache_key.clone())
    }

    // -----------------------------------------------------------------
    // AI-04：审计 / 缓存 / 会话持久化（§10.4 / §11.3）
    // -----------------------------------------------------------------

    /// 写审计起始行（失败只告警，不阻断请求，§16.1）。
    fn audit_start(&self, conn: &Connection, start: AuditStart<'_>) {
        if let Err(e) = audit::record_start(conn, &start) {
            log::warn!(
                "ai audit start failed: id={} error={}",
                start.request_id,
                e
            );
        }
    }

    /// 写审计收尾行（失败只告警）。
    fn audit_finish(&self, conn: &Connection, request_id: &str, finish: AuditFinish<'_>) {
        if let Err(e) = audit::record_finish(conn, request_id, &finish) {
            log::warn!("ai audit finish failed: id={} error={}", request_id, e);
        }
    }

    /// 经 `self.store` 写审计收尾（执行期路径；未装配 store 时跳过）。
    fn audit_finish_via_store(&self, request_id: &str, finish: &AuditFinish<'_>) {
        let status = finish.status.to_string();
        let error_code = finish.error_code.map(|c| c.to_string());
        let usage = finish.usage;
        let latency_ms = finish.latency_ms;
        let finished_at = finish.finished_at.to_string();
        self.with_db("audit finish", move |conn| {
            audit::record_finish(
                conn,
                request_id,
                &AuditFinish {
                    status: &status,
                    error_code: error_code.as_deref(),
                    usage,
                    latency_ms,
                    finished_at: &finished_at,
                },
            )
        });
    }

    /// 成功后写入结果缓存（§11.3）。`session_id` 用于删除会话时级联清理。
    fn cache_put(
        &self,
        conn: &Connection,
        parts: &CacheKeyParts,
        result: &AiResult,
        session_id: Option<&str>,
    ) {
        let Some(cache) = self.cache.as_ref() else {
            return;
        };
        let input = CacheEntryInput {
            parts: parts.clone(),
            result: result.clone(),
            session_id: session_id.map(|s| s.to_string()),
            created_at: chrono::Utc::now().to_rfc3339(),
        };
        if let Err(e) = cache.put(conn, &input) {
            log::warn!("ai cache put failed: {}", e);
        }
    }

    /// 成功后把本轮对话追加到会话（§10.4：**持久化开关开启时才写正文**）。
    fn persist_session_exchange(
        &self,
        conn: &Connection,
        request: &AiRequest,
        result: &AiResult,
    ) {
        let Some(session_id) = request.session_id.as_deref() else {
            return;
        };
        if !session::persistence_enabled(conn).unwrap_or(false) {
            return;
        }
        let user_content = serde_json::json!({
            "messages": request
                .messages
                .iter()
                .map(|m| serde_json::json!({"role": m.role.as_str(), "content": m.content}))
                .collect::<Vec<_>>(),
        });
        let assistant_content = serde_json::to_value(result).unwrap_or_else(|e| {
            log::warn!("ai session: result serialize failed: {}", e);
            serde_json::json!({})
        });
        for (role, content) in [
            (MessageRole::User, user_content),
            (MessageRole::Assistant, assistant_content),
        ] {
            if let Err(e) = session::append_message_unchecked(conn, session_id, role, &content) {
                log::warn!("ai session append failed: session={} error={}", session_id, e);
            }
        }
    }

    /// 提交阶段的受控迁移（记录尚未入表，单独处理事件与日志）。
    fn transition(
        &self,
        request_id: &str,
        request: &AiRequest,
        resolved: &super::model::ResolvedModel,
        to: RequestPhase,
        chunk: Option<AiStreamChunk>,
    ) {
        self.emit_event_with(
            request_id,
            to,
            chunk,
            0,
            Some((request.task_kind, &resolved.provider.id, &resolved.model.id)),
        );
    }

    /// 提交阶段拒绝（Secret/预算）：入表并落终态，返回错误给调用方。
    ///
    /// AI-04：拒绝同样进审计（状态 `rejected` + 错误 code；Secret 只记计数）。
    /// DB 写入用调用方传入的 `conn`（持锁上下文，不能重复取 `self.store`）。
    #[allow(clippy::too_many_arguments)]
    fn reject(
        &self,
        conn: &Connection,
        request_id: &str,
        request: &AiRequest,
        resolved: &super::model::ResolvedModel,
        error: &AiError,
        secret_counts: &BTreeMap<String, i64>,
    ) {
        let context_hash = cache::request_content_hash(request);
        self.audit_start(
            conn,
            AuditStart {
                request_id,
                session_id: request.session_id.as_deref(),
                task_kind: request.task_kind,
                provider_id: &resolved.provider.id,
                model_id: &resolved.model.id,
                input_hash: &context_hash,
                context_manifest: &request.context_manifest,
                status: RequestPhase::Rejected.as_str(),
                secret_counts,
            },
        );
        let error_code = error.code().to_string();
        self.audit_finish(
            conn,
            request_id,
            AuditFinish {
                status: RequestPhase::Rejected.as_str(),
                error_code: Some(&error_code),
                usage: None,
                latency_ms: Some(0),
                finished_at: &chrono::Utc::now().to_rfc3339(),
            },
        );

        let mut lc = Lifecycle::new();
        lc.transition(RequestPhase::ContextBuilding)
            .expect("validated");
        lc.transition(RequestPhase::SecretScanning)
            .expect("validated");
        lc.transition(RequestPhase::Rejected)
            .expect("validated");
        let message = error.to_string();
        self.lock_records().insert(
            request_id.to_string(),
            RequestRecord {
                request: request.clone(),
                provider: resolved.provider.clone(),
                model: resolved.model.clone(),
                lifecycle: lc,
                cancel: super::transport::CancelToken::new(),
                estimated_prompt_tokens: 0,
                output_chars: 0,
                attempts: 0,
                usage: None,
                result: None,
                error: Some((error_code, message)),
                cache_key: None,
                from_cache: false,
            },
        );
        self.emit_event(request_id, RequestPhase::Rejected, None, 0);
        log::warn!(
            "ai request rejected: id={} task={} provider={} model={} code={}",
            request_id,
            request.task_kind.as_str(),
            resolved.provider.id,
            resolved.model.id,
            error.code()
        );
    }

    fn transition_by_id(&self, request_id: &str, to: RequestPhase, chunk: Option<AiStreamChunk>) {
        {
            let mut records = self.lock_records();
            if let Some(rec) = records.get_mut(request_id) {
                if let Err(e) = rec.lifecycle.transition(to) {
                    // 状态表有单元测试保证；执行路径构造的迁移理应合法。
                    log::error!(
                        "ai request invalid transition: id={} {}",
                        request_id,
                        invalid_transition_error(e)
                    );
                    return;
                }
            } else {
                return;
            }
        }
        let output_chars = self.output_chars(request_id);
        self.emit_event(request_id, to, chunk, output_chars);
    }

    /// 失败终态 + 审计收尾（AI-04：错误 code 与耗时进 `ai_requests`）。
    fn fail_with_audit(&self, request_id: &str, error: &AiError, started: Instant) {
        let status = RequestPhase::Failed.as_str().to_string();
        let code = error.code().to_string();
        self.audit_finish_via_store(
            request_id,
            &AuditFinish {
                status: &status,
                error_code: Some(&code),
                usage: None,
                latency_ms: Some(started.elapsed().as_millis() as i64),
                finished_at: &chrono::Utc::now().to_rfc3339(),
            },
        );
        self.fail(request_id, error);
    }

    /// 取消终态 + 审计收尾（AI-04）。
    fn finish_cancelled(&self, request_id: &str, started: Instant) {
        let status = RequestPhase::Cancelled.as_str();
        self.audit_finish_via_store(
            request_id,
            &AuditFinish {
                status,
                error_code: None,
                usage: None,
                latency_ms: Some(started.elapsed().as_millis() as i64),
                finished_at: &chrono::Utc::now().to_rfc3339(),
            },
        );
        self.finalize(request_id, RequestPhase::Cancelled, None);
        self.log_finished(request_id, 0, started, false);
    }

    fn fail(&self, request_id: &str, error: &AiError) {
        {
            let mut records = self.lock_records();
            if let Some(rec) = records.get_mut(request_id) {
                if let Err(e) = rec.lifecycle.transition(RequestPhase::Failed) {
                    log::error!(
                        "ai request invalid transition: id={} {}",
                        request_id,
                        invalid_transition_error(e)
                    );
                    return;
                }
                rec.error = Some((error.code().to_string(), error.to_string()));
            } else {
                return;
            }
        }
        let output_chars = self.output_chars(request_id);
        self.emit_event(request_id, RequestPhase::Failed, None, output_chars);
        log::warn!(
            "ai request failed: id={} code={} recoverable={}",
            request_id,
            error.code(),
            error.recoverable()
        );
    }

    fn finalize(&self, request_id: &str, phase: RequestPhase, chunk: Option<AiStreamChunk>) {
        self.transition_by_id(request_id, phase, chunk);
    }

    fn emit_event(
        &self,
        request_id: &str,
        phase: RequestPhase,
        chunk: Option<AiStreamChunk>,
        output_chars: i64,
    ) {
        self.emit_event_with(request_id, phase, chunk, output_chars, None);
    }

    fn emit_event_with(
        &self,
        request_id: &str,
        phase: RequestPhase,
        chunk: Option<AiStreamChunk>,
        output_chars: i64,
        ids: Option<(super::model::AiTaskKind, &str, &str)>,
    ) {
        let event = AiRequestEvent {
            request_id: request_id.to_string(),
            phase,
            chunk,
            output_chars,
        };
        self.sink.emit(&event);
        // §16.3：状态迁移审计（事件本身也进了 ai.log 路由的调试通道）。
        if let Some((task_kind, provider_id, model_id)) = ids {
            log::debug!(
                "ai request phase: id={} task={} provider={} model={} phase={}",
                request_id,
                task_kind.as_str(),
                provider_id,
                model_id,
                phase.as_str()
            );
        }
    }

    fn log_finished(&self, request_id: &str, attempts: u32, started: Instant, success: bool) {
        let snapshot = match self.status(request_id) {
            Some(s) => s,
            None => return,
        };
        log::info!(
            "ai request finished: id={} phase={} success={} attempts={} latency_ms={} output_chars={} in_tokens={:?} out_tokens={:?}",
            request_id,
            snapshot.phase.as_str(),
            success,
            attempts,
            started.elapsed().as_millis(),
            snapshot.output_chars,
            snapshot.usage.and_then(|u| u.input_tokens),
            snapshot.usage.and_then(|u| u.output_tokens)
        );
    }
}

/// 拼接请求文本（Secret 扫描与 token 估算口径）：system + 全部消息内容。
fn compose_request_text(request: &AiRequest) -> String {
    let mut parts: Vec<&str> = Vec::new();
    if !request.system_instruction.is_empty() {
        parts.push(&request.system_instruction);
    }
    for m in &request.messages {
        parts.push(&m.content);
    }
    parts.join("\n")
}
