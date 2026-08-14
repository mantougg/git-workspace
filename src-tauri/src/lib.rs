mod commands;
mod core;
mod db;
mod error;
mod models;
mod state;
mod task;

pub mod benchmark;

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use commands::{diff, git_ops, graph, repository, workspace};
use core::git_ops::GitOps;
use state::AppState;
use task::manager::TaskManager;
use tauri::Manager;

use crate::error::AppResult;

/// Initialize the SQLite database in the app data directory.
/// Creates the directory if it doesn't exist, then opens/creates the database file
/// and runs schema migrations.
fn init_database(app_data_dir: &std::path::Path) -> AppResult<rusqlite::Connection> {
    // Ensure app data directory exists
    if !app_data_dir.exists() {
        fs::create_dir_all(app_data_dir)?;
    }

    let db_path = app_data_dir.join("gitworkspace.db");
    let mut conn = rusqlite::Connection::open(db_path)?;

    // Apply PRAGMAs (WAL / foreign_keys / busy_timeout / synchronous) and run
    // versioned schema migrations.
    db::init_db(&mut conn)?;

    log::info!("Database initialized at {:?}", app_data_dir);
    Ok(conn)
}

/// Determine the app data directory path.
/// On Windows: %APPDATA%/com.gitworkspace.app
/// On Linux: ~/.local/share/com.gitworkspace.app
/// On macOS: ~/Library/Application Support/com.gitworkspace.app
fn get_app_data_dir() -> PathBuf {
    if let Some(dir) = dirs::config_dir() {
        dir.join("com.gitworkspace.app")
    } else if let Some(dir) = dirs::home_dir() {
        dir.join(".gitworkspace")
    } else {
        PathBuf::from(".gitworkspace")
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Initialize the module-segregating logger (app/git/task/ai/performance.log,
    // secrets redacted) before anything else logs.
    crate::core::logger::init_logger(&crate::core::logger::logs_dir())
        .expect("failed to initialize logger");

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            // Initialize database
            let app_data_dir = get_app_data_dir();
            let conn = init_database(&app_data_dir)?;
            let db = Arc::new(std::sync::Mutex::new(conn));

            // Crash recovery: mark any tasks left queued/running from a previous
            // process as interrupted so they don't appear stuck forever.
            if let Ok(c) = db.lock() {
                let now = chrono::Utc::now().to_rfc3339();
                match db::dao::mark_interrupted_tasks(&c, &now) {
                    Ok(0) => {}
                    Ok(n) => log::warn!(
                        "Marked {} unfinished tasks as interrupted after restart",
                        n
                    ),
                    Err(e) => log::warn!("Failed to mark interrupted tasks: {}", e),
                }
            }

            // Create GitOps with default SSH credentials
            let git_ops = Arc::new(GitOps::with_default_ssh());

            // Create TaskManager with 8 workers
            let task_manager =
                TaskManager::new(8, git_ops, app.handle().clone(), Arc::clone(&db));

            // Create and manage app state
            let state = AppState::new(db, task_manager);
            app.manage(state);

            log::info!("GitWorkspace application started successfully");
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // Workspace commands
            workspace::add_workspace,
            workspace::list_workspaces,
            workspace::remove_workspace,
            workspace::update_workspace,
            // Repository commands
            repository::scan_repositories,
            repository::scan_repository_subtree,
            repository::list_repositories,
            repository::refresh_repository_status,
            repository::get_workspace_changes,
            repository::list_groups,
            repository::create_group,
            repository::delete_group,
            repository::assign_group,
            // Diff commands
            diff::get_diff,
            // Branch commands (T-09)
            commands::branch::list_branches,
            commands::branch::create_branch,
            commands::branch::checkout_branch,
            commands::branch::delete_branch,
            commands::branch::rename_branch,
            commands::branch::set_upstream,
            commands::branch::track_remote_branch,
            commands::branch::push_branch,
            commands::branch::compare_branches,
            // History commands (T-13)
            commands::history::cherry_pick,
            commands::history::revert_commit,
            commands::history::reset_to,
            commands::history::abort_pick,
            commands::history::get_conflict_files,
            // Reflog command (T-14)
            commands::reflog::get_reflog,
            // Git ops commands (batch operations)
            git_ops::batch_add,
            git_ops::batch_restore,
            git_ops::batch_fetch,
            git_ops::batch_pull,
            git_ops::batch_push,
            git_ops::batch_commit,
            git_ops::sync_fetch,
            git_ops::sync_pull,
            git_ops::sync_push,
            git_ops::start_watcher,
            git_ops::stop_watcher,
            // Task commands
            commands::task::submit_tasks,
            commands::task::get_task_status,
            commands::task::cancel_task,
            commands::task::list_active_tasks,
            commands::task::clear_finished_tasks,
            // Graph commands
            graph::get_commit_history,
            graph::get_branches,
            // AI commands
            commands::ai::ai_review,
            commands::ai::build_code_index,
            commands::ai::ai_search,
            commands::ai::clear_code_index,
            // Log commands
            commands::logs::list_log_files,
            commands::logs::open_logs,
            commands::logs::export_logs,
            commands::logs::clear_logs,
        ])
        .run(tauri::generate_context!())
        .expect("error while running GitWorkspace");
}
