use std::sync::Arc;

use dashmap::DashMap;
use tauri::{AppHandle, Emitter};
use tokio::sync::{mpsc, Mutex};

use crate::core::git_ops::GitOps;
use crate::models::task::{
    GitCommandResult, Task, TaskProgress, TaskStatus, TaskType,
};

/// Spawn the worker pool that processes tasks from the shared receiver.
///
/// Each worker pulls tasks from the channel, executes them using GitOps
/// (in a blocking thread), and emits progress events to the frontend.
pub fn spawn_worker_pool(
    worker_count: usize,
    receiver: mpsc::Receiver<super::queue::TaskMessage>,
    git_ops: Arc<GitOps>,
    active_tasks: Arc<DashMap<String, Task>>,
    app_handle: AppHandle,
) {
    let receiver = Arc::new(Mutex::new(receiver));

    tauri::async_runtime::spawn(async move {
        let mut workers = Vec::with_capacity(worker_count);

        for worker_id in 0..worker_count {
            let rx = Arc::clone(&receiver);
            let ops = Arc::clone(&git_ops);
            let tasks = Arc::clone(&active_tasks);
            let app = app_handle.clone();

            workers.push(tauri::async_runtime::spawn(async move {
                log::debug!("Task worker {} started", worker_id);

                loop {
                    let msg = {
                        let mut lock = rx.lock().await;
                        lock.recv().await
                    };

                    match msg {
                        Some(msg) => {
                            execute_task(&ops, &tasks, &app, msg.task).await;
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

/// Execute a single task: update status, run the Git operation, emit progress.
async fn execute_task(
    ops: &Arc<GitOps>,
    tasks: &Arc<DashMap<String, Task>>,
    app: &AppHandle,
    mut task: Task,
) {
    // Update status to Running
    if let Some(mut entry) = tasks.get_mut(&task.id) {
        entry.status = TaskStatus::Running { progress: 0.0 };
        task.status = TaskStatus::Running { progress: 0.0 };
    }

    // Emit Running status
    emit_progress(app, &task);

    // Execute the Git operation in a blocking thread
    let ops = Arc::clone(ops);
    let repo_path = task.repo_path.clone();
    let task_type = task.task_type.clone();
    let task_type_for_exec = task_type.clone();

    let result = tokio::task::spawn_blocking(move || {
        ops.execute(&task_type_for_exec, std::path::Path::new(&repo_path))
    })
    .await;

    // Determine the final status and capture any git command output.
    let (final_status, output) = match result {
        Ok(Ok(Some(out))) => (TaskStatus::Success, Some(out)),
        Ok(Ok(None)) => (TaskStatus::Success, None),
        Ok(Err(e)) => (TaskStatus::Failed { error: e.to_string() }, None),
        Err(e) => (
            TaskStatus::Failed {
                error: format!("Worker panic: {}", e),
            },
            None,
        ),
    };

    // Emit an IDE-style git console event for network operations.
    if matches!(task_type, TaskType::Fetch | TaskType::Pull | TaskType::Push) {
        let command = match &task_type {
            TaskType::Fetch => "git fetch <remote>".to_string(),
            TaskType::Pull => "git pull --ff-only".to_string(),
            TaskType::Push => "git push".to_string(),
            _ => String::new(),
        };
        let (success, out) = match &final_status {
            TaskStatus::Success => (true, output.unwrap_or_default()),
            TaskStatus::Failed { error } => (false, error.clone()),
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

    // Emit final status
    emit_progress(app, &task);

    // Schedule cleanup after a delay
    let tasks = Arc::clone(tasks);
    let task_id = task.id.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(30)).await;
        tasks.remove(&task_id);
    });
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
