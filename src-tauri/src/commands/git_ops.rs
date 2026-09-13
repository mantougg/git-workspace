use std::path::Path;

use tauri::{Emitter, State};

use crate::core::git_ops::GitOps;
use crate::core::git_status;
use crate::db::dao;
use crate::error::AppResult;
use crate::models::commit::{CommitIdentity, CommitScanFinding};
use crate::models::repository::RepoStatus;
use crate::models::task::{TaskRequest, TaskType};
use crate::state::AppState;

/// Batch fetch: create Fetch tasks for each repo path and submit to the task queue.
/// Returns the list of task IDs.
#[tauri::command]
pub fn batch_fetch(repo_paths: Vec<String>, state: State<'_, AppState>) -> AppResult<Vec<String>> {
    let requests: Vec<TaskRequest> = repo_paths
        .iter()
        .map(|p| {
            let name = Path::new(p)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown")
                .to_string();
            TaskRequest {
                task_type: TaskType::Fetch,
                repo_path: p.clone(),
                repo_name: name,
            }
        })
        .collect();

    state.task_manager.submit(&requests)
}

/// Batch pull: create Pull tasks for each repo path.
#[tauri::command]
pub fn batch_pull(repo_paths: Vec<String>, state: State<'_, AppState>) -> AppResult<Vec<String>> {
    let requests: Vec<TaskRequest> = repo_paths
        .iter()
        .map(|p| {
            let name = Path::new(p)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown")
                .to_string();
            TaskRequest {
                task_type: TaskType::Pull,
                repo_path: p.clone(),
                repo_name: name,
            }
        })
        .collect();

    state.task_manager.submit(&requests)
}

/// Batch push: create Push tasks for each repo path.
#[tauri::command]
pub fn batch_push(repo_paths: Vec<String>, state: State<'_, AppState>) -> AppResult<Vec<String>> {
    let requests: Vec<TaskRequest> = repo_paths
        .iter()
        .map(|p| {
            let name = Path::new(p)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown")
                .to_string();
            TaskRequest {
                task_type: TaskType::Push,
                repo_path: p.clone(),
                repo_name: name,
            }
        })
        .collect();

    state.task_manager.submit(&requests)
}

/// Batch commit: create Commit tasks with a message and optional file list.
/// Each entry in `commits` specifies the repo path, message, and files.
/// The commit identity is resolved server-side (repo > group > git default).
#[tauri::command]
pub fn batch_commit(commits: Vec<CommitRequest>, state: State<'_, AppState>) -> AppResult<Vec<String>> {
    let conn = state
        .db
        .lock()
        .map_err(|e| crate::error::AppError::Other(format!("DB lock error: {}", e)))?;

    let requests: Vec<TaskRequest> = commits
        .into_iter()
        .map(|c| {
            let identity = dao::resolve_commit_identity(&conn, &c.repo_path).ok().flatten();
            TaskRequest {
                task_type: TaskType::Commit {
                    message: c.message,
                    files: c.files,
                    amend: c.amend,
                    no_edit: c.no_edit,
                    index_only: c.index_only,
                    then_push: c.then_push,
                    allow_unsafe: c.allow_unsafe,
                    author_name: identity.as_ref().map(|i| i.name.clone()),
                    author_email: identity.as_ref().map(|i| i.email.clone()),
                },
                repo_path: c.repo_path.clone(),
                repo_name: c.repo_name,
            }
        })
        .collect();
    drop(conn);

    state.task_manager.submit(&requests)
}

/// Pre-commit safety scan (T-11, §5): list findings (forbidden / large file /
/// secret) for the paths that would be committed, without committing. The UI
/// shows these and lets the user explicitly override via `allow_unsafe`.
#[tauri::command]
pub fn scan_commit(repo_path: String, files: Vec<String>, index_only: bool) -> AppResult<Vec<CommitScanFinding>> {
    crate::core::git_ops::pre_commit_scan(Path::new(&repo_path), &files, index_only)
}

/// Resolved commit identity for a repository (T-11 §54): repo override >
/// group override; `None` means the git default signature is used.
#[tauri::command]
pub fn get_commit_identity(repo_path: String, state: State<'_, AppState>) -> AppResult<Option<CommitIdentity>> {
    let conn = state
        .db
        .lock()
        .map_err(|e| crate::error::AppError::Other(format!("DB lock error: {}", e)))?;
    dao::resolve_commit_identity(&conn, &repo_path)
}

/// Set or clear the per-repository commit identity override (T-11 §54).
/// Both `None` clears the override.
#[tauri::command]
pub fn set_repo_identity(
    repo_path: String,
    name: Option<String>,
    email: Option<String>,
    state: State<'_, AppState>,
) -> AppResult<()> {
    let conn = state
        .db
        .lock()
        .map_err(|e| crate::error::AppError::Other(format!("DB lock error: {}", e)))?;
    dao::set_repo_identity(&conn, &repo_path, name.as_deref(), email.as_deref())
}

/// Set or clear the per-group commit identity override (T-11 §54).
#[tauri::command]
pub fn set_group_identity(
    group_id: i64,
    name: Option<String>,
    email: Option<String>,
    state: State<'_, AppState>,
) -> AppResult<()> {
    let conn = state
        .db
        .lock()
        .map_err(|e| crate::error::AppError::Other(format!("DB lock error: {}", e)))?;
    dao::set_group_identity(&conn, group_id, name.as_deref(), email.as_deref())
}

