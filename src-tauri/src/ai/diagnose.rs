//! Runtime Assistant 场景编排（AI-06，设计文档 §13 / §3.2 场景 A/B）。
//!
//! 本模块只做**编排**（全局约束 §13）：把 R-14 结构化错误与启动命令摘要
//! 组装成补充上下文，解析目标进程（显式 > 最近一次失败 > 最近记录），
//! 然后复用 AI-03 的统一调用链（Context Builder → Secret 管道 → 预算 →
//! Preview），**不重复实现 Runtime 领域逻辑，也不触网**——网络访问仍由
//! Gateway 的 Preview 闸门之后进行。
//!
//! 硬边界（§13.4）：不修改 `runtimes/*.json`、不运行 Maven/Java/脚本、
//! 不绕过 R-14 错误分类、不阻塞 Runtime 主链路；配置建议只作为文本
//! 返回（`suggestedActions`），不落盘。

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use crate::runtime::launch::{LifecycleStatus, RuntimeProcessInfo};
use crate::runtime::service::RuntimeService;

use super::context::ContextRole;
use super::error::AiError;
use super::model::AiTaskKind;
use super::preview::{self, AiContextPreview, ContextPreviewRequest, SupplementaryContext};
use super::redact::SecretPolicyChoice;
use super::request::ContextKind;

// ---------------------------------------------------------------------------
// 请求契约（IPC `ai_runtime_diagnostic_preview` 入参）
// ---------------------------------------------------------------------------

/// R-14 结构化错误输入（§13.1「优先发送结构化错误」）。`details` 与
/// `logTail` 在错误构造侧已脱敏（error.rs / RingTail），此处直接透传。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticErrorInput {
    /// 错误 code（`BuildFailed` / `ProcessStartFailed` / `PortOccupied` / …）。
    pub code: String,
    /// 用户可读错误信息。
    pub message: String,
    /// R-14 details（module / pid / port / processName / runtime / reason /
    /// logTail，写入侧已脱敏）。
    #[serde(default)]
    pub details: Option<serde_json::Value>,
    /// 错误发生时间（RFC3339；诊断会话关联用）。
    #[serde(default)]
    pub occurred_at: Option<String>,
}

/// 失败诊断 Preview 构建请求（§13.1 / §13.3）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeDiagnosticRequest {
    pub workspace_id: i64,
    pub runtime_name: String,
    /// 指定进程；缺省时自动解析「最近一次失败进程」，无失败记录则取
    /// 最近一条进程记录（Runtime Dashboard 的「当前应用诊断」）。
    #[serde(default)]
    pub process_id: Option<i64>,
    /// R-14 结构化错误（错误横幅入口传入）。
    #[serde(default)]
    pub error: Option<DiagnosticErrorInput>,
    /// 依赖上下文的目标项目（R-02/R-03 Closure；可选）。
    #[serde(default)]
    pub project: Option<String>,
    /// 附带只读配置建议（模块数 / Spring 版本 / JDK → VM Options /
    /// Profile；§4.1 P2 最小版，不落盘）。
    #[serde(default)]
    pub want_config_advice: bool,
    /// 用户补充说明（user 消息，绝不进系统约束，§8.3）。
    #[serde(default)]
    pub user_instruction: String,
    /// 用户排除的 source_id 列表（§10.2；变更后整体重建）。
    #[serde(default)]
    pub exclusions: Vec<String>,
    /// Secret 策略（默认 Block）。
    #[serde(default)]
    pub secret_policy: SecretPolicyChoice,
    /// 日志尾部行数覆盖（默认 200，§8.2）。
    #[serde(default)]
    pub log_tail_lines: Option<usize>,
    /// 用户在 RuntimeLogsView 选中的异常/堆栈片段。存在时仅发送该片段，
    /// 不自动收集未选择的日志尾部（§13.3 场景 B）。
    #[serde(default)]
    pub selected_log: Option<String>,
    /// token 预算覆盖（默认 = 模型上下文上限的 3/4）。
    #[serde(default)]
    pub token_budget: Option<i64>,
    #[serde(default)]
    pub stream: bool,
    /// token 估算校准系数（默认 1.0）。
    #[serde(default)]
    pub token_estimate_factor: Option<f64>,
}

