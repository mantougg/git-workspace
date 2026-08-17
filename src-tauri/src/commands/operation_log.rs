//! Operation log commands (T-34): query the unified operation log and run
//! one-click undo of reversible high-risk operations. Undo executes the
//! reverse op per repo with local libgit2 only (no network); the DB lock is
//! never held across repo IO — load, execute, then persist.

use tauri::State;

use crate::core::operation_log::{self, LogFilter, OperationLogDetail, OperationLogPage, UndoOutcome, UndoPreviewItem};
use crate::error::{AppError, AppResult};
use crate::state::AppState;

/// Page size cap so a huge log history cannot flood the IPC channel
/// (global constraint §2: large payloads must be paged).
const MAX_PAGE_SIZE: u32 = 500;

fn lock_db(state: &AppState) -> AppResult<std::sync::MutexGuard<'_, rusqlite::Connection>> {
    state
        .db
        .lock()
        .map_err(|e| AppError::Other(format!("DB lock error: {}", e)))
}

/// Load a log and reject logs that are already fully undone.
fn load_undoable_detail(state: &AppState, log_id: i64) -> AppResult<OperationLogDetail> {
    let conn = lock_db(state)?;
    let detail = operation_log::get_operation_log(&conn, log_id)?;
    if detail.undone_at.is_some() {
        return Err(AppError::Conflict(format!(
            "操作日志 {} 已整体撤销",
            log_id
        )));
    }
    Ok(detail)
}

/// Query operation logs, newest first, filtered by workspace / repo path
/// substring / op type / created date range (`YYYY-MM-DD` bounds).
#[tauri::command]
pub fn list_operation_logs(
    workspace_id: Option<i64>,
    repo_path: Option<String>,
    op_type: Option<String>,
    date_from: Option<String>,
    date_to: Option<String>,
    limit: Option<u32>,
    offset: Option<u32>,
    state: State<'_, AppState>,
) -> AppResult<OperationLogPage> {
    let conn = lock_db(&state)?;
    let filter = LogFilter {
        workspace_id,
        repo_path: repo_path.as_deref().filter(|s| !s.trim().is_empty()),
        op_type: op_type.as_deref().filter(|s| !s.trim().is_empty()),
        date_from: date_from.as_deref().filter(|s| !s.trim().is_empty()),
        date_to: date_to.as_deref().filter(|s| !s.trim().is_empty()),
    };
    let limit = limit.unwrap_or(50).clamp(1, MAX_PAGE_SIZE) as i64;
    let offset = offset.unwrap_or(0) as i64;
    operation_log::query_operation_logs(&conn, &filter, limit, offset)
}

/// Full detail of one logged operation, including every per-repo ref
/// snapshot (before → after oid).
#[tauri::command]
pub fn get_operation_log_detail(
    log_id: i64,
    state: State<'_, AppState>,
) -> AppResult<OperationLogDetail> {
    let conn = lock_db(&state)?;
    operation_log::get_operation_log(&conn, log_id)
}

/// Per-repo undo plan with live safety checks — the impact list for the
/// §46 Dangerous confirmation dialog shown before undoing.
#[tauri::command]
pub fn preview_undo_operation(
    log_id: i64,
    state: State<'_, AppState>,
) -> AppResult<Vec<UndoPreviewItem>> {
    let detail = load_undoable_detail(&state, log_id)?;
    // Repo IO happens here, after the DB guard is dropped (lock not held).
    Ok(operation_log::preview_undo(&detail))
}

/// One-click undo (§46 Dangerous — the UI confirms with the preview impact
/// list first): apply the reverse operation per repo, then record per-item
/// and whole-log undone timestamps. Repos that fail their safety check are
/// skipped and stay pending, so a later retry is possible.
#[tauri::command]
pub fn undo_operation(log_id: i64, state: State<'_, AppState>) -> AppResult<UndoOutcome> {
    let detail = load_undoable_detail(&state, log_id)?;
    // Reverse ops run without the DB lock held (local libgit2, parallel).
    let results = operation_log::run_undo(&detail);
    let fully_undone = {
        let mut conn = lock_db(&state)?;
        operation_log::persist_undo_results(&mut conn, log_id, &results)?
    };
    Ok(UndoOutcome {
        log_id,
        fully_undone,
        results,
    })
}
