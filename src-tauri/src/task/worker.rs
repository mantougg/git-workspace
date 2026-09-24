use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use dashmap::DashMap;
use rusqlite::Connection;
use tauri::{AppHandle, Emitter};
use tokio::sync::{mpsc, Mutex};

use crate::core::git_ops::GitOps;
use crate::core::workspace_stash::{self, WorkspaceStashKind, WorkspaceStashRunResult};
use crate::db::dao;
use crate::error::{AppError, AppResult};
use crate::models::task::{BatchState, GitCommandResult, Task, TaskProgress, TaskStatus, TaskType};
use crate::task::console::{
    emit_git_op_output, finish_streaming, network_console_command, ConsoleStreamer,
};
use crate::task::dag::{DagContext, DagState};

/// Maximum retries for a failed task (network operations benefit most).
const MAX_RETRIES: usize = 2;
/// Hard timeout for a single task execution.
const TASK_TIMEOUT: Duration = Duration::from_secs(300);
/// Hard outer bound for Runtime tasks (R-12). The real enforcement lives
/// inside the Runtime executors (R-09 build timeout kills the Maven process
/// tree; Stop/Kill have their own grace bounds); this is only a runaway
/// guard. Builds of large workspaces legitimately run for tens of minutes,
/// so `TASK_TIMEOUT` (5 min, git-oriented) cannot be reused.
const RUNTIME_TASK_TIMEOUT: Duration = Duration::from_secs(3600);

/// Task types that legitimately outlive the 5-minute git-oriented bound and
/// whose cancellation must also reach a blocking body that cannot be killed
/// (Runtime builds / launches; GF-10 whole-workspace stash runs over many
/// repos).
fn uses_long_timeout(task_type: &TaskType) -> bool {
    matches!(
        task_type,
        TaskType::Runtime { .. }
            | TaskType::RuntimeUpdateConfig { .. }
            | TaskType::NodeInstall { .. }
            | TaskType::WorkspaceStashSave { .. }
            | TaskType::WorkspaceStashRestore { .. }
    )
}

/// GF-10: workspace stash runs (whole-workspace task, serial per-repo body).
fn is_ws_stash_run(task_type: &TaskType) -> bool {
    matches!(
        task_type,
        TaskType::WorkspaceStashSave { .. } | TaskType::WorkspaceStashRestore { .. }
    )
}

/// Spawn the worker pool that processes tasks from the shared receiver.
///
/// Each worker pulls tasks from the channel, executes them using GitOps
/// (in a blocking thread with a timeout + retries), and emits progress events
/// to the frontend. `dag_sender` is the queue's own sender, handed to the
/// DAG scheduler (T-24) so it can dispatch newly-unblocked nodes. Runtime
/// tasks (R-12) go to `runtime_handler` with the task's cancel flag wired in.
/// `ws_stash_runs` (GF-10) is where whole-workspace stash runs publish their
/// per-repo result for the pending IPC command.
#[allow(clippy::too_many_arguments)]
pub fn spawn_worker_pool(
    worker_count: usize,
    receiver: mpsc::Receiver<super::queue::TaskMessage>,
    dag_sender: mpsc::Sender<super::queue::TaskMessage>,
    git_ops: Arc<GitOps>,
    runtime_handler: Option<Arc<dyn super::runtime::RuntimeTaskHandler>>,
    active_tasks: Arc<DashMap<String, Task>>,
    cancel_flags: Arc<DashMap<String, Arc<AtomicBool>>>,
    app_handle: AppHandle,
    db: Arc<std::sync::Mutex<Connection>>,
    batches: Arc<DashMap<String, BatchState>>,
    dags: Arc<DashMap<String, DagState>>,
    ws_stash_runs: Arc<DashMap<String, WorkspaceStashRunResult>>,
) {
    let receiver = Arc::new(Mutex::new(receiver));

    tauri::async_runtime::spawn(async move {
        let mut workers = Vec::with_capacity(worker_count);

        for worker_id in 0..worker_count {
            let rx = Arc::clone(&receiver);
            let tx = dag_sender.clone();
            let ops = Arc::clone(&git_ops);
            let runtime_handler = runtime_handler.clone();
            let tasks = Arc::clone(&active_tasks);
            let flags = Arc::clone(&cancel_flags);
            let app = app_handle.clone();
            let db = Arc::clone(&db);
            let batch_map = Arc::clone(&batches);
            let dag_map = Arc::clone(&dags);
            let runs = Arc::clone(&ws_stash_runs);

            workers.push(tauri::async_runtime::spawn(async move {
                log::debug!("Task worker {} started", worker_id);

                loop {
                    let msg = {
                        let mut lock = rx.lock().await;
                        lock.recv().await
                    };

                    match msg {
                        Some(msg) => {
                            execute_task(
                                &ops,
                                &runtime_handler,
                                &tasks,
                                &flags,
                                &app,
                                &db,
                                &batch_map,
                                &dag_map,
                                &runs,
                                &tx,
                                msg.task,
                            )
                            .await;
                        }
                        None => {
                            log::debug!("Task worker {} shutting down", worker_id);
                            break;
                        }
                    }
                }
            }));
        }

        // Wait for all workers to complete
        for w in workers {
            let _ = w.await;
        }
    });
}

/// Whether a task's cancellation flag has been set.
fn is_cancelled(flags: &DashMap<String, Arc<AtomicBool>>, task_id: &str) -> bool {
    flags.get(task_id).map(|f| f.load(Ordering::Relaxed)).unwrap_or(false)
}

/// Network task types（PAF-25）：统一走 `*_streaming` 流式底座的操作集合。
fn is_network_task(task_type: &TaskType) -> bool {
    matches!(
        task_type,
        TaskType::Fetch | TaskType::Pull | TaskType::Push | TaskType::Clone { .. }
    )
}