/// 诊断会话作用域（§13.3：请求/结果与 `processId`、`runtimeName`、错误
/// 发生时间关联）。前端创建诊断会话时作为 `ai_create_session` 的
/// `runtimeScope` 传入；审计与消息持久化经 `session_id` 串联。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticSessionScope {
    pub runtime_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub process_id: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
    /// 错误发生时间（RFC3339）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_at: Option<String>,
}

// ---------------------------------------------------------------------------
// 场景指令
// ---------------------------------------------------------------------------

/// 失败诊断的固定用户指令（§13.1；作为 user 消息追加，受信层不掺入）。
const DIAGNOSTIC_INSTRUCTION: &str = "请诊断该 Runtime 的失败/异常原因：\
结合结构化错误、日志尾部与环境事实，按输出 Schema 返回结论、事实、\
可能原因与建议的人工排查步骤。";

/// §4.1 P2 配置建议（AI-06 只读最小版）：基于模块数 / Spring 版本 / JDK
/// 给出 VM Options / Profile 建议。**只作为 suggestedActions 文本返回，
/// 不落盘、不修改 `runtimes/*.json`**（§13.4）。
const CONFIG_ADVICE_INSTRUCTION: &str = "此外，请根据上下文中的模块数、\
Spring / Spring Boot 版本与 JDK 信息，在 suggestedActions 中追加 VM Options \
与 Maven Profile 的优化建议；这些建议仅供参考，需要用户自行确认并手动\
修改配置，请勿声称已应用。";

// ---------------------------------------------------------------------------
// 进程解析
// ---------------------------------------------------------------------------

/// 解析该 Runtime 最近的一条进程记录：**失败记录优先**（「最近一次失败
/// 请求诊断」，§13.3），无失败记录时取最近一条（「当前应用诊断」）。
pub fn latest_process(
    service: &RuntimeService,
    workspace_id: i64,
    runtime_name: &str,
) -> Option<RuntimeProcessInfo> {
    let processes = service.list_processes(workspace_id).ok()?;
    latest_from_processes(processes, runtime_name)
}

fn latest_process_with_connection(
    service: &RuntimeService,
    conn: &Connection,
    workspace_id: i64,
    runtime_name: &str,
) -> Option<RuntimeProcessInfo> {
    let processes = service.list_processes_with_connection(conn, workspace_id).ok()?;
    latest_from_processes(processes, runtime_name)
}

fn latest_from_processes(
    processes: Vec<RuntimeProcessInfo>,
    runtime_name: &str,
) -> Option<RuntimeProcessInfo> {
    let mut relevant: Vec<RuntimeProcessInfo> = processes
        .into_iter()
        .filter(|p| p.runtime_name == runtime_name)
        .collect();
    // 行 id 单调递增，倒序后首条即最新。
    relevant.sort_by(|a, b| b.process_id.cmp(&a.process_id));
    relevant
        .iter()
        .find(|p| p.status == LifecycleStatus::Failed)
        .cloned()
        .or_else(|| relevant.first().cloned())
}

