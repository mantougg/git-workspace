//! Unified operation log + undo (T-34): the third Safety First layer
//! (Roadmap §46/§47) next to pre-op confirmation and the reflog — record
//! what a high-risk operation changed, then undo it with the reverse op.
//!
//! Initial scope (per task spec): Checkout All / Delete Branch All (T-20
//! batch branch ops), Reset, Rebase. Before the op runs, a per-repo ref
//! snapshot is captured (ref name + tip oid — pure data, never libgit2
//! handles, global constraint §3); after the op succeeds, the log row plus
//! all per-repo items are written in one transaction on the single-writer
//! connection (T-03), so failed ops leave no fake records. Undo applies the
//! reverse operation per repo with local libgit2 only (Offline First — no
//! network), is gated by a §46 Dangerous-level confirmation in the UI, and
//! refuses any repo whose state has moved on since the operation.

use std::path::Path;
use std::sync::{Arc, Mutex};

use chrono::Utc;
use rayon::prelude::*;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

use crate::core::branch;
use crate::error::{AppError, AppResult};

/// op_type of a batch checkout (T-20 Checkout All).
pub const OP_CHECKOUT_ALL: &str = "checkout_all";
/// op_type of a batch branch delete (T-20 Delete Branch All).
pub const OP_DELETE_BRANCH_ALL: &str = "delete_branch_all";
/// op_type of a `reset_to` (soft/mixed/hard; the mode is kept in the item's
/// detail so undo can mirror it).
pub const OP_RESET: &str = "reset";
/// op_type of a completed rebase.
pub const OP_REBASE: &str = "rebase";

// ---------------------------------------------------------------------------
// IPC types (serde camelCase; mirrored in src/types/operationLog.ts)
// ---------------------------------------------------------------------------

/// One page of operation log summaries plus the total matching count.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationLogPage {
    pub total: i64,
    pub logs: Vec<OperationLogSummary>,
}

/// List-row view of one logged operation (items aggregated).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationLogSummary {
    pub id: i64,
    pub workspace_id: Option<i64>,
    pub op_type: String,
    pub summary: String,
    pub created_at: String,
    pub undone_at: Option<String>,
    /// How many per-repo items the log has.
    pub repo_count: i64,
    /// How many of them are already undone.
    pub undone_count: i64,
}

/// One per-repo ref snapshot of a logged operation.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationLogItem {
    pub id: i64,
    pub log_id: i64,
    pub repo_path: String,
    /// Short branch name (e.g. "main"); empty when HEAD was detached.
    pub ref_name: String,
    pub before_oid: String,
    /// Tip after the operation; None when unknown (async batch ops) or the
    /// ref ceased to exist (branch delete).
    pub after_oid: Option<String>,
    /// Op-specific extra (e.g. "mode:hard" for reset, "onto:x" for rebase).
    pub detail: Option<String>,
    pub undone_at: Option<String>,
}

/// Full detail of one logged operation including all per-repo items.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationLogDetail {
    pub id: i64,
    pub workspace_id: Option<i64>,
    pub op_type: String,
    pub summary: String,
    pub created_at: String,
    pub undone_at: Option<String>,
    pub items: Vec<OperationLogItem>,
}

/// One repo's undo plan row for the §46 confirmation dialog.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UndoPreviewItem {
    pub item_id: i64,
    pub repo_path: String,
    pub repo_name: String,
    /// Human-readable reverse action, e.g. "重建分支 'feature' → a1b2c3d".
    pub action: String,
    /// Whether the reverse op can run safely right now.
    pub ok: bool,
    /// Safety-check detail (why not ok, or a note); empty when ok.
    pub message: String,
    /// Already undone (reported for completeness; skipped on execution).
    pub undone: bool,
}

/// Per-repo outcome of an undo run.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UndoItemResult {
    pub item_id: i64,
    pub repo_path: String,
    pub repo_name: String,
    pub success: bool,
    pub message: String,
}

/// Aggregate outcome of an undo run over one operation log.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UndoOutcome {
    pub log_id: i64,
    /// True when every item of the log is undone (log marked undone).
    pub fully_undone: bool,
    pub results: Vec<UndoItemResult>,
}

// ---------------------------------------------------------------------------
// Recording (used by the instrumented commands in batch/history/merge_rebase)
// ---------------------------------------------------------------------------

/// One per-repo snapshot row to insert together with a new operation log.
#[derive(Debug, Clone)]
pub struct NewOperationLogItem {
    pub repo_path: String,
    /// Short branch name; empty when HEAD was detached.
    pub ref_name: String,
    pub before_oid: String,
    pub after_oid: Option<String>,
    pub detail: Option<String>,
}

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

// ---------------------------------------------------------------------------
// DAO helpers (pub(crate); deliberately kept out of db/dao.rs while parallel
// task agents share the tree — single-writer rules still apply: batch insert
// in one transaction, prepared statement, no per-row round trips).
// ---------------------------------------------------------------------------

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

