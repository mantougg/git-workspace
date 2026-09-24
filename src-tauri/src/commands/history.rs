//! History operation commands (T-13): cherry-pick / revert / reset / abort.

use std::path::Path;

use tauri::State;

use crate::core::history::{self, PickOutcome, ResetResult};
use crate::core::operation_log::{self, NewOperationLogItem};
use crate::error::AppResult;
use crate::state::AppState;

/// Cherry-pick one or more commits onto HEAD (applied in order).
/// Returns Success or a Conflict payload with the files + abort base oid.
///
/// T-34 (GF-16): a COMPLETED pick is logged (before/after HEAD) so it can be
/// undone by a hard rollback to the pre-pick oid. A conflicted, in-progress
/// pick keeps its own recovery (abort_pick / pick_continue) and is not logged.
#[tauri::command]
pub fn cherry_pick(repo_path: String, oids: Vec<String>, state: State<'_, AppState>) -> AppResult<PickOutcome> {
    let path = Path::new(&repo_path);
    let before = operation_log::snapshot_head(path);
    let outcome = history::cherry_pick(path, &oids)?;
    if let (PickOutcome::Success { picked }, Some((ref_name, before_oid))) = (&outcome, before) {
        let after_oid = operation_log::snapshot_head(path).map(|(_, oid)| oid);
        let summary = if oids.len() == 1 {
            format!("cherry-pick {}", short(&oids[0]))
        } else {
            format!("cherry-pick {} 个提交", picked)
        };
        operation_log::record_operation_best_effort(
            &state.db,
            &repo_path,
            operation_log::OP_CHERRY_PICK,
            &summary,
            vec![NewOperationLogItem {
                repo_path: repo_path.clone(),
                ref_name,
                before_oid,
                after_oid,
                detail: Some(format!("picked:{}", oids.len())),
            }],
        );
    }
    Ok(outcome)
}

/// First 7 chars of an oid for summaries.
fn short(oid: &str) -> &str {
    &oid[..7.min(oid.len())]
}

/// Revert a single commit (creates a revert commit on success).
#[tauri::command]
pub fn revert_commit(repo_path: String, oid: String) -> AppResult<PickOutcome> {
    history::revert(Path::new(&repo_path), &oid)
}

/// Reset HEAD to `target` (default HEAD) with soft / mixed / hard semantics.
/// Hard reset is a Dangerous op — the UI must confirm with impact details.
///
/// T-34: the pre-reset ref snapshot is captured before running, and the log
/// (before + after + mode) is written only after the reset succeeds — a
/// failed reset leaves no record. An unborn HEAD has no before-state to roll
/// back to, so nothing is logged in that case.
#[tauri::command]
pub fn reset_to(
    repo_path: String,
    target: Option<String>,
    mode: String,
    state: State<'_, AppState>,
) -> AppResult<ResetResult> {
    let path = Path::new(&repo_path);
    let before = operation_log::snapshot_head(path);
    let result = history::reset_to(path, target.as_deref(), &mode)?;
    if let Some((ref_name, before_oid)) = before {
        let item = NewOperationLogItem {
            repo_path: repo_path.clone(),
            ref_name,
            before_oid,
            after_oid: Some(result.target.clone()),
            detail: Some(format!("mode:{}", result.mode)),
        };
        let summary = format!(
            "reset --{} → {}",
            result.mode,
            &result.target[..7.min(result.target.len())]
        );
        operation_log::record_operation_best_effort(
            &state.db,
            &repo_path,
            operation_log::OP_RESET,
            &summary,
            vec![item],
        );
    }
    Ok(result)
}

/// Abort an in-progress cherry-pick / revert, restoring the pre-operation
/// state (hard reset to `base_oid` when given, else current HEAD).
///
/// GF-16: closes the repo's open conflict-resolution log session — the
/// driving operation is over, so a re-run must start a fresh log row.
#[tauri::command]
pub fn abort_pick(repo_path: String, base_oid: Option<String>) -> AppResult<()> {
    operation_log::close_conflict_sessions(&repo_path);
    history::abort_pick(Path::new(&repo_path), base_oid.as_deref())
}

/// Currently conflicted files (used to surface an in-progress conflict after
/// view reload / app restart; the T-16 resolver hooks in here later).
#[tauri::command]
pub fn get_conflict_files(repo_path: String) -> AppResult<Vec<String>> {
    history::conflict_files(Path::new(&repo_path))
}

/// Continue an in-progress cherry-pick / revert after conflicts were resolved
/// (T-16). Returns the new commit oid.
///
/// GF-16: closes the repo's open conflict-resolution log session.
#[tauri::command]
pub fn pick_continue(repo_path: String) -> AppResult<String> {
    operation_log::close_conflict_sessions(&repo_path);
    history::pick_continue(Path::new(&repo_path))
}
