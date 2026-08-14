use serde::{Deserialize, Serialize};

/// Types of Git operations that can be submitted to the task queue.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum TaskType {
    Fetch,
    Pull,
    Push,
    Commit {
        message: String,
        files: Vec<String>,
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