/// Truncate a commit message for the Git Console meta line, by **chars** not
/// bytes — a byte slice would panic on a multi-byte UTF-8 boundary (e.g. CJK).
fn shorten_message(message: &str) -> String {
    const LIMIT: usize = 50;
    if message.chars().count() > LIMIT {
        let head: String = message.chars().take(LIMIT - 3).collect();
        format!("{}…", head)
    } else {
        message.to_string()
    }
}

/// GF-10: emit one `workspace_stash_progress` event (called from a blocking
/// worker thread after each repo; Tauri event emission is synchronous and
/// thread-safe, same as the Console streamer).
fn emit_ws_stash_progress(
    app: &AppHandle,
    progress: &workspace_stash::WorkspaceStashProgress,
) {
    if let Err(e) = app.emit("workspace_stash_progress", progress) {
        log::warn!("Failed to emit workspace_stash_progress: {}", e);
    }
}

/// GF-10: body of a `WorkspaceStashSave` task — stash every repo serially
/// (execution model unchanged), emitting progress after each repo and
/// persisting the record for whatever was stashed (including a cancelled
/// run's completed subset, so those repos stay restorable).
///
/// Split out of the worker's blocking closure so the whole path is
/// unit-testable without a Tauri `AppHandle` (the progress emitter is
/// injected). Returns the console summary line; the structured per-repo
/// result lands in `runs` for the pending IPC command.
#[allow(clippy::too_many_arguments)]
fn run_ws_stash_save(
    db: &Arc<std::sync::Mutex<Connection>>,
    runs: &Arc<DashMap<String, WorkspaceStashRunResult>>,
    task_id: &str,
    workspace_id: i64,
    record_name: &str,
    message: Option<&str>,
    include_untracked: bool,
    repo_paths: &[String],
    cancel: &AtomicBool,
    mut emit: impl FnMut(&workspace_stash::WorkspaceStashProgress),
) -> AppResult<String> {
    let total = repo_paths.len();
    let mut done = 0usize;
    let (outcomes, stashed) = workspace_stash::stash_repos_cancellable(
        repo_paths,
        record_name,
        message,
        include_untracked,
        Some(cancel),
        |o| {
            done += 1;
            emit(&workspace_stash::WorkspaceStashProgress {
                task_id: task_id.to_string(),
                kind: WorkspaceStashKind::Save,
                record_name: record_name.to_string(),
                index: done,
                total,
                repo_path: o.repo_path.clone(),
                repo_name: o.repo_name.clone(),
                status: o.status.clone(),
                detail: o.detail.clone(),
            });
        },
    );

    let record_id = if stashed.is_empty() {
        None
    } else {
        // Git phase finished without the DB lock held; the record is one
        // short transaction (single-writer model).
        let mut conn = db.lock().map_err(|e| AppError::Other(format!("DB lock error: {e}")))?;
        match workspace_stash::insert_workspace_stash(
            &mut conn,
            workspace_id,
            record_name,
            message.map(str::trim).filter(|m| !m.is_empty()),
            &stashed,
        ) {
            Ok(id) => Some(id),
            Err(e) => {
                // The per-repo stashes already happened; losing only the
                // association record must not fail the whole run — the repos
                // stay recoverable through each repo's single-repo Stash view.
                log::warn!("workspace stash record insert failed ({record_name}): {e}");
                None
            }
        }
    };
    let run = WorkspaceStashRunResult::save(record_name, record_id, outcomes);
    runs.insert(task_id.to_string(), run.clone());
    Ok(run.summary.summary_line())
}

/// GF-10: body of a `WorkspaceStashRestore` task — re-load the record's items,
/// re-check every one (the §46 pre-check already ran before submission; this
/// is the stale-window safety net) and apply it (kept on the stack, so a
/// partial run can simply be restored again).
#[allow(clippy::too_many_arguments)]
fn run_ws_stash_restore(
    db: &Arc<std::sync::Mutex<Connection>>,
    runs: &Arc<DashMap<String, WorkspaceStashRunResult>>,
    task_id: &str,
    workspace_stash_id: i64,
    record_name: &str,
    allow_branch_mismatch: bool,
    cancel: &AtomicBool,
    mut emit: impl FnMut(&workspace_stash::WorkspaceStashProgress),
) -> AppResult<String> {
    let items = {
        let conn = db.lock().map_err(|e| AppError::Other(format!("DB lock error: {e}")))?;
        workspace_stash::list_workspace_stash_items(&conn, workspace_stash_id)?
    };
    if items.is_empty() {
        return Err(AppError::NotFound("该 Workspace Stash 没有仓库项".into()));
    }
    let total = items.len();
    let mut done = 0usize;
    let outcomes = workspace_stash::restore_items_cancellable(
        &items,
        allow_branch_mismatch,
        Some(cancel),
        |o| {
            done += 1;
            emit(&workspace_stash::WorkspaceStashProgress {
                task_id: task_id.to_string(),
                kind: WorkspaceStashKind::Restore,
                record_name: record_name.to_string(),
                index: done,
                total,
                repo_path: o.repo_path.clone(),
                repo_name: o.repo_name.clone(),
                status: o.status.clone(),
                detail: o.detail.clone(),
            });
        },
    );
    let run = WorkspaceStashRunResult::restore(record_name, outcomes);
    runs.insert(task_id.to_string(), run.clone());
    Ok(run.summary.summary_line())
}

