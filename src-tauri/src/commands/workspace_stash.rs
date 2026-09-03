//! Workspace stash commands (T-21). The git phase runs *outside* the DB
//! lock (the single-writer connection stays responsive); the association
//! record is then written in one transaction.

use std::sync::MutexGuard;

use rusqlite::Connection;
use tauri::State;

use crate::core::workspace_stash::{
    self, SaveWorkspaceStashResult, WorkspaceStashCheckItem, WorkspaceStashItemEntry,
    WorkspaceStashRepoOutcome, WorkspaceStashSummary,
};
use crate::error::{AppError, AppResult};
use crate::state::AppState;

fn lock_db<'a>(state: &'a State<'a, AppState>) -> AppResult<MutexGuard<'a, Connection>> {
    state
        .db
        .lock()
        .map_err(|e| AppError::Other(format!("DB lock error: {}", e)))
}

/// Save a workspace stash (T-21): stash every selected repo (T-10 semantics,
/// untracked per flag), then persist the `Workspace Stash #N` record with
/// its per-repo items in one transaction. Repos with a clean worktree are
/// skipped; per-repo failures are collected in the result. When nothing was
/// stashed, no record is written (`id` is None).
#[tauri::command]
pub fn save_workspace_stash(
    workspace_id: i64,
    repo_paths: Vec<String>,
    message: Option<String>,
    include_untracked: Option<bool>,
    state: State<'_, AppState>,
) -> AppResult<SaveWorkspaceStashResult> {
    if repo_paths.is_empty() {
        return Err(AppError::Other("没有选定仓库".into()));
    }
    let name = {
        let conn = lock_db(&state)?;
        workspace_stash::next_workspace_stash_name(&conn, workspace_id)?
    };
    let include_untracked = include_untracked.unwrap_or(true);

    // Git phase without holding the DB lock.
    let (outcomes, stashed) =
        workspace_stash::stash_repos(&repo_paths, &name, message.as_deref(), include_untracked);

    if stashed.is_empty() {
        return Ok(SaveWorkspaceStashResult {
            id: None,
            name,
            items: outcomes,
        });
    }

    let mut conn = lock_db(&state)?;
    let id = workspace_stash::insert_workspace_stash(
        &mut conn,
        workspace_id,
        &name,
        message.as_deref().map(str::trim).filter(|m| !m.is_empty()),
        &stashed,
    )?;
    Ok(SaveWorkspaceStashResult {
        id: Some(id),
        name,
        items: outcomes,
    })
}

/// List the workspace stash records of a workspace, newest first.
#[tauri::command]
pub fn list_workspace_stashes(
    workspace_id: i64,
    state: State<'_, AppState>,
) -> AppResult<Vec<WorkspaceStashSummary>> {
    let conn = lock_db(&state)?;
    workspace_stash::list_workspace_stashes(&conn, workspace_id)
}

/// Per-repo items of one workspace stash record.
#[tauri::command]
pub fn get_workspace_stash_items(
    workspace_stash_id: i64,
    state: State<'_, AppState>,
) -> AppResult<Vec<WorkspaceStashItemEntry>> {
    let conn = lock_db(&state)?;
    workspace_stash::list_workspace_stash_items(&conn, workspace_stash_id)
}

/// Pre-restore safety check (T-21 §46): per repo, is the stash still on the
/// stack and is the current branch the recorded one? The UI shows this list
/// in the Warning-level confirmation before restoring.
#[tauri::command]
pub fn check_workspace_stash(
    workspace_stash_id: i64,
    state: State<'_, AppState>,
) -> AppResult<Vec<WorkspaceStashCheckItem>> {
    let items = {
        let conn = lock_db(&state)?;
        workspace_stash::list_workspace_stash_items(&conn, workspace_stash_id)?
    };
    if items.is_empty() {
        return Err(AppError::NotFound(
            "该 Workspace Stash 没有仓库项".into(),
        ));
    }
    Ok(workspace_stash::check_restore(&items))
}

/// Restore a workspace stash: re-check every repo, then apply its stash
/// (kept on the stack). Repos failing the check are skipped — a branch
/// mismatch applies only with `allow_branch_mismatch` — and per-repo
/// failures are collected instead of blocking the rest.
#[tauri::command]
pub fn restore_workspace_stash(
    workspace_stash_id: i64,
    allow_branch_mismatch: Option<bool>,
    state: State<'_, AppState>,
) -> AppResult<Vec<WorkspaceStashRepoOutcome>> {
    let items = {
        let conn = lock_db(&state)?;
        workspace_stash::list_workspace_stash_items(&conn, workspace_stash_id)?
    };
    if items.is_empty() {
        return Err(AppError::NotFound(
            "该 Workspace Stash 没有仓库项".into(),
        ));
    }
    Ok(workspace_stash::restore_items(
        &items,
        allow_branch_mismatch.unwrap_or(false),
    ))
}

/// Delete a workspace stash record (items cascade). The per-repo stashes
/// stay on each repo's stack and remain manageable in the T-10 Stash view.
#[tauri::command]
pub fn delete_workspace_stash(
    workspace_stash_id: i64,
    state: State<'_, AppState>,
) -> AppResult<()> {
    let conn = lock_db(&state)?;
    workspace_stash::delete_workspace_stash(&conn, workspace_stash_id)
}
