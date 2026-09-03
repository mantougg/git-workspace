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
use crate::models::task::{BatchState, DagGraph, DagSubmitRequest, Task, TaskRequest, TaskStatus, TaskType};
use crate::task::dag::{self, DagContext, DagNodeState, DagState, NodeState};
use crate::task::queue::{self, TaskMessage};
use crate::task::worker;

/// Manages the background task queue and worker pool.
///
/// Tasks are submitted via `submit()` and processed by a pool of
/// async workers. Status updates are emitted as `task_progress` events.
/// Dependency DAGs (T-24) are submitted via `submit_dag()`; their nodes
/// share the DAG id as `batch_id`, so the T-20 batch aggregate serves as
/// the run-level rollup and `task_items` keeps per-repo sub-results.
pub struct TaskManager {
    sender: tokio::sync::mpsc::Sender<TaskMessage>,
    active_tasks: Arc<DashMap<String, Task>>,
    cancel_flags: Arc<DashMap<String, Arc<AtomicBool>>>,
    /// Shared DB connection for task persistence (history + crash recovery).
    db: Arc<Mutex<Connection>>,
    /// Aggregate state of multi-repo batches (T-20), keyed by batch id.
    batches: Arc<DashMap<String, BatchState>>,
    /// Live DAG states (T-24), keyed by DAG id (= nodes' batch_id).
    dags: Arc<DashMap<String, DagState>>,
    /// Kept to emit progress events for synthetic batch tasks.
    app_handle: AppHandle,
}

impl TaskManager {
    /// Create a new TaskManager and start the worker pool.
    ///
    /// The worker pool runs `worker_count` async tasks, each pulling
    /// from a shared mpsc receiver. Git operations (blocking) are
    /// executed via `tokio::task::spawn_blocking`. Runtime tasks (R-12) are
    /// dispatched to `runtime_handler`; `None` makes them fail fast with an
    /// actionable error (tests / minimal setups).
    pub fn new(
        worker_count: usize,
        git_ops: Arc<GitOps>,
        app_handle: AppHandle,
        db: Arc<Mutex<Connection>>,
        runtime_handler: Option<Arc<dyn crate::task::runtime::RuntimeTaskHandler>>,
    ) -> Self {
        let (sender, receiver) = queue::new_queue(128);
        let active_tasks = Arc::new(DashMap::<String, Task>::new());
        let cancel_flags = Arc::new(DashMap::<String, Arc<AtomicBool>>::new());
        let batches = Arc::new(DashMap::<String, BatchState>::new());
        let dags = Arc::new(DashMap::<String, DagState>::new());

        // Spawn the worker pool using the worker module
        worker::spawn_worker_pool(
            worker_count,
            receiver,
            sender.clone(),
            Arc::clone(&git_ops),
            runtime_handler,
            Arc::clone(&active_tasks),
            Arc::clone(&cancel_flags),
            app_handle.clone(),
            Arc::clone(&db),
            Arc::clone(&batches),
            Arc::clone(&dags),
        );

        log::info!("TaskManager started with {} workers", worker_count);

        TaskManager {
            sender,
            active_tasks,
            cancel_flags,
            db,
            batches,
            dags,
            app_handle,
        }
    }

