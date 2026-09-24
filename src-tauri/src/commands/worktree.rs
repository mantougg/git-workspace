//! Worktree commands (T-17). Worktrees share the main repo's object store;
//! each linked worktree is also discovered by the scanner as its own repo
//! entry (`.git` file form) and thus participates in status/batch ops (T-02).

use std::path::Path;

use tauri::State;

use crate::core::operation_log::{self, NewOperationLogItem};
use crate::core::worktree as wt;
use crate::core::worktree::WorktreeInfo;
use crate::db::dao;
use crate::error::AppResult;
use crate::state::AppState;

/// List worktrees (main + linked) of a repository, persisting a snapshot into
/// the `worktrees` table (T-03). Purely local.
#[tauri::command]
pub fn list_worktrees(repo_path: String, state: State<'_, AppState>) -> AppResult<Vec<WorktreeInfo>> {
    let list = wt::list_worktrees(Path::new(&repo_path))?;

    // Persist the snapshot when the repository is registered in the DB.
    let mut conn = state
        .db
        .lock()
        .map_err(|e| crate::error::AppError::Other(format!("DB lock error: {}", e)))?;
    if let Some(repo_id) = dao::get_repository_id_by_path(&conn, &repo_path)? {
        let rows: Vec<(String, Option<String>)> = list.iter().map(|w| (w.path.clone(), w.branch.clone())).collect();
        dao::replace_worktrees(&mut conn, repo_id, &rows)?;
    }

    Ok(list)
}

/// Create a linked worktree (T-17): with `new_branch` a branch is created at
/// HEAD and checked out in the worktree; with `branch` an existing branch is
/// checked out; with neither the worktree is a detached HEAD.
#[tauri::command]
pub fn create_worktree(
    repo_path: String,
    path: String,
    branch: Option<String>,
    new_branch: Option<String>,
) -> AppResult<()> {
    wt::add_worktree(
        Path::new(&repo_path),
        Path::new(&path),
        branch.as_deref(),
        new_branch.as_deref(),
    )
}

/// Remove a linked worktree (T-17). A dirty worktree is refused unless
/// `force` is set (§46 Warning confirm flow in the UI).
///
/// T-34 (GF-16): the worktree's name / path / checked-out position is
/// snapshotted beforehand, so Undo can recreate it. Uncommitted changes of a
/// force-removed worktree are NOT recoverable — only the position is.
#[tauri::command]
pub fn remove_worktree(
    repo_path: String,
    name: String,
    force: bool,
    state: State<'_, AppState>,
) -> AppResult<()> {
    let snapshot = wt::snapshot_worktree(Path::new(&repo_path), &name);
    wt::remove_worktree(Path::new(&repo_path), &name, force)?;
    if let Some((snap, head_oid)) = snapshot {
        let branch = snap.branch.clone();
        let summary = match &branch {
            Some(b) => format!("移除 worktree '{}'（分支 {}）", snap.name, b),
            None => format!("移除 worktree '{}'（分离 HEAD）", snap.name),
        };
        operation_log::record_operation_best_effort(
            &state.db,
            &repo_path,
            operation_log::OP_WORKTREE_REMOVE,
            &summary,
            vec![NewOperationLogItem {
                repo_path: repo_path.clone(),
                ref_name: branch.unwrap_or_default(),
                before_oid: head_oid,
                after_oid: None,
                detail: Some(operation_log::encode_worktree_snapshot(&snap)),
            }],
        );
    }
    Ok(())
}
