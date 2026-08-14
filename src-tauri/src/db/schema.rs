/// SQL schema for GitWorkspace database.
///
/// Versioned via `PRAGMA user_version` (see `db::migrate`). Each entry in
/// `MIGRATIONS` bumps the version by one. Append new entries at the END and
/// never edit an already-shipped entry — existing databases rely on the
/// historical SQL staying stable.
///
/// All `CREATE TABLE` statements use `IF NOT EXISTS`, so applying v1 over a
/// pre-existing database only creates the missing tables and leaves existing
/// data untouched (lossless upgrade).

/// Version 1 — baseline schema.
/// Existing tables plus the full Roadmap §41 table set.
pub const SCHEMA_V1: &str = r#"
-- =========================== Workspaces ===========================

-- Workspace: a root directory that may contain multiple Git repositories
CREATE TABLE IF NOT EXISTS workspaces (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    name        TEXT NOT NULL,
    path        TEXT NOT NULL UNIQUE,
    scan_depth  INTEGER DEFAULT 5,
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL
);

-- Repository: a discovered Git repository within a workspace
CREATE TABLE IF NOT EXISTS repositories (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    workspace_id  INTEGER NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    path          TEXT NOT NULL UNIQUE,
    name          TEXT NOT NULL,
    relative_path TEXT NOT NULL,
    is_favorite   INTEGER DEFAULT 0,
    tags          TEXT DEFAULT '[]',
    group_id      INTEGER,
    last_scanned  TEXT,
    created_at    TEXT NOT NULL,
    updated_at    TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_repositories_workspace ON repositories(workspace_id);
CREATE INDEX IF NOT EXISTS idx_repositories_group ON repositories(group_id);
CREATE INDEX IF NOT EXISTS idx_repositories_favorite ON repositories(is_favorite);

-- Repository groups: hierarchical grouping of repositories within a workspace
CREATE TABLE IF NOT EXISTS repo_groups (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    workspace_id INTEGER NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    name         TEXT NOT NULL,
    parent_id    INTEGER REFERENCES repo_groups(id) ON DELETE CASCADE,
    sort_order   INTEGER DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_repo_groups_workspace ON repo_groups(workspace_id);
CREATE INDEX IF NOT EXISTS idx_repo_groups_parent ON repo_groups(parent_id);

-- Task history: records of completed Git operations
CREATE TABLE IF NOT EXISTS task_history (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    task_type   TEXT NOT NULL,
    repo_path   TEXT NOT NULL,
    status      TEXT NOT NULL,
    message     TEXT,
    started_at  TEXT NOT NULL,
    finished_at TEXT
);

CREATE INDEX IF NOT EXISTS idx_task_history_repo ON task_history(repo_path);
CREATE INDEX IF NOT EXISTS idx_task_history_type ON task_history(task_type);

-- Code search index: full-text search across repository files
CREATE VIRTUAL TABLE IF NOT EXISTS code_index USING fts5(
    content,
    repo_path,
    file_path,
    tokenize = 'unicode61'
);

-- ===================== Roadmap §41: full table set =====================

-- Local branches of a repository
CREATE TABLE IF NOT EXISTS branches (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    repo_id    INTEGER NOT NULL REFERENCES repositories(id) ON DELETE CASCADE,
    name       TEXT NOT NULL,
    is_head    INTEGER DEFAULT 0,
    ahead      INTEGER DEFAULT 0,
    behind     INTEGER DEFAULT 0,
    updated_at TEXT NOT NULL,
    UNIQUE(repo_id, name)
);

CREATE INDEX IF NOT EXISTS idx_branches_repo ON branches(repo_id);

-- Remote-tracking branches (refs/remotes/*) of a repository
CREATE TABLE IF NOT EXISTS remote_branches (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    repo_id    INTEGER NOT NULL REFERENCES repositories(id) ON DELETE CASCADE,
    name       TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE(repo_id, name)
);

CREATE INDEX IF NOT EXISTS idx_remote_branches_repo ON remote_branches(repo_id);

-- Tags of a repository
CREATE TABLE IF NOT EXISTS tags (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    repo_id    INTEGER NOT NULL REFERENCES repositories(id) ON DELETE CASCADE,
    name       TEXT NOT NULL,
    target_oid TEXT,
    updated_at TEXT NOT NULL,
    UNIQUE(repo_id, name)
);

CREATE INDEX IF NOT EXISTS idx_tags_repo ON tags(repo_id);

-- Commit metadata (persistent cache for graph / history)
CREATE TABLE IF NOT EXISTS commits (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    repo_id      INTEGER NOT NULL REFERENCES repositories(id) ON DELETE CASCADE,
    oid          TEXT NOT NULL,
    message      TEXT,
    author       TEXT,
    committer    TEXT,
    authored_at  TEXT,
    committed_at TEXT,
    UNIQUE(repo_id, oid)
);

CREATE INDEX IF NOT EXISTS idx_commits_repo ON commits(repo_id);

-- Commit parent edges
CREATE TABLE IF NOT EXISTS commit_parents (
    commit_id  INTEGER NOT NULL REFERENCES commits(id) ON DELETE CASCADE,
    parent_oid TEXT NOT NULL,
    PRIMARY KEY (commit_id, parent_oid)
);

-- Files touched by a commit
CREATE TABLE IF NOT EXISTS commit_files (
    commit_id INTEGER NOT NULL REFERENCES commits(id) ON DELETE CASCADE,
    path      TEXT NOT NULL,
    additions INTEGER DEFAULT 0,
    deletions INTEGER DEFAULT 0,
    PRIMARY KEY (commit_id, path)
);

-- Stashes of a repository
CREATE TABLE IF NOT EXISTS stashes (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    repo_id    INTEGER NOT NULL REFERENCES repositories(id) ON DELETE CASCADE,
    stash_ref  TEXT NOT NULL,
    message    TEXT,
    created_at TEXT NOT NULL,
    UNIQUE(repo_id, stash_ref)
);

CREATE INDEX IF NOT EXISTS idx_stashes_repo ON stashes(repo_id);

-- Git worktrees linked to a repository
CREATE TABLE IF NOT EXISTS worktrees (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    repo_id    INTEGER NOT NULL REFERENCES repositories(id) ON DELETE CASCADE,
    path       TEXT NOT NULL,
    branch     TEXT,
    created_at TEXT NOT NULL,
    UNIQUE(repo_id, path)
);

CREATE INDEX IF NOT EXISTS idx_worktrees_repo ON worktrees(repo_id);

-- Persisted snapshot of a repository's status (persistent cache layer)
CREATE TABLE IF NOT EXISTS repo_status (
    repo_id         INTEGER PRIMARY KEY REFERENCES repositories(id) ON DELETE CASCADE,
    branch          TEXT,
    is_dirty        INTEGER DEFAULT 0,
    is_detached     INTEGER DEFAULT 0,
    ahead           INTEGER DEFAULT 0,
    behind          INTEGER DEFAULT 0,
    conflict_count  INTEGER DEFAULT 0,
    modified_count  INTEGER DEFAULT 0,
    untracked_count INTEGER DEFAULT 0,
    updated_at      TEXT NOT NULL
);

-- Per-file status within a repository
CREATE TABLE IF NOT EXISTS file_status (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    repo_id    INTEGER NOT NULL REFERENCES repositories(id) ON DELETE CASCADE,
    path       TEXT NOT NULL,
    status     TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE(repo_id, path)
);

CREATE INDEX IF NOT EXISTS idx_file_status_repo ON file_status(repo_id);

-- Workspace / batch tasks
CREATE TABLE IF NOT EXISTS tasks (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    task_type   TEXT NOT NULL,
    status      TEXT NOT NULL,
    params_json TEXT,
    created_at  TEXT NOT NULL,
    finished_at TEXT
);

-- Per-repository sub-results of a task
CREATE TABLE IF NOT EXISTS task_items (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id     INTEGER NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    repo_path   TEXT NOT NULL,
    status      TEXT NOT NULL,
    message     TEXT,
    started_at  TEXT,
    finished_at TEXT
);

CREATE INDEX IF NOT EXISTS idx_task_items_task ON task_items(task_id);

-- DAG edges between tasks
CREATE TABLE IF NOT EXISTS task_dependencies (
    task_id       INTEGER NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    depends_on_id INTEGER NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    PRIMARY KEY (task_id, depends_on_id)
);

CREATE INDEX IF NOT EXISTS idx_task_dependencies_dep ON task_dependencies(depends_on_id);

-- Workspace change sets (cross-repository feature grouping)
CREATE TABLE IF NOT EXISTS change_sets (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    workspace_id INTEGER NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    name         TEXT NOT NULL,
    description  TEXT,
    created_at   TEXT NOT NULL,
    updated_at   TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_change_sets_workspace ON change_sets(workspace_id);

-- Repositories associated with a change set
CREATE TABLE IF NOT EXISTS change_set_repositories (
    change_set_id INTEGER NOT NULL REFERENCES change_sets(id) ON DELETE CASCADE,
    repo_id       INTEGER NOT NULL REFERENCES repositories(id) ON DELETE CASCADE,
    target_branch TEXT,
    PRIMARY KEY (change_set_id, repo_id)
);

CREATE INDEX IF NOT EXISTS idx_change_set_repos_repo ON change_set_repositories(repo_id);

-- Code symbols extracted by tree-sitter
CREATE TABLE IF NOT EXISTS symbols (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    repo_id    INTEGER NOT NULL REFERENCES repositories(id) ON DELETE CASCADE,
    file_path  TEXT NOT NULL,
    name       TEXT NOT NULL,
    kind       TEXT NOT NULL,
    line       INTEGER,
    updated_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_symbols_repo ON symbols(repo_id);
CREATE INDEX IF NOT EXISTS idx_symbols_name ON symbols(name);

-- Symbol references (named symbol_references to avoid the SQL keyword)
CREATE TABLE IF NOT EXISTS symbol_references (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    symbol_id  INTEGER NOT NULL REFERENCES symbols(id) ON DELETE CASCADE,
    file_path  TEXT NOT NULL,
    line       INTEGER,
    updated_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_symbol_references_symbol ON symbol_references(symbol_id);

-- AI review results
CREATE TABLE IF NOT EXISTS ai_reviews (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    repo_path   TEXT,
    summary     TEXT,
    issues_json TEXT,
    model       TEXT,
    created_at  TEXT NOT NULL
);

-- AI task history
CREATE TABLE IF NOT EXISTS ai_tasks (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    task_type   TEXT NOT NULL,
    repo_path   TEXT,
    status      TEXT NOT NULL,
    result_json TEXT,
    created_at  TEXT NOT NULL,
    finished_at TEXT
);
"#;

/// Version 2 — incremental scan support.
///
/// Adds two columns to `repositories`:
/// - `is_deleted`    — soft-delete flag so a moved/removed repository is marked
///   stale instead of being hard-deleted (preserves tags/group/favorites).
/// - `git_dir_mtime` — `.git` directory mtime (unix millis) recorded at the last
///   successful scan, used as the incremental-scan cache key.
///
/// `ALTER TABLE ADD COLUMN` is non-destructive and safe to apply over v1.
pub const SCHEMA_V2: &str = r#"
ALTER TABLE repositories ADD COLUMN is_deleted INTEGER NOT NULL DEFAULT 0;
ALTER TABLE repositories ADD COLUMN git_dir_mtime INTEGER;
"#;

/// Version 3 — Graph data cache: store the author timezone offset so cached
/// commit metadata renders the same time as a fresh libgit2 read.
pub const SCHEMA_V3: &str = r#"
ALTER TABLE commits ADD COLUMN author_offset INTEGER NOT NULL DEFAULT 0;
"#;

/// Version 4 — task history / crash recovery: persist a stable task UUID so
/// tasks can be located across restarts.
pub const SCHEMA_V4: &str = r#"
ALTER TABLE tasks ADD COLUMN task_uuid TEXT;
CREATE UNIQUE INDEX IF NOT EXISTS idx_tasks_uuid ON tasks(task_uuid);
"#;

/// Version 5 — T-10 Stash: reserve a workspace-level stash association
/// column (T-21 Workspace Stash will group per-repo stashes under a
/// workspace stash ref). Nullable; single-repo stashes leave it NULL.
pub const SCHEMA_V5: &str = r#"
ALTER TABLE stashes ADD COLUMN workspace_ref TEXT;
"#;

/// Ordered schema migrations. Index 0 = version 1, index 1 = version 2, ...
/// Append new entries at the END only.
pub const MIGRATIONS: &[&str] = &[SCHEMA_V1, SCHEMA_V2, SCHEMA_V3, SCHEMA_V4, SCHEMA_V5];
