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
