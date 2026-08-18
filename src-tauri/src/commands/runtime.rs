//! Runtime configuration IPC (R-07).

use std::collections::BTreeMap;
use std::sync::MutexGuard;

use rusqlite::Connection;
use tauri::{command, State};

use crate::error::{AppError, AppResult};
use crate::runtime::{
    create_config, delete_config, get_config, get_workspace_environment, list_configs,
    resolve_environment, set_workspace_environment, update_config, CreateRuntimeConfigRequest,
    RuntimeApplicationConfig, RuntimeConfigSummary, UpdateRuntimeConfigRequest,
};
use crate::state::AppState;

fn lock_db<'a>(state: &'a State<'_, AppState>) -> AppResult<MutexGuard<'a, Connection>> {
    state
        .db
        .lock()
        .map_err(|error| AppError::Other(format!("DB lock error: {error}")))
}

#[command]
pub fn create_runtime_config(
    req: CreateRuntimeConfigRequest,
    state: State<'_, AppState>,
) -> AppResult<RuntimeApplicationConfig> {
    let conn = lock_db(&state)?;
    create_config(&conn, &req)
}

#[command]
pub fn update_runtime_config(
    req: UpdateRuntimeConfigRequest,
    state: State<'_, AppState>,
) -> AppResult<RuntimeApplicationConfig> {
    let conn = lock_db(&state)?;
    update_config(&conn, &req)
}

#[command]
pub fn delete_runtime_config(
    workspace_id: i64,
    name: String,
    state: State<'_, AppState>,
) -> AppResult<()> {
    let conn = lock_db(&state)?;
    delete_config(&conn, workspace_id, &name)
}

#[command]
pub fn list_runtime_configs(
    workspace_id: i64,
    state: State<'_, AppState>,
) -> AppResult<Vec<RuntimeConfigSummary>> {
    let conn = lock_db(&state)?;
    list_configs(&conn, workspace_id)
}

#[command]
pub fn get_runtime_config(
    workspace_id: i64,
    name: String,
    state: State<'_, AppState>,
) -> AppResult<RuntimeApplicationConfig> {
    let conn = lock_db(&state)?;
    get_config(&conn, workspace_id, &name)
}

#[command]
pub fn resolve_runtime_environment(
    workspace_id: i64,
    name: String,
    state: State<'_, AppState>,
) -> AppResult<BTreeMap<String, String>> {
    let conn = lock_db(&state)?;
    let values = resolve_environment(&conn, workspace_id, &name)?;
    // Never return sensitive values over IPC. Launcher internals should call
    // runtime::resolve_environment directly once process execution exists.
    Ok(values
        .into_iter()
        .map(|(key, value)| {
            let sensitive = crate::core::secret::is_sensitive_environment_key(&key);
            (
                key,
                if sensitive {
                    "••••••••".into()
                } else {
                    value
                },
            )
        })
        .collect())
}

#[command]
pub fn get_workspace_runtime_environment(
    workspace_id: i64,
    state: State<'_, AppState>,
) -> AppResult<BTreeMap<String, String>> {
    let conn = lock_db(&state)?;
    get_workspace_environment(&conn, workspace_id)
}

#[command]
pub fn set_workspace_runtime_environment(
    workspace_id: i64,
    environment: BTreeMap<String, String>,
    state: State<'_, AppState>,
) -> AppResult<()> {
    let conn = lock_db(&state)?;
    set_workspace_environment(&conn, workspace_id, environment)
}
