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
/// v6 (T-11 §54): per-repo / per-group commit identity overrides.
pub const SCHEMA_V6: &str = r#"
ALTER TABLE repositories ADD COLUMN author_name TEXT;
ALTER TABLE repositories ADD COLUMN author_email TEXT;
ALTER TABLE repo_groups ADD COLUMN author_name TEXT;
ALTER TABLE repo_groups ADD COLUMN author_email TEXT;
"#;

/// v7 (T-21 + T-34, Phase 2 parallel work): workspace-stash association
/// records and the unified operation log for undo.
pub const SCHEMA_V7: &str = r#"
-- Named multi-repo stash group (T-21): one row per "Workspace Stash #N".
CREATE TABLE IF NOT EXISTS workspace_stashes (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    workspace_id INTEGER NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    name         TEXT NOT NULL,
    message      TEXT,
    created_at   TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_workspace_stashes_ws ON workspace_stashes(workspace_id);

-- Per-repo member of a workspace stash: links the group to each repo's
-- stash (by oid + index so restore can resolve it even after later stashes).
CREATE TABLE IF NOT EXISTS workspace_stash_items (
    id                 INTEGER PRIMARY KEY AUTOINCREMENT,
    workspace_stash_id INTEGER NOT NULL REFERENCES workspace_stashes(id) ON DELETE CASCADE,
    repo_path          TEXT NOT NULL,
    stash_oid          TEXT NOT NULL,
    stash_index        INTEGER NOT NULL,
    branch             TEXT NOT NULL,
    UNIQUE(workspace_stash_id, repo_path)
);

CREATE INDEX IF NOT EXISTS idx_ws_stash_items_group ON workspace_stash_items(workspace_stash_id);

-- Unified operation log (T-34): one batch row per high-risk operation, with
-- per-repo before/after ref snapshots as items (pure data, no libgit2
-- handles — global constraint §3).
CREATE TABLE IF NOT EXISTS operation_logs (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    workspace_id INTEGER REFERENCES workspaces(id) ON DELETE CASCADE,
    op_type      TEXT NOT NULL,
    summary      TEXT NOT NULL,
    created_at   TEXT NOT NULL,
    undone_at    TEXT
);

CREATE INDEX IF NOT EXISTS idx_operation_logs_ws ON operation_logs(workspace_id);

CREATE TABLE IF NOT EXISTS operation_log_items (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    log_id      INTEGER NOT NULL REFERENCES operation_logs(id) ON DELETE CASCADE,
    repo_path   TEXT NOT NULL,
    ref_name    TEXT NOT NULL,
    before_oid  TEXT NOT NULL,
    after_oid   TEXT,
    detail      TEXT,
    undone_at   TEXT
);

CREATE INDEX IF NOT EXISTS idx_operation_log_items_log ON operation_log_items(log_id);
"#;

/// v8 (R-02): persistent Workspace Maven Index and dependency-resolution cache.
///
/// Maven project/dependency rows are derived metadata. A POM rescan replaces
/// them transactionally, while user-owned runtime configuration remains in
/// `.gitworkspace/runtimes/*.json`; `runtime_projects` stores metadata only.
pub const SCHEMA_V8: &str = r#"
CREATE TABLE IF NOT EXISTS maven_projects (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    workspace_id    INTEGER NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    repository_id   INTEGER REFERENCES repositories(id) ON DELETE SET NULL,
    path             TEXT NOT NULL,
    group_id         TEXT NOT NULL,
    artifact_id      TEXT NOT NULL,
    version          TEXT NOT NULL,
    packaging        TEXT NOT NULL DEFAULT 'jar',
    parent_id        INTEGER REFERENCES maven_projects(id) ON DELETE SET NULL,
    pom_hash         TEXT NOT NULL,
    model_hash       TEXT NOT NULL,
    last_scanned_at  TEXT NOT NULL,
    UNIQUE(workspace_id, path)
);

CREATE INDEX IF NOT EXISTS idx_maven_projects_workspace
    ON maven_projects(workspace_id);
CREATE INDEX IF NOT EXISTS idx_maven_projects_gav
    ON maven_projects(workspace_id, group_id, artifact_id, version);
CREATE INDEX IF NOT EXISTS idx_maven_projects_repository
    ON maven_projects(repository_id);

CREATE TABLE IF NOT EXISTS maven_dependencies (
    id                  INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id          INTEGER NOT NULL REFERENCES maven_projects(id) ON DELETE CASCADE,
    group_id             TEXT NOT NULL,
    artifact_id          TEXT NOT NULL,
    version              TEXT,
    scope                TEXT NOT NULL DEFAULT 'compile',
    optional             INTEGER NOT NULL DEFAULT 0,
    dep_type             TEXT NOT NULL DEFAULT 'jar',
    classifier           TEXT,
    exclusions_json      TEXT NOT NULL DEFAULT '[]',
    source_kind          TEXT NOT NULL CHECK(source_kind IN ('workspaceSource', 'localRepository', 'remoteRepository')),
    source_project_id    INTEGER REFERENCES maven_projects(id) ON DELETE SET NULL,
    resolved_path        TEXT,
    resolution_reason    TEXT NOT NULL,
    model_hash           TEXT NOT NULL,
    sort_order           INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_maven_dependencies_project
    ON maven_dependencies(project_id, sort_order);
CREATE INDEX IF NOT EXISTS idx_maven_dependencies_coordinates
    ON maven_dependencies(group_id, artifact_id, version);
CREATE INDEX IF NOT EXISTS idx_maven_dependencies_source_project
    ON maven_dependencies(source_project_id);

CREATE TABLE IF NOT EXISTS maven_modules (
    id                  INTEGER PRIMARY KEY AUTOINCREMENT,
    parent_project_id   INTEGER NOT NULL REFERENCES maven_projects(id) ON DELETE CASCADE,
    module_project_id   INTEGER REFERENCES maven_projects(id) ON DELETE SET NULL,
    declared_path       TEXT NOT NULL,
    UNIQUE(parent_project_id, declared_path)
);

CREATE INDEX IF NOT EXISTS idx_maven_modules_parent
    ON maven_modules(parent_project_id);
CREATE INDEX IF NOT EXISTS idx_maven_modules_project
    ON maven_modules(module_project_id);

CREATE TABLE IF NOT EXISTS maven_artifacts (
    id               INTEGER PRIMARY KEY AUTOINCREMENT,
    workspace_id     INTEGER NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    group_id          TEXT NOT NULL,
    artifact_id       TEXT NOT NULL,
    version           TEXT NOT NULL,
    dep_type          TEXT NOT NULL DEFAULT 'jar',
    classifier        TEXT NOT NULL DEFAULT '',
    local_path        TEXT NOT NULL,
    exists_local      INTEGER NOT NULL DEFAULT 0,
    last_checked_at   TEXT NOT NULL,
    UNIQUE(workspace_id, group_id, artifact_id, version, dep_type, classifier)
);

CREATE INDEX IF NOT EXISTS idx_maven_artifacts_workspace
    ON maven_artifacts(workspace_id);

CREATE TABLE IF NOT EXISTS maven_source_mappings (
    id               INTEGER PRIMARY KEY AUTOINCREMENT,
    workspace_id     INTEGER NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    group_id          TEXT NOT NULL,
    artifact_id       TEXT NOT NULL,
    version           TEXT NOT NULL,
    repository_id    INTEGER REFERENCES repositories(id) ON DELETE SET NULL,
    project_id        INTEGER NOT NULL REFERENCES maven_projects(id) ON DELETE CASCADE,
    project_path      TEXT NOT NULL,
    UNIQUE(workspace_id, group_id, artifact_id, version, project_id)
);

CREATE INDEX IF NOT EXISTS idx_maven_source_mappings_gav
    ON maven_source_mappings(workspace_id, group_id, artifact_id, version);

CREATE TABLE IF NOT EXISTS runtime_projects (
    id                INTEGER PRIMARY KEY AUTOINCREMENT,
    workspace_id      INTEGER NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    name              TEXT NOT NULL,
    root_project_id   INTEGER REFERENCES maven_projects(id) ON DELETE SET NULL,
    main_class        TEXT,
    jdk               TEXT,
    profile           TEXT,
    build_engine      TEXT,
    config_path       TEXT,
    created_at        TEXT NOT NULL,
    updated_at        TEXT NOT NULL,
    UNIQUE(workspace_id, name)
);

CREATE INDEX IF NOT EXISTS idx_runtime_projects_workspace
    ON runtime_projects(workspace_id);

CREATE TABLE IF NOT EXISTS runtime_dependencies (
    id                    INTEGER PRIMARY KEY AUTOINCREMENT,
    runtime_project_id    INTEGER NOT NULL REFERENCES runtime_projects(id) ON DELETE CASCADE,
    maven_project_id      INTEGER NOT NULL REFERENCES maven_projects(id) ON DELETE CASCADE,
    dependency_project_id INTEGER REFERENCES maven_projects(id) ON DELETE SET NULL,
    scope                 TEXT NOT NULL DEFAULT 'compile',
    source_kind           TEXT NOT NULL CHECK(source_kind IN ('workspaceSource', 'localRepository', 'remoteRepository')),
    included              INTEGER NOT NULL DEFAULT 1
);

CREATE INDEX IF NOT EXISTS idx_runtime_dependencies_runtime
    ON runtime_dependencies(runtime_project_id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_runtime_dependencies_unique
    ON runtime_dependencies(
        runtime_project_id,
        maven_project_id,
        COALESCE(dependency_project_id, -1),
        scope
    );
"#;

/// v9 (R-04): JDK registry - discovered and manually-added JDK installations.
///
/// One row per JDK home (`JAVA_HOME` semantics). `home_path` is the unique key
/// (canonicalized by the detector); the registry upserts on it so rescans do
/// not duplicate. Version / vendor fields stay nullable for entries whose
/// `java -version` probe failed (kept `is_valid=false` for user re-validation).
pub const SCHEMA_V9: &str = r#"
CREATE TABLE IF NOT EXISTS jdks (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    home_path       TEXT NOT NULL UNIQUE,
    major_version   INTEGER,
    full_version    TEXT,
    vendor          TEXT,
    architecture    TEXT,
    bitness         INTEGER,
    source          TEXT NOT NULL,
    java_exec       TEXT,
    javac_exec      TEXT,
    is_valid        INTEGER NOT NULL DEFAULT 0,
    last_checked    TEXT NOT NULL,
    raw_version     TEXT,
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_jdks_valid ON jdks(is_valid);
CREATE INDEX IF NOT EXISTS idx_jdks_major ON jdks(major_version);
"#;

/// v10 (R-05): Maven executable registry - caches `mvn -v` probe results.
///
/// One row per detected Maven executable (wrapper / configured / system),
/// keyed by `executable_path`（wrapper 路径是 per-project 的 `/proj/mvnw`，
/// system / configured 路径各不相同，故单列唯一即可区分）。`project_path`
/// 是冗余信息字段（wrapper 记录所属项目目录，非 wrapper 为 NULL）。
/// 版本字段在探测失败时为 `None` + `is_valid=false`，便于用户重检。
pub const SCHEMA_V10: &str = r#"
CREATE TABLE IF NOT EXISTS maven_executables (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    executable_path TEXT NOT NULL UNIQUE,
    project_path    TEXT,
    source          TEXT NOT NULL,
    major_version   INTEGER,
    full_version    TEXT,
    is_valid        INTEGER NOT NULL DEFAULT 0,
    last_checked    TEXT NOT NULL,
    raw_version     TEXT,
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_maven_executables_valid ON maven_executables(is_valid);
CREATE INDEX IF NOT EXISTS idx_maven_executables_source ON maven_executables(source);
"#;

/// v11 (R-07): persist the user-facing Maven project selector alongside the
/// Runtime metadata index. The full Runtime configuration remains in the
/// version-controlled JSON document under `.gitworkspace/runtimes/`.
pub const SCHEMA_V11: &str = r#"
ALTER TABLE runtime_projects ADD COLUMN project TEXT NOT NULL DEFAULT '';
"#;

/// v12 (R-10): one row per launched runtime process.
///
/// The row is a *cache* of the actual OS process state (task doc: 进程状态以
/// 实际 OS 进程为准). `pid_start_time` (sysinfo `Process::start_time`) guards
/// against PID reuse when reconciling after a GitWorkspace restart. Metrics
/// columns (`cpu_percent` / `memory_bytes`) hold the latest sampled values and
/// are refreshed on a throttled cadence by the process manager.
pub const SCHEMA_V12: &str = r#"
CREATE TABLE IF NOT EXISTS runtime_processes (
    id               INTEGER PRIMARY KEY AUTOINCREMENT,
    workspace_id     INTEGER NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    runtime_name     TEXT NOT NULL,
    pid              INTEGER,
    pid_start_time   INTEGER,
    status           TEXT NOT NULL,
    run_strategy     TEXT,
    command_preview  TEXT,
    working_dir      TEXT,
    ports_json       TEXT NOT NULL DEFAULT '[]',
    exit_code        INTEGER,
    cpu_percent      REAL,
    memory_bytes     INTEGER,
    adopted          INTEGER NOT NULL DEFAULT 0,
    started_at       TEXT NOT NULL,
    stopped_at       TEXT,
    last_seen_at     TEXT,
    updated_at       TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_runtime_processes_ws
    ON runtime_processes(workspace_id, runtime_name);
CREATE INDEX IF NOT EXISTS idx_runtime_processes_status
    ON runtime_processes(status);
"#;

/// v13 (AI-01): AI Provider / Model / 任务级默认模型配置表（设计文档 §6 / §11.2）。
///
/// 只存配置元数据：API Key 一律进 OS Credential Store（§6.4），SQLite 只保存
/// `credential_ref` 引用。`ai_reviews` / `ai_tasks` 历史表保留不动（向后兼容，
/// 不做破坏性删除）。
///
/// `ai_task_defaults.workspace_id` 可空：NULL = 全局默认。SQLite 唯一约束把 NULL
/// 视为互不相等，故唯一性用 `COALESCE(workspace_id, -1)` 表达式索引保证
/// （全局每 task_kind 仅一行，Workspace 覆盖每 (task_kind, workspace_id) 仅一行）。
pub const SCHEMA_V13: &str = r#"
CREATE TABLE IF NOT EXISTS ai_providers (
    id             TEXT PRIMARY KEY,
    name           TEXT NOT NULL,
    kind           TEXT NOT NULL CHECK(kind IN ('openaiCompatible', 'ark', 'ollama', 'custom')),
    base_url       TEXT NOT NULL,
    credential_ref TEXT,
    enabled        INTEGER NOT NULL DEFAULT 1,
    network_policy TEXT NOT NULL DEFAULT 'onlineOnly' CHECK(network_policy IN ('onlineOnly', 'localOnly')),
    created_at     TEXT NOT NULL,
    updated_at     TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS ai_models (
    provider_id        TEXT NOT NULL REFERENCES ai_providers(id) ON DELETE CASCADE,
    id                 TEXT NOT NULL,
    display_name       TEXT NOT NULL,
    capabilities_json  TEXT NOT NULL DEFAULT '[]',
    max_context_tokens INTEGER NOT NULL DEFAULT 0,
    defaults_json      TEXT NOT NULL DEFAULT '{}',
    enabled            INTEGER NOT NULL DEFAULT 1,
    created_at         TEXT NOT NULL,
    updated_at         TEXT NOT NULL,
    PRIMARY KEY (provider_id, id)
);

CREATE INDEX IF NOT EXISTS idx_ai_models_provider ON ai_models(provider_id);

CREATE TABLE IF NOT EXISTS ai_task_defaults (
    task_kind    TEXT NOT NULL CHECK(task_kind IN ('chat', 'runtimeDiagnostic', 'gitReview', 'commitMessage', 'conflict')),
    workspace_id INTEGER REFERENCES workspaces(id) ON DELETE CASCADE,
    provider_id  TEXT NOT NULL,
    model_id     TEXT NOT NULL,
    updated_at   TEXT NOT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_ai_task_defaults_unique
    ON ai_task_defaults(task_kind, COALESCE(workspace_id, -1));
"#;

/// v14 (AI-02): `ai_providers.kind` 厂商枚举 → `api_type` 协议枚举
/// （设计修订 §6.1 / §21 决策 9：`openaiChatCompletions` / `openaiResponses` /
/// `anthropicMessages`，不内置厂商清单）。
///
/// SQLite 无法修改列 CHECK 约束，需重建表。存量行**一律映射为
/// `openaiChatCompletions`**（Ollama / Ark / custom 均可按 OpenAI 兼容协议
/// 配置，厂商特判逻辑随之移除；Ollama 用户需把 baseUrl 调整为带 `/v1` 的
/// 兼容端点）。本迁移由 `db::migrate` 特判在**事务外关闭 foreign_keys**
/// 执行：`ai_models` 以外键引用本表，`DROP TABLE` 的隐式 DELETE 在外键
/// 开启时会级联误删模型行。
pub const SCHEMA_V14: &str = r#"
CREATE TABLE ai_providers_v14 (
    id             TEXT PRIMARY KEY,
    name           TEXT NOT NULL,
    api_type       TEXT NOT NULL CHECK(api_type IN ('openaiChatCompletions', 'openaiResponses', 'anthropicMessages')),
    base_url       TEXT NOT NULL,
    credential_ref TEXT,
    enabled        INTEGER NOT NULL DEFAULT 1,
    network_policy TEXT NOT NULL DEFAULT 'onlineOnly' CHECK(network_policy IN ('onlineOnly', 'localOnly')),
    created_at     TEXT NOT NULL,
    updated_at     TEXT NOT NULL
);

INSERT INTO ai_providers_v14 (id, name, api_type, base_url, credential_ref, enabled, network_policy, created_at, updated_at)
SELECT id, name, 'openaiChatCompletions', base_url, credential_ref, enabled, network_policy, created_at, updated_at
FROM ai_providers;

DROP TABLE ai_providers;
ALTER TABLE ai_providers_v14 RENAME TO ai_providers;
"#;

/// v15 (AI-04，设计文档 §11.2 / §10.4 / §11.3)：AI 会话、消息、请求审计、
/// 结果缓存、Action Proposal 预留表与 AI 设置 KV。
///
/// - `ai_sessions` / `ai_messages`：完整会话持久化由用户设置控制
///   （`ai_settings` 键 `persistSessions`，**默认关闭**——§10.4「默认不保存
///   完整 Prompt 中的敏感原文」的保守取向；关闭时只写 `ai_requests` 审计）。
///   删除会话经 FK `ON DELETE CASCADE` 级联删除消息与关联缓存行（§10.4）。
/// - `ai_requests`：审计只存元数据（manifest JSON、内容 hash、Secret 计数
///   与类别、token 用量、耗时、错误 code），**不存 Prompt/结果原文**。
/// - `ai_result_cache`：行主键为组合 hash；`session_id` 仅作级联清理关联。
/// - `ai_proposals`：AI-11 只建表与类型，不实现流程。
pub const SCHEMA_V15: &str = r#"
CREATE TABLE IF NOT EXISTS ai_sessions (
    id                    TEXT PRIMARY KEY,
    title                 TEXT NOT NULL,
    role                  TEXT NOT NULL DEFAULT 'assistant',
    workspace_id          INTEGER REFERENCES workspaces(id) ON DELETE SET NULL,
    repository_scope_json TEXT NOT NULL DEFAULT '[]',
    runtime_scope_json    TEXT NOT NULL DEFAULT '{}',
    created_at            TEXT NOT NULL,
    updated_at            TEXT NOT NULL,
    archived_at           TEXT
);

CREATE INDEX IF NOT EXISTS idx_ai_sessions_listing
    ON ai_sessions(archived_at, updated_at DESC);

CREATE TABLE IF NOT EXISTS ai_messages (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id   TEXT NOT NULL REFERENCES ai_sessions(id) ON DELETE CASCADE,
    role         TEXT NOT NULL CHECK(role IN ('system', 'user', 'assistant')),
    content_json TEXT NOT NULL,
    sequence     INTEGER NOT NULL,
    created_at   TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_ai_messages_session ON ai_messages(session_id, sequence);

CREATE TABLE IF NOT EXISTS ai_requests (
    id                    TEXT PRIMARY KEY,
    session_id            TEXT REFERENCES ai_sessions(id) ON DELETE SET NULL,
    task_kind             TEXT NOT NULL,
    provider_id           TEXT NOT NULL,
    model_id              TEXT NOT NULL,
    input_hash            TEXT NOT NULL,
    context_manifest_json TEXT NOT NULL DEFAULT '[]',
    status                TEXT NOT NULL,
    error_code            TEXT,
    secret_counts_json    TEXT NOT NULL DEFAULT '{}',
    input_tokens          INTEGER,
    output_tokens         INTEGER,
    latency_ms            INTEGER,
    created_at            TEXT NOT NULL,
    finished_at           TEXT
);

CREATE INDEX IF NOT EXISTS idx_ai_requests_created ON ai_requests(created_at DESC);

CREATE TABLE IF NOT EXISTS ai_result_cache (
    cache_key      TEXT PRIMARY KEY,
    task_kind      TEXT NOT NULL,
    provider_id    TEXT NOT NULL,
    model_id       TEXT NOT NULL,
    prompt_version TEXT NOT NULL,
    context_hash   TEXT NOT NULL,
    settings_hash  TEXT NOT NULL,
    result_json    TEXT NOT NULL,
    session_id     TEXT REFERENCES ai_sessions(id) ON DELETE CASCADE,
    created_at     TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_ai_result_cache_created ON ai_result_cache(created_at);

CREATE TABLE IF NOT EXISTS ai_proposals (
    id                TEXT PRIMARY KEY,
    request_id        TEXT,
    action_kind       TEXT NOT NULL,
    risk_level        TEXT NOT NULL,
    target_scope_json TEXT NOT NULL DEFAULT '{}',
    affected_repositories_json TEXT NOT NULL DEFAULT '[]',
    affected_files_json TEXT NOT NULL DEFAULT '[]',
    before_summary    TEXT NOT NULL DEFAULT '',
    after_summary     TEXT NOT NULL DEFAULT '',
    diff_json         TEXT,
    command_preview   TEXT,
    reversible        INTEGER NOT NULL DEFAULT 0,
    expires_at        TEXT NOT NULL DEFAULT '',
    status            TEXT NOT NULL,
    confirmed_at      TEXT,
    executed_task_id  TEXT,
    action_payload_json TEXT NOT NULL DEFAULT '{}',
    created_at        TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS ai_settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
"#;

/// v16 (AI-11)：扩展 Action Proposal DTO 与执行关联字段。
/// Existing v15 installations are rebuilt losslessly because SQLite cannot
/// change column types in place; no foreign keys reference this table.
pub const SCHEMA_V16: &str = r#"
ALTER TABLE ai_proposals RENAME TO ai_proposals_v15;
CREATE TABLE ai_proposals (
    id                TEXT PRIMARY KEY,
    request_id        TEXT,
    action_kind       TEXT NOT NULL,
    risk_level        TEXT NOT NULL,
    target_scope_json TEXT NOT NULL DEFAULT '{}',
    affected_repositories_json TEXT NOT NULL DEFAULT '[]',
    affected_files_json TEXT NOT NULL DEFAULT '[]',
    before_summary    TEXT NOT NULL DEFAULT '',
    after_summary     TEXT NOT NULL DEFAULT '',
    diff_json         TEXT,
    command_preview   TEXT,
    reversible        INTEGER NOT NULL DEFAULT 0,
    expires_at        TEXT NOT NULL DEFAULT '',
    status            TEXT NOT NULL,
    confirmed_at      TEXT,
    executed_task_id  TEXT,
    action_payload_json TEXT NOT NULL DEFAULT '{}',
    created_at        TEXT NOT NULL
);
INSERT INTO ai_proposals
    (id, request_id, action_kind, risk_level, target_scope_json, diff_json,
     status, confirmed_at, executed_task_id, created_at)
SELECT id, request_id, action_kind, risk_level, target_scope_json, diff_json,
       status, confirmed_at, CAST(executed_task_id AS TEXT), created_at
FROM ai_proposals_v15;
DROP TABLE ai_proposals_v15;
"#;

/// v17 (N-02)：workspace Node.js `package.json` metadata index.
pub const SCHEMA_V17: &str = r#"
CREATE TABLE IF NOT EXISTS node_projects (
    id               INTEGER PRIMARY KEY AUTOINCREMENT,
    workspace_id     INTEGER NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    repository_id    INTEGER REFERENCES repositories(id) ON DELETE SET NULL,
    path             TEXT NOT NULL,
    name             TEXT NOT NULL,
    version          TEXT NOT NULL DEFAULT '',
    package_manager  TEXT,
    scripts_json     TEXT NOT NULL,
    pkg_hash         TEXT NOT NULL,
    last_scanned_at  TEXT NOT NULL,
    UNIQUE(workspace_id, path)
);
CREATE INDEX IF NOT EXISTS idx_node_projects_workspace
    ON node_projects(workspace_id);
"#;

/// v18 (N-03)：Runtime 配置技术栈标记，历史行默认 Spring Boot。
pub const SCHEMA_V18: &str = r#"
ALTER TABLE runtime_projects
    ADD COLUMN kind TEXT NOT NULL DEFAULT 'springBoot';
"#;

/// v19 (N-08)：user-registered Node.js / package-manager executables.
pub const SCHEMA_V19: &str = r#"
CREATE TABLE IF NOT EXISTS node_executables (
    id               INTEGER PRIMARY KEY AUTOINCREMENT,
    kind             TEXT NOT NULL,
    package_manager  TEXT,
    executable_path  TEXT NOT NULL UNIQUE,
    version          TEXT,
    raw_output       TEXT NOT NULL DEFAULT '',
    is_valid         INTEGER NOT NULL DEFAULT 0,
    last_checked     TEXT NOT NULL DEFAULT '',
    created_at       TEXT NOT NULL,
    updated_at       TEXT NOT NULL,
    CHECK (
        (kind = 'node' AND package_manager IS NULL)
        OR (kind = 'packageManager' AND package_manager IS NOT NULL)
    )
);
CREATE INDEX IF NOT EXISTS idx_node_executables_kind
    ON node_executables(kind, package_manager, is_valid);
"#;

pub const SCHEMA_V20: &str = r#"
-- T-28 Tree-sitter Symbol Index：扩列 + 按名引用表 + 增量文件表。
-- symbols 扩列（ALTER 逐条，SQLite 不支持多列一条 ADD）。
ALTER TABLE symbols ADD COLUMN end_line INTEGER;
ALTER TABLE symbols ADD COLUMN container TEXT;
ALTER TABLE symbols ADD COLUMN signature TEXT;
CREATE INDEX IF NOT EXISTS idx_symbols_repo_file ON symbols(repo_id, file_path);

-- 按名引用（不依赖符号定义存在；is_call 区分调用点与普通引用）。
-- T-03 的 symbol_references 走 symbol_id 外键，无法表达「无定义的引用」，
-- T-28 引入 name 基存储；旧表保留不动。
CREATE TABLE IF NOT EXISTS symbol_refs (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    repo_id    INTEGER NOT NULL REFERENCES repositories(id) ON DELETE CASCADE,
    name       TEXT NOT NULL,
    file_path  TEXT NOT NULL,
    line       INTEGER,
    is_call    INTEGER NOT NULL DEFAULT 0,
    updated_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_symbol_refs_repo_name ON symbol_refs(repo_id, name);
CREATE INDEX IF NOT EXISTS idx_symbol_refs_repo_file ON symbol_refs(repo_id, file_path);

-- 每文件内容 hash：增量重建只重解析变更文件。
CREATE TABLE IF NOT EXISTS symbol_index_files (
    repo_id      INTEGER NOT NULL REFERENCES repositories(id) ON DELETE CASCADE,
    file_path    TEXT NOT NULL,
    content_hash TEXT NOT NULL,
    updated_at   TEXT NOT NULL,
    PRIMARY KEY (repo_id, file_path)
);
"#;

pub const SCHEMA_V21: &str = r#"
-- T-32 Automation Platform：脚本动作 + 定时任务。
CREATE TABLE IF NOT EXISTS plugin_actions (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    name         TEXT NOT NULL UNIQUE,
    command      TEXT NOT NULL,
    scope        TEXT NOT NULL DEFAULT 'repo',
    timeout_secs INTEGER NOT NULL DEFAULT 120,
    created_at   TEXT NOT NULL,
    updated_at   TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS scheduled_tasks (
    id               INTEGER PRIMARY KEY AUTOINCREMENT,
    name             TEXT NOT NULL UNIQUE,
    kind             TEXT NOT NULL,
    target_id        TEXT NOT NULL,
    schedule_kind    TEXT NOT NULL,
    interval_minutes INTEGER,
    daily_time       TEXT,
    payload          TEXT,
    enabled          INTEGER NOT NULL DEFAULT 1,
    last_run         TEXT,
    next_run         TEXT NOT NULL,
    created_at       TEXT NOT NULL,
    updated_at       TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_scheduled_due ON scheduled_tasks(enabled, next_run);
"#;

/// v22 (F-34)：端口归属确权结果。`ports_json` 的端口可能来自日志文本
/// 引用（vite proxy 目标 / API base URL），v22 起以 OS 监听表核对后的
/// `port_pids_json`（端口 → 树内监听 PID 映射）为准展示；旧行默认空。
pub const SCHEMA_V22: &str = r#"
ALTER TABLE runtime_processes ADD COLUMN port_pids_json TEXT NOT NULL DEFAULT '{}';
"#;

pub const MIGRATIONS: &[&str] = &[
    SCHEMA_V1, SCHEMA_V2, SCHEMA_V3, SCHEMA_V4, SCHEMA_V5, SCHEMA_V6, SCHEMA_V7, SCHEMA_V8, SCHEMA_V9, SCHEMA_V10,
    SCHEMA_V11, SCHEMA_V12, SCHEMA_V13, SCHEMA_V14, SCHEMA_V15, SCHEMA_V16, SCHEMA_V17, SCHEMA_V18, SCHEMA_V19,
    SCHEMA_V20, SCHEMA_V21, SCHEMA_V22,
];
