//! Stash commands (T-10). Single-repo scope; workspace stash is T-21.

use std::path::Path;

use tauri::State;

use crate::core::diff::FileDiff;
use crate::core::operation_log::{self, NewOperationLogItem};
use crate::core::stash::{self, StashEntry, StashSnapshotEntry};
use crate::db::dao;
use crate::error::AppResult;
use crate::state::AppState;

/// List the stash stack (newest first) and persist a snapshot into the
/// `stashes` table so the list survives restarts.
#[tauri::command]
pub fn list_stashes(repo_path: String, state: State<'_, AppState>) -> AppResult<Vec<StashEntry>> {
    let entries = stash::list_stashes(Path::new(&repo_path))?;

    let mut conn = state
        .db
        .lock()
        .map_err(|e| crate::error::AppError::Other(format!("DB lock error: {}", e)))?;
    if let Some(repo_id) = dao::get_repository_id_by_path(&conn, &repo_path)? {
        let rows: Vec<(String, Option<String>, String)> = entries
            .iter()
            .map(|s| {
                (
                    format!("stash@{{{}}}", s.index),
                    Some(s.message.clone()),
                    s.time.clone(),
                )
            })
            .collect();
        dao::replace_stashes(&mut conn, repo_id, &rows)?;
    }

    Ok(entries)
}

/// Stash the working-tree changes (optionally including untracked files).
/// Returns the stash commit oid.
#[tauri::command]
pub fn stash_changes(repo_path: String, message: Option<String>, include_untracked: Option<bool>) -> AppResult<String> {
    stash::stash_save(
        Path::new(&repo_path),
        message.as_deref(),
        include_untracked.unwrap_or(false),
    )
}

/// Apply a stash entry, keeping it on the stack.
#[tauri::command]
pub fn apply_stash(repo_path: String, index: usize) -> AppResult<()> {
    stash::stash_apply(Path::new(&repo_path), index)
}

/// Apply a stash entry and drop it from the stack.
#[tauri::command]
pub fn pop_stash(repo_path: String, index: usize) -> AppResult<()> {
    stash::stash_pop(Path::new(&repo_path), index)
}

/// Drop a stash entry (Warning-level op, the UI confirms first).
///
/// T-34 (GF-16): the whole pre-op stack (oid + reflog message per entry) is
/// snapshotted first, so Undo can restore the dropped entry (and its exact
/// stack position) into `refs/stash`.
#[tauri::command]
pub fn drop_stash(repo_path: String, index: usize, state: State<'_, AppState>) -> AppResult<()> {
    let before = stash::snapshot_stash_stack(Path::new(&repo_path));
    stash::stash_drop(Path::new(&repo_path), index)?;
    record_stash_op(&state.db, &repo_path, "drop", index, before);
    Ok(())
}

/// Clear the whole stash stack (Warning-level op). Returns how many were dropped.
///
/// T-34 (GF-16): same snapshot model as `drop_stash` — the cleared stack is
/// recorded so Undo can restore it.
#[tauri::command]
pub fn clear_stashes(repo_path: String, state: State<'_, AppState>) -> AppResult<usize> {
    let before = stash::snapshot_stash_stack(Path::new(&repo_path));
    let cleared = stash::stash_clear(Path::new(&repo_path))?;
    record_stash_op(&state.db, &repo_path, "clear", 0, before);
    Ok(cleared)
}

/// Best-effort operation-log write for a stash drop / clear. The stash ref
/// itself has no before/after tip worth snapshotting (entries vanish), so the
/// item carries the stack snapshot in `detail` and the dropped/top oid as
/// before-oid.
fn record_stash_op(
    db: &std::sync::Arc<std::sync::Mutex<rusqlite::Connection>>,
    repo_path: &str,
    kind: &str,
    index: usize,
    stack: Vec<StashSnapshotEntry>,
) {
    if stack.is_empty() {
        return;
    }
    let (op_type, summary, before_oid) = if kind == "drop" {
        let Some((oid, msg)) = stack.get(index) else {
            return; // index out of range — nothing was recorded to restore
        };
        (
            operation_log::OP_STASH_DROP,
            format!("丢弃 stash@{{{}}}（{}）", index, stash_msg_excerpt(msg)),
            oid.clone(),
        )
    } else {
        (
            operation_log::OP_STASH_CLEAR,
            format!("清空 stash 栈（{} 条记录）", stack.len()),
            stack[0].0.clone(),
        )
    };
    operation_log::record_operation_best_effort(
        db,
        repo_path,
        op_type,
        &summary,
        vec![NewOperationLogItem {
            repo_path: repo_path.to_string(),
            ref_name: "stash".to_string(),
            before_oid,
            // The stash ref keeps pointing at the remaining top entry; the
            // meaningful state is the stack snapshot in `detail`.
            after_oid: None,
            detail: Some(operation_log::encode_stash_snapshot(&stack)),
        }],
    );
}

/// Char-bounded excerpt of a stash reflog message for the log summary.
fn stash_msg_excerpt(msg: &str) -> String {
    let mut out: String = msg.chars().take(30).collect();
    if msg.chars().count() > 30 {
        out.push('…');
    }
    out
}

/// Diff of a stash entry against its base commit (tracked changes).
#[tauri::command]
pub fn get_stash_diff(repo_path: String, index: usize) -> AppResult<Vec<FileDiff>> {
    stash::stash_diff(Path::new(&repo_path), index)
}

/// Create a branch from a stash entry: branch at the stash's base commit,
/// checkout, apply the stash, drop it on success.
#[tauri::command]
pub fn branch_from_stash(repo_path: String, branch_name: String, index: usize) -> AppResult<()> {
    stash::branch_from_stash(Path::new(&repo_path), &branch_name, index)
}
