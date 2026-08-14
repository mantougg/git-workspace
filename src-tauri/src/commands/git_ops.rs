use std::path::Path;

use tauri::State;

use crate::core::git_ops::GitOps;
use crate::core::git_status;
use crate::error::AppResult;
use crate::models::repository::RepoStatus;
use crate::models::task::{TaskRequest, TaskType};
use crate::state::AppState;

/// Batch fetch: create Fetch tasks for each repo path and submit to the task queue.
/// Returns the list of task IDs.
#[tauri::command]
pub fn batch_fetch(
    repo_paths: Vec<String>,
    state: State<'_, AppState>,
) -> AppResult<Vec<String>> {
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
pub fn batch_pull(
    repo_paths: Vec<String>,
    state: State<'_, AppState>,
) -> AppResult<Vec<String>> {
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
pub fn batch_push(
    repo_paths: Vec<String>,
    state: State<'_, AppState>,
) -> AppResult<Vec<String>> {
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
#[tauri::command]
pub fn batch_commit(
    commits: Vec<CommitRequest>,
    state: State<'_, AppState>,
) -> AppResult<Vec<String>> {
    let requests: Vec<TaskRequest> = commits
        .into_iter()
        .map(|c| TaskRequest {
            task_type: TaskType::Commit {
                message: c.message,
                files: c.files,
            },
            repo_path: c.repo_path.clone(),
            repo_name: c.repo_name,
        })
        .collect();

    state.task_manager.submit(&requests)
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
    let paths: Vec<std::path::PathBuf> = repo_paths
        .iter()
        .map(|p| std::path::PathBuf::from(p))
        .collect();

    let status_cache = std::sync::Arc::clone(&state.status_cache);

    let mut watcher = state
        .watcher
        .lock()
        .map_err(|e| crate::error::AppError::Other(format!("Watcher lock error: {}", e)))?;

    watcher.watch_repositories(paths, status_cache, app_handle)?;

    Ok(())
}

/// Stop the file watcher.
#[tauri::command]
pub fn stop_watcher(state: State<'_, AppState>) -> AppResult<()> {
    let mut watcher = state
        .watcher
        .lock()
        .map_err(|e| crate::error::AppError::Other(format!("Watcher lock error: {}", e)))?;

    watcher.stop();
    Ok(())
}

/// Request payload for batch commit.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitRequest {
    pub repo_path: String,
    pub repo_name: String,
    pub message: String,
    pub files: Vec<String>,
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
/// Returns the list of repository names processed.
#[tauri::command]
pub fn batch_add(requests: Vec<AddRequest>) -> AppResult<Vec<String>> {
    let mut processed = Vec::with_capacity(requests.len());

    for req in requests {
        let repo = git2::Repository::open(&req.repo_path)?;
        let mut index = repo.index()?;

        for file in &req.files {
            let full_path = Path::new(&req.repo_path).join(file);
            if full_path.is_dir() {
                // Untracked directory: recursively stage everything under it.
                index.add_all([file.as_str()], git2::IndexAddOption::DEFAULT, None)?;
            } else if full_path.exists() {
                index.add_path(Path::new(file))?;
            } else {
                // Deleted on disk: record the deletion in the index.
                index.remove_path(Path::new(file))?;
            }
        }

        index.write()?;
        log::info!("Staged {} file(s) in {:?}", req.files.len(), req.repo_path);
        processed.push(req.repo_name);
    }

    Ok(processed)
}

/// Revert working-tree changes for the given files (git restore --staged semantics).
///
/// - Tracked files (present in HEAD) are restored from HEAD into both the
///   index and the working tree, discarding staged and unstaged changes.
/// - Files not in HEAD (untracked or staged-new) are unstaged and deleted
///   from disk.
/// Returns the list of repository names processed.
#[tauri::command]
pub fn batch_restore(requests: Vec<RestoreRequest>) -> AppResult<Vec<String>> {
    let mut processed = Vec::with_capacity(requests.len());

    for req in requests {
        let repo = git2::Repository::open(&req.repo_path)?;

        let head_tree = repo
            .head()
            .ok()
            .and_then(|h| h.peel_to_tree().ok());

        let mut checkout_paths: Vec<&str> = Vec::new();
        let mut index_dirty = false;
        let mut index = repo.index()?;

        for file in &req.files {
            let full_path = Path::new(&req.repo_path).join(file);
            let in_head = head_tree
                .as_ref()
                .and_then(|t| t.get_path(Path::new(file)).ok())
                .is_some();

            if in_head {
                // Restore from HEAD (index + working tree) via checkout_head.
                checkout_paths.push(file.as_str());
            } else {
                // Untracked or staged-new: unstage, then delete from disk.
                if index.remove_path(Path::new(file)).is_ok() {
                    index_dirty = true;
                }
                if full_path.exists() {
                    if full_path.is_dir() {
                        std::fs::remove_dir_all(&full_path)?;
                    } else {
                        std::fs::remove_file(&full_path)?;
                    }
                }
            }
        }

        if index_dirty {
            index.write()?;
        }

        if !checkout_paths.is_empty() {
            let mut opts = git2::build::CheckoutBuilder::new();
            for p in &checkout_paths {
                opts.path(p);
            }
            opts.force();
            repo.checkout_head(Some(&mut opts))?;
        }

        log::info!(
            "Restored {} file(s) in {:?}",
            req.files.len(),
            req.repo_path
        );
        processed.push(req.repo_name);
    }

    Ok(processed)
}
