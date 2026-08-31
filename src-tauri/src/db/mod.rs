pub mod dao;
pub mod schema;

use rusqlite::Connection;

use crate::error::AppResult;

/// Apply connection-level and database-level PRAGMAs.
///
/// - `journal_mode=WAL`   — persistent (DB-file level), enables crash-safe WAL.
/// - `foreign_keys=ON`    — per-connection, must be set on every connection.
/// - `busy_timeout=5000`  — per-connection, waits up to 5s instead of `SQLITE_BUSY`.
/// - `synchronous=NORMAL` — per-connection, balances durability vs. performance.
pub fn apply_pragmas(conn: &Connection) -> AppResult<()> {
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA foreign_keys = ON;
         PRAGMA busy_timeout = 5000;
         PRAGMA synchronous = NORMAL;",
    )?;
    Ok(())
}

/// Run versioned migrations tracked by `PRAGMA user_version`.
///
/// Each entry in `schema::MIGRATIONS` bumps the version by one. A migration is
/// applied atomically inside a transaction: the SQL and the `user_version`
/// bump commit together, so a crash mid-migration leaves the DB at the old
/// version and the migration is retried on next startup.
///
/// FK-sensitive migrations (v14) are the exception: they rebuild a table that
/// other tables reference, and `DROP TABLE` would cascade-delete child rows
/// under an active foreign-key check. `PRAGMA foreign_keys` is a no-op inside
/// a transaction, so those steps run outside the transaction with foreign
/// keys temporarily disabled, followed by an integrity check.
const FK_SENSITIVE_VERSION: i64 = 14;

pub fn migrate(conn: &mut Connection) -> AppResult<()> {
    let current: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;

    for (i, sql) in schema::MIGRATIONS.iter().enumerate() {
        let target = (i + 1) as i64;
        if current < target {
            if target == FK_SENSITIVE_VERSION {
                conn.execute_batch("PRAGMA foreign_keys = OFF;")?;
                conn.execute_batch(sql)?;
                conn.execute_batch(&format!("PRAGMA user_version = {};", target))?;
                let violations: i64 = conn.query_row(
                    "SELECT COUNT(*) FROM pragma_foreign_key_check",
                    [],
                    |row| row.get(0),
                )?;
                conn.execute_batch("PRAGMA foreign_keys = ON;")?;
                if violations > 0 {
                    return Err(crate::error::AppError::Other(format!(
                        "migration v14 left {} dangling foreign keys",
                        violations
                    )));
                }
            } else {
                let tx = conn.transaction()?;
                tx.execute_batch(sql)?;
                tx.execute_batch(&format!("PRAGMA user_version = {};", target))?;
                tx.commit()?;
            }
            log::info!("Applied schema migration -> version {}", target);
        }
    }
    Ok(())
}

