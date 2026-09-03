use tauri::State;

use crate::db::dao;
use crate::error::{AppError, AppResult};
use crate::models::workspace::{CreateWorkspaceRequest, UpdateWorkspaceRequest, Workspace};
use crate::state::AppState;

/// Add a new workspace.
/// The workspace path does not need to be a Git repository itself -
/// it's simply a root directory that may contain multiple repos.
#[tauri::command]
pub fn add_workspace(req: CreateWorkspaceRequest, state: State<'_, AppState>) -> AppResult<Workspace> {
    let conn = state
        .db
        .lock()
        .map_err(|e| AppError::Other(format!("DB lock error: {}", e)))?;
    dao::insert_workspace(&conn, &req)
}

/// List all registered workspaces.
#[tauri::command]
pub fn list_workspaces(state: State<'_, AppState>) -> AppResult<Vec<Workspace>> {
    let conn = state
        .db
        .lock()
        .map_err(|e| AppError::Other(format!("DB lock error: {}", e)))?;
    dao::list_workspaces(&conn)
}

/// Remove a workspace by ID.
/// Cascading delete will remove all associated repositories and groups.
#[tauri::command]
pub fn remove_workspace(id: i64, state: State<'_, AppState>) -> AppResult<()> {
    let conn = state
        .db
        .lock()
        .map_err(|e| AppError::Other(format!("DB lock error: {}", e)))?;
    dao::delete_workspace(&conn, id)
}

/// Update workspace name and/or scan depth.
#[tauri::command]
pub fn update_workspace(id: i64, req: UpdateWorkspaceRequest, state: State<'_, AppState>) -> AppResult<Workspace> {
    let conn = state
        .db
        .lock()
        .map_err(|e| AppError::Other(format!("DB lock error: {}", e)))?;
    dao::update_workspace(&conn, id, &req)
}
