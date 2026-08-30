//! AI-11: controlled write Action Proposals.
//!
//! Proposal creation is deliberately side-effect free with respect to Git and
//! Runtime state.  The only write performed here is the proposal metadata in
//! SQLite; execution is a separate, explicit transition owned by the IPC
//! command and submitted to the existing Task Queue.

use chrono::{DateTime, Duration, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{AppError, AppResult};

use super::error::AiError;

const DEFAULT_TTL_MINUTES: i64 = 15;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ActionKind {
    GitCreateCommit,
    RuntimeStart,
    ConflictApply,
    RuntimeUpdateConfig,
}

impl ActionKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::GitCreateCommit => "gitCreateCommit",
            Self::RuntimeStart => "runtimeStart",
            Self::ConflictApply => "conflictApply",
            Self::RuntimeUpdateConfig => "runtimeUpdateConfig",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RiskLevel {
    Low,
    Medium,
    High,
}

impl RiskLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProposalStatus {
    Pending,
    Confirmed,
    Executed,
    Rejected,
    Expired,
}

impl ProposalStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Confirmed => "confirmed",
            Self::Executed => "executed",
            Self::Rejected => "rejected",
            Self::Expired => "expired",
        }
    }

    fn parse(value: &str) -> Self {
        match value {
            "confirmed" => Self::Confirmed,
            "executed" => Self::Executed,
            "rejected" => Self::Rejected,
            "expired" => Self::Expired,
            _ => Self::Pending,
        }
    }
}

/// Public Proposal DTO.  Action payloads are intentionally not exposed or
/// returned; they remain an implementation detail of the confirmation path.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionProposal {
    pub proposal_id: String,
    pub request_id: Option<String>,
    pub action_kind: ActionKind,
    pub risk_level: RiskLevel,
    pub target_scope: Value,
    pub affected_repositories: Vec<String>,
    pub affected_files: Vec<String>,
    pub before_summary: String,
    pub after_summary: String,
    pub diff: Option<String>,
    pub command_preview: Option<String>,
    pub reversible: bool,
    pub expires_at: String,
    pub status: ProposalStatus,
    pub confirmed_at: Option<String>,
    pub executed_task_id: Option<String>,
    pub created_at: String,
}

/// Internal record used by the confirmation path.  The payload never crosses
/// IPC and is only converted into a typed TaskRequest after confirmation.
#[derive(Debug, Clone)]
pub struct ProposalRecord {
    pub proposal: ActionProposal,
    pub action_payload: Value,
}

pub fn expires_at() -> String {
    (Utc::now() + Duration::minutes(DEFAULT_TTL_MINUTES)).to_rfc3339()
}

pub fn insert(conn: &Connection, proposal: &ActionProposal, action_payload: &Value) -> AppResult<()> {
    conn.execute(
        "INSERT INTO ai_proposals
         (id, request_id, action_kind, risk_level, target_scope_json,
          affected_repositories_json, affected_files_json, before_summary,
          after_summary, diff_json, command_preview, reversible, expires_at,
          status, confirmed_at, executed_task_id, action_payload_json, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13,
                 ?14, ?15, ?16, ?17, ?18)",
        params![
            proposal.proposal_id,
            proposal.request_id,
            proposal.action_kind.as_str(),
            proposal.risk_level.as_str(),
            proposal.target_scope.to_string(),
            serde_json::to_string(&proposal.affected_repositories)?,
            serde_json::to_string(&proposal.affected_files)?,
            proposal.before_summary,
            proposal.after_summary,
            proposal.diff,
            proposal.command_preview,
            proposal.reversible as i64,
            proposal.expires_at,
            proposal.status.as_str(),
            proposal.confirmed_at,
            proposal.executed_task_id,
            action_payload.to_string(),
            proposal.created_at,
        ],
    )?;
    Ok(())
}

fn parse_action_kind(value: &str) -> Option<ActionKind> {
    Some(match value {
        "gitCreateCommit" => ActionKind::GitCreateCommit,
        "runtimeStart" => ActionKind::RuntimeStart,
        "conflictApply" => ActionKind::ConflictApply,
        "runtimeUpdateConfig" => ActionKind::RuntimeUpdateConfig,
        _ => return None,
    })
}

fn parse_risk(value: &str) -> RiskLevel {
    match value {
        "high" => RiskLevel::High,
        "low" => RiskLevel::Low,
        _ => RiskLevel::Medium,
    }
}

