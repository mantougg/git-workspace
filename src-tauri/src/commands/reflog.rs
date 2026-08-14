//! Reflog command (T-14). Recovery ops reuse T-09 `create_branch` and T-13
//! `reset_to` commands directly; this is the read side only.

use std::path::Path;

use crate::core::reflog::{self, ReflogEntry};
use crate::error::AppResult;

/// Read a reflog (default HEAD), newest first. `reference` accepts "HEAD",
/// a full ref ("refs/heads/main"), or a shorthand ("main" / "origin/main").
#[tauri::command]
pub fn get_reflog(
    repo_path: String,
    reference: Option<String>,
    max: Option<usize>,
) -> AppResult<Vec<ReflogEntry>> {
    reflog::read_reflog(
        Path::new(&repo_path),
        reference.as_deref(),
        max.unwrap_or(200),
    )
}