    /// Shared context for DAG scheduler side effects (T-24).
    fn dag_ctx(&self) -> DagContext<'_> {
        DagContext {
            dags: &self.dags,
            sender: &self.sender,
            active_tasks: &self.active_tasks,
            cancel_flags: &self.cancel_flags,
            db: &self.db,
            app: &self.app_handle,
            batches: &self.batches,
        }
    }

    /// Submit a batch of tasks to the queue.
    /// Returns the list of generated task IDs.
    pub fn submit(&self, requests: &[TaskRequest]) -> AppResult<Vec<String>> {
        let mut ids = Vec::with_capacity(requests.len());

        // Multi-repo submits get a synthetic batch task (T-20): it tracks
        // the aggregate (Partial Success etc.) while children keep their
        // per-repo rows. Single submits stay flat (no batch row).
        let batch_id = (requests.len() > 1).then(|| Uuid::new_v4().to_string());
        if let Some(bid) = &batch_id {
            let now = Utc::now().to_rfc3339();
            let batch_task = Task {
                id: bid.clone(),
                task_type: requests[0].task_type.clone(),
                repo_path: String::new(),
                repo_name: format!("批量（{} 个仓库）", requests.len()),
                status: TaskStatus::Running { progress: 0.0 },
                created_at: now,
                batch_id: None,
            };
            let row_id = self.persist_new_task(&batch_task);
            self.batches.insert(
                bid.clone(),
                BatchState {
                    task: batch_task.clone(),
                    db_row_id: row_id.unwrap_or(0),
                    total: requests.len(),
                    finished: 0,
                    succeeded: 0,
                    failed: 0,
                    cancelled: 0,
                },
            );
            worker::emit_progress(&self.app_handle, &batch_task);
        }

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
                batch_id: batch_id.clone(),
            };

            // Persist to the `tasks` table (history + crash recovery).
            self.persist_new_task(&task);

            // Store in active tasks + create cancel flag
            self.active_tasks.insert(id.clone(), task.clone());
            self.cancel_flags.insert(id.clone(), Arc::new(AtomicBool::new(false)));

            // Send to channel
            if let Err(e) = self.sender.try_send(TaskMessage { task: task.clone() }) {
                // Remove from active tasks if sending failed and mark the
                // persisted record failed so crash recovery won't resurrect it.
                self.active_tasks.remove(&id);
                self.cancel_flags.remove(&id);
                let failed = TaskStatus::Failed { error: e.to_string() };
                self.persist_task_status(&id, failed.key());
                // Account the failed child into its batch so the batch
                // cannot hang unfinished (T-20 aggregation).
                if task.batch_id.is_some() {
                    let mut failed_task = task;
                    failed_task.status = failed;
                    worker::update_batch(&self.batches, &self.db, &self.app_handle, &failed_task);
                }
                return Err(AppError::Task(format!("Failed to queue task: {}", e)));
            }

            ids.push(id);
        }

        Ok(ids)
    }

    /// Submit a dependency DAG (T-24): nodes run in topological order as
    /// their dependencies succeed; independent branches proceed in parallel,
    /// bounded by the same worker pool (§45 limits still apply).
    ///
    /// `depends_on` entries are indices into `req.nodes`; they are validated
    /// (range + acyclicity) before anything is persisted. Returns the DAG id
    /// (also the nodes' `batch_id` and the synthetic rollup task's id).
    pub fn submit_dag(&self, req: &DagSubmitRequest) -> AppResult<String> {
        if req.nodes.is_empty() {
            return Err(AppError::Task("DAG 至少需要一个节点".to_string()));
        }

        // Resolve edges (dep_idx, node_idx) and validate up front.
        let mut edges: Vec<(usize, usize)> = Vec::new();
        for (i, n) in req.nodes.iter().enumerate() {
            for &dep in &n.depends_on {
                edges.push((dep, i));
            }
        }
        dag::validate_edges(req.nodes.len(), &edges).map_err(AppError::Task)?;

        let dag_id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();

        // Synthetic batch task: the DAG run's rollup row (Success / Failed /
        // PartialSuccess as the nodes finish, via the T-20 aggregate).
        let batch_task = Task {
            id: dag_id.clone(),
            task_type: req.nodes[0].task.task_type.clone(),
            repo_path: String::new(),
            repo_name: format!("{}（{} 个节点）", req.name, req.nodes.len()),
            status: TaskStatus::Running { progress: 0.0 },
            created_at: now.clone(),
            batch_id: None,
        };
        let batch_row = self.persist_new_task(&batch_task);
        self.batches.insert(
            dag_id.clone(),
            BatchState {
                task: batch_task.clone(),
                db_row_id: batch_row.unwrap_or(0),
                total: req.nodes.len(),
                finished: 0,
                succeeded: 0,
                failed: 0,
                cancelled: 0,
            },
        );
        worker::emit_progress(&self.app_handle, &batch_task);

        // Create the node tasks. All start Queued in active_tasks; blocked
        // nodes are NOT sent to the worker channel until released.
        let mut task_ids: Vec<String> = Vec::with_capacity(req.nodes.len());
        let mut row_ids: Vec<Option<i64>> = Vec::with_capacity(req.nodes.len());
        for n in &req.nodes {
            let id = Uuid::new_v4().to_string();
            let task = Task {
                id: id.clone(),
                task_type: n.task.task_type.clone(),
                repo_path: n.task.repo_path.clone(),
                repo_name: n.task.repo_name.clone(),
                status: TaskStatus::Queued,
                created_at: now.clone(),
                batch_id: Some(dag_id.clone()),
            };
            let row = self.persist_new_task(&task);
            self.active_tasks.insert(id.clone(), task);
            self.cancel_flags.insert(id.clone(), Arc::new(AtomicBool::new(false)));
            task_ids.push(id);
            row_ids.push(row);
        }

        // Persist the dependency edges (`task_dependencies`, one transaction,
        // best effort — the in-memory DAG works without persistence).
        if row_ids.iter().all(Option::is_some) {
            let db_edges: Vec<(i64, i64)> = edges
                .iter()
                .map(|&(dep, node)| (row_ids[node].unwrap(), row_ids[dep].unwrap()))
                .collect();
            if let Ok(conn) = self.db.lock() {
                if let Err(e) = dag::insert_task_dependencies(&conn, &db_edges) {
                    log::warn!("Failed to persist task_dependencies for DAG {}: {}", dag_id, e);
                }
            }
        }

        // In-memory DAG state (scheduler + visualization/report source).
        let nodes: Vec<DagNodeState> = req
            .nodes
            .iter()
            .enumerate()
            .map(|(i, n)| DagNodeState {
                task_id: task_ids[i].clone(),
                label: n.label.clone().unwrap_or_else(|| label_for(&n.task)),
                group: n.group.clone(),
                repo_path: n.task.repo_path.clone(),
                repo_name: n.task.repo_name.clone(),
                depends_on: n.depends_on.iter().map(|&d| task_ids[d].clone()).collect(),
                state: NodeState::Pending,
                skipped: false,
                attempts: 0,
                max_attempts: n.max_attempts.max(1),
                condition: n.condition,
                output: None,
                error: None,
                started_at: None,
                finished_at: None,
            })
            .collect();
        let state =
            DagState::build(dag_id.clone(), req.name.clone(), req.on_failure, nodes, &edges).map_err(AppError::Task)?;

        self.dags.insert(dag_id.clone(), state);
        dag::evict_finished(&self.dags);

        // Dispatch the initial ready set. The state must already be in the
        // map: a fast node could finish before the insert and its dependents
        // would never be released.
        let ctx = self.dag_ctx();
        if let Some(mut entry) = self.dags.get_mut(&dag_id) {
            let ready = entry.value_mut().initial_ready();
            dag::dispatch_ready(entry.value_mut(), ready, &ctx);
        }

        log::info!(
            "DAG {} ({}) submitted with {} nodes, policy {:?}",
            dag_id,
            req.name,
            req.nodes.len(),
            req.on_failure
        );
        Ok(dag_id)
    }

    /// DAG visualization payload: nodes + edges + live states (T-24).
    pub fn get_dag_graph(&self, dag_id: &str) -> Option<DagGraph> {
        self.dags.get(dag_id).map(|e| dag::build_graph(e.value()))
    }

    /// Read-side access to a live DAG state (T-23 run report).
    pub fn with_dag<R>(&self, dag_id: &str, f: impl FnOnce(&DagState) -> R) -> Option<R> {
        self.dags.get(dag_id).map(|e| f(e.value()))
    }

    /// Persist a newly submitted task to the `tasks` table. Failure is logged,
    /// not fatal — the in-memory queue still works without persistence.
    fn persist_new_task(&self, task: &Task) -> Option<i64> {
        let Ok(conn) = self.db.lock() else {
            return None;
        };
        let task_type_json = serde_json::to_string(&task.task_type).unwrap_or_default();
        let params_json = serde_json::to_string(task).unwrap_or_default();
        match dao::insert_task_record(
            &conn,
            &task.id,
            &task_type_json,
            task.status.key(),
            &params_json,
            &task.created_at,
        ) {
            Ok(row_id) => Some(row_id),
            Err(e) => {
                log::warn!("Failed to persist task {}: {}", task.id, e);
                None
            }
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

    /// Cancel a queued or running task (cooperative). For DAG nodes (T-24)
    /// the cancellation propagates along the dependency edges: not-yet-
    /// started downstream tasks are marked cancelled; running downstream
    /// tasks are left to finish (their own cancellation stays user-driven).
    pub fn cancel(&self, task_id: &str) -> AppResult<()> {
        // T-24: a blocked DAG node never reaches the worker, so cancel it (and
        // its pending subtree) right here. Dispatched nodes go through the
        // cooperative flag below and are finalized by the worker path.
        if let Some(task) = self.active_tasks.get(task_id).map(|e| e.clone()) {
            if let Some(dag_id) = task.batch_id.clone() {
                if let Some(mut entry) = self.dags.get_mut(&dag_id) {
                    let dag = entry.value_mut();
                    if let Some(idx) = dag.index_of(task_id) {
                        let ctx = self.dag_ctx();
                        dag::cancel_pending_node(dag, idx, &ctx);
                    }
                }
            }
        }

        if let Some(mut entry) = self.active_tasks.get_mut(task_id) {
            match &entry.status {
                TaskStatus::Queued => {
                    entry.status = TaskStatus::Cancelled;
                    // Also set the cooperative flag: the task may already sit
                    // in the worker channel, and the worker checks the flag
                    // before starting the git operation.
                    if let Some(flag) = self.cancel_flags.get(task_id) {
                        flag.store(true, Ordering::Relaxed);
                    }
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
            Err(AppError::NotFound(format!("Task {} not found", task_id)))
        }
    }

    /// Cancel a whole DAG run (T-24): pending nodes are marked cancelled
    /// immediately; running nodes get the cooperative cancel flag, which the
    /// worker honours between/within operations (subprocess cleanup reuses
    /// the existing cancel flags, T-05).
    pub fn cancel_dag(&self, dag_id: &str) -> AppResult<()> {
        let Some(mut entry) = self.dags.get_mut(dag_id) else {
            return Err(AppError::NotFound(format!("DAG {} not found", dag_id)));
        };
        let dag = entry.value_mut();
        let ctx = self.dag_ctx();
        for idx in 0..dag.nodes.len() {
            match dag.nodes[idx].state {
                NodeState::Pending => {
                    dag::cancel_pending_node(dag, idx, &ctx);
                }
                NodeState::Dispatched => {
                    let task_id = dag.nodes[idx].task_id.clone();
                    if let Some(flag) = self.cancel_flags.get(&task_id) {
                        flag.store(true, Ordering::Relaxed);
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// Get all active tasks (for the task panel display), with the
    /// synthetic batch rows (T-20) appended.
    pub fn list_active(&self) -> Vec<Task> {
        let mut out: Vec<Task> = self.active_tasks.iter().map(|e| e.clone()).collect();
        out.extend(self.batches.iter().map(|e| e.task.clone()));
        out
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

/// Default node label for the DAG view: "<kind> · <repo>".
fn label_for(req: &TaskRequest) -> String {
    let kind = match &req.task_type {
        TaskType::Fetch => "Fetch",
        TaskType::Pull => "Pull",
        TaskType::Push => "Push",
        TaskType::Commit { .. } => "Commit",
        TaskType::ConflictApply { .. } => "Conflict Apply",
        TaskType::BranchOp { .. } => "分支操作",
        TaskType::Clone { .. } => "Clone",
        TaskType::ShellCommand { .. } => "Shell",
        TaskType::Runtime { op, .. } => {
            let kind = match op {
                crate::models::task::RuntimeOp::Build => "Build",
                crate::models::task::RuntimeOp::Start => "Start",
                crate::models::task::RuntimeOp::Stop => "Stop",
                crate::models::task::RuntimeOp::Restart => "Restart",
                crate::models::task::RuntimeOp::ResolveDependencies => "Resolve",
                crate::models::task::RuntimeOp::StartEnvironment => "StartEnvironment",
                crate::models::task::RuntimeOp::StopEnvironment => "StopEnvironment",
                crate::models::task::RuntimeOp::RebuildRestart => "RebuildRestart",
            };
            return format!("Runtime {} · {}", kind, req.repo_name);
        }
        TaskType::RuntimeUpdateConfig { .. } => "Runtime Update Config",
        TaskType::NodeInstall { .. } => "Node Install",
    };
    format!("{} · {}", kind, req.repo_name)
}
