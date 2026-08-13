use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use rayon::prelude::*;
use tauri::{Emitter, State};

use crate::core::git_status;
use crate::core::scanner::RepoScanner;
use crate::db::dao;
use crate::error::{AppError, AppResult};
use crate::models::group::{CreateGroupRequest, RepoGroup};
use crate::models::repository::{
    RepoChanges, RepoStatus, Repository, RepositoryWithStatus, ScanProgress,
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
    // 1. Read workspace config + known paths (brief lock, then release).
    let (workspace_path, scan_depth, known) = {
        let conn = state
            .db
            .lock()
            .map_err(|e| AppError::Other(format!("DB lock error: {}", e)))?;
        let workspace = dao::get_workspace(&conn, workspace_id)?;
        let known = dao::list_repository_paths(&conn, workspace_id)?;
        (
            workspace.path.clone(),
            workspace.scan_depth as usize,
            known,
        )
    };

    // 2. Scan without holding the DB lock (blocking disk IO + libgit2), so the
    // task manager's persistence and other DB commands are not stalled.
    let scanner = RepoScanner::new(scan_depth);
    let scanned = scanner.scan_incremental(Path::new(&workspace_path), None, &known);

    // Emit scan progress: scanning complete, now reading statuses.
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

    // 3. Sync to DB (brief lock): remove stale repos, upsert found repos.
    let repos = {
        let mut conn = state
            .db
            .lock()
            .map_err(|e| AppError::Other(format!("DB lock error: {}", e)))?;
        let found_paths: Vec<String> =
            scanned.iter().map(|s| s.path.clone()).collect();
        dao::cleanup_stale_repositories(&conn, workspace_id, &found_paths)?;
        dao::upsert_repositories_batch(&mut conn, workspace_id, &scanned)?;
        dao::list_repositories_by_workspace(&conn, workspace_id)?
    };

    // If the file watcher is running, sync its watched set with the new repo
    // list: newly discovered repos get mounted, removed ones unmounted.
    sync_watcher(&state, &app_handle, &repos);

    Ok(repos_with_status(repos, workspace_id, &app_handle, &state))
}

/// Scan a specific subtree of a workspace (Scan Selected).
///
/// Only repositories under `sub_path` are discovered, upserted, and stale-
/// cleaned; repositories outside the subtree are left untouched. `sub_path`
/// must be inside the workspace root (guarded against path traversal).
#[tauri::command]
pub fn scan_repository_subtree(
    workspace_id: i64,
    sub_path: String,
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
) -> AppResult<Vec<RepositoryWithStatus>> {
    let sub_root = Path::new(&sub_path);

    // 1. Read workspace config, resolve the subtree, and load known paths
    // (brief lock, then release).
    let (scan_depth, scan_root, known) = {
        let conn = state
            .db
            .lock()
            .map_err(|e| AppError::Other(format!("DB lock error: {}", e)))?;

        let workspace = dao::get_workspace(&conn, workspace_id)?;
        let workspace_root = Path::new(&workspace.path);

        // Guard against path traversal: the subtree must live inside the root.
        // Resolve both to canonical paths (resolving `..` and symlinks); if
        // either cannot be resolved, reject rather than fall back to a raw
        // prefix comparison that could let `D:/ws/../x` through.
        let canonical_root = workspace_root.canonicalize().map_err(|e| {
            AppError::Other(format!(
                "Cannot resolve workspace root {:?}: {}",
                workspace.path, e
            ))
        })?;
        let canonical_sub = sub_root.canonicalize().map_err(|e| {
            AppError::Other(format!(
                "Cannot resolve subtree {:?}: {}",
                sub_path, e
            ))
        })?;
        let rel = canonical_sub.strip_prefix(&canonical_root).map_err(|_| {
            AppError::Other(format!(
                "Subtree {:?} is outside workspace {:?}",
                sub_path, workspace.path
            ))
        })?;

        // Reconstruct the scan root in the workspace's own (non-canonical) path
        // form so scanned paths match what a full scan writes to the DB. On
        // Windows, `canonicalize` adds a `\\?\` verbatim prefix and normalizes
        // case, which would desync from stored paths; joining the canonical
        // relative part onto the original workspace root keeps them consistent.
        let scan_root = workspace_root.join(rel);

        // Restrict the incremental-scan cache to repositories within the subtree.
        let all_known = dao::list_repository_paths(&conn, workspace_id)?;
        let known: std::collections::HashMap<String, Option<i64>> = all_known
            .into_iter()
            .filter(|(p, _)| is_within(p, &scan_root.to_string_lossy()))
            .collect();

        (workspace.scan_depth as usize, scan_root, known)
    };

    // 2. Scan without holding the DB lock (blocking disk IO + libgit2).
    let scanner = RepoScanner::new(scan_depth);
    let scanned = scanner.scan_incremental(&scan_root, None, &known);

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

    // 3. Sync to DB (brief lock): soft-delete vanished subtree repos + upsert.
    let repos = {
        let mut conn = state
            .db
            .lock()
            .map_err(|e| AppError::Other(format!("DB lock error: {}", e)))?;

        let scanned_paths: std::collections::HashSet<String> =
            scanned.iter().map(|s| s.path.clone()).collect();
        let stale: Vec<String> = known
            .keys()
            .filter(|p| !scanned_paths.contains(*p))
            .cloned()
            .collect();
        dao::soft_delete_repositories(&conn, workspace_id, &stale)?;
        dao::upsert_repositories_batch(&mut conn, workspace_id, &scanned)?;

        // Return every repository in the workspace so the UI can refresh its
        // full list; the newly scanned subtree repos are included.
        dao::list_repositories_by_workspace(&conn, workspace_id)?
    };

    // Subtree scans can also discover/remove repositories — sync the watcher.
    sync_watcher(&state, &app_handle, &repos);

    Ok(repos_with_status(repos, workspace_id, &app_handle, &state))
}