/// Initialize the database: apply PRAGMAs, then run migrations.
/// Called once at application startup.
pub fn init_db(conn: &mut Connection) -> AppResult<()> {
    apply_pragmas(conn)?;
    migrate(conn)?;
    log::info!("Database schema initialized");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::repository::{CommitRecord, ScannedRepo};

    fn open_memory() -> rusqlite::Connection {
        rusqlite::Connection::open_in_memory().unwrap()
    }

    /// Apply v1 over an empty DB and assert the full Roadmap §41 table set exists.
    #[test]
    fn migrate_creates_full_schema_and_bumps_version() {
        let mut conn = open_memory();
        init_db(&mut conn).unwrap();

        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, schema::MIGRATIONS.len() as i64);

        for table in [
            "workspaces",
            "repositories",
            "repo_groups",
            "task_history",
            "branches",
            "remote_branches",
            "tags",
            "commits",
            "commit_parents",
            "commit_files",
            "stashes",
            "worktrees",
            "repo_status",
            "file_status",
            "tasks",
            "task_items",
            "task_dependencies",
            "change_sets",
            "change_set_repositories",
            "symbols",
            "symbol_references",
            "ai_reviews",
            "ai_tasks",
            "maven_projects",
            "node_projects",
            "maven_dependencies",
            "maven_modules",
            "maven_artifacts",
            "maven_source_mappings",
            "runtime_projects",
            "runtime_dependencies",
            "jdks",
            "runtime_processes",
            "ai_providers",
            "ai_models",
            "ai_task_defaults",
            // v15（AI-04 §11.2）：会话 / 消息 / 请求审计 / 结果缓存 / 提案预留
            "ai_sessions",
            "ai_messages",
            "ai_requests",
            "ai_result_cache",
            "ai_proposals",
            "ai_settings",
        ] {
            let count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
                    rusqlite::params![table],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(count, 1, "table '{}' missing", table);
        }
    }

    /// Running init twice must be a no-op (idempotent migrations).
    #[test]
    fn migration_is_idempotent() {
        let mut conn = open_memory();
        init_db(&mut conn).unwrap();
        init_db(&mut conn).unwrap();

        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, schema::MIGRATIONS.len() as i64);
    }

    #[test]
    fn runtime_kind_migration_defaults_existing_rows_to_spring_boot() {
        let mut conn = open_memory();
        init_db(&mut conn).unwrap();
        conn.execute(
            "INSERT INTO workspaces (name, path, created_at, updated_at) VALUES ('w', '/tmp/w', 't', 't')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO runtime_projects (workspace_id, name, project, config_path, created_at, updated_at)
             VALUES (1, 'boot', 'repo', '', 't', 't')",
            [],
        )
        .unwrap();
        let kind: String = conn
            .query_row(
                "SELECT kind FROM runtime_projects WHERE name='boot'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(kind, "springBoot");
        init_db(&mut conn).unwrap();
    }

    /// A pre-existing (v0) DB with the original tables must be upgraded
    /// losslessly: data survives, new tables appear, version bumps to 1.
    #[test]
    fn upgrade_preserves_existing_data() {
        let mut conn = open_memory();
        conn.execute_batch(
            "CREATE TABLE workspaces (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                path TEXT NOT NULL UNIQUE,
                scan_depth INTEGER DEFAULT 5,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            INSERT INTO workspaces (name, path, created_at, updated_at)
                VALUES ('legacy', 'D:/legacy', 't', 't');",
        )
        .unwrap();

        init_db(&mut conn).unwrap();

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM workspaces", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1, "existing data must survive migration");

        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, schema::MIGRATIONS.len() as i64);

        let has_branches: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='branches'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(has_branches, 1, "new table must be created during upgrade");
    }

    /// Batch upsert must be transactional and idempotent (upsert by path).
    #[test]
    fn batch_upsert_is_transactional_and_idempotent() {
        let mut conn = open_memory();
        init_db(&mut conn).unwrap();

        conn.execute(
            "INSERT INTO workspaces (name, path, created_at, updated_at) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params!["w", "D:/w", "t", "t"],
        )
        .unwrap();
        let ws_id = conn.last_insert_rowid();

        let repos = vec![
            ScannedRepo {
                path: "D:/w/a".into(),
                name: "a".into(),
                relative_path: "a".into(),
                git_dir_mtime: None,
            },
            ScannedRepo {
                path: "D:/w/b".into(),
                name: "b".into(),
                relative_path: "b".into(),
                git_dir_mtime: None,
            },
        ];

        dao::upsert_repositories_batch(&mut conn, ws_id, &repos).unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM repositories WHERE workspace_id = ?1",
                rusqlite::params![ws_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 2);

        // Upsert the same set again — still 2 rows (upsert by path, no duplicates).
        dao::upsert_repositories_batch(&mut conn, ws_id, &repos).unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM repositories WHERE workspace_id = ?1",
                rusqlite::params![ws_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 2);
    }

    /// A removed repository is soft-deleted (kept in the table, hidden from
    /// listing) and revived when a later scan finds it again.
    #[test]
    fn cleanup_soft_deletes_and_upsert_revives() {
        let mut conn = open_memory();
        init_db(&mut conn).unwrap();

        conn.execute(
            "INSERT INTO workspaces (name, path, created_at, updated_at) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params!["w", "D:/w", "t", "t"],
        )
        .unwrap();
        let ws_id = conn.last_insert_rowid();

        let repos = vec![
            ScannedRepo {
                path: "D:/w/a".into(),
                name: "a".into(),
                relative_path: "a".into(),
                git_dir_mtime: Some(1),
            },
            ScannedRepo {
                path: "D:/w/b".into(),
                name: "b".into(),
                relative_path: "b".into(),
                git_dir_mtime: Some(2),
            },
        ];
        dao::upsert_repositories_batch(&mut conn, ws_id, &repos).unwrap();

        // "b" was moved away — only "a" still exists on disk.
        dao::cleanup_stale_repositories(&conn, ws_id, &["D:/w/a".to_string()])
            .unwrap();

        let listed = dao::list_repositories_by_workspace(&conn, ws_id).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].path, "D:/w/a");

        // The stale row is soft-deleted, not hard-deleted.
        let deleted: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM repositories WHERE workspace_id = ?1 AND is_deleted = 1",
                rusqlite::params![ws_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(deleted, 1);

        // A rescan that finds "b" again revives the same row.
        dao::upsert_repositories_batch(&mut conn, ws_id, &repos).unwrap();
        let listed = dao::list_repositories_by_workspace(&conn, ws_id).unwrap();
        assert_eq!(listed.len(), 2);
    }

    /// `list_repository_paths` exposes the incremental-scan cache (path → mtime).
    #[test]
    fn list_repository_paths_returns_mtime_cache() {
        let mut conn = open_memory();
        init_db(&mut conn).unwrap();

        conn.execute(
            "INSERT INTO workspaces (name, path, created_at, updated_at) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params!["w", "D:/w", "t", "t"],
        )
        .unwrap();
        let ws_id = conn.last_insert_rowid();

        let repos = vec![ScannedRepo {
            path: "D:/w/a".into(),
            name: "a".into(),
            relative_path: "a".into(),
            git_dir_mtime: Some(1234),
        }];
        dao::upsert_repositories_batch(&mut conn, ws_id, &repos).unwrap();

        let paths = dao::list_repository_paths(&conn, ws_id).unwrap();
        assert_eq!(paths.get("D:/w/a"), Some(&Some(1234)));
    }

    /// On a file-backed DB, PRAGMAs must actually switch journal_mode to WAL.
    #[test]
    fn apply_pragmas_sets_wal_on_file_db() {
        let path = std::env::temp_dir().join(format!(
            "gw_wal_test_{}.db",
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        let conn = rusqlite::Connection::open(&path).unwrap();

        apply_pragmas(&conn).unwrap();

        let mode: String = conn
            .query_row("PRAGMA journal_mode", [], |r| r.get(0))
            .unwrap();
        assert_eq!(mode.to_lowercase(), "wal");

        drop(conn);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(path.with_extension("db-wal"));
        let _ = std::fs::remove_file(path.with_extension("db-shm"));
    }

    /// Commit metadata upsert must be idempotent and round-trip through the cache.
    #[test]
    fn commit_cache_upsert_and_read_roundtrip() {
        let mut conn = open_memory();
        init_db(&mut conn).unwrap();

        conn.execute(
            "INSERT INTO workspaces (name, path, created_at, updated_at) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params!["w", "D:/w", "t", "t"],
        )
        .unwrap();
        let ws_id = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO repositories (workspace_id, path, name, relative_path, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
            rusqlite::params![ws_id, "D:/w/r", "r", "r", "t"],
        )
        .unwrap();
        let repo_id = conn.last_insert_rowid();

        let commits = vec![
            CommitRecord {
                oid: "a".into(),
                message: "msg a".into(),
                author: "Alice <a@x>".into(),
                committer: "Alice <a@x>".into(),
                authored_at: 1,
                committed_at: 1,
                offset_minutes: 480,
                parents: vec![],
            },
            CommitRecord {
                oid: "b".into(),
                message: "msg b".into(),
                author: "Bob <b@x>".into(),
                committer: "Bob <b@x>".into(),
                authored_at: 2,
                committed_at: 2,
                offset_minutes: 480,
                parents: vec!["a".into()],
            },
        ];
        dao::upsert_commits_batch(&mut conn, repo_id, &commits).unwrap();

        let rec = dao::get_commit_record(&conn, repo_id, "b").unwrap().unwrap();
        assert_eq!(rec.message, "msg b");
        assert_eq!(rec.author, "Bob <b@x>");
        assert_eq!(rec.committer, "Bob <b@x>");
        assert_eq!(rec.offset_minutes, 480);
        assert_eq!(rec.parents, vec!["a".to_string()]);

        // Upsert again — still 2 rows (idempotent by repo_id + oid).
        dao::upsert_commits_batch(&mut conn, repo_id, &commits).unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM commits WHERE repo_id = ?1",
                rusqlite::params![repo_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 2);
    }

    /// Task persistence: new tasks are recorded, final status updates land, and
    /// unfinished tasks are marked interrupted on restart.
    #[test]
    fn task_persistence_and_crash_recovery() {
        let mut conn = open_memory();
        init_db(&mut conn).unwrap();

        dao::insert_task_record(&conn, "t1", "{}", "queued", "{}", "t").unwrap();
        dao::insert_task_record(&conn, "t2", "{}", "running", "{}", "t").unwrap();
        dao::insert_task_record(&conn, "t3", "{}", "queued", "{}", "t").unwrap();
        dao::update_task_status(&conn, "t3", "success", Some("t")).unwrap();

        // Restart: unfinished (queued/running) tasks are marked interrupted.
        let n = dao::mark_interrupted_tasks(&conn, "now").unwrap();
        assert_eq!(n, 2, "t1 (queued) + t2 (running) must be interrupted");

        let t1_status: String = conn
            .query_row(
                "SELECT status FROM tasks WHERE task_uuid = 't1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(t1_status, "interrupted");

        let t3_status: String = conn
            .query_row(
                "SELECT status FROM tasks WHERE task_uuid = 't3'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(t3_status, "success", "finished task must be untouched");
    }
    /// Commit identity resolution (T-11 §54): repo override wins over the
    /// group override; clearing the repo override falls back to the group;
    /// repos outside the group see nothing.
    #[test]
    fn commit_identity_resolution_prefers_repo_then_group() {
        let mut conn = open_memory();
        init_db(&mut conn).unwrap();

        conn.execute(
            "INSERT INTO workspaces (name, path, created_at, updated_at) VALUES ('w', 'D:/w', 't', 't')",
            [],
        )
        .unwrap();
        let ws_id = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO repo_groups (workspace_id, name) VALUES (?1, 'g')",
            [ws_id],
        )
        .unwrap();
        let gid = conn.last_insert_rowid();

        let repos = vec![
            ScannedRepo { path: "D:/w/a".into(), name: "a".into(), relative_path: "a".into(), git_dir_mtime: None },
            ScannedRepo { path: "D:/w/b".into(), name: "b".into(), relative_path: "b".into(), git_dir_mtime: None },
        ];
        dao::upsert_repositories_batch(&mut conn, ws_id, &repos).unwrap();
        conn.execute(
            "UPDATE repositories SET group_id = ?1 WHERE path = 'D:/w/a'",
            [gid],
        )
        .unwrap();

        // Nothing configured -> None (git default is used by the caller).
        assert!(dao::resolve_commit_identity(&conn, "D:/w/a").unwrap().is_none());

        // Group identity applies to the member repo.
        dao::set_group_identity(&conn, gid, Some("Group Bot"), Some("g@x.c")).unwrap();
        let id = dao::resolve_commit_identity(&conn, "D:/w/a").unwrap().unwrap();
        assert_eq!(id.name, "Group Bot");
        assert_eq!(id.source, "group");

        // Repo identity wins over the group identity.
        dao::set_repo_identity(&conn, "D:/w/a", Some("Repo Bot"), Some("r@x.c")).unwrap();
        let id = dao::resolve_commit_identity(&conn, "D:/w/a").unwrap().unwrap();
        assert_eq!(id.name, "Repo Bot");
        assert_eq!(id.source, "repo");

        // A repo outside the group is unaffected by the group config.
        assert!(dao::resolve_commit_identity(&conn, "D:/w/b").unwrap().is_none());

        // Clearing the repo override falls back to the group.
        dao::set_repo_identity(&conn, "D:/w/a", None, None).unwrap();
        let id = dao::resolve_commit_identity(&conn, "D:/w/a").unwrap().unwrap();
        assert_eq!(id.source, "group");
    }

    /// v15 (AI-04 §11.2 / §10.4): AI 表必须可在含 `ai_reviews` / `ai_tasks`
    /// 存量数据的库上创建，且删除会话级联清理消息与缓存。
    #[test]
    fn v15_creates_ai_tables_and_cascades_session_delete() {
        let mut conn = open_memory();
        init_db(&mut conn).unwrap();

        // 存量原型数据不受影响（向后兼容，不做破坏性删除）。
        conn.execute(
            "INSERT INTO ai_reviews (repo_path, summary, issues_json, model, created_at)
             VALUES ('D:/w/r', 'legacy', '[]', 'gpt', 't')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO ai_sessions (id, title, created_at, updated_at)
             VALUES ('s1', '会话', 't', 't')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO ai_messages (session_id, role, content_json, sequence, created_at)
             VALUES ('s1', 'user', '{}', 0, 't')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO ai_result_cache (cache_key, task_kind, provider_id, model_id, prompt_version, context_hash, settings_hash, result_json, session_id, created_at)
             VALUES ('k1', 'chat', 'p1', 'm1', '1', 'c1', 's1', '{}', 's1', 't')",
            [],
        )
        .unwrap();

        conn.execute("DELETE FROM ai_sessions WHERE id = 's1'", [])
            .unwrap();

        let messages: i64 = conn
            .query_row("SELECT COUNT(*) FROM ai_messages", [], |r| r.get(0))
            .unwrap();
        let cached: i64 = conn
            .query_row("SELECT COUNT(*) FROM ai_result_cache", [], |r| r.get(0))
            .unwrap();
        let reviews: i64 = conn
            .query_row("SELECT COUNT(*) FROM ai_reviews", [], |r| r.get(0))
            .unwrap();
        assert_eq!(messages, 0, "删除会话必须级联删除消息（§10.4）");
        assert_eq!(cached, 0, "删除会话必须级联删除关联缓存（§8 / §10.4）");
        assert_eq!(reviews, 1, "ai_reviews 历史数据必须保留");
    }

    /// v14 (AI-02): `kind` → `api_type` table rebuild must map every legacy
    /// vendor kind to `openaiChatCompletions` and must NOT cascade-delete
    /// `ai_models` rows (the table is rebuilt under a disabled foreign-key
    /// check precisely for that reason).
    #[test]
    fn v14_maps_kind_to_api_type_and_preserves_models() {
        let mut conn = open_memory();
        // Build schema only up to v13, then seed legacy-shaped rows.
        for sql in &schema::MIGRATIONS[..13] {
            conn.execute_batch(sql).unwrap();
        }
        conn.execute_batch("PRAGMA user_version = 13;").unwrap();
        apply_pragmas(&conn).unwrap();

        conn.execute(
            "INSERT INTO ai_providers (id, name, kind, base_url, credential_ref, enabled, network_policy, created_at, updated_at)
             VALUES ('p1', 'Local Ollama', 'ollama', 'http://localhost:11434', 'ai-provider:p1', 1, 'localOnly', 't', 't')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO ai_models (provider_id, id, display_name, capabilities_json, max_context_tokens, defaults_json, enabled, created_at, updated_at)
             VALUES ('p1', 'llama3', 'Llama 3', '[\"chat\"]', 8192, '{}', 1, 't', 't')",
            [],
        )
        .unwrap();

        migrate(&mut conn).unwrap();

        let (api_type, base_url): (String, String) = conn
            .query_row(
                "SELECT api_type, base_url FROM ai_providers WHERE id = 'p1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(
            api_type, "openaiChatCompletions",
            "存量行一律映射为 openaiChatCompletions"
        );
        assert_eq!(base_url, "http://localhost:11434");

        let models: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM ai_models WHERE provider_id = 'p1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(models, 1, "重建表不得级联删除 ai_models 行");

        // The new CHECK constraint rejects vendor-kind values outside §6.1.
        let bad = conn.execute(
            "INSERT INTO ai_providers (id, name, api_type, base_url, enabled, network_policy, created_at, updated_at)
             VALUES ('p2', 'x', 'ark', 'https://x', 1, 'onlineOnly', 't', 't')",
            [],
        );
        assert!(bad.is_err(), "厂商枚举值必须被新 CHECK 约束拒绝");

        // Foreign keys must be re-enabled after the FK-sensitive migration.
        let fk: i64 = conn
            .query_row("PRAGMA foreign_keys", [], |r| r.get(0))
            .unwrap();
        assert_eq!(fk, 1);
    }
}