/// Execute a single task: update status, run the Git operation (with timeout +
/// retries), honour cancellation, and emit progress.
#[allow(clippy::too_many_arguments)]
async fn execute_task(
    ops: &Arc<GitOps>,
    runtime_handler: &Option<Arc<dyn super::runtime::RuntimeTaskHandler>>,
    tasks: &Arc<DashMap<String, Task>>,
    cancel_flags: &Arc<DashMap<String, Arc<AtomicBool>>>,
    app: &AppHandle,
    db: &Arc<std::sync::Mutex<Connection>>,
    batches: &Arc<DashMap<String, BatchState>>,
    dags: &Arc<DashMap<String, DagState>>,
    ws_stash_runs: &Arc<DashMap<String, WorkspaceStashRunResult>>,
    dag_sender: &mpsc::Sender<super::queue::TaskMessage>,
    mut task: Task,
) {
    // Early cancellation: the flag may have been set while the task sat in
    // the channel (queued cancel). Don't even start the git operation.
    if is_cancelled(cancel_flags, &task.id) {
        task.status = TaskStatus::Cancelled;
        if let Some(mut entry) = tasks.get_mut(&task.id) {
            entry.status = TaskStatus::Cancelled;
        }
        persist_final_status(db, &task);
        // GF-10: a workspace stash run cancelled before it started still gets
        // a result (every selected repo reported untouched) so the pending
        // IPC command resolves instead of waiting for its timeout.
        if let Some((kind, record_name, repo_paths)) = workspace_stash::ws_stash_task_info(&task.task_type) {
            ws_stash_runs.insert(
                task.id.clone(),
                WorkspaceStashRunResult::cancelled_before_start(kind, record_name, repo_paths),
            );
        }
        emit_progress(app, &task);
        finish_dag_node(dags, dag_sender, tasks, cancel_flags, db, app, batches, &task, None);
        update_batch(batches, db, app, &task);

        // Same delayed cleanup as the normal path.
        let tasks = Arc::clone(tasks);
        let flags = Arc::clone(cancel_flags);
        let task_id = task.id.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(Duration::from_secs(30)).await;
            tasks.remove(&task_id);
            flags.remove(&task_id);
        });
        return;
    }

    // Update status to Running
    if let Some(mut entry) = tasks.get_mut(&task.id) {
        entry.status = TaskStatus::Running { progress: 0.0 };
        task.status = TaskStatus::Running { progress: 0.0 };
    }
    emit_progress(app, &task);

    let task_type = task.task_type.clone();
    let mut final_status;
    let mut output: Option<String> = None;
    let mut attempt = 0usize;
    let ai_commit_before = if matches!(&task_type, TaskType::Commit { .. }) {
        crate::core::operation_log::snapshot_head(std::path::Path::new(&task.repo_path))
    } else {
        None
    };

    loop {
        if is_cancelled(cancel_flags, &task.id) {
            final_status = TaskStatus::Cancelled;
            break;
        }

        let ops = Arc::clone(ops);
        let repo_path = task.repo_path.clone();
        let task_type_for_exec = task_type.clone();
        // R-12 / GF-10: long-bound tasks (Runtime builds, whole-workspace
        // stash runs) get a longer hard timeout and — on timeout — have their
        // cancel flag set so a blocking body that cannot be killed still
        // stops at its next checkpoint.
        let long_timeout = uses_long_timeout(&task_type_for_exec);
        let runtime_handler = runtime_handler.clone();
        let cancel_flag = cancel_flags.get(&task.id).map(|f| Arc::clone(&f));
        let db_for_exec = Arc::clone(db);
        let app_for_exec = app.clone();
        let task_id_for_exec = task.id.clone();
        let repo_name_for_exec = task.repo_name.clone();
        let runs_for_exec = Arc::clone(ws_stash_runs);
        let hard_timeout = if long_timeout { RUNTIME_TASK_TIMEOUT } else { TASK_TIMEOUT };

        let result = tokio::time::timeout(
            hard_timeout,
            tokio::task::spawn_blocking(move || match &task_type_for_exec {
                TaskType::Runtime { .. } => {
                    let Some(handler) = runtime_handler else {
                        return Err(AppError::Task(
                            "Runtime 任务处理器未装配（应用启动未完成），请稍后重试".into(),
                        ));
                    };
                    let cancel = cancel_flag.unwrap_or_else(|| Arc::new(AtomicBool::new(false)));
                    handler.execute(&task_type_for_exec, cancel)
                }
                TaskType::RuntimeUpdateConfig {
                    workspace_id,
                    name,
                    config_json,
                } => {
                    let config: crate::runtime::UpdateRuntimeConfigRequest = serde_json::from_str(config_json)
                        .map_err(|e| AppError::Task(format!("Runtime 配置提案无效: {e}")))?;
                    let conn = db_for_exec
                        .lock()
                        .map_err(|e| AppError::Other(format!("DB lock error: {e}")))?;
                    if config.workspace_id != *workspace_id || config.name != *name {
                        return Err(AppError::Task("Runtime 配置提案作用域不匹配".into()));
                    }
                    crate::runtime::config::update_config(&conn, &config).map(|_| None)
                }
                TaskType::NodeInstall {
                    project_dir,
                    package_manager,
                } => {
                    let conn = db_for_exec
                        .lock()
                        .map_err(|e| AppError::Other(format!("DB lock error: {e}")))?;
                    let decision = crate::node::PackageManagerDecision {
                        manager: *package_manager,
                        source: crate::node::DecisionSource::Configured,
                        reason: format!("node_install 显式指定 {}", package_manager.name()),
                    };
                    let detection = crate::node::resolve_package_manager_with_registry(&conn, &decision)?;
                    drop(conn);
                    let cancel = cancel_flag.as_deref();
                    let summary = crate::node::install::execute_install(
                        detection,
                        *package_manager,
                        std::path::Path::new(project_dir),
                        cancel,
                        |stream, line| {
                            let _ = app_for_exec.emit(
                                "node_install_output",
                                serde_json::json!({
                                "taskId": task_id_for_exec.clone(),
                                    "stream": stream,
                                    "line": line,
                                }),
                            );
                        },
                    )?;
                    Ok(Some(summary))
                }
                TaskType::ConflictApply {
                    path,
                    strategy,
                    content,
                } => {
                    let repo = std::path::Path::new(&repo_path);
                    let before = crate::core::operation_log::snapshot_head(repo);
                    if let Some(content) = content.as_deref() {
                        crate::core::conflict::resolve_conflict_with_content(repo, path, Some(content))?;
                    } else {
                        crate::core::conflict::resolve_conflict(repo, path, strategy)?;
                    }
                    if let Some((ref_name, before_oid)) = before {
                        crate::core::operation_log::record_operation_best_effort(
                            &db_for_exec,
                            &repo_path,
                            crate::core::operation_log::OP_CONFLICT_RESOLUTION,
                            &format!("resolve conflict: {path}"),
                            vec![crate::core::operation_log::NewOperationLogItem {
                                repo_path: repo_path.clone(),
                                ref_name,
                                before_oid,
                                after_oid: crate::core::operation_log::snapshot_head(repo).map(|(_, oid)| oid),
                                detail: Some(format!("path:{path}")),
                            }],
                        );
                    }
                    Ok(None)
                }
                TaskType::RestoreFiles { files } => {
                    // PAF-11：restore 丢弃工作区改动，属高危操作——执行前快照
                    // HEAD，执行后落 T-34 操作日志（该 op 类型不支持自动撤销，
                    // 日志行用于追溯与定位）。
                    let repo_dir = std::path::Path::new(&repo_path);
                    let before = crate::core::operation_log::snapshot_head(repo_dir);
                    let out = ops.execute(&task_type_for_exec, repo_dir)?;
                    if let Some((ref_name, before_oid)) = before {
                        crate::core::operation_log::record_operation_best_effort(
                            &db_for_exec,
                            &repo_path,
                            crate::core::operation_log::OP_RESTORE_FILES,
                            "batch restore working-tree changes",
                            vec![crate::core::operation_log::NewOperationLogItem {
                                repo_path: repo_path.clone(),
                                ref_name,
                                before_oid,
                                after_oid: crate::core::operation_log::snapshot_head(repo_dir)
                                    .map(|(_, oid)| oid),
                                detail: Some(format!("files:{}", files.join(","))),
                            }],
                        );
                    }
                    Ok(out)
                }
                TaskType::Fetch | TaskType::Pull | TaskType::Push | TaskType::Clone { .. } => {
                    // PAF-08/PAF-25：网络操作统一走 `*_streaming` 流式底座——
                    // 任务级取消与超时直接杀 git 进程树（blocking 线程随即
                    // 回收，不再悬挂占用），输出逐行实时镜像到 Git Console；
                    // tokio 外层超时降级为兜底护栏。
                    let cancel = cancel_flag.unwrap_or_else(|| Arc::new(AtomicBool::new(false)));
                    let command = network_console_command(&task_type_for_exec)
                        .unwrap_or_else(|| "git network op".to_string());
                    let mut streamer = ConsoleStreamer::new(
                        app_for_exec.clone(),
                        repo_path.clone(),
                        repo_name_for_exec.clone(),
                        command.clone(),
                    );
                    streamer.emit_meta_header();

                    let streaming_result = match &task_type_for_exec {
                        TaskType::Fetch => ops.fetch_streaming(
                            std::path::Path::new(&repo_path),
                            Some(cancel.as_ref()),
                            Some(hard_timeout),
                            &mut |s, l| streamer.on_line(s, l),
                        ),
                        TaskType::Pull => ops.pull_streaming(
                            std::path::Path::new(&repo_path),
                            Some(cancel.as_ref()),
                            Some(hard_timeout),
                            &mut |s, l| streamer.on_line(s, l),
                        ),
                        TaskType::Push => ops.push_streaming(
                            std::path::Path::new(&repo_path),
                            Some(cancel.as_ref()),
                            Some(hard_timeout),
                            &mut |s, l| streamer.on_line(s, l),
                        ),
                        TaskType::Clone { url, branch } => ops.clone_streaming(
                            std::path::Path::new(&repo_path),
                            url,
                            branch.as_deref(),
                            Some(cancel.as_ref()),
                            Some(hard_timeout),
                            &mut |s, l| streamer.on_line(s, l),
                        ),
                        _ => unreachable!("network branch guard"),
                    };
                    streamer.flush();

                    // 收尾语义（超时/取消/失败的人话结论行 + stderr 尾部还原
                    // 可读错误）与单仓命令路径共用 task::console::finish_streaming。
                    match finish_streaming(streaming_result, &streamer, hard_timeout) {
                        Ok(out) => Ok(if out.is_empty() { None } else { Some(out) }),
                        Err(e) => Err(e),
                    }
                }
                TaskType::WorkspaceStashSave {
                    workspace_id,
                    record_name,
                    message,
                    include_untracked,
                    repo_paths,
                } => {
                    // GF-10: whole-workspace save — ONE task for the whole
                    // run (the worker pool would otherwise run per-repo tasks
                    // in parallel), repos stashed one by one (serial model
                    // unchanged), progress after each repo, cancel polled
                    // between repos.
                    let cancel = cancel_flag.unwrap_or_else(|| Arc::new(AtomicBool::new(false)));
                    let app = app_for_exec.clone();
                    let task_id = task_id_for_exec.clone();
                    run_ws_stash_save(
                        &db_for_exec,
                        &runs_for_exec,
                        &task_id,
                        *workspace_id,
                        record_name,
                        message.as_deref(),
                        *include_untracked,
                        repo_paths,
                        cancel.as_ref(),
                        |p| emit_ws_stash_progress(&app, p),
                    )
                    .map(Some)
                }
                TaskType::WorkspaceStashRestore {
                    workspace_stash_id,
                    record_name,
                    allow_branch_mismatch,
                } => {
                    // GF-10: whole-workspace restore — per-item re-check is
                    // the stale-window safety net of the pre-submission §46
                    // pre-check; apply keeps the stash on the stack.
                    let cancel = cancel_flag.unwrap_or_else(|| Arc::new(AtomicBool::new(false)));
                    let app = app_for_exec.clone();
                    let task_id = task_id_for_exec.clone();
                    run_ws_stash_restore(
                        &db_for_exec,
                        &runs_for_exec,
                        &task_id,
                        *workspace_stash_id,
                        record_name,
                        *allow_branch_mismatch,
                        cancel.as_ref(),
                        |p| emit_ws_stash_progress(&app, p),
                    )
                    .map(Some)
                }
                _ => ops.execute(&task_type_for_exec, std::path::Path::new(&repo_path)),
            }),
        )
        .await;

        let (status, out) = match result {
            Ok(Ok(Ok(Some(out)))) => (TaskStatus::Success, Some(out)),
            Ok(Ok(Ok(None))) => (TaskStatus::Success, None),
            Ok(Ok(Err(e))) => (TaskStatus::Failed { error: e.to_string() }, None),
            Ok(Err(e)) => (
                TaskStatus::Failed {
                    error: format!("Worker panic: {}", e),
                },
                None,
            ),
            Err(_) => {
                // 超时硬上限触发：阻塞线程仍在跑。长时任务（Runtime / GF-10
                // workspace stash）置 cancel flag 让执行体协作中止（杀掉
                // Maven 进程树 / 停止启动中的应用 / 逐仓循环就近停止），避免
                // 超时后遗留构建进程或继续改仓库。
                if long_timeout {
                    if let Some(flag) = cancel_flags.get(&task.id) {
                        flag.store(true, Ordering::Relaxed);
                    }
                }
                (
                    TaskStatus::Failed {
                        error: "Task timed out".to_string(),
                    },
                    None,
                )
            }
        };

        // Retry on failure with exponential backoff. Only network operations
        // (Fetch/Pull/Push/Clone) are retried: a commit failure is local and a
        // Commit & Push middle-state failure must never re-run the commit
        // (T-11; the push itself is retried inside execute). Cancelled tasks
        // must not retry — the flag is sticky and the retry would immediately
        // re-run an operation the user asked to stop (PAF-08).
        let retryable = matches!(
            task_type,
            TaskType::Fetch | TaskType::Pull | TaskType::Push | TaskType::Clone { .. }
        );
        if retryable
            && matches!(status, TaskStatus::Failed { .. })
            && !is_cancelled(cancel_flags, &task.id)
            && attempt < MAX_RETRIES
        {
            attempt += 1;
            let backoff = Duration::from_millis(500 * 2u64.pow(attempt as u32));
            log::warn!(
                "Task {} failed (attempt {}), retrying in {:?}",
                task.id,
                attempt,
                backoff
            );
            tokio::time::sleep(backoff).await;
            continue;
        }

        final_status = status;
        output = out;

        // GF-10: a workspace stash run reports its per-repo rollup, not a flat
        // Success — mixed failures become PartialSuccess and a mid-run cancel
        // becomes Cancelled (the completed subset stays recoverable through
        // the run result the blocking body published).
        if is_ws_stash_run(&task_type) {
            if let Some(run) = ws_stash_runs.get(&task.id).map(|r| r.clone()) {
                final_status = run.summary.to_task_status();
            }
        }
        break;
    }

    // Final cancellation check (the flag may have been set mid-execution).
    if is_cancelled(cancel_flags, &task.id) {
        final_status = TaskStatus::Cancelled;
    }

    // Emit an IDE-style git console event for all git operations.
    // Network operations and libgit2 operations both get meta lines in Git Console.
    let console_command = match &task_type {
        // Network operations（PAF-08/25：与流式执行共用标题行）
        TaskType::Fetch | TaskType::Pull | TaskType::Push | TaskType::Clone { .. } => {
            network_console_command(&task_type)
        }
        TaskType::Commit { then_push: true, .. } => Some("git commit && git push".to_string()),
        // Shell / Node
        TaskType::ShellCommand { command, .. } => Some(command.clone()),
        TaskType::StageFiles { files } => Some(format!("git add {} file(s)", files.len())),
        TaskType::RestoreFiles { files } => Some(format!("git restore {} file(s)", files.len())),
        TaskType::NodeInstall {
            project_dir,
            package_manager,
        } => Some(format!("{} install (cwd {})", package_manager.name(), project_dir)),
        // TM-04：libgit2 操作合成 meta 行
        TaskType::Commit { message, then_push: false, amend, .. } => {
            if *amend {
                Some("git commit --amend".to_string())
            } else {
                Some(format!("git commit -m \"{}\"", shorten_message(message)))
            }
        }
        TaskType::BranchOp { op, name, .. } => {
            let desc = match op {
                crate::models::task::BranchOpKind::Create => format!("git branch {}", name),
                crate::models::task::BranchOpKind::Delete => format!("git branch -d {}", name),
                crate::models::task::BranchOpKind::Checkout => format!("git checkout {}", name),
            };
            Some(desc)
        }
        TaskType::ConflictApply { path, strategy, .. } => {
            Some(format!("git conflict resolve {} ({})", path, strategy))
        }
        // GF-10: whole-workspace stash runs (one task, N repos inside).
        TaskType::WorkspaceStashSave { repo_paths, .. } => {
            Some(format!("git stash save × {} 个仓库", repo_paths.len()))
        }
        TaskType::WorkspaceStashRestore { record_name, .. } => {
            Some(format!("git stash apply × {record_name}"))
        }
        _ => None,
    };
    if let Some(command) = console_command {
        let (success, out) = match &final_status {
            TaskStatus::Success => (true, output.clone().unwrap_or_default()),
            TaskStatus::Failed { error } => (false, error.clone()),
            TaskStatus::Cancelled => (false, "Cancelled".to_string()),
            _ => (false, String::new()),
        };
        let _ = app.emit(
            "git_command_result",
            &GitCommandResult {
                repo_name: task.repo_name.clone(),
                repo_path: task.repo_path.clone(),
                command: command.clone(),
                success,
                output: out.clone(),
            },
        );

        // TM-04：Git Console 镜像事件（git_op_output）。
        // PAF-25：网络操作已在执行期间实时流式输出（含命令标题行与失败结论
        // 行），收尾不再重复发送，避免 Console 出现两份。
        if !is_network_task(&task_type) {
            emit_git_op_output(
                app,
                &task.repo_path,
                &task.repo_name,
                &command,
                "meta",
                &format!("$ {}", command),
            );
            if !out.is_empty() {
                for line in out.lines() {
                    emit_git_op_output(
                        app,
                        &task.repo_path,
                        &task.repo_name,
                        &command,
                        if success { "stdout" } else { "stderr" },
                        line,
                    );
                }
            }
        }
    }

    // Update stored task
    if let Some(mut entry) = tasks.get_mut(&task.id) {
        entry.status = final_status.clone();
    }
    task.status = final_status;

    // AI commit proposals are logged after the task succeeds. The log stores
    // only ref snapshots and metadata, so T-34 can safely offer Undo without
    // retaining commit contents or proposal prompt text.
    if matches!(&task.task_type, TaskType::Commit { .. }) && matches!(task.status, TaskStatus::Success) {
        if let Some((ref_name, before_oid)) = ai_commit_before {
            crate::core::operation_log::record_operation_best_effort(
                db,
                &task.repo_path,
                crate::core::operation_log::OP_AI_COMMIT,
                "AI Action Proposal commit",
                vec![crate::core::operation_log::NewOperationLogItem {
                    repo_path: task.repo_path.clone(),
                    ref_name,
                    before_oid,
                    after_oid: crate::core::operation_log::snapshot_head(std::path::Path::new(&task.repo_path))
                        .map(|(_, oid)| oid),
                    detail: Some("source:ai_action_proposal".into()),
                }],
            );
        }
    }

    // Persist the final status for crash recovery / history.
    persist_final_status(db, &task);

    // Emit final status
    emit_progress(app, &task);

    // T-24: evolve the DAG this node belongs to (release dependents,
    // propagate failure/cancellation). A retried node must not be accounted
    // into the batch aggregate yet, nor cleaned up.
    let retried = finish_dag_node(dags, dag_sender, tasks, cancel_flags, db, app, batches, &task, output);

    if !retried {
        // Aggregate into the parent batch (T-20): evolves the synthetic batch
        // task towards Success / Failed / PartialSuccess and persists the
        // per-repo sub-result into task_items.
        update_batch(batches, db, app, &task);

        // Schedule cleanup after a delay
        let tasks = Arc::clone(tasks);
        let flags = Arc::clone(cancel_flags);
        let task_id = task.id.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(Duration::from_secs(30)).await;
            tasks.remove(&task_id);
            flags.remove(&task_id);
        });
    }
}

