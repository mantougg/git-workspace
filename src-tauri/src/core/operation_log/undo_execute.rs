//! Undo execution: apply the planned reverse operation per repo (parallel,
//! local libgit2 only — no network) and persist the outcome to the log.
//! Every pending item is re-checked via `plan_item` right before execution,
//! so repos whose HEAD / branch / worktree moved on since the operation are
//! refused (§4.6). The git execution never touches the DB;
//! `persist_undo_results` runs afterwards on the caller's connection, so the
//! DB lock is never held across repo IO.

use std::path::Path;

use chrono::Utc;
use rayon::prelude::*;
use rusqlite::{params, Connection};

use crate::core::branch;
use crate::error::AppResult;

use super::undo_plan::{plan_item, repo_name_of, reset_mode, short_oid};
use super::{
    OperationLogDetail, OperationLogItem, UndoItemResult, OP_CHECKOUT_ALL, OP_DELETE_BRANCH_ALL,
    OP_REBASE, OP_RESET,
};

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
        let oid =
            git2::Oid::from_str(&item.before_oid).map_err(|_| "记录的 oid 无效".to_string())?;
        let obj = repo
            .find_object(oid, Some(git2::ObjectType::Commit))
            .map_err(|e| e.message().to_string())?;
        // Safe checkout (same protection as branch checkout): local changes
        // that would be overwritten abort the undo with an error.
        repo.checkout_tree(&obj, None)
            .map_err(|e| format!("检出操作前提交失败：{}", e.message()))?;
        repo.set_head_detached(oid)
            .map_err(|e| e.message().to_string())?;
        Ok(format!(
            "已恢复分离 HEAD 到 {}",
            short_oid(&item.before_oid)
        ))
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
