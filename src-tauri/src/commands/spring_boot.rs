//! Spring Boot application discovery IPC (R-06).

use std::path::Path;

use tauri::{command, State};

use crate::error::{AppError, AppResult};
use crate::maven::discover_poms;
use crate::runtime::spring_boot::{detect_spring_boot_workspace, SpringBootWorkspaceResult};
use crate::state::AppState;

/// Discover Maven projects below a workspace and return Spring Boot candidates.
///
/// The command only reads the workspace. POM/source caches are process-local and
/// are keyed by content fingerprints, so changed POMs or Java files refresh the
/// result automatically.
#[command]
pub fn detect_spring_boot(
    state: State<'_, AppState>,
    workspace_root: String,
    scan_depth: Option<usize>,
) -> AppResult<SpringBootWorkspaceResult> {
    let root = Path::new(&workspace_root);
    if !root.is_dir() {
        return Err(AppError::ProjectNotFound(format!(
            "Workspace 目录不存在：{workspace_root}"
        )));
    }
    let discovery = discover_poms(root, scan_depth.unwrap_or(5), Some(&state.pom_cache), None);
    Ok(detect_spring_boot_workspace(
        &discovery.projects,
        &discovery.effective,
        Some(&state.spring_boot_cache),
    ))
}
