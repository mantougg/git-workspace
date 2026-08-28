use serde::{Deserialize, Serialize};

/// Branch operation kinds for bulk branch tasks (T-20).
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BranchOpKind {
    Checkout,
    Create,
    Delete,
}

/// Types of Git operations that can be submitted to the task queue.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum TaskType {
    Fetch,
    Pull,
    Push,
    Commit {
        message: String,
        files: Vec<String>,
        /// Amend the HEAD commit (T-11).
        #[serde(default)]
        amend: bool,
        /// With `amend`: keep the original message (T-11 --no-edit).
        #[serde(default)]
        no_edit: bool,
        /// Commit the index as-is, preserving hunk/line staging (T-11+T-12).
        #[serde(default)]
        index_only: bool,
        /// Push after a successful commit (T-11 Commit & Push).
        #[serde(default)]
        then_push: bool,
        /// Proceed despite pre-commit safety findings (explicit user override).
        #[serde(default)]
        allow_unsafe: bool,
        /// Per-repo/group identity override (T-11 §54); resolved server-side.
        #[serde(default)]
        author_name: Option<String>,
        #[serde(default)]
        author_email: Option<String>,
    },
    /// Bulk branch operation across repositories (T-20): checkout / create /
    /// delete a branch per repo.
    BranchOp {
        op: BranchOpKind,
        name: String,
        #[serde(default)]
        force: bool,
    },
    /// Clone a repository from a remote URL into `repo_path` (T-33 batch
    /// clone; the destination must not exist yet). Runs the system git CLI
    /// (network boundary), retryable by the worker.
    Clone {
        url: String,
        #[serde(default)]
        branch: Option<String>,
    },
    /// Run a user-defined shell command in the repo directory (T-23 pipeline
    /// Build / Test steps). `timeout_secs` is enforced inside the executor;
    /// the worker's outer task timeout still applies as the hard bound.
    ShellCommand {
        command: String,
        #[serde(default)]
        timeout_secs: Option<u64>,
    },
    /// Runtime Workspace operation (R-12): build / start / stop / restart a
    /// Runtime configuration, or refresh the workspace dependency index.
    /// Executed by the Runtime task handler (not `GitOps`); the worker applies
    /// a longer hard timeout and passes the cancel flag through so in-flight
    /// Maven builds / launches abort promptly.
    Runtime {
        op: RuntimeOp,
        workspace_id: i64,
        /// Runtime config name; empty for `ResolveDependencies`.
        #[serde(default)]
        runtime_name: String,
        #[serde(default)]
        options: RuntimeTaskOptions,
    },
}

/// Runtime task operations (R-12, §63/§65). Plain camelCase string union.
/// R-15/R-17 扩展：环境编排 / 重建重启。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RuntimeOp {
    Build,
    Start,
    Stop,
    Restart,
    /// Refresh the workspace Maven discovery + dependency index (R-02 sync).
    ResolveDependencies,
    /// R-15 §38：Start Environment——按拓扑序编排启动环境内全部服务。
    /// 目标环境名放在 `runtime_name` 字段。
    StartEnvironment,
    /// R-15 §38：Stop Environment——逆拓扑序停止环境内全部服务。
    StopEnvironment,
    /// R-17/R-21：Stop → 完整构建 → Start（区别于 Restart 的 skip_build
    /// 复用；源码变更后的自动重启 / Rebuild & Restart 入口用）。
    RebuildRestart,
}

/// User-tunable options carried by a Runtime task. Mapped onto
/// `BuildOptions` / `StartOptions` by the Runtime task handler.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeTaskOptions {
    /// Run Strategy override (§30); `None` = profile-based default.
    #[serde(default)]
    pub strategy: Option<crate::runtime::build::RunStrategy>,
    /// Start-only: reuse the latest build artifacts (R-10 skip-build).
    #[serde(default)]
    pub skip_build: bool,
    /// `None` = follow `BuildOptions` default (skip tests, IDEA Build 语义)。
    #[serde(default)]
    pub skip_tests: Option<bool>,
    #[serde(default)]
    pub offline: bool,
    /// R-17 §44 增量构建：watch 影响分析给出的受影响模块 GA 子集（已含
    /// 反向依赖传播）。非空时流水线以其为必建下限，与 R-18 指纹子集合并。
    #[serde(default)]
    pub affected_modules: Vec<String>,
}

/// Status of a background task.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum TaskStatus {
    Queued,
    Running {
        progress: f32,
    },
    Success,
    /// Batch task where some repositories succeeded and some failed.
    PartialSuccess {
        succeeded: usize,
        failed: usize,
    },
    Failed {
        error: String,
    },
    Cancelled,
}

