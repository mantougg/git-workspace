//! Workspace stash commands (T-21). The git phase runs *outside* the DB
//! lock (the single-writer connection stays responsive); the association
//! record is then written in one transaction.
//!
//! GF-10: the multi-repo save / restore runs are queued as **one task each**
//! (`TaskType::WorkspaceStashSave` / `WorkspaceStashRestore`) and executed
//! serially by a worker — the git execution model is unchanged, only its
//! observability: per-repo `workspace_stash_progress` events feed the
//! TaskPanel, the cooperative cancel flag stops the loop between repos, and
//! the pending command awaits the run result (a cancelled run resolves with
//! its partial, recoverable list instead of hanging).
//!
//! The commands keep their original signatures — only the implementation
//! became `async` (F-43: the macro spawns the body onto the async runtime,
//! so awaiting here never blocks the WebView IPC callback thread and the UI
//! stays interactive while the queue runs).

use std::time::{Duration, Instant};

use rusqlite::Connection;
use tauri::State;

use crate::core::workspace_stash::{
    self, SaveWorkspaceStashResult, WorkspaceStashCheckItem, WorkspaceStashItemEntry, WorkspaceStashRepoOutcome,
    WorkspaceStashRunResult, WorkspaceStashSummary,
};
use crate::error::{AppError, AppResult};
use crate::models::task::{TaskRequest, TaskStatus, TaskType};
use crate::state::AppState;

/// How long the pending command waits for a queued run before giving up (the
/// worker's own runaway guard is 1 h). On timeout the task keeps running and
/// stays visible / cancellable in the TaskPanel.
const RUN_WAIT_TIMEOUT: Duration = Duration::from_secs(1800);
/// Grace period after the task reaches a terminal state without a recorded
/// run result. Every worker completion path records one (GF-10), so this only
/// covers the queued-cancel window between "user cancelled" and "a worker
/// dequeued the task" before the command gives up.
const RUN_RESULT_GRACE: Duration = Duration::from_secs(10);
/// Poll interval while waiting for the run result.
const RUN_POLL_INTERVAL: Duration = Duration::from_millis(150);

fn lock_db<'a>(state: &'a State<'a, AppState>) -> AppResult<std::sync::MutexGuard<'a, Connection>> {
    state
        .db
        .lock()
        .map_err(|e| AppError::Other(format!("DB lock error: {}", e)))
}

/// Queue one workspace stash run (GF-10) and return its task id.
fn submit_ws_stash_run(state: &State<'_, AppState>, task_type: TaskType, label: String) -> AppResult<String> {
    let ids = state.task_manager.submit(&[TaskRequest {
        task_type,
        // The run's identity is the record, not a single repo.
        repo_path: String::new(),
        repo_name: label,
    }])?;
    ids.into_iter()
        .next()
        .ok_or_else(|| AppError::Task("Workspace Stash 任务入队失败".into()))
}

/// Wait for a queued run and take its per-repo result (GF-10). The worker
/// records the result on every completion path — including a cancel, whose
/// items distinguish "already stashed" (restorable through the record) from
/// "untouched" (re-run the save) — so a cancelled run never leaves the caller
/// waiting blind.
async fn wait_for_ws_stash_run(state: &State<'_, AppState>, task_id: &str) -> AppResult<WorkspaceStashRunResult> {
    let deadline = Instant::now() + RUN_WAIT_TIMEOUT;
    let mut terminal_since: Option<Instant> = None;
    loop {
        if let Some(run) = state.task_manager.take_ws_stash_run(task_id) {
            return Ok(run);
        }
        // The task may already be gone (cleared from the active list) — then
        // it finished without a recorded result.
        let terminal = state
            .task_manager
            .get_status(std::slice::from_ref(&task_id.to_string()))
            .first()
            .map(|t| {
                matches!(
                    t.status,
                    TaskStatus::Success
                        | TaskStatus::Failed { .. }
                        | TaskStatus::Cancelled
                        | TaskStatus::PartialSuccess { .. }
                )
            })
            .unwrap_or(true);
        if terminal {
            match terminal_since {
                Some(since) if since.elapsed() >= RUN_RESULT_GRACE => {
                    return Err(AppError::Task(
                        "未获取到 Workspace Stash 运行结果（任务可能已被清除），请在命令流面板查看进度".into(),
                    ));
                }
                Some(_) => {}
                None => terminal_since = Some(Instant::now()),
            }
        } else {
            terminal_since = None;
        }
        if Instant::now() >= deadline {
            return Err(AppError::Task(
                "Workspace Stash 任务执行超时，请在命令流面板查看进度（可取消）".into(),
            ));
        }
        tokio::time::sleep(RUN_POLL_INTERVAL).await;
    }
}

