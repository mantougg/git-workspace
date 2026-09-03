use std::path::Path;

use rayon::prelude::*;
use tauri::State;

use crate::core::{git_status, health};
use crate::db::dao;
use crate::error::{AppError, AppResult};
use crate::state::AppState;

/// Workspace health from the T-02 status cache (light checks only — the
/// heavy large-file / LFS / submodule checks go through `get_health_extras`
/// so this command never leaves the sub-50ms aggregation path).
///
/// Scoring weights come from `health-weights.json` in the app data dir
/// (defaults when absent) and are returned so the UI can re-score after
/// merging async heavy-check results.
#[tauri::command]
pub fn get_workspace_health(workspace_id: i64, state: State<'_, AppState>) -> AppResult<health::WorkspaceHealth> {
    let repos = {
        let conn = state
            .db
            .lock()
            .map_err(|e| AppError::Other(format!("DB lock error: {}", e)))?;
        dao::list_repositories_by_workspace(&conn, workspace_id)?
    };
    let weights = health::load_health_weights(&crate::get_app_data_dir());

    let entries: Vec<health::RepoHealth> = repos
        .into_par_iter()
        .filter_map(|repo| {
            // Cache first; fall back to live status (same contract as
            // `list_repositories`). Repos whose status cannot be read are
            // excluded from scoring rather than scored as healthy.
            let status = match state.status_cache.get(&repo.path) {
                Some(s) => s,
                None => match git_status::get_repo_status(Path::new(&repo.path)) {
                    Ok(s) => {
                        state.status_cache.insert(repo.path.clone(), s.clone());
                        s
                    }
                    Err(e) => {
                        log::warn!("health: status failed for {:?}: {}", repo.path, e);
                        return None;
                    }
                },
            };
            let anomalies: Vec<String> = health::anomalies_of(&status).into_iter().map(String::from).collect();
            let score = health::score_of(anomalies.iter().map(String::as_str), &weights);
            Some(health::RepoHealth {
                repo_path: repo.path,
                repo_name: repo.name,
                branch: status.branch.clone(),
                anomalies,
                score,
            })
        })
        .collect();

    Ok(health::aggregate_health(entries, weights))
}

/// Heavy health checks (large files / LFS / submodule), run on demand when
/// the Health page opens. Rayon-parallel (bounded pool); `git lfs` is probed
/// once for the whole batch, not per repo.
#[tauri::command]
pub fn get_health_extras(repo_paths: Vec<String>) -> AppResult<Vec<health::RepoHealthExtra>> {
    let lfs = health::lfs_available();
    Ok(repo_paths
        .into_par_iter()
        .map(|p| health::compute_health_extra(Path::new(&p), lfs))
        .collect())
}