impl TaskStatus {
    /// Stable short key used for DB persistence and crash-recovery matching.
    pub fn key(&self) -> &'static str {
        match self {
            TaskStatus::Queued => "queued",
            TaskStatus::Running { .. } => "running",
            TaskStatus::Success => "success",
            TaskStatus::PartialSuccess { .. } => "partial_success",
            TaskStatus::Failed { .. } => "failed",
            TaskStatus::Cancelled => "cancelled",
        }
    }
}

/// A unit of work submitted to the background task system.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub id: String,
    pub task_type: TaskType,
    pub repo_path: String,
    pub repo_name: String,
    pub status: TaskStatus,
    pub created_at: String,
    /// Batch this task belongs to (T-20): tasks submitted in one `submit()`
    /// call of >1 repo share a batch id; the batch itself appears as a
    /// synthetic task whose id equals the batch id and whose `batchId` is null.
    #[serde(default)]
    pub batch_id: Option<String>,
}

/// Aggregate state of a multi-repo batch (T-05/T-20), tracked in memory and
/// persisted: batch row in `tasks`, per-repo sub-results in `task_items`.
#[derive(Debug)]
pub struct BatchState {
    /// The synthetic batch task (its status evolves as children finish).
    pub task: Task,
    /// `tasks.id` row of the batch (task_items linkage).
    pub db_row_id: i64,
    pub total: usize,
    pub finished: usize,
    pub succeeded: usize,
    pub failed: usize,
    pub cancelled: usize,
}

impl BatchState {
    /// Record a child's final status and evolve the aggregate status.
    /// Returns true when the batch just finished. (Pure part of
    /// `worker::update_batch`; side effects — DB writes, events — live there
    /// so this stays unit-testable.)
    pub fn record_child(&mut self, status: &TaskStatus) -> bool {
        match status {
            TaskStatus::Success => self.succeeded += 1,
            TaskStatus::Failed { .. } => self.failed += 1,
            TaskStatus::Cancelled => self.cancelled += 1,
            _ => return false, // not a final status
        }
        self.finished += 1;

        self.task.status = if self.finished < self.total {
            TaskStatus::Running {
                progress: self.finished as f32 / self.total as f32,
            }
        } else if self.failed == 0 && self.cancelled == 0 {
            TaskStatus::Success
        } else if self.succeeded == 0 && self.failed == self.total {
            TaskStatus::Failed {
                error: format!("{} 个仓库全部失败", self.failed),
            }
        } else if self.cancelled == self.total {
            TaskStatus::Cancelled
        } else {
            // Partial Success: some succeeded, some failed/cancelled (T-05).
            TaskStatus::PartialSuccess {
                succeeded: self.succeeded,
                failed: self.failed + self.cancelled,
            }
        };
        self.finished == self.total
    }
}

/// Request payload for submitting a batch of tasks.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskRequest {
    pub task_type: TaskType,
    pub repo_path: String,
    pub repo_name: String,
}

/// Payload for the `task_progress` Tauri event.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskProgress {
    pub task_id: String,
    pub task_type: TaskType,
    pub repo_path: String,
    pub repo_name: String,
    pub status: TaskStatus,
    /// Batch the task belongs to (null for the batch row itself and for
    /// single-repo submits), used by the task panel for grouping (T-20).
    pub batch_id: Option<String>,
}

/// Payload for the `git_command_result` Tauri event (IDE-style git console).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitCommandResult {
    pub repo_name: String,
    pub repo_path: String,
    /// Human-readable command description, e.g. "git fetch origin".
    pub command: String,
    pub success: bool,
    /// Combined stdout/stderr (or the error message on failure).
    pub output: String,
}

// ---------------------------------------------------------------------------
// T-24 Task DAG
// ---------------------------------------------------------------------------

/// What the DAG scheduler does when a node fails (T-24, configurable per DAG).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FailurePolicy {
    /// Cancel every unfinished node in the DAG on the first failure.
    FailFast,
    /// Skip only the (transitive) dependents of the failed node; independent
    /// branches keep running.
    Continue,
}

impl Default for FailurePolicy {
    fn default() -> Self {
        FailurePolicy::Continue
    }
}

/// Runtime condition gating a DAG node's dispatch (T-23 Conditional steps).
/// Evaluated in memory when the node becomes ready; a false result marks the
/// node skipped (its dependents are still released).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum NodeCondition {
    /// Dispatch only when the repository working tree is clean.
    RepoClean,
}

