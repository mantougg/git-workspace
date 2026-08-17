use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use dashmap::DashMap;
use rusqlite::Connection;
use tauri::{AppHandle, Emitter};
use tokio::sync::{mpsc, Mutex};

use crate::core::git_ops::GitOps;
use crate::db::dao;
use crate::models::task::{GitCommandResult, Task, TaskProgress, TaskStatus, TaskType};

/// Maximum retries for a failed task (network operations benefit most).
const MAX_RETRIES: usize = 2;
/// Hard timeout for a single task execution.
const TASK_TIMEOUT: Duration = Duration::from_secs(300);

/// Spawn the worker pool that processes tasks from the shared receiver.
///
/// Each worker pulls tasks from the channel, executes them using GitOps
/// (in a blocking thread with a timeout + retries), and emits progress events
/// to the frontend.
pub fn spawn_worker_pool(
    worker_count: usize,
    receiver: mpsc::Receiver<super::queue::TaskMessage>,
    git_ops: Arc<GitOps>,
    active_tasks: Arc<DashMap<String, Task>>,
    cancel_flags: Arc<DashMap<String, Arc<AtomicBool>>>,
    app_handle: AppHandle,
    db: Arc<std::sync::Mutex<Connection>>,
) {
    let receiver = Arc::new(Mutex::new(receiver));

    tauri::async_runtime::spawn(async move {
        let mut workers = Vec::with_capacity(worker_count);

        for worker_id in 0..worker_count {
            let rx = Arc::clone(&receiver);
            let ops = Arc::clone(&git_ops);
            let tasks = Arc::clone(&active_tasks);
            let flags = Arc::clone(&cancel_flags);
            let app = app_handle.clone();
            let db = Arc::clone(&db);

            workers.push(tauri::async_runtime::spawn(async move {
                log::debug!("Task worker {} started", worker_id);

                loop {
                    let msg = {
                        let mut lock = rx.lock().await;
                        lock.recv().await
                    };

                    match msg {
                        Some(msg) => {
                            execute_task(&ops, &tasks, &flags, &app, &db, msg.task).await;
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
async fn execute_task(
    ops: &Arc<GitOps>,
    tasks: &Arc<DashMap<String, Task>>,
    cancel_flags: &Arc<DashMap<String, Arc<AtomicBool>>>,
    app: &AppHandle,
    db: &Arc<std::sync::Mutex<Connection>>,
    mut task: Task,
) {
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

        let result = tokio::time::timeout(
            TASK_TIMEOUT,
            tokio::task::spawn_blocking(move || {
                ops.execute(&task_type_for_exec, std::path::Path::new(&repo_path))
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
            Err(_) => (
                TaskStatus::Failed {
                    error: "Task timed out".to_string(),
                },
                None,
            ),
        };

        // Retry on failure with exponential backoff. Only network operations
        // (Fetch/Pull/Push) are retried: a commit failure is local and a
        // Commit & Push middle-state failure must never re-run the commit
        // (T-11; the push itself is retried inside execute).
        let retryable = matches!(
            task_type,
            TaskType::Fetch | TaskType::Pull | TaskType::Push
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
        TaskType::Commit {
            then_push: true, ..
        } => Some("git commit && git push".to_string()),
        _ => None,
    };
    if let Some(command) = console_command {
        let (success, out) = match &final_status {
            TaskStatus::Success => (true, output.unwrap_or_default()),
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
fn emit_progress(app: &AppHandle, task: &Task) {
    let progress = TaskProgress {
        task_id: task.id.clone(),
        task_type: task.task_type.clone(),
        repo_path: task.repo_path.clone(),
        repo_name: task.repo_name.clone(),
        status: task.status.clone(),
    };

    if let Err(e) = app.emit("task_progress", &progress) {
        log::warn!("Failed to emit task_progress: {}", e);
    }
}
