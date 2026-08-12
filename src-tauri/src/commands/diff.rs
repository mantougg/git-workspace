use std::path::Path;

use crate::core::diff::{self, FileDiff};
use crate::error::AppResult;

/// Get the working directory diff for a repository.
///
/// Computes the diff between HEAD and the working tree (including index).
/// Returns a list of changed files, each with their hunks and line-level details.
///
/// For repositories with no commits, all files appear as "added".
#[tauri::command]
pub fn get_diff(repo_path: String) -> AppResult<Vec<FileDiff>> {
    diff::get_workdir_diff(Path::new(&repo_path))
}
