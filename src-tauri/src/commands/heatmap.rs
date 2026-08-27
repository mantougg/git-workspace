//! 提交热力图 IPC（F-01b）。

use tauri::State;

use crate::core::heatmap::{self, CommitHeatmap};
use crate::db::dao;
use crate::error::{AppError, AppResult};
use crate::state::AppState;

/// 当前用户在指定 workspace 全部仓库的提交按天计数（默认最近 365 天）。
#[tauri::command]
pub fn get_commit_heatmap(
    workspace_id: i64,
    days: Option<u32>,
    state: State<'_, AppState>,
) -> AppResult<CommitHeatmap> {
    let repo_paths = {
        let conn = state
            .db
            .lock()
            .map_err(|e| AppError::Other(format!("DB lock error: {}", e)))?;
        dao::list_repositories_by_workspace(&conn, workspace_id)?
            .into_iter()
            .map(|repo| repo.path)
            .collect::<Vec<_>>()
    };
    let days = days.unwrap_or(365).min(366 * 5) as i64;
    let since = chrono::Utc::now().timestamp() - days * 24 * 3600;
    Ok(heatmap::workspace_heatmap(&repo_paths, since))
}
