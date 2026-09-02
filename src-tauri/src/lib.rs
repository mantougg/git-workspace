pub mod ai;
mod commands;
mod core;
mod db;
mod error;
pub mod java;
pub mod maven;
mod models;
pub mod node;
pub mod process;
pub mod remote;
pub mod runtime;
mod state;
pub mod symbols;
mod task;

pub mod benchmark;

#[cfg(test)]
pub(crate) mod test_support;

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

/// F-06：显式设置 Windows 任务栏图标。
///
/// tao 的 `set_window_icon`（Tauri `set_icon` 走的也是它）只设置
/// WM_SETICON ICON_SMALL；Windows 任务栏按钮用的是 ICON_BIG，实测打包
/// 后 ICON_BIG 为 0 → 任务栏显示默认空白图标。tao 的大图标接口
/// （`set_taskbar_icon`）没有被 tauri-runtime-wry 暴露，这里直接从 exe
/// 内嵌资源加载 HICON（tauri-build 以固定 ID 32512 嵌入 `icons/icon.ico`），
/// 再用 WM_SETICON 同时设置大/小图标。
#[cfg(windows)]
fn set_windows_taskbar_icon(app: &tauri::App) {
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        LoadImageW, SendMessageW, ICON_BIG, ICON_SMALL, IMAGE_ICON, LR_DEFAULTSIZE, WM_SETICON,
    };

    /// tauri-build `set_icon_with_id(..., "32512")` 的固定资源 ID。
    const APP_ICON_RESOURCE_ID: usize = 32512;

    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let Ok(hwnd) = window.hwnd() else {
        return;
    };
    // tauri 返回 windows crate 的 HWND（newtype 包装 isize），转成裸指针。
    let hwnd = hwnd.0 as *mut std::ffi::c_void;
    unsafe {
        let module = GetModuleHandleW(std::ptr::null());
        let icon = LoadImageW(
            module,
            APP_ICON_RESOURCE_ID as *const u16,
            IMAGE_ICON,
            0,
            0,
            LR_DEFAULTSIZE,
        );
        if icon.is_null() {
            log::warn!("F-06: failed to load embedded icon resource 32512");
            return;
        }
        let hwnd = hwnd as windows_sys::Win32::Foundation::HWND;
        SendMessageW(hwnd, WM_SETICON, ICON_BIG as usize, icon as isize);
        SendMessageW(hwnd, WM_SETICON, ICON_SMALL as usize, icon as isize);
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Initialize the module-segregating logger (app/git/task/ai/performance.log,
    // secrets redacted) before anything else logs.
    crate::core::logger::init_logger(&crate::core::logger::logs_dir())
        .expect("failed to initialize logger");

    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_window_state::Builder::default().build())
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
                    Ok(n) => {
                        log::warn!("Marked {} unfinished tasks as interrupted after restart", n)
                    }
                    Err(e) => log::warn!("Failed to mark interrupted tasks: {}", e),
                }
            }

            // Create GitOps with default SSH credentials
            let git_ops = Arc::new(GitOps::with_default_ssh());

            // R-12：RuntimeService（§63 读侧 + §65 Runtime 任务执行体），
            // 与 AppState 共享同一个 PomCache。
            let pom_cache = Arc::new(crate::maven::PomCache::new());
            let runtime_service = crate::runtime::service::RuntimeService::new(
                app.handle().clone(),
                Arc::clone(&db),
                Arc::clone(&pom_cache),
            );
            let runtime_handler: Arc<dyn crate::task::runtime::RuntimeTaskHandler> =
                runtime_service.clone();

            // Create TaskManager with 8 workers (Runtime tasks dispatched to
            // the RuntimeService handler, R-12).
            let task_manager = Arc::new(TaskManager::new(
                8,
                git_ops,
                app.handle().clone(),
                Arc::clone(&db),
                Some(runtime_handler),
            ));

            // R-17 File Watch 引擎：只监听「autoRestart 开启且进程活跃」应用的
            // 闭包模块目录；变化 → 防抖 → 影响分析 → RebuildRestart 任务。
            // 与 RuntimeService 共享同一批进程管理器 / 图 / 闭包缓存实例。
            let (watch_processes, watch_graph_cache, watch_closure_cache) =
                runtime_service.watch_shared_parts();
            let watch_engine = crate::runtime::watch::RuntimeWatchEngine::spawn(
                Arc::clone(&db),
                watch_graph_cache,
                watch_closure_cache,
                Arc::new(crate::runtime::events::TauriRuntimeEmitter::new(
                    app.handle().clone(),
                )),
                watch_processes,
            );
            watch_engine.attach_task_manager({
                let submitter: Arc<dyn crate::runtime::watch::WatchTaskSubmitter> =
                    task_manager.clone();
                submitter
            });

            // R-21 Git 联动引擎：§47 Status 提示 / §48 分支切换复核，
            // 与 RuntimeService 共享同一批图 / 闭包缓存实例。
            let (_processes, link_graph_cache, link_closure_cache) =
                runtime_service.watch_shared_parts();
            let git_link = crate::runtime::git_link::GitLinkEngine::spawn(
                Arc::clone(&db),
                link_graph_cache,
                link_closure_cache,
                Arc::new(crate::runtime::events::TauriRuntimeEmitter::new(
                    app.handle().clone(),
                )),
            );
            git_link.attach_task_manager({
                let submitter: Arc<dyn crate::runtime::watch::WatchTaskSubmitter> =
                    task_manager.clone();
                submitter
            });

            // Create and manage app state
            let mut state = AppState::new(
                Arc::clone(&db),
                task_manager,
                Arc::clone(&runtime_service),
                pom_cache,
                git_link,
            );
            // AI-02：Gateway 事件出口指向 Tauri（`ai-request://progress`）；
            // AI-04：同时装配 DB 句柄（会话/审计）与结果缓存。
            state.ai_gateway = Arc::new(
                crate::ai::AiGateway::new(
                    crate::ai::GatewayConfig::default(),
                    std::sync::Arc::new(
                        crate::ai::transport::ReqwestTransport::new().expect("reqwest transport"),
                    ),
                    std::sync::Arc::new(crate::ai::events::TauriAiEventSink::new(
                        app.handle().clone(),
                    )),
                )
                .with_store(Arc::clone(&db))
                .with_cache(Arc::clone(&state.ai_result_cache)),
            );
            // AI-12：外部 Agent Adapter 共用的工具执行上下文（必须在 state
            // 被 manage 之前构建），工具唯一来源是 AI-05 注册表。
            let ai_tool_context = crate::ai::ToolContext::from_state(&state);
            app.manage(state);

            // AI-12：启动本地 MCP 端点（仅 127.0.0.1，生命周期随应用启停；
            // Offline First——失败只记日志，不影响应用启动）。
            crate::ai::external::server::spawn(ai_tool_context);

            // F-06：修复打包后 Windows 任务栏无图标（详见函数注释）。
            #[cfg(windows)]
            set_windows_taskbar_icon(&app);

            // R-10/R-12：启动对账——接管上次会话遗留的活跃 Runtime 进程、
            // 死去的落终态。后台线程执行，不阻塞启动。
            runtime_service.reconcile_on_startup();

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
            git_ops::watcher_status,
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
            // Commit heatmap (F-01b)
            commands::heatmap::get_commit_heatmap,
            // AI commands
            commands::ai::ai_review,
            commands::ai::build_code_index,
            commands::ai::ai_search,
            commands::ai::clear_code_index,
            // AI-01：Provider / Model / Credential / 任务默认值 / Settings
            commands::ai::ai_list_providers,
            commands::ai::ai_save_provider,
            commands::ai::ai_remove_provider,
            commands::ai::ai_test_provider,
            commands::ai::ai_list_models,
            commands::ai::ai_save_model,
            commands::ai::ai_remove_model,
            commands::ai::ai_set_task_default_model,
            commands::ai::ai_clear_task_default_model,
            commands::ai::ai_get_settings_summary,
            commands::ai::ai_set_credential,
            commands::ai::ai_clear_credential,
            commands::ai::ai_submit_request,
            commands::ai::ai_approve_request,
            commands::ai::ai_cancel_request,
            commands::ai::ai_get_request_status,
            // AI-11：Action Proposal 确认与状态
            commands::ai::ai_list_proposals,
            commands::ai::ai_get_proposal,
            commands::ai::ai_confirm_proposal,
            commands::ai::ai_reject_proposal,
            commands::ai::ai_build_context_preview,
            commands::ai::ai_runtime_diagnostic_preview,
            // AI commands（AI-04：会话 / 审计 / 缓存）
            commands::ai::ai_create_session,
            commands::ai::ai_list_sessions,
            commands::ai::ai_get_session,
            commands::ai::ai_rename_session,
            commands::ai::ai_archive_session,
            commands::ai::ai_delete_session,
            commands::ai::ai_export_session,
            commands::ai::ai_get_session_persistence,
            commands::ai::ai_set_session_persistence,
            commands::ai::ai_get_request_audit,
            commands::ai::ai_list_session_audits,
            commands::ai::ai_clear_result_cache,
            // AI-05：只读工具注册表与受限执行器
            commands::ai::ai_list_tools,
            commands::ai::ai_execute_tool,
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
            // JDK Manager commands (R-04)
            commands::jdk::discover_jdks,
            commands::jdk::list_jdks,
            commands::jdk::get_jdk,
            commands::jdk::add_jdk_manual,
            commands::jdk::validate_jdk,
            commands::jdk::prune_invalid_jdks,
            commands::jdk::remove_jdk,
            // Maven 检测与执行策略 commands (R-05)
            commands::maven::detect_maven,
            commands::maven::detect_mvnd,
            commands::maven::list_maven_executables_cmd,
            commands::maven::get_maven_executable_cmd,
            commands::maven::validate_maven_executable,
            commands::maven::prune_invalid_maven,
            commands::maven::remove_maven_executable_cmd,
            commands::maven::resolve_local_repo,
            commands::maven::preview_maven_command,
            commands::maven::list_maven_candidates,
            commands::maven::build_maven_command,
            // F-16：Maven 扫描/手动添加/本地仓库覆盖
            commands::maven::scan_maven_installations,
            commands::maven::add_maven_executable,
            commands::maven::get_maven_local_repo_override,
            commands::maven::set_maven_local_repo_override,
            // Spring Boot application discovery (R-06)
            commands::spring_boot::detect_spring_boot,
            // Node.js package.json discovery (N-02)
            commands::node::node_list_projects,
            // N-08：Node / package-manager executable registry
            commands::node::node_list_executables,
            commands::node::node_add_executable,
            commands::node::node_validate_executable,
            commands::node::node_remove_executable,
            commands::node::node_prune_executables,
            // N-10：本机 Node 工具链扫描（只读候选发现）
            commands::node::node_scan_executables,
            commands::node::node_install,
            // Runtime configuration (R-07)
            commands::runtime::create_runtime_config,
            commands::runtime::update_runtime_config,
            commands::runtime::delete_runtime_config,
            commands::runtime::list_runtime_configs,
            commands::runtime::get_runtime_config,
            commands::runtime::resolve_runtime_environment,
            commands::runtime::get_workspace_runtime_environment,
            commands::runtime::set_workspace_runtime_environment,
            // Runtime 控制面（R-12，§63）
            commands::runtime::runtime_list_projects,
            commands::runtime::runtime_list_unified_projects,
            commands::runtime::runtime_inspect_project,
            commands::runtime::runtime_resolve_dependencies,
            commands::runtime::runtime_get_dependency_graph,
            commands::runtime::runtime_get_closure,
            commands::runtime::runtime_build,
            commands::runtime::runtime_start,
            commands::runtime::runtime_stop,
            commands::runtime::runtime_restart,
            commands::runtime::runtime_rebuild_restart,
            commands::runtime::runtime_list_processes,
            commands::runtime::runtime_process_status,
            commands::runtime::runtime_running_briefs,
            commands::runtime::runtime_stop_blocking,
            commands::runtime::runtime_get_logs,
            commands::runtime::runtime_clear_logs,
            commands::runtime::runtime_export_logs,
            commands::runtime::runtime_start_environment,
            commands::runtime::runtime_stop_environment,
            commands::runtime::runtime_get_scheduler_config,
            commands::runtime::runtime_set_scheduler_config,
            // R-14 §75 Command Safety：脚本确认状态
            commands::runtime::runtime_get_script_approvals,
            commands::runtime::runtime_approve_script,
            commands::runtime::runtime_reset_script_approvals,
            // R-16 §41/§81：健康检查 + 端口管理
            commands::runtime::runtime_get_health,
            commands::runtime::runtime_list_health,
            commands::runtime::runtime_check_port,
            commands::runtime::runtime_kill_port_process,
            commands::runtime::runtime_change_runtime_port,
            // R-15 §38/§39/§40：Multi-Service Runtime Environment
            commands::runtime::runtime_list_environments,
            commands::runtime::runtime_save_environment,
            commands::runtime::runtime_delete_environment,
            commands::runtime::runtime_start_named_environment,
            commands::runtime::runtime_stop_named_environment,
            // R-19 §83：Runtime Templates
            commands::runtime::runtime_list_templates,
            commands::runtime::runtime_save_template,
            commands::runtime::runtime_delete_template,
            commands::runtime::runtime_save_config_as_template,
            commands::runtime::runtime_apply_template,
            // Application lifecycle commands
            commands::app::restart_app,
            // Terminal / IDE integration commands (T-31)
            commands::integration::open_in_terminal,
            commands::integration::open_in_ide,
            commands::integration::list_integration_targets,
            // Symbol index commands (T-28)
            commands::symbols::build_symbol_index,
            commands::symbols::search_symbols,
            commands::symbols::find_symbol_definitions,
            commands::symbols::find_symbol_references,
            commands::symbols::symbol_call_hierarchy,
            // Remote platform commands (T-29)
            commands::remote::detect_remote,
            commands::remote::remote_open_url,
            commands::remote::create_pull_request,
            commands::remote::get_ci_status,
            commands::remote::resolve_remote_token,
            commands::remote::save_remote_token,
            commands::remote::delete_remote_token,
        ])
        .build(tauri::generate_context!())
        .expect("error while building GitWorkspace")
        .run(|_app_handle, event| {
            // AI-12：应用退出时停止外部 Agent 端点并清理 discovery 文件。
            if matches!(event, tauri::RunEvent::Exit) {
                crate::ai::external::server::shutdown();
            }
        });
}