/// 目标进程：显式指定（校验归属）> 最近一次失败 > 最近记录；都没有则
/// `None`（跳过日志上下文，仅发送配置/环境/错误事实）。
fn resolve_process(
    service: &RuntimeService,
    conn: &Connection,
    req: &RuntimeDiagnosticRequest,
) -> AppResult<Option<RuntimeProcessInfo>> {
    match req.process_id {
        Some(id) => {
            let process = service
                .process_status_with_connection(conn, id)?
                .ok_or_else(|| {
                    AppError::Ai(AiError::NotConfigured {
                        message: format!("进程 #{id} 不存在（记录可能已被清理）"),
                    })
                })?;
            if process.workspace_id != req.workspace_id
                || process.runtime_name != req.runtime_name
            {
                return Err(AppError::Ai(AiError::NotConfigured {
                    message: format!(
                        "进程 #{id} 不属于 Workspace #{} 的 Runtime「{}」",
                        req.workspace_id, req.runtime_name
                    ),
                }));
            }
            Ok(Some(process))
        }
        None => Ok(latest_process_with_connection(
            service,
            conn,
            req.workspace_id,
            &req.runtime_name,
        )),
    }
}

// ---------------------------------------------------------------------------
// 补充上下文条目（§13.1）
// ---------------------------------------------------------------------------

/// 结构化错误条目：`code / message / details / occurred_at`。
/// 标记 `redacted`（来源侧 R-14 / RingTail 已脱敏）；Secret 管道仍会扫描
/// 全部未排除条目（§10.2，验收 §18.2 的阻断断言由管道保证）。
fn structured_error_item(error: &DiagnosticErrorInput) -> SupplementaryContext {
    let mut content = format!("code: {}\nmessage: {}", error.code, error.message);
    if let Some(details) = error.details.as_ref().filter(|d| !d.is_null()) {
        let rendered =
            serde_json::to_string_pretty(details).unwrap_or_else(|_| details.to_string());
        content.push_str("\ndetails: ");
        content.push_str(&rendered);
    }
    if let Some(at) = error.occurred_at.as_deref() {
        content.push_str(&format!("\noccurred_at: {at}"));
    }
    SupplementaryContext {
        role: ContextRole::StructuredError,
        kind: ContextKind::Error,
        source_id: format!("error:{}", error.code),
        display_name: format!("结构化错误（{}）", error.code),
        content,
        redacted: true,
    }
}

/// 启动命令摘要条目（§13.1 build command preview）：进程行的
/// `command_preview`（§75 可追溯，构造侧已保证不含环境变量值）。
fn build_command_item(
    workspace_id: i64,
    runtime_name: &str,
    process: &RuntimeProcessInfo,
) -> Option<SupplementaryContext> {
    let command = process.command_preview.as_deref()?;
    Some(SupplementaryContext {
        role: ContextRole::ProcessInfo,
        kind: ContextKind::Runtime,
        source_id: format!(
            "runtime:{workspace_id}:{runtime_name}:{}:command",
            process.process_id
        ),
        display_name: format!("启动命令摘要（进程 #{}）", process.process_id),
        content: format!("command: {command}"),
        redacted: false,
    })
}

// ---------------------------------------------------------------------------
// Preview 构建（零网络；统一调用链的编排入口）
// ---------------------------------------------------------------------------

