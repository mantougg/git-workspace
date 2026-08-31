//! Node.js project discovery IPC (N-02).

use tauri::{command, State};

use crate::error::{AppError, AppResult};
use crate::node::{
    discover_package_jsons, global_package_cache, sync_node_projects, NodeProjectNode,
};
use crate::runtime::config::workspace_root;
use crate::state::AppState;

/// Discover and index workspace `package.json` files, then return the hot-path
/// SQLite list. The workspace path and scan depth are read from the DB so the
/// command cannot escape the configured workspace boundary.
#[command]
pub fn node_list_projects(
    workspace_id: i64,
    state: State<'_, AppState>,
) -> AppResult<Vec<NodeProjectNode>> {
    let (root, scan_depth) = {
        let conn = state
            .db
            .lock()
            .map_err(|error| AppError::Other(format!("DB lock error: {error}")))?;
        let root = workspace_root(&conn, workspace_id)?;
        let depth: i64 = conn.query_row(
            "SELECT scan_depth FROM workspaces WHERE id = ?1",
            [workspace_id],
            |row| row.get(0),
        )?;
        (root, depth.max(1) as usize)
    };

    let discovery = discover_package_jsons(&root, scan_depth, Some(global_package_cache()), None);
    let mut conn = state
        .db
        .lock()
        .map_err(|error| AppError::Other(format!("DB lock error: {error}")))?;
    sync_node_projects(&mut conn, workspace_id, &discovery)
}
