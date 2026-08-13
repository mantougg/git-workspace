use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

use rayon::prelude::*;
use tauri::{Emitter, State};

use crate::core::git_status;
use crate::core::scanner::RepoScanner;
use crate::db::dao;
use crate::error::{AppError, AppResult};
use crate::models::group::{CreateGroupRequest, RepoGroup};
use crate::models::repository::{
    RepoChanges, RepoStatus, RepositoryWithStatus, ScanProgress,
};
use crate::state::AppState;

/// Get the file-level change list for every repository in a workspace.
/// Used to build the change tree on the home page.
#[tauri::command]
pub fn get_workspace_changes(
    workspace_id: i64,
    state: State<'_, AppState>,
) -> AppResult<Vec<RepoChanges>> {
    log::info!("get_workspace_changes called for workspace_id={}", workspace_id);
    let repos = {
        let conn = state
            .db
            .lock()
            .map_err(|e| AppError::Other(format!("DB lock error: {}", e)))?;
        dao::list_repositories_by_workspace(&conn, workspace_id)?
    };
    log::info!(
        "get_workspace_changes: {} repos loaded from DB",
        repos.len()
    );

    let mut result = Vec::with_capacity(repos.len());
    for repo in repos {
        match git_status::get_repo_changes(Path::new(&repo.path)) {
            Ok(mut changes) => {
                changes.relative_path = repo.relative_path;
                result.push(changes);
            }
            Err(e) => {
                log::warn!("Failed to read changes for {:?}: {}", repo.path, e);
                result.push(RepoChanges {
                    repo_path: repo.path,
                    repo_name: repo.name,
                    relative_path: repo.relative_path,
                    branch: "(error)".to_string(),
                    is_detached: false,
                    ahead: 0,
                    behind: 0,
                    changes: Vec::new(),
                });
            }
        }
    }
    log::info!(
        "get_workspace_changes: returning {} repo change summaries",
        result.len()
    );
    Ok(result)
}

/// Scan a workspace directory for Git repositories.
///
/// This is the primary discovery command:
/// 1. Reads the workspace path and scan_depth from DB
/// 2. Runs the multi-threaded scanner to find all `.git` directories
/// 3. Upserts found repos to DB and removes stale entries
/// 4. Reads Git status for each repository (branch, file counts, ahead/behind)
/// 5. Returns the complete list with live status
#[tauri::command]
pub fn scan_repositories(
    workspace_id: i64,
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
) -> AppResult<Vec<RepositoryWithStatus>> {
    // Scope DB operations so the MutexGuard is released before status_cache access
    let repos = {
        let mut conn = state
            .db
            .lock()
            .map_err(|e| AppError::Other(format!("DB lock error: {}", e)))?;

        // 1. Get workspace config
        let workspace = dao::get_workspace(&conn, workspace_id)?;

        // 2. Scan for repos
        let scanner = RepoScanner::new(workspace.scan_depth as usize);
        let scanned = scanner.scan(Path::new(&workspace.path));

        // Emit scan progress: scanning complete, now reading statuses
        let total = scanned.len();
        let _ = app_handle.emit(
            "scan_progress",
            &ScanProgress {
                workspace_id,
                found: total,
                current: 0,
                total: Some(total),
            },
        );

        // 3. Sync to DB: remove stale repos, upsert found repos
        let found_paths: Vec<String> =
            scanned.iter().map(|s| s.path.clone()).collect();
        dao::cleanup_stale_repositories(&conn, workspace_id, &found_paths)?;
        dao::upsert_repositories_batch(&mut conn, workspace_id, &scanned)?;

        // 4. Read all repos from DB (now includes IDs and metadata)
        dao::list_repositories_by_workspace(&conn, workspace_id)?
    }; // conn guard dropped here

    // 5. Get live Git status for each repo in parallel with progress reporting.
    // Concurrency is bounded by the rayon thread pool (Roadmap §45: status ~16).
    let total = repos.len();
    let done = AtomicUsize::new(0);
    let result: Vec<RepositoryWithStatus> = repos
        .into_par_iter()
        .map(|repo| {
            let (status, error) =
                match git_status::get_repo_status(Path::new(&repo.path)) {
                    Ok(s) => {
                        state
                            .status_cache
                            .insert(repo.path.clone(), s.clone());
                        (Some(s), None)
                    }
                    Err(e) => (None, Some(e.to_string())),
                };

            let current = done.fetch_add(1, Ordering::Relaxed) + 1;
            let _ = app_handle.emit(
                "scan_progress",
                &ScanProgress {
                    workspace_id,
                    found: total,
                    current,
                    total: Some(total),
                },
            );

            RepositoryWithStatus {
                repository: repo,
                status,
                last_error: error,
            }
        })
        .collect();

    Ok(result)
}