/// 构建失败诊断的发送前 Preview（§13.1 / §13.3）。组装结构化错误与启动
/// 命令摘要为补充上下文，委托 [`preview::build`] 走统一管道；返回的
/// `request` 可直接提交 Gateway（Preview 闸门语义不变）。
pub fn build_diagnostic_preview(
    conn: &Connection,
    runtime: Option<&RuntimeService>,
    req: RuntimeDiagnosticRequest,
) -> AppResult<AiContextPreview> {
    let runtime = runtime.ok_or_else(|| {
        AppError::Ai(AiError::NotConfigured {
            message: "Runtime 诊断需要 Runtime 服务（应用内发起）".into(),
        })
    })?;
    if req.runtime_name.trim().is_empty() {
        return Err(AppError::Ai(AiError::NotConfigured {
            message: "runtimeName 不能为空（目标 Runtime）".into(),
        }));
    }

    // 1. 目标进程（显式 > 最近失败 > 最近记录）。
    let process = resolve_process(runtime, conn, &req)?;

    // 2. 补充上下文（§13.1：结构化错误优先，其后为构建命令摘要）。
    let mut supplementary = Vec::new();
    if let Some(error) = req.error.as_ref() {
        supplementary.push(structured_error_item(error));
    }
    if let Some(process) = process.as_ref() {
        if let Some(item) = build_command_item(req.workspace_id, &req.runtime_name, process) {
            supplementary.push(item);
        }
    }
    if let Some(selected_log) = req.selected_log.as_deref().filter(|s| !s.trim().is_empty()) {
        supplementary.push(SupplementaryContext {
            role: ContextRole::ErrorLog,
            kind: ContextKind::Log,
            source_id: format!(
                "runtime:{}:{}:{}:selected-log",
                req.workspace_id,
                req.runtime_name,
                process.as_ref().map(|p| p.process_id).unwrap_or_default()
            ),
            display_name: "用户选中的日志片段".into(),
            content: selected_log.to_string(),
            redacted: false,
        });
    }

    // 3. 用户指令（§8.3 user 消息层；配置建议为可选追加段）。
    let mut user_instruction = req.user_instruction.trim().to_string();
    if user_instruction.is_empty() {
        user_instruction = DIAGNOSTIC_INSTRUCTION.to_string();
    }
    if req.want_config_advice {
        user_instruction.push_str("\n\n");
        user_instruction.push_str(CONFIG_ADVICE_INSTRUCTION);
    }

    let ctx_req = ContextPreviewRequest {
        task_kind: AiTaskKind::RuntimeDiagnostic,
        provider_id: None,
        model_id: None,
        workspace_id: Some(req.workspace_id),
        repo_path: None,
        runtime_name: Some(req.runtime_name.clone()),
        process_id: process.as_ref().map(|p| p.process_id),
        project: req.project.clone(),
        user_instruction,
        diff_scope: None,
        diff_selection: None,
        supplementary,
        exclusions: req.exclusions.clone(),
        secret_policy: req.secret_policy,
        // 默认走 §8.2 错误诊断预算（结构化错误 > 错误日志 > 日志尾部 > 环境）。
        budget_strategy: None,
        stream: req.stream,
        token_estimate_factor: req.token_estimate_factor,
        log_tail_lines: req.log_tail_lines,
        token_budget: req.token_budget,
        include_runtime_logs: req.selected_log.is_none(),
    };
    let preview = preview::build(conn, Some(runtime), ctx_req)?;

    // §16.3：只记计量与关联键，不记内容。
    log::info!(
        "ai runtime diagnostic preview: workspace={} runtime={} process={:?} error={:?} config_advice={} items={} blocked={}",
        req.workspace_id,
        req.runtime_name,
        process.as_ref().map(|p| p.process_id),
        req.error.as_ref().map(|e| e.code.as_str()),
        req.want_config_advice,
        preview.items.len(),
        preview.blocked,
    );
    Ok(preview)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    use crate::ai::model::{save_model, AiModelDefaults, ModelCapability, SaveAiModelRequest};
    use crate::ai::provider::{save_provider, ApiType, NetworkPolicy, SaveAiProviderRequest};
    use crate::maven::PomCache;
    use crate::runtime::build::RunStrategy;
    use crate::runtime::config::{CreateRuntimeConfigRequest, RuntimeApplicationConfig};
    use crate::runtime::events::{RuntimeEmission, RuntimeEventEmitter};
    use crate::runtime::launch::store as process_store;
    use crate::runtime::service::{RuntimeService, RuntimeServiceOverrides};
    use crate::test_support::write;

    /// 静默 emitter（诊断路径只读，不消费事件）。
    struct NoopEmitter;
    impl RuntimeEventEmitter for NoopEmitter {
        fn emit(&self, _emission: RuntimeEmission) {}
    }

    struct Fixture {
        root: std::path::PathBuf,
        /// 与 RuntimeService 共享的同一连接（进程/配置行都从这里读）。
        db: Arc<Mutex<Connection>>,
        workspace_id: i64,
        service: Arc<RuntimeService>,
    }

    fn seed_model(conn: &Connection) {
        let provider = save_provider(
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
        .unwrap();
        save_model(
            conn,
            &SaveAiModelRequest {
                provider_id: provider.id,
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

    /// workspace（临时目录）+ Runtime 配置 + 共享 DB 的 RuntimeService。
    fn fixture(tag: &str) -> Fixture {
        let root = std::env::temp_dir().join(format!(
            "gw_ai06_{tag}_{}",
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        std::fs::create_dir_all(&root).unwrap();
        write(
            &root.join("repo/pom.xml"),
            "<project><modelVersion>4.0.0</modelVersion></project>",
        );

        let mut conn = Connection::open_in_memory().unwrap();
        crate::db::init_db(&mut conn).unwrap();
        conn.execute(
            "INSERT INTO workspaces (name, path, created_at, updated_at) VALUES ('w', ?1, 't', 't')",
            [root.to_string_lossy().to_string()],
        )
        .unwrap();
        let workspace_id = conn.last_insert_rowid();
        crate::runtime::config::create_config(
            &conn,
            &CreateRuntimeConfigRequest {
                workspace_id,
                config: RuntimeApplicationConfig {
                    name: "app".into(),
                    project: root.join("repo/pom.xml").to_string_lossy().to_string(),
                    main_class: Some("com.example.Application".into()),
                    ..Default::default()
                },
            },
        )
        .unwrap();
        seed_model(&conn);

        let db = Arc::new(Mutex::new(conn));
        let service = RuntimeService::assemble(
            Arc::clone(&db),
            Arc::new(NoopEmitter),
            Arc::new(PomCache::new()),
            Default::default(),
            root.join("scheduler.json"),
            root.join("approvals.json"),
            RuntimeServiceOverrides::default(),
        );
        Fixture {
            root,
            db,
            workspace_id,
            service,
        }
    }

    /// 在共享连接上持锁执行（Fixture 的唯一 DB 访问入口）。
    fn with_conn<T>(fx: &Fixture, f: impl FnOnce(&Connection) -> T) -> T {
        let conn = fx.db.lock().unwrap();
        f(&conn)
    }

    /// 播种一条进程记录并写日志文件；返回 process_id。
    fn seed_process(
        fx: &Fixture,
        status: LifecycleStatus,
        exit_code: Option<i32>,
        log_lines: &[&str],
    ) -> i64 {
        let id = with_conn(fx, |conn| {
            let id = process_store::insert_process(conn, fx.workspace_id, "app").unwrap();
            if exit_code.is_some() {
                process_store::transition_status(conn, id, status, Some(exit_code)).unwrap();
            }
            process_store::set_launched_meta(
                conn,
                id,
                RunStrategy::ClasspathRun,
                "java -cp app.jar com.example.Application",
                &fx.root.join("repo"),
            )
            .unwrap();
            id
        });
        if !log_lines.is_empty() {
            let dir = fx.root.join(".gitworkspace/logs/app");
            std::fs::create_dir_all(&dir).unwrap();
            let mut content = String::new();
            for line in log_lines {
                content.push_str(line);
                content.push('\n');
            }
            write(&dir.join(format!("{id}.log")), &content);
        }
        id
    }

    fn request(fx: &Fixture) -> RuntimeDiagnosticRequest {
        RuntimeDiagnosticRequest {
            workspace_id: fx.workspace_id,
            runtime_name: "app".into(),
            process_id: None,
            error: None,
            project: None,
            want_config_advice: false,
            user_instruction: String::new(),
            exclusions: vec![],
            secret_policy: Default::default(),
            log_tail_lines: None,
            selected_log: None,
            token_budget: None,
            stream: false,
            token_estimate_factor: None,
        }
    }

    fn build(fx: &Fixture, req: RuntimeDiagnosticRequest) -> AppResult<AiContextPreview> {
        with_conn(fx, |conn| {
            build_diagnostic_preview(conn, Some(fx.service.as_ref()), req)
        })
    }

    fn manifest_sources(preview: &AiContextPreview) -> Vec<String> {
        preview.items.iter().map(|i| i.source_id.clone()).collect()
    }

    fn sent_text(preview: &AiContextPreview) -> String {
        preview
            .request
            .messages
            .iter()
            .map(|m| m.content.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn error_input(code: &str, details: serde_json::Value) -> DiagnosticErrorInput {
        DiagnosticErrorInput {
            code: code.into(),
            message: format!("{code} 失败"),
            details: Some(details),
            occurred_at: Some("2026-08-30T10:00:00Z".into()),
        }
    }

    /// §13.1：结构化错误进入上下文（code/message/details/occurred_at），
    /// 且排在 Manifest 首位（ErrorDiagnosis tier 0）；启动命令摘要随之。
    #[test]
    fn structured_error_is_first_class_context() {
        let fx = fixture("struct_err");
        let process = seed_process(&fx, LifecycleStatus::Failed, Some(1), &["ERROR boom"]);
        let mut req = request(&fx);
        req.process_id = Some(process);
        req.error = Some(DiagnosticErrorInput {
            code: "BuildFailed".into(),
            message: "构建失败".into(),
            details: Some(serde_json::json!({"module": "app", "exitCode": 1})),
            occurred_at: Some("2026-08-30T10:00:00Z".into()),
        });
        let preview = build(&fx, req).unwrap();

        assert_eq!(preview.items[0].source_id, "error:BuildFailed");
        assert!(preview.items[0].redacted);
        let sent = sent_text(&preview);
        assert!(sent.contains("code: BuildFailed"));
        assert!(sent.contains("构建失败"));
        assert!(sent.contains("\"module\": \"app\""));
        assert!(sent.contains("occurred_at: 2026-08-30T10:00:00Z"));
        // 启动命令摘要（§13.1 build command preview）。
        let sources = manifest_sources(&preview);
        assert!(
            sources.iter().any(|s| s.ends_with(":command")),
            "应有启动命令摘要条目: {sources:?}"
        );
        assert!(sent.contains("java -cp app.jar com.example.Application"));
        // 日志与配置/环境上下文（AI-03 收集器）仍在。
        assert!(sources.iter().any(|s| s.contains(":errors")));
        assert!(sources.iter().any(|s| s.contains(":tail")));
        assert!(sources.iter().any(|s| s.ends_with(":config")));
        assert_eq!(preview.target.process_id, Some(process));
        let _ = std::fs::remove_dir_all(&fx.root);
    }

    /// §13.3：缺省 processId 时自动解析「最近一次失败」；无失败记录时
    /// 取最近一条（「当前应用诊断」）。
    #[test]
    fn latest_failed_process_is_resolved() {
        let fx = fixture("latest_failed");
        let _running = seed_process(&fx, LifecycleStatus::Running, None, &["INFO up"]);
        let failed = seed_process(&fx, LifecycleStatus::Failed, Some(137), &["ERROR dead"]);
        let preview = build(&fx, request(&fx)).unwrap();
        assert_eq!(preview.target.process_id, Some(failed));

        let fx2 = fixture("latest_any");
        let latest = seed_process(&fx2, LifecycleStatus::Running, None, &["INFO up"]);
        let preview2 = build(&fx2, request(&fx2)).unwrap();
        assert_eq!(preview2.target.process_id, Some(latest));
        let _ = std::fs::remove_dir_all(&fx.root);
        let _ = std::fs::remove_dir_all(&fx2.root);
    }

    /// 无任何进程记录（如 JdkNotFound 在 spawn 前失败）：仍可构建 Preview，
    /// 跳过日志类条目，保留配置/环境/错误事实（§13.1 的弹性输入）。
    #[test]
    fn builds_without_any_process_record() {
        let fx = fixture("no_process");
        let mut req = request(&fx);
        req.error = Some(error_input("JdkNotFound", serde_json::json!({})));
        let preview = build(&fx, req).unwrap();
        assert_eq!(preview.target.process_id, None);
        let sources = manifest_sources(&preview);
        assert!(
            !sources.iter().any(|s| s.contains(":tail")),
            "无进程记录时不得编造日志上下文: {sources:?}"
        );
        assert!(sources.contains(&"error:JdkNotFound".to_string()));
        assert!(sources.iter().any(|s| s.ends_with(":config")));
        let _ = std::fs::remove_dir_all(&fx.root);
    }

    /// 显式 processId 校验归属（不串到别的 Runtime/Workspace）。
    #[test]
    fn explicit_process_must_belong_to_target() {
        let fx = fixture("mismatch");
        let process = seed_process(&fx, LifecycleStatus::Failed, Some(1), &[]);
        let mut req = request(&fx);
        req.runtime_name = "other".into();
        req.process_id = Some(process);
        let err = build(&fx, req).unwrap_err();
        assert!(err.to_string().contains("不属于"), "{err}");

        let mut req = request(&fx);
        req.process_id = Some(999_999);
        let err = build(&fx, req).unwrap_err();
        assert!(err.to_string().contains("不存在"), "{err}");
        let _ = std::fs::remove_dir_all(&fx.root);
    }

    /// §4.1 P2 配置建议：wantConfigAdvice 追加只读建议指令（user 消息层，
    /// 绝不进 system）。
    #[test]
    fn config_advice_instruction_is_appended_as_user_message() {
        let fx = fixture("config_advice");
        seed_process(&fx, LifecycleStatus::Failed, Some(1), &["ERROR x"]);
        let mut req = request(&fx);
        req.want_config_advice = true;
        req.user_instruction = "顺便看看内存参数".into();
        let preview = build(&fx, req).unwrap();
        let last = preview
            .request
            .messages
            .last()
            .expect("user message")
            .content
            .clone();
        assert!(last.contains("顺便看看内存参数"), "用户指令保留: {last}");
        assert!(last.contains("VM Options"), "配置建议指令追加: {last}");
        assert!(last.contains("请勿声称已应用"), "建议必须标注待确认: {last}");
        assert!(
            !preview.request.system_instruction.contains("顺便看看内存参数"),
            "用户内容不得进入 system 层"
        );
        assert!(
            !preview
                .request
                .system_instruction
                .contains("VM Options 与 Maven Profile"),
            "场景指令不得进入 system 层"
        );
        let _ = std::fs::remove_dir_all(&fx.root);
    }

    /// 默认（无用户指令）注入场景诊断指令。
    #[test]
    fn default_instruction_is_used_when_empty() {
        let fx = fixture("default_instr");
        seed_process(&fx, LifecycleStatus::Failed, Some(1), &["ERROR x"]);
        let preview = build(&fx, request(&fx)).unwrap();
        let last = preview.request.messages.last().expect("user message");
        assert!(last.content.contains("诊断该 Runtime"));
        let _ = std::fs::remove_dir_all(&fx.root);
    }

    /// 场景 B：用户选中日志片段时，片段进入 Secret/预算管道，但未选中的
    /// 日志尾部不进入 Preview。
    #[test]
    fn selected_log_is_scanned_without_collecting_unselected_tail() {
        let fx = fixture("selected_log");
        let process = seed_process(&fx, LifecycleStatus::Failed, Some(1), &["ERROR unselected"]);
        let mut req = request(&fx);
        req.process_id = Some(process);
        req.selected_log = Some("Exception: selected stack\n at App.run(App.java:1)".into());
        let preview = build(&fx, req).unwrap();
        let sources = manifest_sources(&preview);
        assert!(sources.iter().any(|s| s.ends_with(":selected-log")), "{sources:?}");
        assert!(!sources.iter().any(|s| s.ends_with(":tail")), "{sources:?}");
        assert!(!sent_text(&preview).contains("ERROR unselected"));
        assert!(sent_text(&preview).contains("Exception: selected stack"));
        let _ = std::fs::remove_dir_all(&fx.root);
    }

    /// §18.2 验收：五种典型失败场景均能生成正确上下文（结构化错误 +
    /// 日志/环境/配置事实），且 Preview 未被阻断。
    #[test]
    fn typical_failure_scenarios_generate_context() {
        let scenarios: [(&str, &str); 5] = [
            ("PortOccupied", "Web server failed to start. Port 8080 was already in use"),
            ("DependencyResolveFailed", "Could not resolve dependencies for project app"),
            ("JdkNotFound", "No JAVA_HOME could be located"),
            ("MavenNotFound", "mvn: command not found"),
            ("ProcessCrashed", "APPLICATION FAILED TO START"),
        ];
        for (tag, (code, log_line)) in scenarios.iter().enumerate() {
            let fx = fixture(&format!("scenario_{tag}"));
            let process = seed_process(&fx, LifecycleStatus::Failed, Some(1), &[log_line]);
            let mut req = request(&fx);
            req.process_id = Some(process);
            req.error = Some(error_input(
                code,
                serde_json::json!({"runtime": "app", "module": "app", "reason": log_line}),
            ));
            let preview = build(&fx, req)
                .unwrap_or_else(|e| panic!("{} preview 构建失败: {e}", code));
            assert!(!preview.blocked, "{code} 不应被 Secret 阻断");
            let sources = manifest_sources(&preview);
            assert!(
                sources.contains(&format!("error:{code}")),
                "{code} 缺少结构化错误条目: {sources:?}"
            );
            assert!(
                sources.iter().any(|s| s.ends_with(":tail")),
                "{code} 缺少日志尾部: {sources:?}"
            );
            let sent = sent_text(&preview);
            assert!(sent.contains(log_line), "{code} 日志行未进入上下文");
            let _ = std::fs::remove_dir_all(&fx.root);
        }
    }

    /// Secret 检测在诊断路径生效（验收 §18.2）：details 中混入 AWS Key 时
    /// 默认 Block 阻断 Preview 发送（T-08 复用，不另起规则）。
    #[test]
    fn secret_detection_blocks_diagnostic_preview() {
        let fx = fixture("secret");
        let process = seed_process(&fx, LifecycleStatus::Failed, Some(1), &["ERROR x"]);
        let mut req = request(&fx);
        req.process_id = Some(process);
        req.error = Some(DiagnosticErrorInput {
            code: "ProcessStartFailed".into(),
            message: "启动失败".into(),
            details: Some(serde_json::json!({
                "reason": "config rejected: AKIAIOSFODNN7EXAMPLE"
            })),
            occurred_at: None,
        });
        let preview = build(&fx, req).unwrap();
        assert!(preview.blocked, "结构化错误中的 Secret 必须阻断发送");
        assert!(!preview.block_reasons.is_empty());
        let _ = std::fs::remove_dir_all(&fx.root);
    }

    /// 空名/缺 Runtime 服务返回可行动错误（§17）。
    #[test]
    fn invalid_targets_return_actionable_errors() {
        let fx = fixture("invalid");
        let mut req = request(&fx);
        req.runtime_name = "  ".into();
        let err = build(&fx, req).unwrap_err();
        assert!(err.to_string().contains("runtimeName"), "{err}");

        let err = with_conn(&fx, |conn| {
            build_diagnostic_preview(conn, None, request(&fx))
        })
        .unwrap_err();
        assert!(err.to_string().contains("Runtime 服务"), "{err}");
        let _ = std::fs::remove_dir_all(&fx.root);
    }
}
