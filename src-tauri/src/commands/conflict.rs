//! Conflict Resolver commands (T-16).

use std::path::Path;
use std::sync::{Arc, Mutex};

use rusqlite::Connection;
use tauri::State;

use crate::core::conflict::{self, ConflictContent, OperationState};
use crate::core::operation_log::{self, NewOperationLogItem};
use crate::error::AppResult;
use crate::state::AppState;

/// The repo's current operation + conflict state (CONFLICT detection;
/// routes Continue / Abort to the right state machine).
#[tauri::command]
pub fn get_operation_state(repo_path: String) -> AppResult<OperationState> {
    conflict::operation_state(Path::new(&repo_path))
}

/// Load BASE / OURS / THEIRS + worktree content of one conflicted file.
#[tauri::command]
pub fn get_conflict_content(repo_path: String, path: String) -> AppResult<ConflictContent> {
    conflict::conflict_content(Path::new(&repo_path), &path)
}

/// Resolve one conflicted file: "ours" | "theirs" | "both".
#[tauri::command]
pub fn resolve_conflict(
    repo_path: String,
    path: String,
    strategy: String,
    state: State<'_, AppState>,
) -> AppResult<()> {
    let before = operation_log::snapshot_head(Path::new(&repo_path));
    conflict::resolve_conflict(Path::new(&repo_path), &path, &strategy)?;
    record_conflict_resolution(&repo_path, &path, before, &state.db);
    Ok(())
}

/// Resolve one conflicted file with manually edited content (null = delete).
#[tauri::command]
pub fn resolve_conflict_with_content(
    repo_path: String,
    path: String,
    content: Option<String>,
    state: State<'_, AppState>,
) -> AppResult<()> {
    let before = operation_log::snapshot_head(Path::new(&repo_path));
    conflict::resolve_conflict_with_content(Path::new(&repo_path), &path, content.as_deref())?;
    record_conflict_resolution(&repo_path, &path, before, &state.db);
    Ok(())
}

