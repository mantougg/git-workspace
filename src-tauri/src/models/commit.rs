use serde::Serialize;

/// One pre-commit safety finding (T-11, global constraint §5): forbidden file,
/// oversized file, or suspected secret in the content to be committed.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitScanFinding {
    pub path: String,
    /// "forbidden" | "large_file" | "secret"
    pub kind: String,
    pub detail: String,
}

/// Resolved commit identity for a repository (T-11 §54): per-repo config wins
/// over the repo's group config, which wins over the git default.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitIdentity {
    pub name: String,
    pub email: String,
    /// "repo" | "group" | "mixed" (fields coming from both levels)
    pub source: String,
}
