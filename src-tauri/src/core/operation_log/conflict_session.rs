//! GF-16: one operation log per conflict-resolution session.
//!
//! Resolving a merge / rebase / cherry-pick conflict is a sequence of
//! per-file `resolve_conflict` calls. Logging each one individually flooded
//! the Operation Log with dozens of same-source rows for one user action
//! ("solve the conflicts"). A session is identified by the git operation
//! driving it — the MERGE_HEAD / CHERRY_PICK_HEAD / REVERT_HEAD oid, or the
//! rebase's original head — and all resolves of one session share one log
//! row whose items accumulate the file paths.
//!
//! The session registry is process memory: a mid-session app restart starts
//! a fresh log on the next resolve (the earlier row simply stays closed).
//! Completing the driving operation (continue / abort / skip) closes the
//! session so a re-run of the same operation — same key, e.g. merging the
//! same branch again — cannot append to the previous row.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use rusqlite::Connection;

use crate::error::AppResult;

use super::model::{NewOperationLogItem, OP_CONFLICT_RESOLUTION};
use super::record::{insert_operation_log, resolve_workspace_id};

/// (repo_path, session_key) → open log id.
type SessionKey = (String, String);

fn registry() -> &'static Mutex<HashMap<SessionKey, i64>> {
    static REGISTRY: OnceLock<Mutex<HashMap<SessionKey, i64>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Paths recorded in `path:<file>` details, in insertion order.
fn resolved_paths(items: &[NewOperationLogItem]) -> Vec<String> {
    items
        .iter()
        .filter_map(|it| it.detail.as_deref().and_then(|d| d.strip_prefix("path:")).map(String::from))
        .collect()
}

/// One-line summary: `解决 N 个文件冲突：a、b、c…` (first five paths).
fn session_summary(paths: &[String]) -> String {
    const MAX_SHOWN: usize = 5;
    let mut s = format!("解决 {} 个文件冲突", paths.len());
    if !paths.is_empty() {
        s.push('：');
        s.push_str(&paths.iter().take(MAX_SHOWN).cloned().collect::<Vec<_>>().join("、"));
        if paths.len() > MAX_SHOWN {
            s.push_str(&format!(" 等 {} 个", paths.len()));
        }
    }
    s
}

/// Whether the log row is still open (not fully undone).
fn log_is_open(conn: &Connection, log_id: i64) -> bool {
    conn.query_row(
        "SELECT undone_at IS NULL FROM operation_logs WHERE id = ?1",
        rusqlite::params![log_id],
        |r| r.get::<_, bool>(0),
    )
    .unwrap_or(false)
}

