//! 请求审计（设计文档 §10.4 / §11.2 / §12.1 / §16.3）。
//!
//! `ai_requests` 只记录**元数据**：
//!
//! - 请求类型、Provider、模型、会话 ID；
//! - 上下文 manifest（来源与计量，不含正文）；
//! - 内容 hash（`input_hash`，与缓存 Key 的 `contextHash` 同口径）；
//! - Secret 命中的**类别与计数**（`secret_counts_json`，**永不存原文**）；
//! - 状态迁移结果、错误 code、token 用量（Provider 返回时）、耗时。
//!
//! 状态取值：正常为生命周期阶段名（`previewRequired` / `succeeded` /
//! `failed` / `cancelled` / `rejected` …），缓存命中另记 `cached`
//! （§11.3：UI 不得把缓存结果显示成当前事实，审计需可区分）。

use std::collections::BTreeMap;

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::core::secret::SecretFinding;
use crate::error::AppResult;

use super::model::AiTaskKind;
use super::request::{AiTokenUsage, ContextItem};

/// 请求审计记录（IPC `ai_get_request_audit` / `ai_list_session_audits` 返回）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiRequestAudit {
    pub id: String,
    pub session_id: Option<String>,
    pub task_kind: AiTaskKind,
    pub provider_id: String,
    pub model_id: String,
    /// 最终内容 hash（与缓存 `contextHash` 同口径）。
    pub input_hash: String,
    /// 上下文 manifest（来源与计量；不含正文）。
    pub context_manifest: Vec<ContextItem>,
    /// 终态：`succeeded` / `cached` / `failed` / `cancelled` / `rejected`
    /// 或进行中的阶段名。
    pub status: String,
    pub error_code: Option<String>,
    /// Secret 类别 → 命中次数（**只含计数与类别，不含原文**，§10.4）。
    pub secret_counts: BTreeMap<String, i64>,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub latency_ms: Option<i64>,
    pub created_at: String,
    pub finished_at: Option<String>,
}

/// 审计起始入参（请求进入 Gateway 时写入一次）。
#[derive(Debug, Clone)]
pub struct AuditStart<'a> {
    pub request_id: &'a str,
    pub session_id: Option<&'a str>,
    pub task_kind: AiTaskKind,
    pub provider_id: &'a str,
    pub model_id: &'a str,
    pub input_hash: &'a str,
    pub context_manifest: &'a [ContextItem],
    pub status: &'a str,
    pub secret_counts: &'a BTreeMap<String, i64>,
}

/// 审计收尾入参。
#[derive(Debug, Clone, Default)]
pub struct AuditFinish<'a> {
    pub status: &'a str,
    pub error_code: Option<&'a str>,
    pub usage: Option<AiTokenUsage>,
    pub latency_ms: Option<i64>,
    pub finished_at: &'a str,
}

/// §10.4：把 T-08 的 Secret 命中摘要成「类别 → 计数」。
///
/// 只保留类别标签与次数，**不保留原文、位置与行号**（全局约束 §12）。
pub fn secret_counts(findings: &[SecretFinding]) -> BTreeMap<String, i64> {
    let mut counts: BTreeMap<String, i64> = BTreeMap::new();
    for finding in findings {
        *counts.entry(finding.kind.label().to_string()).or_insert(0) += 1;
    }
    counts
}

/// 请求进入 Gateway：写审计起始行（进行中状态；终态由 [`record_finish`] 收尾）。
pub fn record_start(conn: &Connection, start: &AuditStart<'_>) -> AppResult<()> {
    conn.execute(
        "INSERT OR REPLACE INTO ai_requests
         (id, session_id, task_kind, provider_id, model_id, input_hash,
          context_manifest_json, status, secret_counts_json, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            start.request_id,
            start.session_id,
            start.task_kind.as_str(),
            start.provider_id,
            start.model_id,
            start.input_hash,
            serde_json::to_string(&start.context_manifest)?,
            start.status,
            serde_json::to_string(&start.secret_counts)?,
            chrono::Utc::now().to_rfc3339(),
        ],
    )?;
    Ok(())
}

/// 请求到终态：补写状态、错误 code、token 用量与耗时。
///
/// 幂等：请求 ID 不存在时无副作用（如审计被清理或写入失败）。
pub fn record_finish(conn: &Connection, request_id: &str, finish: &AuditFinish<'_>) -> AppResult<()> {
    let updated = conn.execute(
        "UPDATE ai_requests
         SET status = ?2, error_code = ?3, input_tokens = ?4, output_tokens = ?5,
             latency_ms = ?6, finished_at = ?7
         WHERE id = ?1",
        params![
            request_id,
            finish.status,
            finish.error_code,
            finish.usage.and_then(|u| u.input_tokens),
            finish.usage.and_then(|u| u.output_tokens),
            finish.latency_ms,
            finish.finished_at,
        ],
    )?;
    if updated == 0 {
        log::debug!(
            "ai audit finish skipped: id={} status={} (no audit row)",
            request_id,
            finish.status
        );
    }
    log::info!(
        "ai audit finished: id={} status={} code={:?} in_tokens={:?} out_tokens={:?} latency_ms={:?}",
        request_id,
        finish.status,
        finish.error_code,
        finish.usage.and_then(|u| u.input_tokens),
        finish.usage.and_then(|u| u.output_tokens),
        finish.latency_ms,
    );
    Ok(())
}

