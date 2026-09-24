//! GF-17: structured previews for destructive single-repo operations.
//!
//! Thin `#[tauri::command]` wrappers over `core::preview` (the logic and its
//! unit tests live there). Both commands are **synchronous and local-only**
//! read operations — no CLI git, no network, no repo mutation — the same
//! shape as the existing sync read commands (`get_conflict_files`,
//! `batch_dry_run`). F-43: a sync command must never spawn tokio work; these
//! do none.
//!
//! Roadmap §46 (Repository / Branch / Files / Potential Data Loss) is
//! satisfied by the payload itself: the UI renders it inside the Dangerous
//! confirm before the operation runs.

use std::path::Path;

use crate::core::preview::{self, MergePreview, ResetPreview};
use crate::error::AppResult;

/// Preview a `reset` before running it: commits the branch will drop
/// (`target..HEAD`) and the tracked worktree/index changes that will be
/// discarded. `target` `None` resets to HEAD (same default as `reset_to`).
#[tauri::command]
pub fn preview_reset(repo_path: String, target: Option<String>, mode: String) -> AppResult<ResetPreview> {
    preview::preview_reset(Path::new(&repo_path), target.as_deref(), &mode)
}

/// Preview a `merge` before running it: the commits that will be merged in,
/// the affected files, and a conflict prediction (in-memory `merge_commits`,
/// the same technique GF-15's `batch_dry_run` uses).
#[tauri::command]
pub fn preview_merge(repo_path: String, branch: String, mode: String) -> AppResult<MergePreview> {
    preview::preview_merge(Path::new(&repo_path), &branch, &mode)
}