/// Record the confirmed T-16 Apply action without persisting user file
/// content. T-34 Undo is ref-snapshot based, so a conflict resolution remains
/// recoverable through the active Git operation's Abort flow or manual edits.
///
/// GF-16: consecutive resolves of the same merge/rebase/cherry-pick operation
/// are merged into ONE operation log (items accumulate the file paths) —
/// `operation_log` derives the session from the repo's in-progress operation,
/// so this call site just hands over the snapshot.
fn record_conflict_resolution(
    repo_path: &str,
    path: &str,
    before: Option<(String, String)>,
    db: &Arc<Mutex<Connection>>,
) {
    let Some((ref_name, before_oid)) = before else {
        return;
    };
    let after_oid = operation_log::snapshot_head(Path::new(repo_path)).map(|(_, oid)| oid);
    operation_log::record_conflict_resolution(
        db,
        repo_path,
        conflict::session_key(Path::new(repo_path)).as_deref(),
        vec![NewOperationLogItem {
            repo_path: repo_path.to_string(),
            ref_name,
            before_oid,
            after_oid,
            detail: Some(format!("path:{path}")),
        }],
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::operation_log::{get_operation_log, query_operation_logs, LogFilter};
    use std::path::Path;

    fn tmpdir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "gw_conflict_cmd_{}_{}",
            tag,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn commit_file(repo: &git2::Repository, dir: &Path, name: &str, content: &str, msg: &str) {
        std::fs::write(dir.join(name), content).unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new(name)).unwrap();
        index.write().unwrap();
        let tree_oid = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_oid).unwrap();
        let sig = git2::Signature::now("tester", "t@example.com").unwrap();
        let parent = repo
            .head()
            .ok()
            .and_then(|h| h.target())
            .map(|oid| repo.find_commit(oid).unwrap());
        let parents: Vec<&git2::Commit> = parent.iter().collect();
        repo.commit(Some("HEAD"), &sig, &sig, msg, &tree, &parents).unwrap();
    }

    #[test]
    fn logs_resolution_metadata_without_conflict_content() {
        let mut conn = Connection::open_in_memory().unwrap();
        crate::db::init_db(&mut conn).unwrap();
        let db = Arc::new(Mutex::new(conn));

        // Repo does not exist on disk -> no session key -> standalone log.
        record_conflict_resolution(
            "/workspace/project",
            "src/config.rs",
            Some(("main".to_string(), "a".repeat(40))),
            &db,
        );

        let conn = db.lock().unwrap();
        let page = query_operation_logs(
            &conn,
            &LogFilter {
                op_type: Some(operation_log::OP_CONFLICT_RESOLUTION),
                ..Default::default()
            },
            10,
            0,
        )
        .unwrap();
        assert_eq!(page.total, 1);
        assert_eq!(page.logs[0].summary, "解决 1 个文件冲突：src/config.rs");

        let detail = get_operation_log(&conn, page.logs[0].id).unwrap();
        assert_eq!(detail.items[0].detail.as_deref(), Some("path:src/config.rs"));
        assert_eq!(detail.items[0].after_oid, None);
    }

    /// GF-16 acceptance: a 10-file conflict resolution produces exactly ONE
    /// operation log whose items carry the per-file paths.
    #[test]
    fn ten_file_resolution_produces_one_log() {
        let dir = tmpdir("ten_files");
        // Merge conflict on ten files.
        {
            let repo = git2::Repository::init(&dir).unwrap();
            for i in 0..10 {
                commit_file(&repo, &dir, &format!("f{i}.txt"), "base\n", "init");
            }
            let head = repo.head().unwrap().peel_to_commit().unwrap();
            repo.branch("side", &head, false).unwrap();
            drop(head);
            for i in 0..10 {
                commit_file(&repo, &dir, &format!("f{i}.txt"), "ours\n", "master change");
            }
            drop(repo);
        }
        crate::core::branch::checkout_branch(&dir, "side").unwrap();
        {
            let repo = git2::Repository::open(&dir).unwrap();
            for i in 0..10 {
                commit_file(&repo, &dir, &format!("f{i}.txt"), "theirs\n", "side change");
            }
            drop(repo);
        }
        crate::core::branch::checkout_branch(&dir, "master").unwrap();
        let outcome = crate::core::merge::merge(&dir, "side", "normal").unwrap();
        assert!(
            matches!(outcome, crate::core::merge::MergeOutcome::Conflict { .. }),
            "expected a conflicted merge"
        );

        let mut conn = Connection::open_in_memory().unwrap();
        crate::db::init_db(&mut conn).unwrap();
        let db = Arc::new(Mutex::new(conn));
        let repo_path = dir.to_string_lossy().to_string();

        // Resolve all ten files through the command path.
        for i in 0..10 {
            let path = format!("f{i}.txt");
            let before = operation_log::snapshot_head(Path::new(&repo_path));
            crate::core::conflict::resolve_conflict(Path::new(&repo_path), &path, "theirs").unwrap();
            record_conflict_resolution(&repo_path, &path, before, &db);
        }

        {
            let conn = db.lock().unwrap();
            let page = query_operation_logs(
                &conn,
                &LogFilter {
                    op_type: Some(operation_log::OP_CONFLICT_RESOLUTION),
                    ..Default::default()
                },
                10,
                0,
            )
            .unwrap();
            assert_eq!(page.total, 1, "ten resolves of one merge must share one log");
            assert_eq!(page.logs[0].repo_count, 10);
            assert_eq!(
                page.logs[0].summary,
                "解决 10 个文件冲突：f0.txt、f1.txt、f2.txt、f3.txt、f4.txt 等 10 个"
            );

            // The session key is the MERGE_HEAD-driven merge session.
            let detail = get_operation_log(&conn, page.logs[0].id).unwrap();
            assert_eq!(detail.items.len(), 10);
            for (i, item) in detail.items.iter().enumerate() {
                assert_eq!(item.detail.as_deref(), Some(format!("path:f{i}.txt").as_str()));
            }
        }

        // Completing the merge closes the session: a re-run of the same
        // merge starts a fresh log row (done with the DB guard released —
        // the recorder takes it itself).
        operation_log::close_conflict_sessions(&repo_path);
        record_conflict_resolution(
            &repo_path,
            "f0.txt",
            Some(("main".to_string(), "a".repeat(40))),
            &db,
        );
        {
            let conn = db.lock().unwrap();
            let page = query_operation_logs(
                &conn,
                &LogFilter {
                    op_type: Some(operation_log::OP_CONFLICT_RESOLUTION),
                    ..Default::default()
                },
                10,
                0,
            )
            .unwrap();
            assert_eq!(page.total, 2);
        }

        let _ = std::fs::remove_dir_all(&dir);
    }
}
