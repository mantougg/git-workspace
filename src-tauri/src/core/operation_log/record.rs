//! Recording path of the operation log: pre-op ref snapshots, the
//! best-effort log write used by instrumented commands, and the DAO helpers
//! that persist log rows.
//!
//! Deliberately kept out of db/dao.rs while parallel task agents share the
//! tree — single-writer rules still apply: batch insert in one transaction,
//! prepared statement, no per-row round trips.

use std::path::Path;
use std::sync::{Arc, Mutex};

use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};

use crate::error::{AppError, AppResult};

use super::model::NewOperationLogItem;

/// Snapshot the repo's current HEAD as (branch short name, tip oid).
/// `ref_name` is empty for a detached HEAD (undo then restores the detached
/// oid). Returns None for an unborn HEAD — there is nothing to roll back to,
/// so the op is not loggable for that repo.
pub fn snapshot_head(repo_path: &Path) -> Option<(String, String)> {
    let repo = git2::Repository::open(repo_path).ok()?;
    let head = repo.head().ok()?;
    let oid = head.target()?;
    let name = if head.is_branch() {
        head.shorthand().unwrap_or_default().to_string()
    } else {
        String::new()
    };
    Some((name, oid.to_string()))
}

/// Snapshot a local branch tip as (branch name, tip oid); None when the
/// branch does not exist (nothing to record / restore).
pub fn snapshot_branch(repo_path: &Path, branch_name: &str) -> Option<(String, String)> {
    let repo = git2::Repository::open(repo_path).ok()?;
    let branch = repo
        .find_branch(branch_name, git2::BranchType::Local)
        .ok()?;
    let oid = branch.get().target()?;
    Some((branch_name.to_string(), oid.to_string()))
}

/// Best-effort log write for instrumented commands: resolves the workspace
/// from any involved repo path, then writes log + items in one transaction.
/// Failures only produce a log warning — the git operation already ran, and
/// a logging failure must never surface as an operation failure.
pub fn record_operation_best_effort(
    db: &Arc<Mutex<Connection>>,
    any_repo_path: &str,
    op_type: &str,
    summary: &str,
    items: Vec<NewOperationLogItem>,
) {
    if items.is_empty() {
        return;
    }
    match db.lock() {
        Ok(mut conn) => {
            let workspace_id = resolve_workspace_id(&conn, any_repo_path);
            if let Err(e) = insert_operation_log(&mut conn, workspace_id, op_type, summary, &items)
            {
                log::warn!("T-34: operation log write failed (op already ran): {}", e);
            }
        }
        Err(e) => log::warn!("T-34: operation log DB lock failed: {}", e),
    }
}

/// Resolve the workspace a repo path belongs to (None when the repo is not
/// registered, e.g. opened ad-hoc — the log row stays workspace-less).
pub(crate) fn resolve_workspace_id(conn: &Connection, repo_path: &str) -> Option<i64> {
    conn.query_row(
        "SELECT workspace_id FROM repositories WHERE path = ?1",
        params![repo_path],
        |r| r.get(0),
    )
    .optional()
    .ok()
    .flatten()
}

/// Insert one operation log plus all its per-repo items in a single
/// transaction. Returns the new log id.
pub(crate) fn insert_operation_log(
    conn: &mut Connection,
    workspace_id: Option<i64>,
    op_type: &str,
    summary: &str,
    items: &[NewOperationLogItem],
) -> AppResult<i64> {
    if items.is_empty() {
        return Err(AppError::Other(
            "operation log needs at least one repo snapshot".to_string(),
        ));
    }
    let now = Utc::now().to_rfc3339();
    let tx = conn.transaction()?;
    tx.execute(
        "INSERT INTO operation_logs (workspace_id, op_type, summary, created_at) VALUES (?1, ?2, ?3, ?4)",
        params![workspace_id, op_type, summary, now],
    )?;
    let log_id = tx.last_insert_rowid();
    {
        let mut stmt = tx.prepare(
            "INSERT INTO operation_log_items (log_id, repo_path, ref_name, before_oid, after_oid, detail)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        )?;
        for it in items {
            stmt.execute(params![
                log_id,
                it.repo_path,
                it.ref_name,
                it.before_oid,
                it.after_oid,
                it.detail
            ])?;
        }
    }
    tx.commit()?;
    Ok(log_id)
}
