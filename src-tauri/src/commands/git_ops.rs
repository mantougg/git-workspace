use std::path::Path;
use std::time::Duration;

use serde::Serialize;
use tauri::{Emitter, State};

use crate::core::git_ops::GitOps;
use crate::core::git_status;
use crate::core::history;
use crate::core::merge;
use crate::db::dao;
use crate::error::{AppError, AppResult};
use crate::models::commit::{CommitIdentity, CommitScanFinding};
use crate::models::repository::RepoStatus;
use crate::models::task::{TaskRequest, TaskType};
use crate::state::AppState;

/// PAF-08：sync 网络命令硬超时（与任务队列 TASK_TIMEOUT 对齐）。超时后
/// `run_git_streaming` 杀掉 git 进程树，避免无限占用执行线程。
const SYNC_GIT_TIMEOUT: Duration = Duration::from_secs(300);

/// Result of a smart pull operation (fetch + intelligent merge).
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum SmartPullResult {
    /// HEAD already matches the remote; nothing to do.
    UpToDate,
    /// Pull succeeded (fast-forward or merge commit created).
    #[serde(rename_all = "camelCase")]
    Success { commit_oid: String },
    /// Merge conflicts detected; repo is in merge state (MERGE_HEAD set).
    #[serde(rename_all = "camelCase")]
    Conflict {
        files: Vec<String>,
        /// HEAD before the merge (abort target).
        base_oid: Option<String>,
    },
}

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

/// Sync fetch for a single repo (not queued through the task system).
/// Useful for quick status refresh without the task system.
///
/// PAF-08：原为同步命令——git 网络挂起时在 Tauri 主线程无限阻塞且无超时。
/// 改 async + `spawn_blocking` 并走 `fetch_streaming`：执行移出主线程，
/// 超时杀 git 进程树。
#[tauri::command]
pub async fn sync_fetch(repo_path: String) -> AppResult<()> {
    tauri::async_runtime::spawn_blocking(move || {
        let ops = GitOps::with_default_ssh();
        ops.fetch_streaming(Path::new(&repo_path), None, Some(SYNC_GIT_TIMEOUT), &mut |_, _| {})
            .map(|_| ())
    })
    .await
    .map_err(|e| AppError::Other(format!("sync_fetch join error: {e}")))?
}

/// Sync pull for a single repo (not queued). Returns the refreshed status after pulling.
#[tauri::command]
pub async fn sync_pull(repo_path: String) -> AppResult<RepoStatus> {
    tauri::async_runtime::spawn_blocking(move || {
        let ops = GitOps::with_default_ssh();
        ops.pull_streaming(Path::new(&repo_path), None, Some(SYNC_GIT_TIMEOUT), &mut |_, _| {})?;
        git_status::get_repo_status(Path::new(&repo_path))
    })
    .await
    .map_err(|e| AppError::Other(format!("sync_pull join error: {e}")))?
}

/// Smart pull: fetch via CLI then merge via libgit2.
/// If fast-forward is possible, performs FF; otherwise does a full merge.
/// Returns conflict info when the merge cannot be auto-resolved.
#[tauri::command]
pub async fn smart_pull(repo_path: String) -> AppResult<SmartPullResult> {
    tauri::async_runtime::spawn_blocking(move || {
        let path = Path::new(&repo_path);
        let ops = GitOps::with_default_ssh();

        // 1. Fetch via CLI (credential manager / SSH support).
        ops.fetch_streaming(path, None, Some(SYNC_GIT_TIMEOUT), &mut |_, _| {})?;

        // 2. Determine upstream branch name (must resolve before borrowing repo).
        let upstream_name = {
            let repo = git2::Repository::open(path)?;
            repo.head()
                .ok()
                .and_then(|h| h.shorthand().map(String::from))
        };
        let shorthand = match upstream_name {
            Some(s) => s,
            None => {
                ops.pull_streaming(path, None, Some(SYNC_GIT_TIMEOUT), &mut |_, _| {})?;
                return Ok(SmartPullResult::Success {
                    commit_oid: String::new(),
                });
            }
        };

        let upstream = {
            let repo = git2::Repository::open(path)?;
            repo.find_branch(&shorthand, git2::BranchType::Local)
                .ok()
                .and_then(|b| b.upstream().ok())
                .and_then(|u| u.name().ok().flatten().map(String::from))
        };
        let upstream = match upstream {
            Some(u) => u,
            None => {
                ops.pull_streaming(path, None, Some(SYNC_GIT_TIMEOUT), &mut |_, _| {})?;
                return Ok(SmartPullResult::Success {
                    commit_oid: String::new(),
                });
            }
        };

        // 3. Analyze merge possibility via libgit2.
        smart_pull_inner(path, &upstream)
    })
    .await
    .map_err(|e| AppError::Other(format!("smart_pull join error: {e}")))?
}

/// Core smart-pull logic: merge_analysis → fast-forward or full merge.
fn smart_pull_inner(path: &Path, upstream: &str) -> AppResult<SmartPullResult> {
    let repo = git2::Repository::open(path)?;
    let their_commit = repo
        .revparse_single(upstream)
        .and_then(|o| o.peel_to_commit())
        .map_err(|_| AppError::NotFound(format!("upstream '{}' not found", upstream)))?;
    let their_annotated = repo.find_annotated_commit(their_commit.id())?;
    let (analysis, _preference) = repo.merge_analysis(&[&their_annotated])?;

    if analysis.is_up_to_date() {
        return Ok(SmartPullResult::UpToDate);
    }

    let base_oid = repo.head().ok().and_then(|h| h.target()).map(|o| o.to_string());

    // Fast-forward.
    if analysis.is_fast_forward() {
        let head_ref = repo.head()?.name().unwrap_or("HEAD").to_string();
        repo.checkout_tree(their_commit.as_object(), None)?;
        repo.find_reference(&head_ref)?
            .set_target(their_commit.id(), "smart-pull: fast-forward")?;
        return Ok(SmartPullResult::Success {
            commit_oid: their_commit.id().to_string(),
        });
    }

    // Full merge.
    repo.merge(&[&their_annotated], None, None)?;
    let mut index = repo.index()?;
    if index.has_conflicts() {
        return Ok(SmartPullResult::Conflict {
            files: history::conflict_paths(&index)?,
            base_oid,
        });
    }

    // Clean merge — create merge commit.
    let tree_oid = index.write_tree()?;
    let tree = repo.find_tree(tree_oid)?;
    let sig = crate::core::signature_or_default(&repo)?;
    let head_commit = repo.head()?.peel_to_commit()?;
    let message = format!("Merge '{}'", upstream);
    let oid = repo.commit(
        Some("HEAD"),
        &sig,
        &sig,
        &message,
        &tree,
        &[&head_commit, &their_commit],
    )?;
    repo.cleanup_state()?;
    Ok(SmartPullResult::Success {
        commit_oid: oid.to_string(),
    })
}

/// Sync push for a single repo (not queued).
#[tauri::command]
pub async fn sync_push(repo_path: String) -> AppResult<()> {
    tauri::async_runtime::spawn_blocking(move || {
        let ops = GitOps::with_default_ssh();
        ops.push_streaming(Path::new(&repo_path), None, Some(SYNC_GIT_TIMEOUT), &mut |_, _| {})
            .map(|_| ())
    })
    .await
    .map_err(|e| AppError::Other(format!("sync_push join error: {e}")))?
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
