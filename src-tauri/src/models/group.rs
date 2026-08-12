use serde::{Deserialize, Serialize};

/// A hierarchical group for organizing repositories within a workspace.
/// Groups can be nested via `parent_id`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoGroup {
    pub id: i64,
    pub workspace_id: i64,
    pub name: String,
    pub parent_id: Option<i64>,
    pub sort_order: i32,
}

/// Request payload for creating a new group.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateGroupRequest {
    pub workspace_id: i64,
    pub name: String,
    pub parent_id: Option<i64>,
}