fn row_to_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<ProposalRecord> {
    let target_scope: String = row.get("target_scope_json")?;
    let repos: String = row.get("affected_repositories_json")?;
    let files: String = row.get("affected_files_json")?;
    let action_kind: String = row.get("action_kind")?;
    let payload: String = row.get("action_payload_json")?;
    Ok(ProposalRecord {
        proposal: ActionProposal {
            proposal_id: row.get("id")?,
            request_id: row.get("request_id")?,
            action_kind: parse_action_kind(&action_kind).unwrap_or(ActionKind::GitCreateCommit),
            risk_level: parse_risk(&row.get::<_, String>("risk_level")?),
            target_scope: serde_json::from_str(&target_scope).unwrap_or_else(|_| Value::Object(Default::default())),
            affected_repositories: serde_json::from_str(&repos).unwrap_or_default(),
            affected_files: serde_json::from_str(&files).unwrap_or_default(),
            before_summary: row.get("before_summary")?,
            after_summary: row.get("after_summary")?,
            diff: row.get("diff_json")?,
            command_preview: row.get("command_preview")?,
            reversible: row.get::<_, i64>("reversible")? != 0,
            expires_at: row.get("expires_at")?,
            status: ProposalStatus::parse(&row.get::<_, String>("status")?),
            confirmed_at: row.get("confirmed_at")?,
            executed_task_id: row.get("executed_task_id")?,
            created_at: row.get("created_at")?,
        },
        action_payload: serde_json::from_str(&payload).unwrap_or(Value::Null),
    })
}

const COLS: &str = "id, request_id, action_kind, risk_level, target_scope_json,
    affected_repositories_json, affected_files_json, before_summary, after_summary,
    diff_json, command_preview, reversible, expires_at, status, confirmed_at,
    executed_task_id, action_payload_json, created_at";

pub fn get(conn: &Connection, proposal_id: &str) -> AppResult<Option<ProposalRecord>> {
    let mut stmt = conn.prepare(&format!("SELECT {COLS} FROM ai_proposals WHERE id = ?1"))?;
    Ok(stmt
        .query_row(params![proposal_id], row_to_record)
        .optional()?)
}

pub fn list(conn: &Connection, status: Option<ProposalStatus>) -> AppResult<Vec<ActionProposal>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLS} FROM ai_proposals {} ORDER BY created_at DESC",
        if status.is_some() { "WHERE status = ?1" } else { "" }
    ))?;
    let rows = if let Some(status) = status {
        stmt.query_map(params![status.as_str()], row_to_record)?
    } else {
        stmt.query_map([], row_to_record)?
    };
    Ok(rows.filter_map(Result::ok).map(|r| r.proposal).collect())
}

fn expiry_check(conn: &Connection, record: &ProposalRecord) -> AppResult<()> {
    if record.proposal.status != ProposalStatus::Pending {
        return Err(AppError::Ai(AiError::ProposalStateInvalid {
            proposal_id: record.proposal.proposal_id.clone(),
            status: record.proposal.status.as_str().to_string(),
        }));
    }
    let expired = DateTime::parse_from_rfc3339(&record.proposal.expires_at)
        .map(|at| Utc::now() >= at.with_timezone(&Utc))
        .unwrap_or(true);
    if expired {
        conn.execute(
            "UPDATE ai_proposals SET status = 'expired' WHERE id = ?1 AND status = 'pending'",
            params![record.proposal.proposal_id],
        )?;
        return Err(AppError::Ai(AiError::ProposalExpired {
            proposal_id: record.proposal.proposal_id.clone(),
        }));
    }
    Ok(())
}

pub fn confirm(conn: &Connection, proposal_id: &str, second_confirmation: bool) -> AppResult<ActionProposal> {
    let record = get(conn, proposal_id)?.ok_or_else(|| AppError::Ai(AiError::ProposalNotFound {
        proposal_id: proposal_id.to_string(),
    }))?;
    expiry_check(conn, &record)?;
    if record.proposal.risk_level == RiskLevel::High && !second_confirmation {
        return Err(AppError::Ai(AiError::ActionConfirmationRequired {
            proposal_id: proposal_id.to_string(),
        }));
    }
    let now = Utc::now().to_rfc3339();
    let changed = conn.execute(
        "UPDATE ai_proposals SET status = 'confirmed', confirmed_at = ?1
         WHERE id = ?2 AND status = 'pending'",
        params![now, proposal_id],
    )?;
    if changed != 1 {
        let status = get(conn, proposal_id)
            .ok()
            .flatten()
            .map(|r| r.proposal.status.as_str().to_string())
            .unwrap_or_else(|| "unknown".into());
        return Err(AppError::Ai(AiError::ProposalStateInvalid {
            proposal_id: proposal_id.to_string(),
            status,
        }));
    }
    let mut proposal = record.proposal;
    proposal.status = ProposalStatus::Confirmed;
    proposal.confirmed_at = Some(now);
    Ok(proposal)
}

pub fn reject(conn: &Connection, proposal_id: &str) -> AppResult<ActionProposal> {
    let record = get(conn, proposal_id)?.ok_or_else(|| AppError::Ai(AiError::ProposalNotFound {
        proposal_id: proposal_id.to_string(),
    }))?;
    expiry_check(conn, &record)?;
    conn.execute(
        "UPDATE ai_proposals SET status = 'rejected' WHERE id = ?1 AND status = 'pending'",
        params![proposal_id],
    )?;
    let mut proposal = record.proposal;
    proposal.status = ProposalStatus::Rejected;
    Ok(proposal)
}