/// Save a workspace stash (T-21 + GF-10): stash every selected repo (T-10
/// semantics, untracked per flag) as ONE queued task — serial per repo, with
/// per-repo progress and cooperative cancellation — then persist the
/// `Workspace Stash #N` record with its per-repo items in one transaction
/// (written by the worker, including a cancelled run's completed subset so
/// those repos stay restorable). Repos with a clean worktree are skipped;
/// per-repo failures are collected in the result. When nothing was stashed,
/// no record is written (`id` is None).
#[tauri::command]
pub async fn save_workspace_stash(
    workspace_id: i64,
    repo_paths: Vec<String>,
    message: Option<String>,
    include_untracked: Option<bool>,
    state: State<'_, AppState>,
) -> AppResult<SaveWorkspaceStashResult> {
    if repo_paths.is_empty() {
        return Err(AppError::Other("没有选定仓库".into()));
    }
    // The record name is allocated up front: numbering stays monotonic and
    // each per-repo stash message carries it (recognizable in the T-10 view).
    let name = {
        let conn = lock_db(&state)?;
        workspace_stash::next_workspace_stash_name(&conn, workspace_id)?
    };
    let include_untracked = include_untracked.unwrap_or(true);

    let task_id = submit_ws_stash_run(
        &state,
        TaskType::WorkspaceStashSave {
            workspace_id,
            record_name: name.clone(),
            message: message.clone(),
            include_untracked,
            repo_paths: repo_paths.clone(),
        },
        format!("{name}（保存 {} 个仓库）", repo_paths.len()),
    )?;
    let run = wait_for_ws_stash_run(&state, &task_id).await?;
    Ok(SaveWorkspaceStashResult {
        id: run.record_id,
        name: run.record_name,
        items: run.items,
    })
}

/// List the workspace stash records of a workspace, newest first.
#[tauri::command]
pub fn list_workspace_stashes(workspace_id: i64, state: State<'_, AppState>) -> AppResult<Vec<WorkspaceStashSummary>> {
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
        return Err(AppError::NotFound("该 Workspace Stash 没有仓库项".into()));
    }
    Ok(workspace_stash::check_restore(&items))
}

/// Restore a workspace stash (T-21 + GF-10): as ONE queued task — the §46
/// pre-check runs **before** queueing (fail fast when no repo is applicable,
/// so a no-op run is never queued), then every repo is re-checked at
/// execution time (stale-window safety net), applied (kept on the stack) and
/// reported. Repos failing the check are skipped — a branch mismatch applies
/// only with `allow_branch_mismatch` — and per-repo failures are collected
/// instead of blocking the rest. Cancelling stops between repos: already
/// applied repos keep their changes, the untouched ones keep their stash on
/// the stack, so re-running the restore is always safe.
#[tauri::command]
pub async fn restore_workspace_stash(
    workspace_stash_id: i64,
    allow_branch_mismatch: Option<bool>,
    state: State<'_, AppState>,
) -> AppResult<Vec<WorkspaceStashRepoOutcome>> {
    let items = {
        let conn = lock_db(&state)?;
        workspace_stash::list_workspace_stash_items(&conn, workspace_stash_id)?
    };
    if items.is_empty() {
        return Err(AppError::NotFound("该 Workspace Stash 没有仓库项".into()));
    }
    let record_name = {
        let conn = lock_db(&state)?;
        workspace_stash::workspace_stash_name(&conn, workspace_stash_id)?
    };
    let allow_branch_mismatch = allow_branch_mismatch.unwrap_or(false);

    // §46 pre-flight before queueing (GF-10 checklist #3).
    let checks = workspace_stash::check_restore(&items);
    if !checks
        .iter()
        .any(|c| workspace_stash::check_allows_apply(&c.status, allow_branch_mismatch))
    {
        // Nothing applicable: report every repo as skipped with its reason
        // instead of queueing a no-op run.
        return Ok(items
            .into_iter()
            .zip(checks)
            .map(|(item, check)| WorkspaceStashRepoOutcome {
                repo_path: item.repo_path,
                repo_name: check.repo_name,
                status: "skipped".into(),
                stash_oid: Some(item.stash_oid),
                detail: check.detail,
            })
            .collect());
    }

    let task_id = submit_ws_stash_run(
        &state,
        TaskType::WorkspaceStashRestore {
            workspace_stash_id,
            record_name: record_name.clone(),
            allow_branch_mismatch,
        },
        format!("{record_name}（恢复）"),
    )?;
    let run = wait_for_ws_stash_run(&state, &task_id).await?;
    Ok(run.items)
}

/// Delete a workspace stash record (items cascade). The per-repo stashes
/// stay on each repo's stack and remain manageable in the T-10 Stash view.
#[tauri::command]
pub fn delete_workspace_stash(workspace_stash_id: i64, state: State<'_, AppState>) -> AppResult<()> {
    let conn = lock_db(&state)?;
    workspace_stash::delete_workspace_stash(&conn, workspace_stash_id)
}
