/// SQL schema for GitWorkspace database.
/// All tables use `CREATE TABLE IF NOT EXISTS` for idempotent initialization.

pub const CREATE_TABLES: &str = r#"
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
-- Uses FTS5 virtual table for fast text matching
CREATE VIRTUAL TABLE IF NOT EXISTS code_index USING fts5(
    content,
    repo_path,
    file_path,
    tokenize = 'unicode61'
);
"#;
