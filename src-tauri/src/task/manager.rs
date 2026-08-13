use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use chrono::Utc;
use dashmap::DashMap;
use rusqlite::Connection;
use tauri::AppHandle;
use uuid::Uuid;

use crate::core::git_ops::GitOps;
use crate::db::dao;
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
    cancel_flags: Arc<DashMap<String, Arc<AtomicBool>>>,
    /// Shared DB connection for task persistence (history + crash recovery).
    db: Arc<Mutex<Connection>>,
}

impl TaskManager {
    /// Create a new TaskManager and start the worker pool.
    ///
    /// The worker pool runs `worker_count` async tasks, each pulling
    /// from a shared mpsc receiver. Git operations (blocking) are
    /// executed via `tokio::task::spawn_blocking`.
    pub fn new(
        worker_count: usize,
        git_ops: Arc<GitOps>,
        app_handle: AppHandle,
        db: Arc<Mutex<Connection>>,
    ) -> Self {
        let (sender, receiver) = queue::new_queue(128);
        let active_tasks = Arc::new(DashMap::<String, Task>::new());
        let cancel_flags = Arc::new(DashMap::<String, Arc<AtomicBool>>::new());

        // Spawn the worker pool using the worker module
        worker::spawn_worker_pool(
            worker_count,
            receiver,
            Arc::clone(&git_ops),
            Arc::clone(&active_tasks),
            Arc::clone(&cancel_flags),
            app_handle,
            Arc::clone(&db),
        );

        log::info!("TaskManager started with {} workers", worker_count);

        TaskManager {
            sender,
            active_tasks,
            cancel_flags,
            db,
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

            // Persist to the `tasks` table (history + crash recovery).
            self.persist_new_task(&task);

            // Store in active tasks + create cancel flag
            self.active_tasks.insert(id.clone(), task.clone());
            self.cancel_flags
                .insert(id.clone(), Arc::new(AtomicBool::new(false)));

            // Send to channel
            if let Err(e) = self.sender.try_send(TaskMessage { task }) {
                // Remove from active tasks if sending failed and mark the
                // persisted record failed so crash recovery won't resurrect it.
                self.active_tasks.remove(&id);
                self.cancel_flags.remove(&id);
                self.persist_task_status(
                    &id,
                    TaskStatus::Failed { error: e.to_string() }.key(),
                );
                return Err(AppError::Task(format!(
                    "Failed to queue task: {}",
                    e
                )));
            }

            ids.push(id);
        }

        Ok(ids)
    }

    /// Persist a newly submitted task to the `tasks` table. Failure is logged,
    /// not fatal — the in-memory queue still works without persistence.
    fn persist_new_task(&self, task: &Task) {
        let Ok(conn) = self.db.lock() else {
            return;
        };
        let task_type_json = serde_json::to_string(&task.task_type).unwrap_or_default();
        let params_json = serde_json::to_string(task).unwrap_or_default();
        if let Err(e) = dao::insert_task_record(
            &conn,
            &task.id,
            &task_type_json,
            task.status.key(),
            &params_json,
            &task.created_at,
        ) {
            log::warn!("Failed to persist task {}: {}", task.id, e);
        }
    }

    /// Persist a status transition (e.g. cancellation) to the `tasks` table.
    fn persist_task_status(&self, task_id: &str, status: &str) {
        let Ok(conn) = self.db.lock() else {
            return;
        };
        let now = chrono::Utc::now().to_rfc3339();
        if let Err(e) = dao::update_task_status(&conn, task_id, status, Some(&now)) {
            log::warn!("Failed to persist task {} status: {}", task_id, e);
        }
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
                    entry.status = TaskStatus::Cancelled;
                    self.persist_task_status(task_id, TaskStatus::Cancelled.key());
                    Ok(())
                }
                TaskStatus::Running { .. } => {
                    // Cooperative cancel: set the flag; the worker marks the
                    // task Cancelled once the in-flight git op finishes.
                    if let Some(flag) = self.cancel_flags.get(task_id) {
                        flag.store(true, Ordering::Relaxed);
                        Ok(())
                    } else {
                        Err(AppError::Task("cancel flag missing".to_string()))
                    }
                }
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
                TaskStatus::Success
                | TaskStatus::Failed { .. }
                | TaskStatus::Cancelled
                | TaskStatus::PartialSuccess { .. } => Some(e.key().clone()),
                _ => None,
            })
            .collect();

        for id in to_remove {
            self.active_tasks.remove(&id);
        }
    }
}