pub fn mark_executed(conn: &Connection, proposal_id: &str, task_id: &str) -> AppResult<ActionProposal> {
    let record = get(conn, proposal_id)?.ok_or_else(|| AppError::Ai(AiError::ProposalNotFound {
        proposal_id: proposal_id.to_string(),
    }))?;
    if record.proposal.status != ProposalStatus::Confirmed {
        return Err(AppError::Ai(AiError::ProposalStateInvalid {
            proposal_id: proposal_id.to_string(),
            status: record.proposal.status.as_str().to_string(),
        }));
    }
    conn.execute(
        "UPDATE ai_proposals SET status = 'executed', executed_task_id = ?1 WHERE id = ?2 AND status = 'confirmed'",
        params![task_id, proposal_id],
    )?;
    let mut proposal = record.proposal;
    proposal.status = ProposalStatus::Executed;
    proposal.executed_task_id = Some(task_id.to_string());
    Ok(proposal)
}

/// Revert a confirmation when Task Queue submission fails; no domain action
/// has happened in this case, so the user may safely retry the proposal.
pub fn revert_confirmation(conn: &Connection, proposal_id: &str) -> AppResult<()> {
    conn.execute(
        "UPDATE ai_proposals SET status = 'pending', confirmed_at = NULL WHERE id = ?1 AND status = 'confirmed'",
        params![proposal_id],
    )?;
    Ok(())
}

pub fn new_proposal(
    request_id: Option<String>,
    action_kind: ActionKind,
    risk_level: RiskLevel,
    target_scope: Value,
    affected_repositories: Vec<String>,
    affected_files: Vec<String>,
    before_summary: impl Into<String>,
    after_summary: impl Into<String>,
    diff: Option<String>,
    command_preview: Option<String>,
    reversible: bool,
    action_payload: Value,
) -> (ActionProposal, Value) {
    let now = Utc::now().to_rfc3339();
    let proposal = ActionProposal {
        proposal_id: uuid::Uuid::new_v4().to_string(),
        request_id,
        action_kind,
        risk_level,
        target_scope,
        affected_repositories,
        affected_files,
        before_summary: before_summary.into(),
        after_summary: after_summary.into(),
        diff,
        command_preview,
        reversible,
        expires_at: expires_at(),
        status: ProposalStatus::Pending,
        confirmed_at: None,
        executed_task_id: None,
        created_at: now,
    };
    (proposal, action_payload)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        crate::db::init_db(&mut conn).unwrap();
        conn
    }

    fn sample(conn: &Connection, risk: RiskLevel) -> ActionProposal {
        let (proposal, payload) = new_proposal(
            Some("request-1".into()),
            ActionKind::GitCreateCommit,
            risk,
            serde_json::json!({"repoPath":"/ws/repo"}),
            vec!["/ws/repo".into()],
            vec!["a.txt".into()],
            "dirty",
            "commit changes",
            None,
            Some("git commit -m <message>".into()),
            true,
            serde_json::json!({"message":"test","files":["a.txt"]}),
        );
        insert(conn, &proposal, &payload).unwrap();
        proposal
    }

    #[test]
    fn state_machine_covers_confirm_execute_reject() {
        let conn = db();
        let p = sample(&conn, RiskLevel::Medium);
        assert_eq!(confirm(&conn, &p.proposal_id, false).unwrap().status, ProposalStatus::Confirmed);
        assert_eq!(mark_executed(&conn, &p.proposal_id, "task-1").unwrap().status, ProposalStatus::Executed);
        assert!(matches!(reject(&conn, &p.proposal_id), Err(AppError::Ai(AiError::ProposalStateInvalid { .. }))));
    }

    #[test]
    fn high_risk_requires_second_confirmation() {
        let conn = db();
        let p = sample(&conn, RiskLevel::High);
        assert!(matches!(confirm(&conn, &p.proposal_id, false), Err(AppError::Ai(AiError::ActionConfirmationRequired { .. }))));
        assert_eq!(confirm(&conn, &p.proposal_id, true).unwrap().status, ProposalStatus::Confirmed);
    }

    #[test]
    fn expired_proposal_has_no_transition() {
        let conn = db();
        let mut p = sample(&conn, RiskLevel::Low);
        p.expires_at = (Utc::now() - Duration::minutes(1)).to_rfc3339();
        conn.execute("UPDATE ai_proposals SET expires_at = ?1 WHERE id = ?2", params![p.expires_at, p.proposal_id]).unwrap();
        assert!(matches!(confirm(&conn, &p.proposal_id, false), Err(AppError::Ai(AiError::ProposalExpired { .. }))));
        assert_eq!(get(&conn, &p.proposal_id).unwrap().unwrap().proposal.status, ProposalStatus::Expired);
    }
}
