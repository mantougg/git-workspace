use serde::{Deserialize, Serialize};

/// A Git Workspace - a root directory containing one or more Git repositories.
/// The workspace directory itself does not need to be a Git repository.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Workspace {
    pub id: i64,
    pub name: String,
    pub path: String,
    pub scan_depth: i32,
    pub created_at: String,
    pub updated_at: String,
}

/// Request payload for creating a new workspace.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateWorkspaceRequest {
    pub name: String,
    pub path: String,
    /// Maximum directory recursion depth for repository scanning.
    /// Defaults to 5 if not specified.
    pub scan_depth: Option<i32>,
}

/// Request payload for updating an existing workspace.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateWorkspaceRequest {
    pub name: Option<String>,
    pub scan_depth: Option<i32>,
}
