use std::path::Path;

use crate::core::graph::{self, BranchInfo, CommitInfo};
use crate::error::AppResult;

/// Get commit history for a repository, starting from HEAD.
/// Returns up to `max_count` commits, sorted topologically.
#[tauri::command]
pub fn get_commit_history(
    repo_path: String,
    max_count: Option<usize>,
) -> AppResult<Vec<CommitInfo>> {
    let max = max_count.unwrap_or(100);
    graph::get_commit_history(Path::new(&repo_path), max)
}

/// Get all branches (local and remote) for a repository.
#[tauri::command]
pub fn get_branches(repo_path: String) -> AppResult<Vec<BranchInfo>> {
    graph::get_branches(Path::new(&repo_path))
}
