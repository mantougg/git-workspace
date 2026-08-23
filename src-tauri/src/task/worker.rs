use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use dashmap::DashMap;
use rusqlite::Connection;
use tauri::{AppHandle, Emitter};
use tokio::sync::{mpsc, Mutex};

use crate::core::git_ops::GitOps;
use crate::db::dao;
use crate::error::AppError;
use crate::models::task::{BatchState, GitCommandResult, Task, TaskProgress, TaskStatus, TaskType};
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

/// Spawn the worker pool that processes tasks from the shared receiver.
///
/// Each worker pulls tasks from the channel, executes them using GitOps
/// (in a blocking thread with a timeout + retries), and emits progress events
/// to the frontend. `dag_sender` is the queue's own sender, handed to the
/// DAG scheduler (T-24) so it can dispatch newly-unblocked nodes. Runtime
/// tasks (R-12) go to `runtime_handler` with the task's cancel flag wired in.
#[allow(clippy::too_many_arguments)]
pub fn spawn_worker_pool(
    worker_count: usize,
    receiver: mpsc::Receiver<super::queue::TaskMessage>,
    dag_sender: mpsc::Sender<super::queue::TaskMessage>,
    git_ops: Arc<GitOps>,
    runtime_handler: Option<Arc<super::runtime::RuntimeTaskHandler>>,
    active_tasks: Arc<DashMap<String, Task>>,
    cancel_flags: Arc<DashMap<String, Arc<AtomicBool>>>,
    app_handle: AppHandle,
    db: Arc<std::sync::Mutex<Connection>>,
    batches: Arc<DashMap<String, BatchState>>,
    dags: Arc<DashMap<String, DagState>>,
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
    flags
        .get(task_id)
        .map(|f| f.load(Ordering::Relaxed))
        .unwrap_or(false)
}

/// Execute a single task: update status, run the Git operation (with timeout +
/// retries), honour cancellation, and emit progress.
#[allow(clippy::too_many_arguments)]
async fn execute_task(
    ops: &Arc<GitOps>,
    runtime_handler: &Option<Arc<super::runtime::RuntimeTaskHandler>>,
    tasks: &Arc<DashMap<String, Task>>,
    cancel_flags: &Arc<DashMap<String, Arc<AtomicBool>>>,
    app: &AppHandle,
    db: &Arc<std::sync::Mutex<Connection>>,
    batches: &Arc<DashMap<String, BatchState>>,
    dags: &Arc<DashMap<String, DagState>>,
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

    loop {
        if is_cancelled(cancel_flags, &task.id) {
            final_status = TaskStatus::Cancelled;
            break;
        }

        let ops = Arc::clone(ops);
        let repo_path = task.repo_path.clone();
        let task_type_for_exec = task_type.clone();
        // R-12: Runtime 任务拿到自己的 cancel flag（构建/启动可中途终止），
        // 并使用更长的硬超时（git 的 5 分钟上限对构建不适用）。
        let is_runtime = matches!(task_type_for_exec, TaskType::Runtime { .. });
        let runtime_handler = runtime_handler.clone();
        let cancel_flag = cancel_flags.get(&task.id).map(|f| Arc::clone(&f));
        let hard_timeout = if is_runtime {
            RUNTIME_TASK_TIMEOUT
        } else {
            TASK_TIMEOUT
        };

        let result = tokio::time::timeout(
            hard_timeout,
            tokio::task::spawn_blocking(move || {
                if let TaskType::Runtime { .. } = &task_type_for_exec {
                    let Some(handler) = runtime_handler else {
                        return Err(AppError::Task(
                            "Runtime 任务处理器未装配（应用启动未完成），请稍后重试".into(),
                        ));
                    };
                    let cancel = cancel_flag.unwrap_or_else(|| Arc::new(AtomicBool::new(false)));
                    handler.execute(&task_type_for_exec, cancel)
                } else {
                    ops.execute(&task_type_for_exec, std::path::Path::new(&repo_path))
                }
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
                // 超时硬上限触发：阻塞线程仍在跑。Runtime 任务置 cancel flag
                // 让执行体协作中止（杀掉 Maven 进程树 / 停止启动中的应用），
                // 避免超时后遗留构建进程。
                if is_runtime {
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
        // (T-11; the push itself is retried inside execute).
        let retryable = matches!(
            task_type,
            TaskType::Fetch | TaskType::Pull | TaskType::Push | TaskType::Clone { .. }
        );
        if retryable && matches!(status, TaskStatus::Failed { .. }) && attempt < MAX_RETRIES {
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
        break;
    }

    // Final cancellation check (the flag may have been set mid-execution).
    if is_cancelled(cancel_flags, &task.id) {
        final_status = TaskStatus::Cancelled;
    }

    // Emit an IDE-style git console event for network operations (and for
    // Commit & Push, which runs a network push as its second phase).
    let console_command = match &task_type {
        TaskType::Fetch => Some("git fetch <remote>".to_string()),
        TaskType::Pull => Some("git pull --ff-only".to_string()),
        TaskType::Push => Some("git push".to_string()),
        TaskType::Clone { url, .. } => Some(format!("git clone {}", url)),
        TaskType::ShellCommand { command, .. } => Some(command.clone()),
        TaskType::Commit {
            then_push: true, ..
        } => Some("git commit && git push".to_string()),
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
                command,
                success,
                output: out,
            },
        );
    }

    // Update stored task
    if let Some(mut entry) = tasks.get_mut(&task.id) {
        entry.status = final_status.clone();
    }
    task.status = final_status;

    // Persist the final status for crash recovery / history.
    persist_final_status(db, &task);

    // Emit final status
    emit_progress(app, &task);

    // T-24: evolve the DAG this node belongs to (release dependents,
    // propagate failure/cancellation). A retried node must not be accounted
    // into the batch aggregate yet, nor cleaned up.
    let retried = finish_dag_node(
        dags,
        dag_sender,
        tasks,
        cancel_flags,
        db,
        app,
        batches,
        &task,
        output,
    );

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
            if let Err(e) =
                dao::update_task_status(&conn, &batch_id, batch_task.status.key(), Some(&now))
            {
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
