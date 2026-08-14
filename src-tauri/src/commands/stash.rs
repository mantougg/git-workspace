//! Stash commands (T-10). Single-repo scope; workspace stash is T-21.

use std::path::Path;

use tauri::State;

use crate::core::diff::FileDiff;
use crate::core::stash::{self, StashEntry};
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
pub fn stash_changes(
    repo_path: String,
    message: Option<String>,
    include_untracked: Option<bool>,
) -> AppResult<String> {
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
#[tauri::command]
pub fn drop_stash(repo_path: String, index: usize) -> AppResult<()> {
    stash::stash_drop(Path::new(&repo_path), index)
}

/// Clear the whole stash stack (Warning-level op). Returns how many were dropped.
#[tauri::command]
pub fn clear_stashes(repo_path: String) -> AppResult<usize> {
    stash::stash_clear(Path::new(&repo_path))
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
