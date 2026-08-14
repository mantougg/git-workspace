//! History operation commands (T-13): cherry-pick / revert / reset / abort.

use std::path::Path;

use crate::core::history::{self, PickOutcome, ResetResult};
use crate::error::AppResult;

/// Cherry-pick one or more commits onto HEAD (applied in order).
/// Returns Success or a Conflict payload with the files + abort base oid.
#[tauri::command]
pub fn cherry_pick(repo_path: String, oids: Vec<String>) -> AppResult<PickOutcome> {
    history::cherry_pick(Path::new(&repo_path), &oids)
}

/// Revert a single commit (creates a revert commit on success).
#[tauri::command]
pub fn revert_commit(repo_path: String, oid: String) -> AppResult<PickOutcome> {
    history::revert(Path::new(&repo_path), &oid)
}

/// Reset HEAD to `target` (default HEAD) with soft / mixed / hard semantics.
/// Hard reset is a Dangerous op — the UI must confirm with impact details.
#[tauri::command]
pub fn reset_to(
    repo_path: String,
    target: Option<String>,
    mode: String,
) -> AppResult<ResetResult> {
    history::reset_to(Path::new(&repo_path), target.as_deref(), &mode)
}

/// Abort an in-progress cherry-pick / revert, restoring the pre-operation
/// state (hard reset to `base_oid` when given, else current HEAD).
#[tauri::command]
pub fn abort_pick(repo_path: String, base_oid: Option<String>) -> AppResult<()> {
    history::abort_pick(Path::new(&repo_path), base_oid.as_deref())
}

/// Currently conflicted files (used to surface an in-progress conflict after
/// view reload / app restart; the T-16 resolver hooks in here later).
#[tauri::command]
pub fn get_conflict_files(repo_path: String) -> AppResult<Vec<String>> {
    history::conflict_files(Path::new(&repo_path))
}