/// Query filters for the operation log list. `date_from`/`date_to` are
/// `YYYY-MM-DD` bounds compared against the UTC date part of `created_at`.
#[derive(Debug, Default)]
pub(crate) struct LogFilter<'a> {
    pub workspace_id: Option<i64>,
    pub repo_path: Option<&'a str>,
    pub op_type: Option<&'a str>,
    pub date_from: Option<&'a str>,
    pub date_to: Option<&'a str>,
}

const LOG_WHERE: &str = "WHERE (?1 IS NULL OR l.workspace_id = ?1)
       AND (?2 IS NULL OR l.op_type = ?2)
       AND (?3 IS NULL OR EXISTS (SELECT 1 FROM operation_log_items x
                                  WHERE x.log_id = l.id
                                    AND x.repo_path LIKE '%' || ?3 || '%'))
       AND (?4 IS NULL OR substr(l.created_at, 1, 10) >= ?4)
       AND (?5 IS NULL OR substr(l.created_at, 1, 10) <= ?5)";

/// Query one page of operation logs (newest first) plus the total count.
pub(crate) fn query_operation_logs(
    conn: &Connection,
    filter: &LogFilter,
    limit: i64,
    offset: i64,
) -> AppResult<OperationLogPage> {
    let where_params = params![
        filter.workspace_id,
        filter.op_type,
        filter.repo_path,
        filter.date_from,
        filter.date_to
    ];
    let total: i64 = conn.query_row(
        &format!("SELECT COUNT(*) FROM operation_logs l {LOG_WHERE}"),
        where_params,
        |r| r.get(0),
    )?;

    let mut stmt = conn.prepare(&format!(
        "SELECT l.id, l.workspace_id, l.op_type, l.summary, l.created_at, l.undone_at,
                (SELECT COUNT(*) FROM operation_log_items i WHERE i.log_id = l.id),
                (SELECT COUNT(*) FROM operation_log_items i WHERE i.log_id = l.id AND i.undone_at IS NOT NULL)
         FROM operation_logs l {LOG_WHERE}
         ORDER BY l.id DESC LIMIT ?6 OFFSET ?7"
    ))?;
    let logs = stmt
        .query_map(
            params![
                filter.workspace_id,
                filter.op_type,
                filter.repo_path,
                filter.date_from,
                filter.date_to,
                limit,
                offset
            ],
            |row| {
                Ok(OperationLogSummary {
                    id: row.get(0)?,
                    workspace_id: row.get(1)?,
                    op_type: row.get(2)?,
                    summary: row.get(3)?,
                    created_at: row.get(4)?,
                    undone_at: row.get(5)?,
                    repo_count: row.get(6)?,
                    undone_count: row.get(7)?,
                })
            },
        )?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(OperationLogPage { total, logs })
}