/// One node of a DAG submission: a task plus the indices (into the same
/// `nodes` list) of the nodes it depends on.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DagNodeRequest {
    pub task: TaskRequest,
    /// Indices into the same submission's `nodes` array; the referenced
    /// nodes must all succeed before this node is dispatched.
    #[serde(default)]
    pub depends_on: Vec<usize>,
    /// Extra scheduler-level attempts (1 = no retry; the worker's own
    /// network retry still applies per attempt).
    #[serde(default = "default_max_attempts")]
    pub max_attempts: u32,
    /// Optional dispatch condition (evaluated in memory, T-23).
    #[serde(default)]
    pub condition: Option<NodeCondition>,
    /// Optional grouping label (T-23 uses the pipeline step id).
    #[serde(default)]
    pub group: Option<String>,
    /// Human-readable node label for the DAG view.
    #[serde(default)]
    pub label: Option<String>,
}

fn default_max_attempts() -> u32 {
    1
}

/// Submit a dependency DAG of tasks (T-24). Nodes are executed in
/// topological order as their dependencies succeed; independent branches run
/// in parallel bounded by the worker pool (§45 limits still apply).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DagSubmitRequest {
    /// Display name (also used as the synthetic batch task's name).
    pub name: String,
    pub nodes: Vec<DagNodeRequest>,
    #[serde(default)]
    pub on_failure: FailurePolicy,
}

/// One node in the DAG visualization / report query (T-24/T-23).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DagNodeInfo {
    pub task_id: String,
    pub label: String,
    /// Grouping label (pipeline step id for pipeline runs).
    pub group: Option<String>,
    pub repo_path: String,
    pub repo_name: String,
    /// Task ids this node depends on.
    pub depends_on: Vec<String>,
    pub status: TaskStatus,
    /// True when the node was skipped (dependency failed or condition false)
    /// rather than executed; its task status reads `cancelled`.
    pub skipped: bool,
    /// Scheduler-level attempts so far.
    pub attempts: u32,
    /// Captured output tail (bounded) for reports.
    pub output: Option<String>,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
}

/// Dependency edge of the DAG view (`from` must finish before `to`).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DagEdge {
    pub from: String,
    pub to: String,
}

/// DAG visualization payload: nodes + edges + live states (T-24).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DagGraph {
    pub dag_id: String,
    pub name: String,
    pub on_failure: FailurePolicy,
    pub nodes: Vec<DagNodeInfo>,
    pub edges: Vec<DagEdge>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn batch(total: usize) -> BatchState {
        BatchState {
            task: Task {
                id: "b-1".into(),
                task_type: TaskType::Pull,
                repo_path: String::new(),
                repo_name: "批量".into(),
                status: TaskStatus::Running { progress: 0.0 },
                created_at: "t".into(),
                batch_id: None,
            },
            db_row_id: 1,
            total,
            finished: 0,
            succeeded: 0,
            failed: 0,
            cancelled: 0,
        }
    }

    /// T-05 acceptance: a batch with some failures ends PartialSuccess.
    #[test]
    fn batch_with_mixed_results_ends_partial_success() {
        let mut b = batch(3);
        assert!(!b.record_child(&TaskStatus::Success));
        assert!(matches!(b.task.status, TaskStatus::Running { .. }));
        assert!(!b.record_child(&TaskStatus::Failed { error: "x".into() }));
        assert!(b.record_child(&TaskStatus::Success));
        match &b.task.status {
            TaskStatus::PartialSuccess { succeeded, failed } => {
                assert_eq!(*succeeded, 2);
                assert_eq!(*failed, 1);
            }
            other => panic!("expected PartialSuccess, got {:?}", other),
        }
    }

    #[test]
    fn batch_all_success_ends_success() {
        let mut b = batch(2);
        b.record_child(&TaskStatus::Success);
        assert!(b.record_child(&TaskStatus::Success));
        assert!(matches!(b.task.status, TaskStatus::Success));
    }

    #[test]
    fn batch_all_failed_ends_failed_and_cancelled_mix_is_partial() {
        let mut b = batch(2);
        b.record_child(&TaskStatus::Failed { error: "x".into() });
        b.record_child(&TaskStatus::Failed { error: "y".into() });
        assert!(matches!(b.task.status, TaskStatus::Failed { .. }));

        let mut b2 = batch(2);
        b2.record_child(&TaskStatus::Success);
        b2.record_child(&TaskStatus::Cancelled);
        assert!(matches!(b2.task.status, TaskStatus::PartialSuccess { .. }));
    }

    /// Non-final statuses must not move the aggregate.
    #[test]
    fn batch_ignores_non_final_child_status() {
        let mut b = batch(1);
        assert!(!b.record_child(&TaskStatus::Running { progress: 0.5 }));
        assert_eq!(b.finished, 0);
        assert!(b.record_child(&TaskStatus::Success));
    }
}
