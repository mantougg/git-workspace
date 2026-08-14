//! Conflict Resolver commands (T-16).

use std::path::Path;

use crate::core::conflict::{self, ConflictContent, OperationState};
use crate::error::AppResult;

/// The repo's current operation + conflict state (CONFLICT detection;
/// routes Continue / Abort to the right state machine).
#[tauri::command]
pub fn get_operation_state(repo_path: String) -> AppResult<OperationState> {
    conflict::operation_state(Path::new(&repo_path))
}

/// Load BASE / OURS / THEIRS + worktree content of one conflicted file.
#[tauri::command]
pub fn get_conflict_content(repo_path: String, path: String) -> AppResult<ConflictContent> {
    conflict::conflict_content(Path::new(&repo_path), &path)
}

/// Resolve one conflicted file: "ours" | "theirs" | "both".
#[tauri::command]
pub fn resolve_conflict(repo_path: String, path: String, strategy: String) -> AppResult<()> {
    conflict::resolve_conflict(Path::new(&repo_path), &path, &strategy)
}

/// Resolve one conflicted file with manually edited content (null = delete).
#[tauri::command]
pub fn resolve_conflict_with_content(
    repo_path: String,
    path: String,
    content: Option<String>,
) -> AppResult<()> {
    conflict::resolve_conflict_with_content(
        Path::new(&repo_path),
        &path,
        content.as_deref(),
    )
}