/// Load one operation log with all its per-repo items.
pub(crate) fn get_operation_log(conn: &Connection, log_id: i64) -> AppResult<OperationLogDetail> {
    let (id, workspace_id, op_type, summary, created_at, undone_at) = conn
        .query_row(
            "SELECT id, workspace_id, op_type, summary, created_at, undone_at
             FROM operation_logs WHERE id = ?1",
            params![log_id],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, Option<i64>>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, Option<String>>(5)?,
                ))
            },
        )
        .optional()?
        .ok_or_else(|| AppError::NotFound(format!("operation log {} not found", log_id)))?;

    let mut stmt = conn.prepare(
        "SELECT id, log_id, repo_path, ref_name, before_oid, after_oid, detail, undone_at
         FROM operation_log_items WHERE log_id = ?1 ORDER BY id",
    )?;
    let items = stmt
        .query_map(params![log_id], |row| {
            Ok(OperationLogItem {
                id: row.get(0)?,
                log_id: row.get(1)?,
                repo_path: row.get(2)?,
                ref_name: row.get(3)?,
                before_oid: row.get(4)?,
                after_oid: row.get(5)?,
                detail: row.get(6)?,
                undone_at: row.get(7)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(OperationLogDetail {
        id,
        workspace_id,
        op_type,
        summary,
        created_at,
        undone_at,
        items,
    })
}

/// Persist undo results: mark each succeeded item undone (idempotent), then
/// mark the whole log undone when no pending item remains. Returns whether
/// the log is now fully undone. One transaction (single-writer model).
pub(crate) fn persist_undo_results(
    conn: &mut Connection,
    log_id: i64,
    results: &[UndoItemResult],
) -> AppResult<bool> {
    let now = Utc::now().to_rfc3339();
    let tx = conn.transaction()?;
    for r in results.iter().filter(|r| r.success) {
        tx.execute(
            "UPDATE operation_log_items SET undone_at = ?1 WHERE id = ?2 AND undone_at IS NULL",
            params![now, r.item_id],
        )?;
    }
    let pending: i64 = tx.query_row(
        "SELECT COUNT(*) FROM operation_log_items WHERE log_id = ?1 AND undone_at IS NULL",
        params![log_id],
        |r| r.get(0),
    )?;
    if pending == 0 {
        tx.execute(
            "UPDATE operation_logs SET undone_at = ?1 WHERE id = ?2 AND undone_at IS NULL",
            params![now, log_id],
        )?;
    }
    tx.commit()?;
    Ok(pending == 0)
}

// ---------------------------------------------------------------------------
// Undo planning + execution (local libgit2 only; no DB access here so the DB
// lock is never held across repo IO).
// ---------------------------------------------------------------------------

fn repo_name_of(repo_path: &str) -> String {
    Path::new(repo_path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string()
}

fn short_oid(oid: &str) -> &str {
    &oid[..7.min(oid.len())]
}

/// Compute the undo plan (reverse action + live safety check) of every item.
/// Parallel over repos; pure repo IO, no DB.
pub fn preview_undo(detail: &OperationLogDetail) -> Vec<UndoPreviewItem> {
    detail
        .items
        .par_iter()
        .map(|item| {
            let plan = plan_item(&detail.op_type, item);
            UndoPreviewItem {
                item_id: item.id,
                repo_path: item.repo_path.clone(),
                repo_name: repo_name_of(&item.repo_path),
                action: plan.action,
                ok: plan.check.is_ok(),
                message: plan.check.err().unwrap_or_default(),
                undone: item.undone_at.is_some(),
            }
        })
        .collect()
}

/// Execute the undo of every pending item (parallel over repos). Items
/// already undone or failing the safety check are reported but not touched.
/// Call `persist_undo_results` afterwards to record the outcome.
pub fn run_undo(detail: &OperationLogDetail) -> Vec<UndoItemResult> {
    detail
        .items
        .par_iter()
        .map(|item| {
            let repo_name = repo_name_of(&item.repo_path);
            let base = |success: bool, message: String| UndoItemResult {
                item_id: item.id,
                repo_path: item.repo_path.clone(),
                repo_name: repo_name.clone(),
                success,
                message,
            };
            if item.undone_at.is_some() {
                return base(true, "此前已撤销".to_string());
            }
            let plan = plan_item(&detail.op_type, item);
            if let Err(reason) = plan.check {
                return base(false, reason);
            }
            match execute_item(&detail.op_type, item) {
                Ok(msg) => base(true, msg),
                Err(e) => base(false, e),
            }
        })
        .collect()
}

struct UndoPlan {
    action: String,
    /// Ok(()) when the reverse op may run; Err(reason) when unsafe / moot.
    check: Result<(), String>,
}

fn plan_item(op_type: &str, item: &OperationLogItem) -> UndoPlan {
    if item.undone_at.is_some() {
        return UndoPlan {
            action: String::new(),
            check: Err("已撤销".to_string()),
        };
    }
    match op_type {
        OP_CHECKOUT_ALL => plan_checkout_undo(item),
        OP_DELETE_BRANCH_ALL => plan_delete_undo(item),
        OP_RESET => plan_ref_rollback(item, false),
        OP_REBASE => plan_ref_rollback(item, true),
        other => UndoPlan {
            action: String::new(),
            check: Err(format!("操作类型 '{}' 不支持撤销", other)),
        },
    }
}

/// Checkout All undo: switch back to the ref recorded before the batch op.
fn plan_checkout_undo(item: &OperationLogItem) -> UndoPlan {
    let path = Path::new(&item.repo_path);
    let action = if item.ref_name.is_empty() {
        format!("恢复分离 HEAD 到 {}", short_oid(&item.before_oid))
    } else {
        format!("切回分支 '{}'（{}）", item.ref_name, short_oid(&item.before_oid))
    };
    let check = (|| -> Result<(), String> {
        let repo = git2::Repository::open(path)
            .map_err(|e| format!("仓库无法打开：{}", e.message()))?;
        if item.ref_name.is_empty() {
            // Detached-before case: the commit must still exist.
            let oid = git2::Oid::from_str(&item.before_oid)
                .map_err(|_| "记录的 oid 无效".to_string())?;
            repo.find_commit(oid)
                .map_err(|_| "操作前提交已不存在（可能已被 GC）".to_string())?;
            let head_oid = repo.head().ok().and_then(|h| h.target());
            if head_oid == Some(oid) {
                return Err("HEAD 已处于操作前提交，无需撤销".to_string());
            }
        } else {
            repo.find_branch(&item.ref_name, git2::BranchType::Local)
                .map_err(|_| format!("原分支 '{}' 已不存在，无法切回", item.ref_name))?;
            let current = repo
                .head()
                .ok()
                .filter(|h| h.is_branch())
                .and_then(|h| h.shorthand().map(String::from));
            if current.as_deref() == Some(item.ref_name.as_str()) {
                return Err("已处于原分支，无需撤销".to_string());
            }
        }
        Ok(())
    })();
    UndoPlan { action, check }
}

/// Delete Branch All undo: recreate the branch at its recorded tip.
fn plan_delete_undo(item: &OperationLogItem) -> UndoPlan {
    let path = Path::new(&item.repo_path);
    let action = format!(
        "重建分支 '{}' → {}",
        item.ref_name,
        short_oid(&item.before_oid)
    );
    let check = (|| -> Result<(), String> {
        let repo = git2::Repository::open(path)
            .map_err(|e| format!("仓库无法打开：{}", e.message()))?;
        if repo
            .find_branch(&item.ref_name, git2::BranchType::Local)
            .is_ok()
        {
            return Err(format!("分支 '{}' 已存在（可能已人工重建）", item.ref_name));
        }
        let oid = git2::Oid::from_str(&item.before_oid)
            .map_err(|_| "记录的 oid 无效".to_string())?;
        repo.find_commit(oid)
            .map_err(|_| "删除前的分支提交已不存在（可能已被 GC）".to_string())?;
        Ok(())
    })();
    UndoPlan { action, check }
}

/// Reset / Rebase undo: roll the branch ref back to the recorded before-oid.
/// Hard-mode rollbacks additionally require a clean worktree; rebase undo is
/// always hard (the rebase itself required a clean tree). Safety rule (§46
/// 可恢复): refuse when the branch has moved on from the recorded after-oid —
/// undoing would silently discard later commits.
fn plan_ref_rollback(item: &OperationLogItem, is_rebase: bool) -> UndoPlan {
    let path = Path::new(&item.repo_path);
    let mode = if is_rebase {
        "hard".to_string()
    } else {
        reset_mode(item.detail.as_deref())
    };
    let action = if is_rebase {
        format!(
            "将分支 '{}' 硬回退到 rebase 前 {}",
            item.ref_name,
            short_oid(&item.before_oid)
        )
    } else {
        format!(
            "将分支 '{}' 回退到 {}（reset --{}）",
            item.ref_name,
            short_oid(&item.before_oid),
            mode
        )
    };
    let check = (|| -> Result<(), String> {
        let after = item
            .after_oid
            .as_deref()
            .ok_or_else(|| "缺少操作后快照，无法校验当前状态，拒绝撤销".to_string())?;
        let repo = git2::Repository::open(path)
            .map_err(|e| format!("仓库无法打开：{}", e.message()))?;
        // An in-progress rebase has its own recovery (rebase_abort); rolling
        // refs underneath it would corrupt that state.
        if crate::core::rebase::get_rebase_state(path)
            .ok()
            .flatten()
            .is_some()
        {
            return Err("仓库存在进行中的 rebase，请先 continue / abort".to_string());
        }
        let head = repo.head().map_err(|e| format!("HEAD 异常：{}", e.message()))?;
        if !item.ref_name.is_empty() {
            let current = if head.is_branch() {
                head.shorthand().unwrap_or_default()
            } else {
                ""
            };
            if current != item.ref_name {
                return Err(format!(
                    "当前分支 '{}' 与记录的分支 '{}' 不符，拒绝撤销",
                    current, item.ref_name
                ));
            }
        }
        let tip = head
            .target()
            .ok_or_else(|| "HEAD 未指向提交".to_string())?
            .to_string();
        if tip != after {
            return Err(format!(
                "已有后续变更（当前 {} ≠ 操作后 {}），拒绝自动撤销",
                short_oid(&tip),
                short_oid(after)
            ));
        }
        let before = git2::Oid::from_str(&item.before_oid)
            .map_err(|_| "记录的 oid 无效".to_string())?;
        repo.find_commit(before)
            .map_err(|_| "操作前提交已不存在（可能已被 GC）".to_string())?;
        if tip == item.before_oid {
            return Err("分支已处于操作前状态，无需撤销".to_string());
        }
        if mode == "hard" && worktree_dirty(&repo) {
            return Err("工作区存在未提交变更，硬回退会丢失，已拒绝".to_string());
        }
        Ok(())
    })();
    UndoPlan { action, check }
}

/// Parse the recorded reset mode from the item detail ("mode:hard" etc.);
/// defaults to mixed (never touches the worktree) when unknown.
fn reset_mode(detail: Option<&str>) -> String {
    match detail.and_then(|d| d.strip_prefix("mode:")) {
        Some("soft") => "soft".to_string(),
        Some("hard") => "hard".to_string(),
        _ => "mixed".to_string(),
    }
}

/// Conservative dirty probe: any tracked modification or untracked file
/// counts; on error assume dirty (fail-safe).
fn worktree_dirty(repo: &git2::Repository) -> bool {
    let mut opts = git2::StatusOptions::new();
    opts.include_untracked(true);
    repo.statuses(Some(&mut opts))
        .map(|s| !s.is_empty())
        .unwrap_or(true)
}

fn execute_item(op_type: &str, item: &OperationLogItem) -> Result<String, String> {
    let path = Path::new(&item.repo_path);
    match op_type {
        OP_CHECKOUT_ALL => undo_checkout(path, item),
        OP_DELETE_BRANCH_ALL => undo_delete(path, item),
        OP_RESET => undo_ref_rollback(path, item, &reset_mode(item.detail.as_deref())),
        OP_REBASE => undo_ref_rollback(path, item, "hard"),
        other => Err(format!("操作类型 '{}' 不支持撤销", other)),
    }
}

fn undo_checkout(path: &Path, item: &OperationLogItem) -> Result<String, String> {
    if item.ref_name.is_empty() {
        let repo = git2::Repository::open(path).map_err(|e| e.message().to_string())?;
        let oid = git2::Oid::from_str(&item.before_oid)
            .map_err(|_| "记录的 oid 无效".to_string())?;
        let obj = repo
            .find_object(oid, Some(git2::ObjectType::Commit))
            .map_err(|e| e.message().to_string())?;
        // Safe checkout (same protection as branch checkout): local changes
        // that would be overwritten abort the undo with an error.
        repo.checkout_tree(&obj, None)
            .map_err(|e| format!("检出操作前提交失败：{}", e.message()))?;
        repo.set_head_detached(oid)
            .map_err(|e| e.message().to_string())?;
        Ok(format!("已恢复分离 HEAD 到 {}", short_oid(&item.before_oid)))
    } else {
        branch::checkout_branch(path, &item.ref_name)
            .map_err(|e| format!("切回分支失败：{}", e))?;
        Ok(format!("已切回分支 '{}'", item.ref_name))
    }
}

fn undo_delete(path: &Path, item: &OperationLogItem) -> Result<String, String> {
    let repo = git2::Repository::open(path).map_err(|e| e.message().to_string())?;
    let oid = git2::Oid::from_str(&item.before_oid).map_err(|_| "记录的 oid 无效".to_string())?;
    let commit = repo.find_commit(oid).map_err(|e| e.message().to_string())?;
    // force = false: an existing branch errors out instead of being moved.
    repo.branch(&item.ref_name, &commit, false)
        .map_err(|e| format!("重建分支失败：{}", e.message()))?;
    Ok(format!(
        "已重建分支 '{}' → {}",
        item.ref_name,
        short_oid(&item.before_oid)
    ))
}

fn undo_ref_rollback(path: &Path, item: &OperationLogItem, mode: &str) -> Result<String, String> {
    let repo = git2::Repository::open(path).map_err(|e| e.message().to_string())?;
    let oid = git2::Oid::from_str(&item.before_oid).map_err(|_| "记录的 oid 无效".to_string())?;
    let obj = repo
        .find_object(oid, Some(git2::ObjectType::Commit))
        .map_err(|e| e.message().to_string())?;
    match mode {
        "soft" => repo
            .reset(&obj, git2::ResetType::Soft, None)
            .map_err(|e| e.message().to_string())?,
        "hard" => {
            let mut co = git2::build::CheckoutBuilder::new();
            co.force();
            repo.reset(&obj, git2::ResetType::Hard, Some(&mut co))
                .map_err(|e| e.message().to_string())?;
        }
        _ => repo
            .reset(&obj, git2::ResetType::Mixed, None)
            .map_err(|e| e.message().to_string())?,
    }
    Ok(format!(
        "已回退到 {}（reset --{}）",
        short_oid(&item.before_oid),
        mode
    ))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "gw_oplog_{}_{}",
            tag,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn open_db() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        crate::db::init_db(&mut conn).unwrap();
        conn
    }

    fn commit_file(repo: &git2::Repository, dir: &Path, name: &str, content: &str, msg: &str) {
        std::fs::write(dir.join(name), content).unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new(name)).unwrap();
        index.write().unwrap();
        let tree_oid = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_oid).unwrap();
        let sig = git2::Signature::now("tester", "t@example.com").unwrap();
        let parent = repo
            .head()
            .ok()
            .and_then(|h| h.target())
            .map(|oid| repo.find_commit(oid).unwrap());
        let parents: Vec<&git2::Commit> = parent.iter().collect();
        repo.commit(Some("HEAD"), &sig, &sig, msg, &tree, &parents)
            .unwrap();
    }

    fn init_repo(dir: &Path) -> git2::Repository {
        let repo = git2::Repository::init(dir).unwrap();
        commit_file(&repo, dir, "a.txt", "one\n", "c1");
        repo
    }

    fn head_tip(dir: &Path) -> String {
        git2::Repository::open(dir)
            .unwrap()
            .head()
            .unwrap()
            .target()
            .unwrap()
            .to_string()
    }

    fn item(repo: &Path, ref_name: &str, before: &str, after: Option<&str>) -> NewOperationLogItem {
        NewOperationLogItem {
            repo_path: repo.to_string_lossy().to_string(),
            ref_name: ref_name.to_string(),
            before_oid: before.to_string(),
            after_oid: after.map(String::from),
            detail: None,
        }
    }

    /// Insert + query roundtrip with every filter dimension, workspace
    /// resolution, and detail loading (acceptance: 日志可按仓库/时间/类型查询).
    #[test]
    fn log_insert_and_query_roundtrip() {
        let mut conn = open_db();
        conn.execute(
            "INSERT INTO workspaces (name, path, created_at, updated_at) VALUES ('w', 'D:/w', 't', 't')",
            [],
        )
        .unwrap();
        let ws_id = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO repositories (workspace_id, path, name, relative_path, created_at, updated_at)
             VALUES (?1, 'D:/w/repo-a', 'repo-a', 'repo-a', 't', 't')",
            params![ws_id],
        )
        .unwrap();
        assert_eq!(resolve_workspace_id(&conn, "D:/w/repo-a"), Some(ws_id));
        assert_eq!(resolve_workspace_id(&conn, "D:/nowhere"), None);

        let items = vec![
            NewOperationLogItem {
                repo_path: "D:/w/repo-a".into(),
                ref_name: "main".into(),
                before_oid: "a".repeat(40),
                after_oid: Some("b".repeat(40)),
                detail: Some("mode:hard".into()),
            },
            NewOperationLogItem {
                repo_path: "D:/w/repo-b".into(),
                ref_name: "dev".into(),
                before_oid: "c".repeat(40),
                after_oid: None,
                detail: None,
            },
        ];
        let log_id =
            insert_operation_log(&mut conn, Some(ws_id), OP_RESET, "reset --hard", &items).unwrap();

        // Unfiltered page.
        let page = query_operation_logs(&conn, &LogFilter::default(), 50, 0).unwrap();
        assert_eq!(page.total, 1);
        assert_eq!(page.logs[0].repo_count, 2);
        assert_eq!(page.logs[0].undone_count, 0);
        assert_eq!(page.logs[0].workspace_id, Some(ws_id));

        // Filter by workspace / op_type / repo substring.
        let f = LogFilter { workspace_id: Some(ws_id), ..Default::default() };
        assert_eq!(query_operation_logs(&conn, &f, 50, 0).unwrap().total, 1);
        let f = LogFilter { workspace_id: Some(ws_id + 9), ..Default::default() };
        assert_eq!(query_operation_logs(&conn, &f, 50, 0).unwrap().total, 0);
        let f = LogFilter { op_type: Some(OP_RESET), ..Default::default() };
        assert_eq!(query_operation_logs(&conn, &f, 50, 0).unwrap().total, 1);
        let f = LogFilter { op_type: Some(OP_REBASE), ..Default::default() };
        assert_eq!(query_operation_logs(&conn, &f, 50, 0).unwrap().total, 0);
        let f = LogFilter { repo_path: Some("repo-b"), ..Default::default() };
        assert_eq!(query_operation_logs(&conn, &f, 50, 0).unwrap().total, 1);
        let f = LogFilter { repo_path: Some("nope"), ..Default::default() };
        assert_eq!(query_operation_logs(&conn, &f, 50, 0).unwrap().total, 0);

        // Date filter against the UTC date part of created_at.
        let today = Utc::now().format("%Y-%m-%d").to_string();
        let f = LogFilter { date_from: Some(&today), date_to: Some(&today), ..Default::default() };
        assert_eq!(query_operation_logs(&conn, &f, 50, 0).unwrap().total, 1);
        let f = LogFilter { date_from: Some("1999-01-01"), date_to: Some("1999-01-02"), ..Default::default() };
        assert_eq!(query_operation_logs(&conn, &f, 50, 0).unwrap().total, 0);

        // Detail loads all items in order.
        let detail = get_operation_log(&conn, log_id).unwrap();
        assert_eq!(detail.items.len(), 2);
        assert_eq!(detail.items[0].ref_name, "main");
        assert_eq!(detail.items[1].after_oid, None);
        assert!(get_operation_log(&conn, log_id + 99).is_err());

        // Empty items are rejected.
        assert!(insert_operation_log(&mut conn, None, OP_RESET, "x", &[]).is_err());
    }

    /// Checkout All undo: HEAD returns to the recorded branch, and the
    /// undo marks items + log as undone (acceptance: 一键撤销恢复操作前状态).
    #[test]
    fn undo_checkout_restores_original_branch() {
        let dir = tmpdir("checkout");
        let master_tip = {
            let repo = init_repo(&dir);
            let tip = repo.head().unwrap().target().unwrap().to_string();
            drop(repo);
            branch::create_branch(&dir, "feature", None).unwrap();
            tip
        };
        // Simulate the batch op: repo was on master, now checked out to feature.
        branch::checkout_branch(&dir, "feature").unwrap();

        let mut conn = open_db();
        let log_id = insert_operation_log(
            &mut conn,
            None,
            OP_CHECKOUT_ALL,
            "批量检出 'feature'",
            &[item(&dir, "master", &master_tip, None)],
        )
        .unwrap();
        let detail = get_operation_log(&conn, log_id).unwrap();

        // Preview says the reverse op is safe.
        let preview = preview_undo(&detail);
        assert_eq!(preview.len(), 1);
        assert!(preview[0].ok, "{}", preview[0].message);
        assert!(preview[0].action.contains("master"));

        let results = run_undo(&detail);
        assert!(results[0].success, "{}", results[0].message);
        let repo = git2::Repository::open(&dir).unwrap();
        let head = repo.head().unwrap();
        assert_eq!(head.shorthand(), Some("master"));
        drop(head);
        drop(repo);

        // Persisting marks the item and, with nothing pending, the whole log.
        let fully = persist_undo_results(&mut conn, log_id, &results).unwrap();
        assert!(fully);
        let detail = get_operation_log(&conn, log_id).unwrap();
        assert!(detail.undone_at.is_some());
        assert!(detail.items.iter().all(|i| i.undone_at.is_some()));

        // A second run is a graceful no-op.
        let results = run_undo(&detail);
        assert!(results[0].success);
        assert!(results[0].message.contains("此前已撤销"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Delete Branch All undo: the branch is recreated at its recorded tip.
    #[test]
    fn undo_delete_recreates_branch_at_tip() {
        let dir = tmpdir("delete");
        {
            let repo = init_repo(&dir);
            drop(repo);
        }
        branch::create_branch(&dir, "work", None).unwrap();
        branch::checkout_branch(&dir, "work").unwrap();
        {
            let repo = git2::Repository::open(&dir).unwrap();
            commit_file(&repo, &dir, "b.txt", "two\n", "work commit");
            drop(repo);
        }
        let work_tip = head_tip(&dir);
        branch::checkout_branch(&dir, "master").unwrap();
        branch::delete_branch(&dir, "work", true).unwrap();

        let mut conn = open_db();
        let log_id = insert_operation_log(
            &mut conn,
            None,
            OP_DELETE_BRANCH_ALL,
            "批量删除分支 'work'",
            &[item(&dir, "work", &work_tip, None)],
        )
        .unwrap();
        let detail = get_operation_log(&conn, log_id).unwrap();

        let preview = preview_undo(&detail);
        assert!(preview[0].ok, "{}", preview[0].message);

        let results = run_undo(&detail);
        assert!(results[0].success, "{}", results[0].message);
        {
            let repo = git2::Repository::open(&dir).unwrap();
            let b = repo.find_branch("work", git2::BranchType::Local).unwrap();
            assert_eq!(b.get().target().unwrap().to_string(), work_tip);
        }

        // Undo again (results not yet persisted): the branch now exists, so
        // the safety check refuses to clobber it.
        let results = run_undo(&get_operation_log(&conn, log_id).unwrap());
        assert!(!results[0].success);
        assert!(results[0].message.contains("已存在"), "{}", results[0].message);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Read a text file with CRLF normalized — the Windows CI git config
    /// (core.autocrlf) rewrites checked-out files.
    fn read_norm(path: &Path) -> String {
        std::fs::read_to_string(path).unwrap().replace("\r\n", "\n")
    }

    /// Reset --hard undo: the branch ref rolls back and the worktree files
    /// are restored to the before-state (acceptance: ref 回退可验证).
    #[test]
    fn undo_hard_reset_restores_tip_and_files() {
        let dir = tmpdir("reset");
        let (c1, c2) = {
            let repo = init_repo(&dir);
            let c1 = repo.head().unwrap().target().unwrap().to_string();
            commit_file(&repo, &dir, "a.txt", "two\n", "c2");
            let c2 = repo.head().unwrap().target().unwrap().to_string();
            drop(repo);
            (c1, c2)
        };
        // The op: reset --hard to c1 (before = c2, after = c1).
        crate::core::history::reset_to(&dir, Some(&c1), "hard").unwrap();
        assert_eq!(read_norm(&dir.join("a.txt")), "one\n");

        let mut conn = open_db();
        let mut it = item(&dir, "master", &c2, Some(&c1));
        it.detail = Some("mode:hard".into());
        let log_id =
            insert_operation_log(&mut conn, None, OP_RESET, "reset --hard", &[it]).unwrap();
        let detail = get_operation_log(&conn, log_id).unwrap();

        let results = run_undo(&detail);
        assert!(results[0].success, "{}", results[0].message);
        assert_eq!(head_tip(&dir), c2);
        assert_eq!(read_norm(&dir.join("a.txt")), "two\n");

        let fully = persist_undo_results(&mut conn, log_id, &results).unwrap();
        assert!(fully);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Safety (§46): undo refuses repos whose ref moved on after the op, and
    /// hard rollbacks refuse a dirty worktree; the log stays un-undone.
    #[test]
    fn undo_refuses_when_state_moved_on() {
        let dir = tmpdir("moved");
        let (c1, c2) = {
            let repo = init_repo(&dir);
            let c1 = repo.head().unwrap().target().unwrap().to_string();
            commit_file(&repo, &dir, "a.txt", "two\n", "c2");
            let c2 = repo.head().unwrap().target().unwrap().to_string();
            drop(repo);
            (c1, c2)
        };
        crate::core::history::reset_to(&dir, Some(&c1), "hard").unwrap();

        let mut conn = open_db();
        let mut it = item(&dir, "master", &c2, Some(&c1));
        it.detail = Some("mode:hard".into());
        let log_id =
            insert_operation_log(&mut conn, None, OP_RESET, "reset --hard", &[it]).unwrap();

        // Case 1: a later commit on top of the reset target -> refuse.
        {
            let repo = git2::Repository::open(&dir).unwrap();
            commit_file(&repo, &dir, "a.txt", "three\n", "c3");
            drop(repo);
        }
        let detail = get_operation_log(&conn, log_id).unwrap();
        let preview = preview_undo(&detail);
        assert!(!preview[0].ok);
        assert!(preview[0].message.contains("后续变更"), "{}", preview[0].message);
        let results = run_undo(&detail);
        assert!(!results[0].success);
        let fully = persist_undo_results(&mut conn, log_id, &results).unwrap();
        assert!(!fully, "failed items keep the log un-undone");
        assert!(get_operation_log(&conn, log_id).unwrap().undone_at.is_none());

        // Case 2: back at the after-oid but with a dirty worktree (hard undo
        // would lose those edits) -> refuse.
        crate::core::history::reset_to(&dir, Some(&c1), "hard").unwrap();
        std::fs::write(dir.join("dirty.txt"), "x\n").unwrap();
        let detail = get_operation_log(&conn, log_id).unwrap();
        let preview = preview_undo(&detail);
        assert!(!preview[0].ok);
        assert!(preview[0].message.contains("未提交变更"), "{}", preview[0].message);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Rebase undo: hard rollback to the pre-rebase head; refused while a
    /// rebase is still in progress.
    #[test]
    fn undo_rebase_rolls_back_and_respects_in_progress() {
        let dir = tmpdir("rebase");
        let (c1, c2) = {
            let repo = init_repo(&dir);
            let c1 = repo.head().unwrap().target().unwrap().to_string();
            commit_file(&repo, &dir, "a.txt", "two\n", "c2");
            let c2 = repo.head().unwrap().target().unwrap().to_string();
            drop(repo);
            (c1, c2)
        };

        let mut conn = open_db();
        let mut it = item(&dir, "master", &c1, Some(&c2));
        it.detail = Some("onto:upstream".into());
        let log_id =
            insert_operation_log(&mut conn, None, OP_REBASE, "rebase → upstream", &[it]).unwrap();

        // In-progress rebase state blocks the undo (rebase_abort owns recovery).
        let state = serde_json::json!({
            "originalHead": c1, "onto": "upstream", "ops": [], "position": 0, "prevCommit": c1
        });
        std::fs::write(
            dir.join(".git").join("gitworkspace-rebase.json"),
            state.to_string(),
        )
        .unwrap();
        let detail = get_operation_log(&conn, log_id).unwrap();
        let preview = preview_undo(&detail);
        assert!(!preview[0].ok);
        assert!(preview[0].message.contains("rebase"), "{}", preview[0].message);
        std::fs::remove_file(dir.join(".git").join("gitworkspace-rebase.json")).unwrap();

        // Clean state at the after-oid: rollback to c1 succeeds.
        let detail = get_operation_log(&conn, log_id).unwrap();
        let results = run_undo(&detail);
        assert!(results[0].success, "{}", results[0].message);
        assert_eq!(head_tip(&dir), c1);
        assert_eq!(read_norm(&dir.join("a.txt")), "one\n");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Snapshot helpers: branch tips and HEAD (branch + detached forms).
    #[test]
    fn snapshots_capture_ref_and_oid() {
        let dir = tmpdir("snapshot");
        let tip = {
            let repo = init_repo(&dir);
            let tip = repo.head().unwrap().target().unwrap().to_string();
            drop(repo);
            tip
        };
        assert_eq!(
            snapshot_head(&dir),
            Some(("master".to_string(), tip.clone()))
        );
        assert_eq!(
            snapshot_branch(&dir, "master"),
            Some(("master".to_string(), tip.clone()))
        );
        assert_eq!(snapshot_branch(&dir, "missing"), None);

        // Detached HEAD: empty ref_name, oid preserved.
        {
            let repo = git2::Repository::open(&dir).unwrap();
            let oid = git2::Oid::from_str(&tip).unwrap();
            repo.set_head_detached(oid).unwrap();
            drop(repo);
        }
        assert_eq!(snapshot_head(&dir), Some((String::new(), tip)));

        // Unborn HEAD: not loggable.
        let empty = tmpdir("unborn");
        {
            let repo = git2::Repository::init(&empty).unwrap();
            drop(repo);
        }
        assert_eq!(snapshot_head(&empty), None);

        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&empty);
    }
}
