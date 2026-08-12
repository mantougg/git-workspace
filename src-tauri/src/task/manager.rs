use std::sync::Arc;

use chrono::Utc;
use dashmap::DashMap;
use tauri::AppHandle;
use uuid::Uuid;

use crate::core::git_ops::GitOps;
use crate::error::{AppError, AppResult};
use crate::models::task::{Task, TaskRequest, TaskStatus};
use crate::task::queue::{self, TaskMessage};
use crate::task::worker;

/// Manages the background task queue and worker pool.
///
/// Tasks are submitted via `submit()` and processed by a pool of
/// async workers. Status updates are emitted as `task_progress` events.
pub struct TaskManager {
    sender: tokio::sync::mpsc::Sender<TaskMessage>,
    active_tasks: Arc<DashMap<String, Task>>,
}

impl TaskManager {
    /// Create a new TaskManager and start the worker pool.
    ///
    /// The worker pool runs `worker_count` async tasks, each pulling
    /// from a shared mpsc receiver. Git operations (blocking) are
    /// executed via `tokio::task::spawn_blocking`.
    pub fn new(worker_count: usize, git_ops: Arc<GitOps>, app_handle: AppHandle) -> Self {
        let (sender, receiver) = queue::new_queue(128);
        let active_tasks = Arc::new(DashMap::<String, Task>::new());

        // Spawn the worker pool using the worker module
        worker::spawn_worker_pool(
            worker_count,
            receiver,
            Arc::clone(&git_ops),
            Arc::clone(&active_tasks),
            app_handle,
        );

        log::info!("TaskManager started with {} workers", worker_count);

        TaskManager {
            sender,
            active_tasks,
        }
    }

    /// Submit a batch of tasks to the queue.
    /// Returns the list of generated task IDs.
    pub fn submit(&self, requests: &[TaskRequest]) -> AppResult<Vec<String>> {
        let mut ids = Vec::with_capacity(requests.len());

        for req in requests {
            let id = Uuid::new_v4().to_string();
            let now = Utc::now().to_rfc3339();

            let task = Task {
                id: id.clone(),
                task_type: req.task_type.clone(),
                repo_path: req.repo_path.clone(),
                repo_name: req.repo_name.clone(),
                status: TaskStatus::Queued,
                created_at: now,
            };

            // Store in active tasks
            self.active_tasks.insert(id.clone(), task.clone());

            // Send to channel
            if let Err(e) = self.sender.try_send(TaskMessage { task }) {
                // Remove from active tasks if sending failed
                self.active_tasks.remove(&id);
                return Err(AppError::Task(format!(
                    "Failed to queue task: {}",
                    e
                )));
            }

            ids.push(id);
        }

        Ok(ids)
    }

    /// Get the current status of multiple tasks.
    pub fn get_status(&self, task_ids: &[String]) -> Vec<Task> {
        task_ids
            .iter()
            .filter_map(|id| self.active_tasks.get(id).map(|e| e.clone()))
            .collect()
    }

    /// Cancel a queued task (only if it hasn't started running yet).
    pub fn cancel(&self, task_id: &str) -> AppResult<()> {
        if let Some(mut entry) = self.active_tasks.get_mut(task_id) {
            match &entry.status {
                TaskStatus::Queued => {
                    entry.status = TaskStatus::Failed {
                        error: "Cancelled by user".to_string(),
                    };
                    Ok(())
                }
                TaskStatus::Running { .. } => Err(AppError::Task(
                    "Cannot cancel a running task".to_string(),
                )),
                _ => Ok(()), // Already finished
            }
        } else {
            Err(AppError::NotFound(format!(
                "Task {} not found",
                task_id
            )))
        }
    }

    /// Get all active tasks (for the task panel display).
    pub fn list_active(&self) -> Vec<Task> {
        self.active_tasks.iter().map(|e| e.clone()).collect()
    }

    /// Remove finished tasks from the active list (cleanup).
    pub fn cleanup_finished(&self) {
        let to_remove: Vec<String> = self
            .active_tasks
            .iter()
            .filter_map(|e| match &e.status {
                TaskStatus::Success | TaskStatus::Failed { .. } => Some(e.key().clone()),
                _ => None,
            })
            .collect();

        for id in to_remove {
            self.active_tasks.remove(&id);
        }
    }
}