/// Append items to an open session log and refresh its summary from the
/// accumulated file list. One transaction (single-writer model).
fn append_session_items(conn: &mut Connection, log_id: i64, items: &[NewOperationLogItem]) -> AppResult<()> {
    use rusqlite::params;
    let tx = conn.transaction()?;
    {
        let mut stmt = tx.prepare(
            "INSERT INTO operation_log_items (log_id, repo_path, ref_name, before_oid, after_oid, detail)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        )?;
        for it in items {
            stmt.execute(params![
                log_id,
                it.repo_path,
                it.ref_name,
                it.before_oid,
                it.after_oid,
                it.detail
            ])?;
        }
    }
    let paths: Vec<String> = {
        let mut stmt = tx.prepare("SELECT detail FROM operation_log_items WHERE log_id = ?1 ORDER BY id")?;
        let rows = stmt
            .query_map(params![log_id], |row| row.get::<_, Option<String>>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        rows.iter()
            .filter_map(|d| d.as_deref().and_then(|d| d.strip_prefix("path:")).map(String::from))
            .collect()
    };
    tx.execute(
        "UPDATE operation_logs SET summary = ?1 WHERE id = ?2 AND undone_at IS NULL",
        params![session_summary(&paths), log_id],
    )?;
    tx.commit()?;
    Ok(())
}

/// Record one resolved file of a conflict session (best-effort: the git
/// operation already ran, a logging failure must not surface as an error).
///
/// `session` is the key from [`crate::core::conflict::session_key`]; `None`
/// (no driving operation on record) falls back to a standalone per-resolve
/// log so manually staged conflict markers are still traceable.
pub fn record_conflict_resolution(
    db: &Arc<Mutex<Connection>>,
    repo_path: &str,
    session: Option<&str>,
    items: Vec<NewOperationLogItem>,
) {
    if items.is_empty() {
        return;
    }
    let Some(session) = session else {
        // `record_operation_log` (not the best_effort router) — the router
        // would session-key again and recurse.
        let summary = session_summary(&resolved_paths(&items));
        super::record_operation_log(db, repo_path, OP_CONFLICT_RESOLUTION, &summary, items);
        return;
    };
    let Ok(mut conn) = db.lock() else {
        log::warn!("T-34: operation log DB lock failed");
        return;
    };
    let key: SessionKey = (repo_path.to_string(), session.to_string());
    let open = registry().lock().ok().and_then(|m| m.get(&key).copied()).filter(|id| log_is_open(&conn, *id));
    match open {
        Some(log_id) => {
            if let Err(e) = append_session_items(&mut conn, log_id, &items) {
                log::warn!("T-34: conflict session append failed: {}", e);
            }
        }
        None => {
            let summary = session_summary(&resolved_paths(&items));
            let workspace_id = resolve_workspace_id(&conn, repo_path);
            match insert_operation_log(&mut conn, workspace_id, OP_CONFLICT_RESOLUTION, &summary, &items) {
                Ok(log_id) => {
                    if let Ok(mut m) = registry().lock() {
                        m.insert(key, log_id);
                    }
                }
                Err(e) => log::warn!("T-34: operation log write failed (op already ran): {}", e),
            }
        }
    }
}

/// Close every open conflict session of a repo — called when the driving
/// operation finishes (merge continue/abort, rebase continue/skip/abort,
/// cherry-pick continue/abort). A later re-run of the same operation then
/// starts a fresh log row.
pub fn close_conflict_sessions(repo_path: &str) {
    if let Ok(mut m) = registry().lock() {
        m.retain(|(p, _), _| p != repo_path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_db;

    fn open_db() -> Arc<Mutex<Connection>> {
        let mut conn = Connection::open_in_memory().unwrap();
        init_db(&mut conn).unwrap();
        Arc::new(Mutex::new(conn))
    }

    fn item(repo: &str, path: &str) -> NewOperationLogItem {
        NewOperationLogItem {
            repo_path: repo.to_string(),
            ref_name: "main".into(),
            before_oid: "a".repeat(40),
            after_oid: None,
            detail: Some(format!("path:{path}")),
        }
    }

    fn summaries(db: &Arc<Mutex<Connection>>) -> Vec<String> {
        let conn = db.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT summary FROM operation_logs WHERE op_type = 'conflict_resolution' ORDER BY id")
            .unwrap();
        let rows = stmt
            .query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        rows
    }

    #[test]
    fn same_session_merges_into_one_log() {
        let db = open_db();
        for i in 0..10 {
            record_conflict_resolution(&db, "/repo/merge", Some("merge:abc"), vec![item("/repo/merge", &format!("f{i}.txt"))]);
        }
        let s = summaries(&db);
        assert_eq!(s.len(), 1, "10 resolves of one session must share one log");
        assert_eq!(s[0], "解决 10 个文件冲突：f0.txt、f1.txt、f2.txt、f3.txt、f4.txt 等 10 个");

        let conn = db.lock().unwrap();
        let items: i64 = conn
            .query_row("SELECT COUNT(*) FROM operation_log_items", [], |r| r.get(0))
            .unwrap();
        assert_eq!(items, 10);
    }

    #[test]
    fn different_sessions_and_repos_stay_separate() {
        let db = open_db();
        record_conflict_resolution(&db, "/repo/sep", Some("merge:abc"), vec![item("/repo/sep", "a.txt")]);
        record_conflict_resolution(&db, "/repo/sep", Some("merge:abc"), vec![item("/repo/sep", "b.txt")]);
        // Same repo, different driving operation (e.g. merging again after abort
        // without the session being closed) — separate log.
        record_conflict_resolution(&db, "/repo/sep", Some("merge:def"), vec![item("/repo/sep", "c.txt")]);
        // Different repo — separate log.
        record_conflict_resolution(&db, "/other/sep", Some("merge:abc"), vec![item("/other/sep", "d.txt")]);
        assert_eq!(summaries(&db).len(), 3);
    }

    #[test]
    fn closing_a_session_starts_a_fresh_log() {
        let db = open_db();
        record_conflict_resolution(&db, "/repo/close", Some("merge:abc"), vec![item("/repo/close", "a.txt")]);
        close_conflict_sessions("/repo/close");
        record_conflict_resolution(&db, "/repo/close", Some("merge:abc"), vec![item("/repo/close", "b.txt")]);
        let s = summaries(&db);
        assert_eq!(s.len(), 2);
        assert_eq!(s[1], "解决 1 个文件冲突：b.txt");

        // Closing another repo leaves this one alone.
        close_conflict_sessions("/other/close");
        record_conflict_resolution(&db, "/repo/close", Some("merge:abc"), vec![item("/repo/close", "c.txt")]);
        assert_eq!(summaries(&db).len(), 2);
        assert_eq!(summaries(&db)[1], "解决 2 个文件冲突：b.txt、c.txt");
    }

    #[test]
    fn no_session_falls_back_to_standalone_log() {
        let db = open_db();
        record_conflict_resolution(&db, "/repo/nosession", None, vec![item("/repo/nosession", "a.txt")]);
        record_conflict_resolution(&db, "/repo/nosession", None, vec![item("/repo/nosession", "b.txt")]);
        let s = summaries(&db);
        assert_eq!(s.len(), 2, "no driving operation => one log per resolve");
        assert_eq!(s[0], "解决 1 个文件冲突：a.txt");
    }

    #[test]
    fn summary_lists_first_five_paths_only() {
        let db = open_db();
        let items: Vec<_> = (0..7).map(|i| item("/repo/summary", &format!("f{i}.txt"))).collect();
        record_conflict_resolution(&db, "/repo/summary", Some("merge:xyz"), items);
        assert_eq!(summaries(&db)[0], "解决 7 个文件冲突：f0.txt、f1.txt、f2.txt、f3.txt、f4.txt 等 7 个");
    }
}
