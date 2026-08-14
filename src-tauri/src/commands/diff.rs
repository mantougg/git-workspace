use std::path::Path;

use serde::Deserialize;

use crate::core::diff::{self, FileDiff};
use crate::error::AppResult;

/// Diff rendering options from the UI (Roadmap §9 diff settings).
#[derive(Debug, Clone, Default, serde::Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DiffOptionsParam {
    pub ignore_whitespace: bool,
    pub ignore_whitespace_eol: bool,
    pub ignore_case: bool,
}

/// Get the working directory diff for a repository.
///
/// Computes the diff between HEAD and the working tree (including index).
/// Returns a list of changed files, each with their hunks and line-level details.
///
/// For repositories with no commits, all files appear as "added".
#[tauri::command]
pub fn get_diff(repo_path: String, options: Option<DiffOptionsParam>) -> AppResult<Vec<FileDiff>> {
    let opt = options.unwrap_or_default();
    diff::get_workdir_diff_with_config(
        Path::new(&repo_path),
        &diff::DiffConfig {
            ignore_whitespace: opt.ignore_whitespace,
            ignore_whitespace_eol: opt.ignore_whitespace_eol,
            ignore_case: opt.ignore_case,
        },
    )
}