pub fn get_audit(conn: &Connection, request_id: &str) -> AppResult<Option<AiRequestAudit>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {} FROM ai_requests WHERE id = ?1",
        AUDIT_COLS
    ))?;
    let mut rows = stmt.query_map(params![request_id], row_to_audit)?;
    match rows.next() {
        Some(row) => Ok(Some(row?)),
        None => Ok(None),
    }
}

/// 会话维度的审计列表（最近的在前，Drawer 的「请求历史」用）。
pub fn list_session_audits(
    conn: &Connection,
    session_id: &str,
    limit: i64,
) -> AppResult<Vec<AiRequestAudit>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {} FROM ai_requests WHERE session_id = ?1
         ORDER BY created_at DESC, id DESC LIMIT ?2",
        AUDIT_COLS
    ))?;
    let rows = stmt.query_map(params![session_id, limit.clamp(1, 200)], row_to_audit)?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

const AUDIT_COLS: &str = "id, session_id, task_kind, provider_id, model_id, input_hash, \
     context_manifest_json, status, error_code, secret_counts_json, input_tokens, \
     output_tokens, latency_ms, created_at, finished_at";

fn row_to_audit(row: &rusqlite::Row) -> rusqlite::Result<AiRequestAudit> {
    let task_kind_str: String = row.get("task_kind")?;
    let manifest_json: String = row.get("context_manifest_json")?;
    let counts_json: String = row.get("secret_counts_json")?;
    Ok(AiRequestAudit {
        id: row.get("id")?,
        session_id: row.get("session_id")?,
        task_kind: AiTaskKind::parse(&task_kind_str).unwrap_or(AiTaskKind::Chat),
        provider_id: row.get("provider_id")?,
        model_id: row.get("model_id")?,
        input_hash: row.get("input_hash")?,
        context_manifest: serde_json::from_str(&manifest_json).unwrap_or_default(),
        status: row.get("status")?,
        error_code: row.get("error_code")?,
        secret_counts: serde_json::from_str(&counts_json).unwrap_or_default(),
        input_tokens: row.get("input_tokens")?,
        output_tokens: row.get("output_tokens")?,
        latency_ms: row.get("latency_ms")?,
        created_at: row.get("created_at")?,
        finished_at: row.get("finished_at")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::request::{ContextKind, MessageRole};
    use crate::core::secret::scan_secrets;

    fn open_db() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        crate::db::init_db(&mut conn).unwrap();
        conn
    }

    fn manifest() -> Vec<ContextItem> {
        vec![ContextItem {
            kind: ContextKind::Log,
            source_id: "runtime/app:latest".into(),
            display_name: "日志尾部".into(),
            char_count: 100,
            estimated_tokens: 25,
            redacted: true,
            truncated: false,
            excluded: false,
            exclusion_reason: None,
        }]
    }

    /// 建一个会话（审计的 `session_id` 有外键约束）。
    fn make_session(conn: &Connection) -> String {
        crate::ai::session::create_session(
            conn,
            &crate::ai::session::CreateAiSessionRequest {
                title: "会话".into(),
                role: None,
                workspace_id: None,
                repository_scope: vec![],
                runtime_scope: None,
            },
        )
        .unwrap()
        .id
    }

    /// 写一条审计起始行（状态默认为 `previewRequired`）。
    fn record(conn: &Connection, request_id: &str, session_id: Option<&str>, status: &str) {
        let context_manifest = manifest();
        let secret_counts = BTreeMap::new();
        record_start(
            conn,
            &AuditStart {
                request_id,
                session_id,
                task_kind: AiTaskKind::RuntimeDiagnostic,
                provider_id: "p1",
                model_id: "m1",
                input_hash: "hash-1",
                context_manifest: &context_manifest,
                status,
                secret_counts: &secret_counts,
            },
        )
        .unwrap();
    }

    /// §10.4：Secret 摘要只含类别与计数，不含原文。
    #[test]
    fn secret_counts_keep_kinds_and_counts_only() {
        let leak = "AKIAIOSFODNN7EXAMPLE password=hunter2".to_string();
        let findings = scan_secrets(&leak);
        assert!(!findings.is_empty());
        let counts = secret_counts(&findings);

        assert!(counts.values().all(|c| *c > 0));
        let serialized = serde_json::to_string(&counts).unwrap();
        assert!(
            !serialized.contains("AKIAIOSFODNN7EXAMPLE") && !serialized.contains("hunter2"),
            "Secret 摘要不得包含原文: {serialized}"
        );
        // 类别标签来自 T-08，不另起规则（全局约束 §13）。
        assert!(counts.keys().all(|k| !k.is_empty()));
    }

    /// 审计写入与收尾（状态迁移、错误 code、token、耗时）。
    #[test]
    fn audit_records_lifecycle_and_usage() {
        let conn = open_db();
        let session_id = make_session(&conn);
        record(&conn, "req-1", Some(&session_id), "previewRequired");

        let pending = get_audit(&conn, "req-1").unwrap().expect("audit row");
        assert_eq!(pending.status, "previewRequired");
        assert_eq!(pending.session_id.as_deref(), Some(session_id.as_str()));
        assert_eq!(pending.task_kind, AiTaskKind::RuntimeDiagnostic);
        assert_eq!(pending.input_hash, "hash-1");
        assert_eq!(pending.context_manifest.len(), 1);
        assert!(pending.finished_at.is_none());

        record_finish(
            &conn,
            "req-1",
            &AuditFinish {
                status: "succeeded",
                error_code: None,
                usage: Some(AiTokenUsage {
                    input_tokens: Some(100),
                    output_tokens: Some(20),
                }),
                latency_ms: Some(1500),
                finished_at: "2026-01-01T00:00:00Z",
            },
        )
        .unwrap();

        let done = get_audit(&conn, "req-1").unwrap().expect("audit row");
        assert_eq!(done.status, "succeeded");
        assert_eq!(done.input_tokens, Some(100));
        assert_eq!(done.output_tokens, Some(20));
        assert_eq!(done.latency_ms, Some(1500));
        assert_eq!(done.finished_at.as_deref(), Some("2026-01-01T00:00:00Z"));
    }

    /// 失败路径记录错误 code；未知请求收尾无副作用。
    #[test]
    fn audit_finish_records_error_code_and_is_idempotent() {
        let conn = open_db();
        record(&conn, "req-2", None, "previewRequired");

        record_finish(
            &conn,
            "req-2",
            &AuditFinish {
                status: "failed",
                error_code: Some("AiProviderUnavailable"),
                usage: None,
                latency_ms: Some(30),
                finished_at: "t",
            },
        )
        .unwrap();
        let failed = get_audit(&conn, "req-2").unwrap().unwrap();
        assert_eq!(failed.status, "failed");
        assert_eq!(failed.error_code.as_deref(), Some("AiProviderUnavailable"));

        record_finish(
            &conn,
            "missing",
            &AuditFinish {
                status: "failed",
                error_code: None,
                usage: None,
                latency_ms: None,
                finished_at: "t",
            },
        )
        .unwrap();
        assert!(get_audit(&conn, "missing").unwrap().is_none());
    }

    /// §10.4：审计表结构上就没有存放 Prompt/结果原文的列。
    #[test]
    fn audit_table_has_no_content_columns() {
        let conn = open_db();
        let mut stmt = conn.prepare("PRAGMA table_info(ai_requests)").unwrap();
        let columns: Vec<String> = stmt
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        for forbidden in ["content", "content_json", "prompt", "messages", "result", "text"] {
            assert!(
                !columns.iter().any(|c| c == forbidden),
                "ai_requests 不得有正文列 {forbidden}：{columns:?}"
            );
        }
        assert!(columns.contains(&"input_hash".to_string()));
        assert!(columns.contains(&"secret_counts_json".to_string()));
    }

    /// 会话维度的审计列表（最近的在前）。
    #[test]
    fn list_session_audits_returns_recent_first() {
        let conn = open_db();
        let session_id = make_session(&conn);
        let other_session = make_session(&conn);
        for (id, status) in [("r1", "succeeded"), ("r2", "failed")] {
            record(&conn, id, Some(&session_id), status);
        }
        record(&conn, "r3", Some(&other_session), "succeeded");

        let audits = list_session_audits(&conn, &session_id, 10).unwrap();
        assert_eq!(audits.len(), 2);
        assert_eq!(audits[0].id, "r2", "最近的在前");
        assert!(audits
            .iter()
            .all(|a| a.session_id.as_deref() == Some(session_id.as_str())));
    }

    /// 删除会话后审计保留但 session_id 置空（§10.4：审计不是会话数据）。
    #[test]
    fn session_delete_orphans_audit_rows() {
        let conn = open_db();
        crate::ai::session::create_session(
            &conn,
            &crate::ai::session::CreateAiSessionRequest {
                title: "会话".into(),
                role: None,
                workspace_id: None,
                repository_scope: vec![],
                runtime_scope: None,
            },
        )
        .unwrap();
        let session_id = conn
            .query_row("SELECT id FROM ai_sessions", [], |r| r.get::<_, String>(0))
            .unwrap();

        record(&conn, "req-4", Some(&session_id), "succeeded");

        crate::ai::session::delete_session(&conn, &session_id).unwrap();

        let audit = get_audit(&conn, "req-4").unwrap().expect("audit 保留");
        assert_eq!(audit.session_id, None);
    }

    /// 角色枚举往返（供消息落库用）。
    #[test]
    fn message_role_roundtrip() {
        assert_eq!(MessageRole::parse("assistant"), Some(MessageRole::Assistant));
        assert_eq!(MessageRole::System.as_str(), "system");
    }
}