/// Hand a finished task to the DAG scheduler (T-24). Returns true when the
/// node is being retried at scheduler level (skip accounting + cleanup).
#[allow(clippy::too_many_arguments)]
fn finish_dag_node(
    dags: &Arc<DashMap<String, DagState>>,
    sender: &mpsc::Sender<super::queue::TaskMessage>,
    tasks: &Arc<DashMap<String, Task>>,
    cancel_flags: &Arc<DashMap<String, Arc<AtomicBool>>>,
    db: &Arc<std::sync::Mutex<Connection>>,
    app: &AppHandle,
    batches: &Arc<DashMap<String, BatchState>>,
    task: &Task,
    output: Option<String>,
) -> bool {
    if task.batch_id.is_none() {
        return false;
    }
    let ctx = DagContext {
        dags,
        sender,
        active_tasks: tasks,
        cancel_flags,
        db,
        app,
        batches,
    };
    crate::task::dag::on_task_finished(&ctx, task, output)
}

/// Persist a task's final status (and finished_at) to the `tasks` table.
fn persist_final_status(db: &Arc<std::sync::Mutex<Connection>>, task: &Task) {
    let Ok(conn) = db.lock() else {
        return;
    };
    let now = chrono::Utc::now().to_rfc3339();
    if let Err(e) = dao::update_task_status(&conn, &task.id, task.status.key(), Some(&now)) {
        log::warn!("Failed to persist task {} status: {}", task.id, e);
    }
}

