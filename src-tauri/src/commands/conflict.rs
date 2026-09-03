//! Conflict Resolver commands (T-16).

use std::path::Path;
use std::sync::{Arc, Mutex};

use rusqlite::Connection;
use tauri::State;

use crate::core::conflict::{self, ConflictContent, OperationState};
use crate::core::operation_log::{self, NewOperationLogItem};
use crate::error::AppResult;
use crate::state::AppState;

/// The repo's current operation + conflict state (CONFLICT detection;
/// routes Continue / Abort to the right state machine).
#[tauri::command]
pub fn get_operation_state(repo_path: String) -> AppResult<OperationState> {
    conflict::operation_state(Path::new(&repo_path))
}

/// Load BASE / OURS / THEIRS + worktree content of one conflicted file.
#[tauri::command]
pub fn get_conflict_content(repo_path: String, path: String) -> AppResult<ConflictContent> {
    conflict::conflict_content(Path::new(&repo_path), &path)
}

/// Resolve one conflicted file: "ours" | "theirs" | "both".
#[tauri::command]
pub fn resolve_conflict(
    repo_path: String,
    path: String,
    strategy: String,
    state: State<'_, AppState>,
) -> AppResult<()> {
    let before = operation_log::snapshot_head(Path::new(&repo_path));
    conflict::resolve_conflict(Path::new(&repo_path), &path, &strategy)?;
    record_conflict_resolution(&repo_path, &path, before, &state.db);
    Ok(())
}

/// Resolve one conflicted file with manually edited content (null = delete).
#[tauri::command]
pub fn resolve_conflict_with_content(
    repo_path: String,
    path: String,
    content: Option<String>,
    state: State<'_, AppState>,
) -> AppResult<()> {
    let before = operation_log::snapshot_head(Path::new(&repo_path));
    conflict::resolve_conflict_with_content(Path::new(&repo_path), &path, content.as_deref())?;
    record_conflict_resolution(&repo_path, &path, before, &state.db);
    Ok(())
}

/// Record the confirmed T-16 Apply action without persisting user file
/// content. T-34 Undo is ref-snapshot based, so a conflict resolution remains
/// recoverable through the active Git operation's Abort flow or manual edits.
fn record_conflict_resolution(
    repo_path: &str,
    path: &str,
    before: Option<(String, String)>,
    db: &Arc<Mutex<Connection>>,
) {
    let Some((ref_name, before_oid)) = before else {
        return;
    };
    let after_oid = operation_log::snapshot_head(Path::new(repo_path)).map(|(_, oid)| oid);
    operation_log::record_operation_best_effort(
        db,
        repo_path,
        operation_log::OP_CONFLICT_RESOLUTION,
        &format!("resolve conflict: {path}"),
        vec![NewOperationLogItem {
            repo_path: repo_path.to_string(),
            ref_name,
            before_oid,
            after_oid,
            detail: Some(format!("path:{path}")),
        }],
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::operation_log::{get_operation_log, query_operation_logs, LogFilter};

    #[test]
    fn logs_resolution_metadata_without_conflict_content() {
        let mut conn = Connection::open_in_memory().unwrap();
        crate::db::init_db(&mut conn).unwrap();
        let db = Arc::new(Mutex::new(conn));

        record_conflict_resolution(
            "/workspace/project",
            "src/config.rs",
            Some(("main".to_string(), "a".repeat(40))),
            &db,
        );

        let conn = db.lock().unwrap();
        let page = query_operation_logs(
            &conn,
            &LogFilter {
                op_type: Some(operation_log::OP_CONFLICT_RESOLUTION),
                ..Default::default()
            },
            10,
            0,
        )
        .unwrap();
        assert_eq!(page.total, 1);
        assert_eq!(page.logs[0].summary, "resolve conflict: src/config.rs");

        let detail = get_operation_log(&conn, page.logs[0].id).unwrap();
        assert_eq!(detail.items[0].detail.as_deref(), Some("path:src/config.rs"));
        assert_eq!(detail.items[0].after_oid, None);
    }
}