/// If the file watcher is running, synchronize its watched set with the
/// repository list after a scan — newly discovered repos are mounted and
/// removed ones are unmounted automatically.
fn sync_watcher(state: &AppState, app_handle: &tauri::AppHandle, repos: &[Repository]) {
    let running = state
        .watcher
        .lock()
        .map(|w| w.is_running())
        .unwrap_or(false);
    if !running {
        return;
    }

    let paths: Vec<PathBuf> = repos.iter().map(|r| PathBuf::from(&r.path)).collect();
    if let Ok(mut watcher) = state.watcher.lock() {
        if let Err(e) =
            watcher.watch_repositories(paths, Arc::clone(&state.status_cache), app_handle.clone())
        {
            log::warn!("Failed to sync watcher after scan: {}", e);
        }
    }
}

/// Read live Git status for a set of repositories in parallel, updating the
/// in-memory cache and emitting `scan_progress` events. Concurrency is bounded
/// by the rayon thread pool (Roadmap §45: status ~16).
fn repos_with_status(
    repos: Vec<Repository>,
    workspace_id: i64,
    app_handle: &tauri::AppHandle,
    state: &AppState,
) -> Vec<RepositoryWithStatus> {
    let total = repos.len();
    let done = AtomicUsize::new(0);
    repos
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
        .collect()
}

/// True when `path` equals `root` or lives under it (path-prefix match on the
/// string representation, matching how the scanner emits `relative_path`).
///
/// A trailing separator on `root` is tolerated, and the boundary is checked so
/// `root = D:/ws/a` does not match `D:/ws/ab`.
fn is_within(path: &str, root: &str) -> bool {
    let root = root.trim_end_matches(['/', '\\']);
    if root.is_empty() {
        // Filesystem root ("/" or a run of separators): everything is inside.
        return true;
    }
    if path == root {
        return true;
    }
    if !path.starts_with(root) {
        return false;
    }
    let rest = &path[root.len()..];
    rest.starts_with('/') || rest.starts_with('\\')
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
                    status: Some(cached),
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

#[cfg(test)]
mod tests {
    use super::is_within;

    #[test]
    fn is_within_matches_exact_and_descendant() {
        assert!(is_within("D:/ws/a", "D:/ws/a"));
        assert!(is_within("D:/ws/a/b", "D:/ws/a"));
        assert!(is_within("D:/ws/a\\b", "D:/ws/a"));
    }

    #[test]
    fn is_within_rejects_sibling_prefix() {
        assert!(!is_within("D:/ws/ab", "D:/ws/a"));
        assert!(!is_within("D:/ws/a2", "D:/ws/a"));
        assert!(!is_within("D:/ws", "D:/ws/a"));
    }

    #[test]
    fn is_within_tolerates_trailing_separator_and_root() {
        assert!(is_within("D:/ws/a/b", "D:/ws/a/"));
        assert!(is_within("D:/ws/a", "D:/ws/a/"));
        assert!(is_within("anything", ""));
    }
}