/// Emit a task_progress event to the frontend.
pub(crate) fn emit_progress(app: &AppHandle, task: &Task) {
    let progress = TaskProgress {
        task_id: task.id.clone(),
        task_type: task.task_type.clone(),
        repo_path: task.repo_path.clone(),
        repo_name: task.repo_name.clone(),
        status: task.status.clone(),
        batch_id: task.batch_id.clone(),
    };

    if let Err(e) = app.emit("task_progress", &progress) {
        log::warn!("Failed to emit task_progress: {}", e);
    }
}

/// Aggregate a finished child task into its parent batch (T-20): updates the
/// synthetic batch task's status (PartialSuccess when mixed), persists the
/// per-repo sub-result into `task_items`, and emits the batch's progress.
/// Also called by the manager when a child fails to even queue.
pub(crate) fn update_batch(
    batches: &Arc<DashMap<String, BatchState>>,
    db: &Arc<std::sync::Mutex<Connection>>,
    app: &AppHandle,
    child: &Task,
) {
    let Some(batch_id) = child.batch_id.clone() else {
        return;
    };
    let Some(mut entry) = batches.get_mut(&batch_id) else {
        return;
    };
    let b = entry.value_mut();

    let error_msg = match &child.status {
        TaskStatus::Failed { error } => Some(error.clone()),
        _ => None,
    };
    // Evolve the aggregate (pure part lives in BatchState::record_child).
    let done = b.record_child(&child.status);
    if matches!(child.status, TaskStatus::Queued | TaskStatus::Running { .. }) {
        return; // not a final status
    }

    // Per-repo sub-result into task_items (T-05 schema intent).
    if let Ok(conn) = db.lock() {
        let now = chrono::Utc::now().to_rfc3339();
        if let Err(e) = dao::insert_task_item(
            &conn,
            b.db_row_id,
            &child.repo_path,
            child.status.key(),
            error_msg.as_deref(),
            &now,
        ) {
            log::warn!("Failed to persist task item for {}: {}", child.id, e);
        }
    }

    let batch_task = b.task.clone();
    if done {
        if let Ok(conn) = db.lock() {
            let now = chrono::Utc::now().to_rfc3339();
            if let Err(e) = dao::update_task_status(&conn, &batch_id, batch_task.status.key(), Some(&now)) {
                log::warn!("Failed to persist batch {} status: {}", batch_id, e);
            }
        }
    }
    drop(entry);
    emit_progress(app, &batch_task);

    // Schedule cleanup of the finished batch aggregate.
    if done {
        let batches = Arc::clone(batches);
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(Duration::from_secs(30)).await;
            batches.remove(&batch_id);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::workspace_stash::{self, WorkspaceStashItemEntry};
    use std::path::Path;

    fn tmpdir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "gw_wsstash_worker_{}_{}",
            tag,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn init_repo(dir: &Path) {
        let repo = git2::Repository::init(dir).unwrap();
        std::fs::write(dir.join("a.txt"), "one\n").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("a.txt")).unwrap();
        index.write().unwrap();
        let tree_oid = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_oid).unwrap();
        let sig = git2::Signature::now("tester", "t@example.com").unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[]).unwrap();
        repo.config().unwrap().set_str("user.name", "tester").unwrap();
        repo.config().unwrap().set_str("user.email", "t@example.com").unwrap();
    }

    fn mem_db() -> Arc<std::sync::Mutex<Connection>> {
        let mut conn = Connection::open_in_memory().unwrap();
        crate::db::init_db(&mut conn).unwrap();
        conn.execute(
            "INSERT INTO workspaces (name, path, created_at, updated_at) VALUES ('w', 'D:/w', 't', 't')",
            [],
        )
        .unwrap();
        Arc::new(std::sync::Mutex::new(conn))
    }

    #[test]
    fn short_message_unchanged() {
        assert_eq!(shorten_message("fix: parser"), "fix: parser");
    }

    #[test]
    fn ascii_long_message_truncated() {
        let msg = "a".repeat(80);
        let out = shorten_message(&msg);
        assert_eq!(out.chars().count(), 48); // 47 chars + '…'
        assert!(out.ends_with('…'));
    }

    // PAF-01 回归：多字节字符落在截断点附近时按字符截断，不再 panic。
    #[test]
    fn cjk_long_message_truncated_without_panic() {
        // 60 个汉字 = 180 字节：旧代码 `&message[..47]` 在字节边界直接 panic。
        let msg = "修".repeat(60);
        let out = shorten_message(&msg);
        assert_eq!(out.chars().count(), 48); // 47 chars + '…'
        assert!(out.ends_with('…'));
    }

    // PAF-01 回归：字节数超阈值但字符数未超，按字符语义不截断也不 panic。
    #[test]
    fn cjk_byte_over_threshold_char_under_threshold() {
        let msg = "修".repeat(40); // 40 chars = 120 bytes；旧代码按字节切片在此 panic
        assert_eq!(shorten_message(&msg), msg);
    }

    #[test]
    fn mixed_multibyte_boundary_truncated_without_panic() {
        // ASCII 与 CJK 混排，任意字节长度组合都不允许 panic。
        for n in 0..=60 {
            let msg = format!("{}{}", "x", "中".repeat(n));
            let out = shorten_message(&msg);
            assert!(out.chars().count() <= 50);
        }
    }

    #[test]
    fn exactly_at_limit_unchanged() {
        let msg = "中".repeat(50);
        assert_eq!(shorten_message(&msg), msg);
    }

    // -----------------------------------------------------------------------
    // GF-10: workspace stash run bodies (queue path, no AppHandle needed)
    // -----------------------------------------------------------------------

    /// Full save run through the worker body: progress event per repo, record
    /// persisted (with only the stashed repo as a member), rollup Success and
    /// the structured result parked for the pending command.
    #[test]
    fn ws_stash_save_run_persists_record_and_reports_progress() {
        let dir = tmpdir("save");
        let a = dir.join("a");
        let b = dir.join("b");
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        init_repo(&a);
        init_repo(&b);
        std::fs::write(a.join("a.txt"), "one\nwork\n").unwrap();
        let paths: Vec<String> = [&a, &b]
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect();

        let db = mem_db();
        let runs: Arc<DashMap<String, WorkspaceStashRunResult>> = Arc::new(DashMap::new());
        let mut events: Vec<(usize, usize, String, String)> = Vec::new();
        let cancel = AtomicBool::new(false);
        let line = run_ws_stash_save(
            &db,
            &runs,
            "task-1",
            1,
            "Workspace Stash #1",
            Some("sprint"),
            true,
            &paths,
            &cancel,
            |p| {
                events.push((p.index, p.total, p.repo_name.clone(), p.status.clone()));
            },
        )
        .unwrap();

        assert_eq!(
            events,
            vec![
                (1, 2, "a".to_string(), "stashed".to_string()),
                (2, 2, "b".to_string(), "skipped_clean".to_string()),
            ]
        );
        assert!(line.contains("完成 1 个仓库"), "{line}");

        let run = runs.get("task-1").expect("run result must be parked");
        assert_eq!(run.record_id, Some(1));
        assert!(!run.cancelled);
        assert_eq!(run.items.len(), 2);
        assert!(matches!(run.summary.to_task_status(), TaskStatus::Success));
        drop(run);

        // The record row + its single member were persisted.
        let conn = db.lock().unwrap();
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM workspace_stashes", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM workspace_stash_items", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            1
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Mid-run cancel: the untouched repos are reported as cancelled, their
    /// working-tree change is untouched, and the completed subset is still
    /// written as a record (restorable through the normal restore flow).
    #[test]
    fn ws_stash_save_run_cancel_after_first_repo_keeps_partial_record() {
        let dir = tmpdir("save_cancel");
        let repos: Vec<std::path::PathBuf> = (0..3).map(|i| dir.join(format!("r{i}"))).collect();
        for r in &repos {
            std::fs::create_dir_all(r).unwrap();
            init_repo(r);
            std::fs::write(r.join("a.txt"), "one\nwork\n").unwrap();
        }
        let paths: Vec<String> = repos.iter().map(|p| p.to_string_lossy().to_string()).collect();

        let db = mem_db();
        let runs: Arc<DashMap<String, WorkspaceStashRunResult>> = Arc::new(DashMap::new());
        let cancel = AtomicBool::new(false);
        let mut events: Vec<(usize, String)> = Vec::new();
        let _ = run_ws_stash_save(&db, &runs, "task-1", 1, "Workspace Stash #1", None, true, &paths, &cancel, |p| {
            events.push((p.index, p.status.clone()));
            // Cancel right after the first repo is processed.
            if p.index == 1 {
                cancel.store(true, Ordering::Relaxed);
            }
        })
        .unwrap();

        assert_eq!(
            events,
            vec![
                (1, "stashed".to_string()),
                (2, "cancelled".to_string()),
                (3, "cancelled".to_string()),
            ]
        );
        let run = runs.get("task-1").unwrap();
        assert!(run.cancelled);
        assert_eq!(run.summary, workspace_stash::WorkspaceStashRunSummary {
            total: 3,
            stashed: 1,
            skipped: 0,
            failed: 0,
            cancelled: 2,
        });
        assert!(matches!(run.summary.to_task_status(), TaskStatus::Cancelled));
        // Partial record persisted -> restorable later.
        assert_eq!(run.record_id, Some(1));
        drop(run);
        let conn = db.lock().unwrap();
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM workspace_stash_items", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            1
        );

        // Untouched repos keep their change; the processed one does not.
        assert!(std::fs::read_to_string(repos[1].join("a.txt")).unwrap().contains("work"));
        assert!(std::fs::read_to_string(repos[2].join("a.txt")).unwrap().contains("work"));
        assert!(!std::fs::read_to_string(repos[0].join("a.txt")).unwrap().contains("work"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Restore run through the worker body: items re-checked and applied, the
    /// stash kept on the stack, one progress event per item.
    #[test]
    fn ws_stash_restore_run_applies_and_reports() {
        let dir = tmpdir("restore");
        let repo = dir.join("r");
        std::fs::create_dir_all(&repo).unwrap();
        init_repo(&repo);
        std::fs::write(repo.join("a.txt"), "one\nwork\n").unwrap();

        let paths = vec![repo.to_string_lossy().to_string()];
        let (_outcomes, stashed) =
            workspace_stash::stash_repos_cancellable(&paths, "Workspace Stash #1", None, true, None, |_| {});
        assert_eq!(stashed.len(), 1);

        let db = mem_db();
        let record_id = {
            let mut conn = db.lock().unwrap();
            workspace_stash::insert_workspace_stash(&mut conn, 1, "Workspace Stash #1", None, &stashed).unwrap()
        };
        assert_eq!(std::fs::read_to_string(repo.join("a.txt")).unwrap().trim_end(), "one");

        let runs: Arc<DashMap<String, WorkspaceStashRunResult>> = Arc::new(DashMap::new());
        let mut events: Vec<(String, String)> = Vec::new();
        let cancel = AtomicBool::new(false);
        let line = run_ws_stash_restore(
            &db,
            &runs,
            "task-1",
            record_id,
            "Workspace Stash #1",
            false,
            &cancel,
            |p| events.push((p.repo_name.clone(), p.status.clone())),
        )
        .unwrap();

        assert_eq!(events, vec![("r".to_string(), "applied".to_string())]);
        assert!(line.contains("完成 1 个仓库"), "{line}");
        let run = runs.get("task-1").unwrap();
        assert!(run.record_id.is_none(), "restore results carry no record id");
        assert!(!run.cancelled);
        assert_eq!(run.items[0].status, "applied");
        // Apply keeps the stash on the stack (T-10 semantics).
        drop(run);
        assert_eq!(crate::core::stash::list_stashes(&repo).unwrap().len(), 1);
        assert!(std::fs::read_to_string(repo.join("a.txt")).unwrap().contains("work"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A record with no items (or an unknown id) fails fast instead of
    /// queueing a no-op run.
    #[test]
    fn ws_stash_restore_run_without_items_errors() {
        let db = mem_db();
        let runs: Arc<DashMap<String, WorkspaceStashRunResult>> = Arc::new(DashMap::new());
        let cancel = AtomicBool::new(false);
        let err = run_ws_stash_restore(&db, &runs, "task-1", 999, "Workspace Stash #999", false, &cancel, |_| {})
            .unwrap_err();
        assert!(matches!(err, AppError::NotFound(_)), "{err}");
        assert!(runs.is_empty(), "no result must be parked for a failed run");
    }

    /// The record name / workspace id plumbing: seeding via `insert_workspace_stash`
    /// with an `WorkspaceStashItemEntry` shape round-trips through the run.
    #[test]
    fn ws_stash_restore_run_reads_items_from_db() {
        let db = mem_db();
        let item = WorkspaceStashItemEntry {
            repo_path: "D:/w/a".into(),
            stash_oid: "abc123".into(),
            stash_index: 0,
            branch: "main".into(),
        };
        let record_id = {
            let mut conn = db.lock().unwrap();
            workspace_stash::insert_workspace_stash(&mut conn, 1, "Workspace Stash #1", None, &[item]).unwrap()
        };
        let conn = db.lock().unwrap();
        let items = workspace_stash::list_workspace_stash_items(&conn, record_id).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].stash_oid, "abc123");
    }
}
