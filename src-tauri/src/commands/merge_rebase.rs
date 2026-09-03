//! Merge / Rebase commands (T-15).

use std::path::Path;

use tauri::State;

use crate::core::merge::{self, MergeOutcome};
use crate::core::operation_log::{self, NewOperationLogItem};
use crate::core::rebase::{self, RebaseOp, RebaseOutcome, RebaseState};
use crate::error::AppResult;
use crate::state::AppState;

/// Merge `branch` into the current HEAD. `mode`: "normal" | "no-ff" | "squash".
/// Merge is a Warning-level op (§46) — the UI confirms first.
#[tauri::command]
pub fn merge_branch(repo_path: String, branch: String, mode: String) -> AppResult<MergeOutcome> {
    merge::merge(Path::new(&repo_path), &branch, &mode)
}

/// Finalize a conflicted merge after the user resolved the index.
#[tauri::command]
pub fn merge_continue(repo_path: String, message: Option<String>) -> AppResult<String> {
    merge::merge_continue(Path::new(&repo_path), message.as_deref())
}

/// Abort a conflicted merge, restoring the pre-merge state.
#[tauri::command]
pub fn merge_abort(repo_path: String) -> AppResult<()> {
    merge::merge_abort(Path::new(&repo_path))
}

/// Whether a merge is in progress (MERGE_HEAD exists).
#[tauri::command]
pub fn get_merge_in_progress(repo_path: String) -> AppResult<bool> {
    merge::merge_in_progress(Path::new(&repo_path))
}

/// Default rebase todo: commits of `branch` (default HEAD) not in `upstream`,
/// oldest first, as pick ops.
#[tauri::command]
pub fn list_rebase_commits(repo_path: String, upstream: String, branch: Option<String>) -> AppResult<Vec<RebaseOp>> {
    rebase::list_rebase_commits(Path::new(&repo_path), &upstream, branch.as_deref())
}

/// Start a rebase: replay `ops` onto `onto`. Covers basic / --onto /
/// interactive (user-arranged ops).
///
/// T-34: only a COMPLETED rebase is logged (before + after ref snapshots) —
/// a conflicted, in-progress rebase already has its own recovery path
/// (`rebase_abort` restores `original_head`), and undo refuses repos with a
/// rebase still in progress.
#[tauri::command]
pub fn start_rebase(
    repo_path: String,
    onto: String,
    ops: Vec<RebaseOp>,
    state: State<'_, AppState>,
) -> AppResult<RebaseOutcome> {
    let path = Path::new(&repo_path);
    let before = operation_log::snapshot_head(path);
    let outcome = rebase::start_rebase(path, &onto, ops)?;
    if let (RebaseOutcome::Success { .. }, Some((ref_name, before_oid))) = (&outcome, before) {
        let after_oid = operation_log::snapshot_head(path).map(|(_, oid)| oid);
        let item = NewOperationLogItem {
            repo_path: repo_path.clone(),
            ref_name,
            before_oid,
            after_oid,
            detail: Some(format!("onto:{onto}")),
        };
        let summary = format!("rebase → {onto}");
        operation_log::record_operation_best_effort(
            &state.db,
            &repo_path,
            operation_log::OP_REBASE,
            &summary,
            vec![item],
        );
    }
    Ok(outcome)
}

/// Continue after the conflicted op was resolved (index must be clean).
#[tauri::command]
pub fn rebase_continue(repo_path: String) -> AppResult<RebaseOutcome> {
    rebase::rebase_continue(Path::new(&repo_path))
}

/// Skip the current (conflicting) op and replay the rest.
#[tauri::command]
pub fn rebase_skip(repo_path: String) -> AppResult<RebaseOutcome> {
    rebase::rebase_skip(Path::new(&repo_path))
}

/// Abort the rebase, restoring the branch to its pre-rebase HEAD.
#[tauri::command]
pub fn rebase_abort(repo_path: String) -> AppResult<()> {
    rebase::rebase_abort(Path::new(&repo_path))
}

/// Current rebase progress (restart detection), if any.
#[tauri::command]
pub fn get_rebase_state(repo_path: String) -> AppResult<Option<RebaseState>> {
    rebase::get_rebase_state(Path::new(&repo_path))
}
