use serde::{Deserialize, Serialize};

/// A discovered Git repository within a workspace.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Repository {
    pub id: Option<i64>,
    pub workspace_id: i64,
    /// Absolute path to the repository root (containing the .git directory).
    pub path: String,
    /// Repository name (directory name).
    pub name: String,
    /// Path relative to the workspace root.
    pub relative_path: String,
    pub is_favorite: bool,
    pub tags: Vec<String>,
    pub group_id: Option<i64>,
}

/// Real-time Git status of a repository.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoStatus {
    /// Current branch name, or "HEAD" if detached.
    pub branch: String,
    pub is_detached: bool,
    /// Commits ahead of upstream remote.
    pub ahead: usize,
    /// Commits behind upstream remote.
    pub behind: usize,
    /// Number of modified files (working tree).
    pub modified: usize,
    /// Number of newly added files (staged).
    pub added: usize,
    /// Number of deleted files.
    pub deleted: usize,
    /// Number of untracked files (not in index).
    pub untracked: usize,
    /// Number of staged files (in index, not committed).
    pub staged: usize,
    /// Number of conflicted files (unresolved merge/rebase/cherry-pick
    /// conflicts; counted separately from modified/deleted).
    pub conflicted: usize,
    /// Whether any git remote is configured (T-19 "Missing Remote" check).
    pub has_remote: bool,
    pub is_clean: bool,
}

/// Repository data combined with its current Git status.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryWithStatus {
    pub repository: Repository,
    pub status: Option<RepoStatus>,
    /// Error message if status retrieval failed.
    pub last_error: Option<String>,
}

/// Internal structure used by the scanner before persisting to the database.
#[derive(Debug, Clone)]
pub struct ScannedRepo {
    pub path: String,
    pub name: String,
    pub relative_path: String,
    /// `.git` directory mtime (unix milliseconds) recorded at scan time.
    /// Used as the incremental-scan cache key: a known repository whose mtime
    /// is unchanged skips the expensive `git2::Repository::open` validation.
    pub git_dir_mtime: Option<i64>,
}

/// Payload for the `scan_progress` Tauri event.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanProgress {
    pub workspace_id: i64,
    pub found: usize,
    pub current: usize,
    pub total: Option<usize>,
}

/// Payload for the `repo_status_changed_batch` Tauri event (file watcher).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoStatusUpdate {
    pub repo_path: String,
    pub status: RepoStatus,
}

/// A single changed file within a repository.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileChange {
    /// Path relative to the repository root, using `/` separators.
    pub path: String,
    /// Change category: `untracked` | `modified` | `deleted` | `staged` | `renamed` | `typechange`.
    pub status: String,
}

/// File-level change summary for one repository.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoChanges {
    pub repo_path: String,
    pub repo_name: String,
    /// Path relative to the workspace root, used to build the directory tree.
    pub relative_path: String,
    pub branch: String,
    pub is_detached: bool,
    pub ahead: usize,
    pub behind: usize,
    /// Sorted list of changed files (empty means the repo is clean).
    pub changes: Vec<FileChange>,
}

/// Persistent commit-metadata cache entry, mirroring the `commits` /
/// `commit_parents` tables. Used to avoid re-parsing commit objects on repeat
/// Graph loads (T-04 Graph data cache).
#[derive(Debug, Clone)]
pub struct CommitRecord {
    pub oid: String,
    pub message: String,
    /// Author in git's `Name <email>` form.
    pub author: String,
    /// Committer in git's `Name <email>` form.
    pub committer: String,
    /// Unix timestamp (seconds).
    pub authored_at: i64,
    pub committed_at: i64,
    /// Author timezone offset in minutes.
    pub offset_minutes: i32,
    pub parents: Vec<String>,
}
