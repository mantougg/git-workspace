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
///
/// `pub(crate)` so commands that keep config files next to the DB (e.g. the
/// T-19 health weights) resolve the same location.
pub(crate) fn get_app_data_dir() -> PathBuf {
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
            diff::get_unstaged_diff,
            diff::get_staged_diff,
            diff::get_revision_diff,
            diff::get_commit_diff,
            // Hunk / line staging commands (T-12)
            diff::stage_hunk,
            diff::unstage_hunk,
            diff::stage_lines,
            diff::unstage_lines,
            // Batch operation commands (T-20)
            commands::batch::select_repos,
            commands::batch::batch_branch_op,
            commands::batch::batch_dry_run,
            // Worktree commands (T-17)
            commands::worktree::list_worktrees,
            commands::worktree::create_worktree,
            commands::worktree::remove_worktree,
            // Commit 增强 commands (T-11)
            git_ops::scan_commit,
            git_ops::get_commit_identity,
            git_ops::set_repo_identity,
            git_ops::set_group_identity,
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
            // Stash commands (T-10)
            commands::stash::list_stashes,
            commands::stash::stash_changes,
            commands::stash::apply_stash,
            commands::stash::pop_stash,
            commands::stash::drop_stash,
            commands::stash::clear_stashes,
            commands::stash::get_stash_diff,
            commands::stash::branch_from_stash,
            // Merge / Rebase commands (T-15)
            commands::merge_rebase::merge_branch,
            commands::merge_rebase::merge_continue,
            commands::merge_rebase::merge_abort,
            commands::merge_rebase::get_merge_in_progress,
            commands::merge_rebase::list_rebase_commits,
            commands::merge_rebase::start_rebase,
            commands::merge_rebase::rebase_continue,
            commands::merge_rebase::rebase_skip,
            commands::merge_rebase::rebase_abort,
            commands::merge_rebase::get_rebase_state,
            // Conflict Resolver commands (T-16)
            commands::conflict::get_operation_state,
            commands::conflict::get_conflict_content,
            commands::conflict::resolve_conflict,
            commands::conflict::resolve_conflict_with_content,
            commands::history::pick_continue,
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
            // Workspace Health commands (T-19)
            commands::health::get_workspace_health,
            commands::health::get_health_extras,
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
            // Workspace Stash commands (T-21)
            commands::workspace_stash::save_workspace_stash,
            commands::workspace_stash::list_workspace_stashes,
            commands::workspace_stash::get_workspace_stash_items,
            commands::workspace_stash::check_workspace_stash,
            commands::workspace_stash::restore_workspace_stash,
            commands::workspace_stash::delete_workspace_stash,
            // Workspace Change Set commands (T-22)
            commands::change_set::list_change_sets,
            commands::change_set::create_change_set,
            commands::change_set::update_change_set,
            commands::change_set::delete_change_set,
            commands::change_set::add_change_set_repositories,
            commands::change_set::remove_change_set_repository,
            commands::change_set::get_change_set_summary,
            // Pipeline / Task DAG commands (T-23 / T-24)
            commands::pipeline::submit_dag_tasks,
            commands::pipeline::get_dag_graph,
            commands::pipeline::cancel_dag,
            commands::pipeline::list_pipeline_templates,
            commands::pipeline::save_pipeline_template,
            commands::pipeline::delete_pipeline_template,
            commands::pipeline::get_sample_pipeline,
            commands::pipeline::run_pipeline,
            commands::pipeline::get_pipeline_run,
            // Workspace Manifest commands (T-33)
            commands::manifest::export_workspace_manifest,
            commands::manifest::read_manifest_file,
            commands::manifest::plan_manifest_clone,
            // Operation Log / Undo commands (T-34)
            commands::operation_log::list_operation_logs,
            commands::operation_log::get_operation_log_detail,
            commands::operation_log::preview_undo_operation,
            commands::operation_log::undo_operation,
        ])
        .run(tauri::generate_context!())
        .expect("error while running GitWorkspace");
}