/// Sync fetch for a single repo (synchronous, not queued).
/// Useful for quick status refresh without the task system.
#[tauri::command]
pub fn sync_fetch(repo_path: String) -> AppResult<()> {
    let ops = GitOps::with_default_ssh();
    ops.fetch(Path::new(&repo_path)).map(|_| ())
}

/// Sync pull for a single repo (synchronous, not queued).
/// Returns the refreshed status after pulling.
#[tauri::command]
pub fn sync_pull(repo_path: String) -> AppResult<RepoStatus> {
    let ops = GitOps::with_default_ssh();
    ops.pull(Path::new(&repo_path))?;

    // Return fresh status after pull
    let status = git_status::get_repo_status(Path::new(&repo_path))?;
    Ok(status)
}

/// Sync push for a single repo (synchronous, not queued).
#[tauri::command]
pub fn sync_push(repo_path: String) -> AppResult<()> {
    let ops = GitOps::with_default_ssh();
    ops.push(Path::new(&repo_path)).map(|_| ())
}

/// Start watching repositories for file changes.
/// When files change, statuses are refreshed and `repo_status_changed_batch` events are emitted.
#[tauri::command]
pub fn start_watcher(
    repo_paths: Vec<String>,
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
) -> AppResult<()> {
    let paths: Vec<std::path::PathBuf> = repo_paths.iter().map(|p| std::path::PathBuf::from(p)).collect();

    let status_cache = std::sync::Arc::clone(&state.status_cache);

    let mut watcher = state
        .watcher
        .lock()
        .map_err(|e| crate::error::AppError::Other(format!("Watcher lock error: {}", e)))?;

    watcher.watch_repositories(paths, status_cache, app_handle.clone())?;
    drop(watcher);
    if let Err(e) = app_handle.emit("watcher_status_changed", true) {
        log::warn!("Failed to emit watcher_status_changed: {}", e);
    }

    Ok(())
}

/// Return whether the repository file watcher is currently running.
#[tauri::command]
pub fn watcher_status(state: State<'_, AppState>) -> AppResult<bool> {
    let watcher = state
        .watcher
        .lock()
        .map_err(|e| crate::error::AppError::Other(format!("Watcher lock error: {e}")))?;
    Ok(watcher.is_running())
}

/// Stop the file watcher.
#[tauri::command]
pub fn stop_watcher(app_handle: tauri::AppHandle, state: State<'_, AppState>) -> AppResult<()> {
    let mut watcher = state
        .watcher
        .lock()
        .map_err(|e| crate::error::AppError::Other(format!("Watcher lock error: {}", e)))?;

    watcher.stop();
    drop(watcher);
    if let Err(e) = app_handle.emit("watcher_status_changed", false) {
        log::warn!("Failed to emit watcher_status_changed: {}", e);
    }
    Ok(())
}

/// Request payload for batch commit.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CommitRequest {
    pub repo_path: String,
    pub repo_name: String,
    pub message: String,
    pub files: Vec<String>,
    /// Amend the HEAD commit (T-11).
    pub amend: bool,
    /// With `amend`: keep the original message (T-11 --no-edit).
    pub no_edit: bool,
    /// Commit the index as-is, preserving hunk/line staging (T-11+T-12).
    pub index_only: bool,
    /// Push after a successful commit (T-11 Commit & Push).
    pub then_push: bool,
    /// Proceed despite pre-commit safety findings (explicit user override).
    pub allow_unsafe: bool,
}

/// Request payload for staging files (git add).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddRequest {
    pub repo_path: String,
    pub repo_name: String,
    pub files: Vec<String>,
}

/// Request payload for reverting working-tree changes (git restore).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreRequest {
    pub repo_path: String,
    pub repo_name: String,
    pub files: Vec<String>,
}

/// Stage (git add) the given files in each repository.
/// Files deleted on disk are removed from the index instead.
/// PAF-11：收编 T-05 任务队列（一仓一任务）——进度事件可见、可定位失败仓，
/// 部分失败语义与 batch_fetch/pull/push 一致。返回任务 ID 列表。
#[tauri::command]
pub fn batch_add(requests: Vec<AddRequest>, state: State<'_, AppState>) -> AppResult<Vec<String>> {
    let task_requests: Vec<TaskRequest> = requests
        .into_iter()
        .map(|req| TaskRequest {
            task_type: TaskType::StageFiles { files: req.files },
            repo_path: req.repo_path,
            repo_name: req.repo_name,
        })
        .collect();
    state.task_manager.submit(&task_requests)
}

/// Revert working-tree changes for the given files (git restore --staged semantics).
///
/// - Tracked files (present in HEAD) are restored from HEAD into both the
///   index and the working tree, discarding staged and unstaged changes.
/// - Files not in HEAD (untracked or staged-new) are unstaged and deleted
///   from disk.
/// PAF-11：收编 T-05 任务队列（一仓一任务）；restore 属高危操作，worker
/// 执行前快照 HEAD 并落 T-34 操作日志。返回任务 ID 列表。
#[tauri::command]
pub fn batch_restore(requests: Vec<RestoreRequest>, state: State<'_, AppState>) -> AppResult<Vec<String>> {
    let task_requests: Vec<TaskRequest> = requests
        .into_iter()
        .map(|req| TaskRequest {
            task_type: TaskType::RestoreFiles { files: req.files },
            repo_path: req.repo_path,
            repo_name: req.repo_name,
        })
        .collect();
    state.task_manager.submit(&task_requests)
}