/// List repositories for a workspace from the database cache.
/// Does NOT trigger a new scan - use `scan_repositories` for that.
/// Statuses are read from the in-memory cache first, falling back to
/// live Git status if not cached.
#[tauri::command]
pub fn list_repositories(
    workspace_id: i64,
    state: State<'_, AppState>,
) -> AppResult<Vec<RepositoryWithStatus>> {
    let repos = {
        let conn = state
            .db
            .lock()
            .map_err(|e| AppError::Other(format!("DB lock error: {}", e)))?;
        dao::list_repositories_by_workspace(&conn, workspace_id)?
    };

    let result: Vec<RepositoryWithStatus> = repos
        .into_par_iter()
        .map(|repo| {
            // Try cache first
            if let Some(cached) = state.status_cache.get(&repo.path) {
                return RepositoryWithStatus {
                    repository: repo,
                    status: Some(cached.clone()),
                    last_error: None,
                };
            }
            // Fall back to live status
            let (status, error) =
                match git_status::get_repo_status(Path::new(&repo.path)) {
                    Ok(s) => {
                        state
                            .status_cache
                            .insert(repo.path.clone(), s.clone());
                        (Some(s), None)
                    }
                    Err(e) => (None, Some(e.to_string())),
                };
            RepositoryWithStatus {
                repository: repo,
                status,
                last_error: error,
            }
        })
        .collect();

    Ok(result)
}

/// Refresh the Git status of a single repository.
/// Updates the in-memory cache and returns the fresh status.
#[tauri::command]
pub fn refresh_repository_status(
    repo_path: String,
    state: State<'_, AppState>,
) -> AppResult<RepoStatus> {
    let status = git_status::get_repo_status(Path::new(&repo_path))?;
    state
        .status_cache
        .insert(repo_path, status.clone());
    Ok(status)
}

// ---------------------------------------------------------------------------
// Repository Group commands
// ---------------------------------------------------------------------------

/// List all groups for a workspace.
#[tauri::command]
pub fn list_groups(workspace_id: i64, state: State<'_, AppState>) -> AppResult<Vec<RepoGroup>> {
    let conn = state
        .db
        .lock()
        .map_err(|e| AppError::Other(format!("DB lock error: {}", e)))?;
    dao::list_groups(&conn, workspace_id)
}

/// Create a new repository group.
#[tauri::command]
pub fn create_group(req: CreateGroupRequest, state: State<'_, AppState>) -> AppResult<RepoGroup> {
    let conn = state
        .db
        .lock()
        .map_err(|e| AppError::Other(format!("DB lock error: {}", e)))?;
    dao::create_group(&conn, &req)
}

/// Delete a group by ID.
#[tauri::command]
pub fn delete_group(id: i64, state: State<'_, AppState>) -> AppResult<()> {
    let conn = state
        .db
        .lock()
        .map_err(|e| AppError::Other(format!("DB lock error: {}", e)))?;
    dao::delete_group(&conn, id)
}

/// Assign a repository to a group by repo path.
#[tauri::command]
pub fn assign_group(
    repo_path: String,
    group_id: Option<i64>,
    state: State<'_, AppState>,
) -> AppResult<()> {
    let conn = state
        .db
        .lock()
        .map_err(|e| AppError::Other(format!("DB lock error: {}", e)))?;
    dao::assign_group_by_path(&conn, &repo_path, group_id)
}
