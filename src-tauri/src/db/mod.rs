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
pub fn migrate(conn: &mut Connection) -> AppResult<()> {
    let current: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;

    for (i, sql) in schema::MIGRATIONS.iter().enumerate() {
        let target = (i + 1) as i64;
        if current < target {
            let tx = conn.transaction()?;
            tx.execute_batch(sql)?;
            tx.execute_batch(&format!("PRAGMA user_version = {};", target))?;
            tx.commit()?;
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
}
