//! Workspace Change Set commands (T-22).
//!
//! CRUD and repo association run against SQLite directly. The summary command
//! reuses the T-02 status cache (branch / ahead / behind — live fallback read
//! for uncached repos, same pattern as `list_repositories`) plus a per-repo
//! libgit2 diff-stat pass; it never triggers a workspace-wide rescan.
//!
//! Bulk Commit All / Push All are intentionally NOT new commands: the UI
//! submits them through the existing task queue (`batch_commit` / `batch_push`
//! → `submit_tasks`, T-05/T-20), so progress and partial failures surface in
//! the shared TaskPanel.

use std::path::Path;

use rayon::prelude::*;
use tauri::State;

use crate::core::change_set::{
    self, ChangeSet, ChangeSetRepo, ChangeSetRepoInput, ChangeSetRepoSummary, ChangeSetSummary,
    CreateChangeSetRequest, UpdateChangeSetRequest,
};
use crate::core::git_status;
use crate::error::{AppError, AppResult};
use crate::state::AppState;

fn lock_db<'a>(
    state: &'a State<'_, AppState>,
) -> AppResult<std::sync::MutexGuard<'a, rusqlite::Connection>> {
    state
        .db
        .lock()
        .map_err(|e| AppError::Other(format!("DB lock error: {}", e)))
}

/// List all change sets of a workspace (most recently updated first).
#[tauri::command]
pub fn list_change_sets(
    workspace_id: i64,
    state: State<'_, AppState>,
) -> AppResult<Vec<ChangeSet>> {
    let conn = lock_db(&state)?;
    change_set::list_change_sets(&conn, workspace_id)
}

/// Create a change set, optionally attaching an initial set of repositories
/// (validated and inserted together with the set in one transaction).
#[tauri::command]
pub fn create_change_set(
    req: CreateChangeSetRequest,
    state: State<'_, AppState>,
) -> AppResult<ChangeSet> {
    let name = req.name.trim();
    if name.is_empty() {
        return Err(AppError::Other("Change Set 名称不能为空".to_string()));
    }
    let mut conn = lock_db(&state)?;
    change_set::create_change_set(
        &mut conn,
        req.workspace_id,
        name,
        req.description.as_deref().map(str::trim).filter(|d| !d.is_empty()),
        &req.repos,
    )
}

/// Rename / re-describe a change set (`None` fields keep their values).
#[tauri::command]
pub fn update_change_set(
    req: UpdateChangeSetRequest,
    state: State<'_, AppState>,
) -> AppResult<ChangeSet> {
    if let Some(name) = req.name.as_deref() {
        if name.trim().is_empty() {
            return Err(AppError::Other("Change Set 名称不能为空".to_string()));
        }
    }
    let conn = lock_db(&state)?;
    change_set::update_change_set(&conn, req.id, req.name.as_deref(), req.description.as_deref())
}

/// Delete a change set (membership rows cascade).
#[tauri::command]
pub fn delete_change_set(id: i64, state: State<'_, AppState>) -> AppResult<()> {
    let conn = lock_db(&state)?;
    change_set::delete_change_set(&conn, id)
}

/// Add repos to (or update target branches within) a change set. Returns the
/// full membership after the change.
#[tauri::command]
pub fn add_change_set_repositories(
    change_set_id: i64,
    repos: Vec<ChangeSetRepoInput>,
    state: State<'_, AppState>,
) -> AppResult<Vec<ChangeSetRepo>> {
    let mut conn = lock_db(&state)?;
    change_set::add_change_set_repos(&mut conn, change_set_id, &repos)?;
    change_set::list_change_set_repos(&conn, change_set_id)
}

/// Remove one repo from a change set. Returns the remaining membership.
#[tauri::command]
pub fn remove_change_set_repository(
    change_set_id: i64,
    repo_id: i64,
    state: State<'_, AppState>,
) -> AppResult<Vec<ChangeSetRepo>> {
    let conn = lock_db(&state)?;
    change_set::remove_change_set_repo(&conn, change_set_id, repo_id)?;
    change_set::list_change_set_repos(&conn, change_set_id)
}

/// Aggregate summary (统一汇总): per-repo workdir diff stats (files / added /
/// deleted) plus branch and unpushed-commit counts from the T-02 status
/// cache. Member repos are processed in parallel (rayon); each repo opens
/// and drops its own libgit2 handle (global constraint §3). A repo that
/// fails to read degrades to a row with `error` set — it never fails the
/// whole summary.
#[tauri::command]
pub fn get_change_set_summary(
    id: i64,
    state: State<'_, AppState>,
) -> AppResult<ChangeSetSummary> {
    let (cs, repos) = {
        let conn = lock_db(&state)?;
        (
            change_set::get_change_set(&conn, id)?,
            change_set::list_change_set_repos(&conn, id)?,
        )
    };
    let status_cache = std::sync::Arc::clone(&state.status_cache);

    let rows: Vec<ChangeSetRepoSummary> = repos
        .into_par_iter()
        .map(|repo| {
            let path = Path::new(&repo.repo_path);

            // Branch / ahead / behind: T-02 status cache first, single live
            // fallback read (the `list_repositories` pattern — no rescan).
            let mut error = None;
            let (branch, ahead, behind) = match status_cache.get(&repo.repo_path) {
                Some(s) => (Some(s.branch.clone()), s.ahead, s.behind),
                None => match git_status::get_repo_status(path) {
                    Ok(s) => {
                        status_cache.insert(repo.repo_path.clone(), s.clone());
                        (Some(s.branch), s.ahead, s.behind)
                    }
                    Err(e) => {
                        error = Some(e.to_string());
                        (None, 0, 0)
                    }
                },
            };

            // Workdir diff stats (per-repo libgit2, streaming line count).
            let (files, added, deleted) = match change_set::change_stats(path) {
                Ok(st) => (st.files, st.added, st.deleted),
                Err(e) => {
                    error = Some(match error {
                        Some(prev) => format!("{}; {}", prev, e),
                        None => e.to_string(),
                    });
                    (0, 0, 0)
                }
            };

            ChangeSetRepoSummary {
                repo,
                current_branch: branch,
                ahead,
                behind,
                files,
                added,
                deleted,
                error,
            }
        })
        .collect();

    Ok(ChangeSetSummary {
        repositories: rows.len(),
        files: rows.iter().map(|r| r.files).sum(),
        added: rows.iter().map(|r| r.added).sum(),
        deleted: rows.iter().map(|r| r.deleted).sum(),
        commits: rows.iter().map(|r| r.ahead).sum(),
        change_set: cs,
        repos: rows,
    })
}
