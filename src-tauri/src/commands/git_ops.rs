use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use tauri::{Emitter, State};
use uuid::Uuid;

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
use crate::task::console::{
    emit_git_op_finished, emit_git_op_started, finish_streaming, ConsoleStreamer,
};
use crate::task::single_ops::SingleOpGuard;

/// PAF-08：sync 网络命令硬超时（与任务队列 TASK_TIMEOUT 对齐）。超时后
/// `run_git_streaming` 杀掉 git 进程树，避免无限占用执行线程。
/// GF-07：单仓 `push_branch`（commands/branch.rs）共用同一预算。
pub(crate) const SYNC_GIT_TIMEOUT: Duration = Duration::from_secs(300);

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
///
/// GF-07：`on_line` 接流式——git 输出逐行（100ms 聚合）镜像到 Git Console
/// （`git_op_output`，与批次操作同一事件），并经 `git_op_started` /
/// `git_op_finished` 生命周期事件提供前端取消入口（`cancel_git_op`）。
/// `op_id` 缺省时后端生成。
#[tauri::command]
pub async fn sync_fetch(
    repo_path: String,
    op_id: Option<String>,
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> AppResult<()> {
    let (op_id, cancel, _guard) = register_single_op(&state, op_id);
    let repo_name = repo_display_name(&repo_path);
    let command = "git fetch <remote>".to_string();
    emit_git_op_started(&app, &op_id, &repo_path, &repo_name, &command);
    let app_for_finish = app.clone();

    let result = tauri::async_runtime::spawn_blocking(move || {
        let ops = GitOps::with_default_ssh();
        let mut streamer = ConsoleStreamer::new(app, repo_path.clone(), repo_name, command);
        streamer.emit_meta_header();
        let r = ops.fetch_streaming(
            Path::new(&repo_path),
            Some(cancel.as_ref()),
            Some(SYNC_GIT_TIMEOUT),
            &mut |s, l| streamer.on_line(s, l),
        );
        streamer.flush();
        finish_streaming(r, &streamer, SYNC_GIT_TIMEOUT)
    })
    .await
    .map_err(|e| AppError::Other(format!("sync_fetch join error: {e}")))?;

    emit_op_finished(&app_for_finish, &op_id, &result);
    result.map(|_| ())
}

/// Sync pull for a single repo (not queued). Returns the refreshed status after pulling.
///
/// GF-07：同 `sync_fetch`——流式镜像 + 取消入口（见 `git_op_started` /
/// `git_op_finished` 与 `cancel_git_op`）。
#[tauri::command]
pub async fn sync_pull(
    repo_path: String,
    op_id: Option<String>,
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> AppResult<RepoStatus> {
    let (op_id, cancel, _guard) = register_single_op(&state, op_id);
    let repo_name = repo_display_name(&repo_path);
    let command = "git pull --ff-only".to_string();
    emit_git_op_started(&app, &op_id, &repo_path, &repo_name, &command);
    let app_for_finish = app.clone();

    let result = tauri::async_runtime::spawn_blocking(move || {
        let ops = GitOps::with_default_ssh();
        let mut streamer = ConsoleStreamer::new(app, repo_path.clone(), repo_name, command);
        streamer.emit_meta_header();
        let r = ops.pull_streaming(
            Path::new(&repo_path),
            Some(cancel.as_ref()),
            Some(SYNC_GIT_TIMEOUT),
            &mut |s, l| streamer.on_line(s, l),
        );
        streamer.flush();
        finish_streaming(r, &streamer, SYNC_GIT_TIMEOUT)?;
        git_status::get_repo_status(Path::new(&repo_path))
    })
    .await
    .map_err(|e| AppError::Other(format!("sync_pull join error: {e}")))?;

    emit_op_finished(&app_for_finish, &op_id, &result);
    result
}

/// Smart pull: fetch via CLI then merge via libgit2.
/// If fast-forward is possible, performs FF; otherwise does a full merge.
/// Returns conflict info when the merge cannot be auto-resolved.
///
/// GF-07：fetch / 回退 pull 阶段均流式镜像到 Git Console（各阶段一个
/// ConsoleStreamer，命令标题行区分），整操作用同一 `op_id` 登记取消。
#[tauri::command]
pub async fn smart_pull(
    repo_path: String,
    op_id: Option<String>,
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> AppResult<SmartPullResult> {
    let (op_id, cancel, _guard) = register_single_op(&state, op_id);
    let repo_name = repo_display_name(&repo_path);
    let command = "git fetch <remote>".to_string();
    emit_git_op_started(&app, &op_id, &repo_path, &repo_name, &command);
    let app_for_finish = app.clone();

    let result = tauri::async_runtime::spawn_blocking(move || {
        let path = Path::new(&repo_path);
        let ops = GitOps::with_default_ssh();

        // 1. Fetch via CLI (credential manager / SSH support) — 流式镜像。
        {
            let mut streamer =
                ConsoleStreamer::new(app.clone(), repo_path.clone(), repo_name.clone(), command.clone());
            streamer.emit_meta_header();
            let r = ops.fetch_streaming(
                path,
                Some(cancel.as_ref()),
                Some(SYNC_GIT_TIMEOUT),
                &mut |s, l| streamer.on_line(s, l),
            );
            streamer.flush();
            finish_streaming(r, &streamer, SYNC_GIT_TIMEOUT)?;
        }

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
                smart_pull_fallback_pull(&app, &repo_path, &repo_name, &cancel)?;
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
                smart_pull_fallback_pull(&app, &repo_path, &repo_name, &cancel)?;
                return Ok(SmartPullResult::Success {
                    commit_oid: String::new(),
                });
            }
        };

        // 3. Analyze merge possibility via libgit2.
        smart_pull_inner(path, &upstream)
    })
    .await
    .map_err(|e| AppError::Other(format!("smart_pull join error: {e}")))?;

    emit_op_finished(&app_for_finish, &op_id, &result);
    result
}

/// smart_pull 的回退路径：拿不到 upstream 时退回 `git pull --ff-only`
/// （与批次 Pull 同一命令标题行），流式镜像到 Git Console。
fn smart_pull_fallback_pull(
    app: &tauri::AppHandle,
    repo_path: &str,
    repo_name: &str,
    cancel: &AtomicBool,
) -> AppResult<()> {
    let ops = GitOps::with_default_ssh();
    let mut streamer = ConsoleStreamer::new(
        app.clone(),
        repo_path.to_string(),
        repo_name.to_string(),
        "git pull --ff-only".to_string(),
    );
    streamer.emit_meta_header();
    let r = ops.pull_streaming(
        Path::new(repo_path),
        Some(cancel),
        Some(SYNC_GIT_TIMEOUT),
        &mut |s, l| streamer.on_line(s, l),
    );
    streamer.flush();
    finish_streaming(r, &streamer, SYNC_GIT_TIMEOUT).map(|_| ())
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
///
/// GF-07：同 `sync_fetch`——流式镜像 + 取消入口。
#[tauri::command]
pub async fn sync_push(
    repo_path: String,
    op_id: Option<String>,
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> AppResult<()> {
    let (op_id, cancel, _guard) = register_single_op(&state, op_id);
    let repo_name = repo_display_name(&repo_path);
    let command = "git push".to_string();
    emit_git_op_started(&app, &op_id, &repo_path, &repo_name, &command);
    let app_for_finish = app.clone();

    let result = tauri::async_runtime::spawn_blocking(move || {
        let ops = GitOps::with_default_ssh();
        let mut streamer = ConsoleStreamer::new(app, repo_path.clone(), repo_name, command);
        streamer.emit_meta_header();
        let r = ops.push_streaming(
            Path::new(&repo_path),
            Some(cancel.as_ref()),
            Some(SYNC_GIT_TIMEOUT),
            &mut |s, l| streamer.on_line(s, l),
        );
        streamer.flush();
        finish_streaming(r, &streamer, SYNC_GIT_TIMEOUT)
    })
    .await
    .map_err(|e| AppError::Other(format!("sync_push join error: {e}")))?;

    emit_op_finished(&app_for_finish, &op_id, &result);
    result.map(|_| ())
}

/// GF-07：取消一个进行中的单仓网络操作（sync_fetch/sync_pull/sync_push/
/// smart_pull/push_branch）。`op_id` 来自命令入参或 `git_op_started` 事件。
/// op 已结束（或从未存在）时返回 NotFound——前端取消入口通常已因
/// `git_op_finished` 撤下，此时静默忽略即可。
#[tauri::command]
pub fn cancel_git_op(op_id: String, state: State<'_, AppState>) -> AppResult<()> {
    if state.single_ops.cancel(&op_id) {
        log::info!("单仓网络操作 {} 已请求取消", op_id);
        Ok(())
    } else {
        Err(AppError::NotFound(format!("操作 {} 不存在或已结束", op_id)))
    }
}

/// GF-07：命令展示名——仓库路径末段（与 `batch_fetch` 的任务名同源）。
pub(super) fn repo_display_name(repo_path: &str) -> String {
    Path::new(repo_path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string()
}

/// GF-07：登记一次单仓网络操作：`op_id` 缺省时后端生成（经 `git_op_started`
/// 下发），返回取消 flag 与 RAII 守卫（drop 时从注册表摘除，防 panic 泄漏）。
pub(super) fn register_single_op(
    state: &AppState,
    op_id: Option<String>,
) -> (String, Arc<AtomicBool>, SingleOpGuard) {
    let op_id = op_id.unwrap_or_else(|| Uuid::new_v4().to_string());
    let flag = state.single_ops.register(&op_id);
    let guard = SingleOpGuard::new(Arc::clone(&state.single_ops), op_id.clone());
    (op_id, flag, guard)
}

/// GF-07：操作收尾——发 `git_op_finished`（错误信息一并带出，供前端提示）。
pub(super) fn emit_op_finished<T>(app: &tauri::AppHandle, op_id: &str, result: &AppResult<T>) {
    let error = result.as_ref().err().map(|e| e.to_string());
    emit_git_op_finished(app, op_id, error.is_none(), error.as_deref());
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
