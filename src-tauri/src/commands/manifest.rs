//! Manifest IPC commands (T-33): export the current workspace as a
//! rebuildable `gitworkspace.json`, and parse/preview an imported manifest
//! before the frontend submits batch clones through the T-05 task queue.

use std::collections::HashMap;
use std::path::Path;

use rayon::prelude::*;
use tauri::State;

use crate::core::manifest::{self, ClonePlan, ManifestRepo, WorkspaceManifest, MANIFEST_VERSION};
use crate::db::dao;
use crate::error::{AppError, AppResult};
use crate::state::AppState;

/// Export a workspace as a manifest and write it to `file_path` (chosen via
/// the frontend save dialog). Remote URL / default branch are read locally
/// per repo (libgit2, no network, parallel over rayon); repos without a
/// remote get `remote_url: None` and are surfaced as not-cloneable on import.
/// Returns the manifest so the UI can show an export summary.
#[tauri::command]
pub fn export_workspace_manifest(
    workspace_id: i64,
    file_path: String,
    state: State<'_, AppState>,
) -> AppResult<WorkspaceManifest> {
    let (workspace, repos, groups) = {
        let conn = state
            .db
            .lock()
            .map_err(|e| AppError::Other(format!("DB lock error: {}", e)))?;
        let workspace = dao::get_workspace(&conn, workspace_id)?;
        let repos = dao::list_repositories_by_workspace(&conn, workspace_id)?;
        let groups: HashMap<i64, String> = dao::list_groups(&conn, workspace_id)?
            .into_iter()
            .map(|g| (g.id, g.name))
            .collect();
        (workspace, repos, groups)
    };

    let mut repositories: Vec<ManifestRepo> = repos
        .par_iter()
        .map(|repo| {
            let (remote_url, default_branch) = manifest::read_remote_info(Path::new(&repo.path));
            ManifestRepo {
                path: repo.relative_path.replace('\\', "/"),
                name: repo.name.clone(),
                remote_url,
                default_branch,
                group: repo.group_id.and_then(|id| groups.get(&id).cloned()),
                tags: repo.tags.clone(),
            }
        })
        .collect();
    repositories.sort_by(|a, b| a.path.cmp(&b.path));

    let manifest = WorkspaceManifest {
        version: MANIFEST_VERSION,
        name: workspace.name,
        exported_at: chrono::Utc::now().to_rfc3339(),
        repositories,
    };

    let json = manifest::serialize_manifest(&manifest)?;
    std::fs::write(&file_path, json)?;
    log::info!(
        "Exported workspace manifest ({} repos) to {}",
        manifest.repositories.len(),
        file_path
    );
    Ok(manifest)
}

/// Read and validate a manifest file chosen via the frontend open dialog.
#[tauri::command]
pub fn read_manifest_file(file_path: String) -> AppResult<WorkspaceManifest> {
    let content = std::fs::read_to_string(&file_path)
        .map_err(|e| AppError::Other(format!("无法读取 Manifest 文件 {}: {}", file_path, e)))?;
    manifest::parse_manifest(&content)
}

/// Compute the import preview / clone plan for a manifest against a target
/// workspace root: which repos will be cloned, which are skipped because the
/// destination already exists, and which have no remote URL. The frontend
/// then submits the `Clone` entries as `TaskType::Clone` tasks.
#[tauri::command]
pub fn plan_manifest_clone(manifest: WorkspaceManifest, workspace_root: String) -> AppResult<ClonePlan> {
    let root = Path::new(&workspace_root);
    if !root.is_dir() {
        return Err(AppError::Other(format!("目标目录不存在或不是目录: {}", workspace_root)));
    }
    manifest::build_clone_plan(&manifest, root)
}
