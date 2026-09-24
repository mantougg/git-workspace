use std::path::Path;

use tauri::State;

use crate::core::graph::{self, BranchInfo, CommitInfo};
use crate::db::dao;
use crate::error::{AppError, AppResult};
use crate::models::repository::CommitRecord;
use crate::state::AppState;

/// Get commit history for a repository, starting from HEAD.
/// Returns up to `max_count` commits, newest first.
///
/// Commit metadata is cached in SQLite (`commits` / `commit_parents`): repeat
/// loads only parse commits that are not yet cached, avoiding re-reading every
/// commit object from the repository.
///
/// Pagination: `offset` skips the newest `offset` commits and `limit` caps the
/// page size, so page `k` transfers only `limit` commits instead of
/// re-fetching the whole prefix (the old `prevCount + PAGE_SIZE` pattern made
/// cumulative transfer O(k²)). The legacy `max_count` parameter is kept for
/// backward compatibility — with `limit` omitted it still acts as the page
/// size, and `offset` omitted means "from HEAD".
#[tauri::command]
pub fn get_commit_history(
    repo_path: String,
    max_count: Option<usize>,
    offset: Option<usize>,
    limit: Option<usize>,
    state: State<'_, AppState>,
) -> AppResult<Vec<CommitInfo>> {
    let (offset, limit) = resolve_page(max_count, offset, limit);
    let mut conn = state
        .db
        .lock()
        .map_err(|e| AppError::Other(format!("DB lock error: {}", e)))?;
    load_commit_history_page(&mut conn, Path::new(&repo_path), offset, limit)
}

/// Resolve the legacy `max_count` + new `offset`/`limit` params into
/// `(offset, limit)`.
///
/// `limit` wins when both are given (explicit page size); `max_count` stays a
/// backward-compatible alias. Both omitted → first 100 commits (unchanged
/// legacy default). Pure function so the mapping is unit-testable.
fn resolve_page(
    max_count: Option<usize>,
    offset: Option<usize>,
    limit: Option<usize>,
) -> (usize, usize) {
    (offset.unwrap_or(0), limit.or(max_count).unwrap_or(100))
}

/// Commit-history load with the SQLite metadata cache (command body, shared
/// with the T-07 benchmark harness so the measured path is the real one).
/// Delegates to the paged variant with `offset = 0` so the benchmark's
/// first-screen path stays byte-identical.
pub(crate) fn load_commit_history_cached(
    conn: &mut rusqlite::Connection,
    repo_path: &Path,
    max: usize,
) -> AppResult<Vec<CommitInfo>> {
    load_commit_history_page(conn, repo_path, 0, max)
}

/// Paged commit-history load with the SQLite metadata cache.
///
/// Walks `offset + limit` OIDs from HEAD (lazy heap walk) and returns the
/// `[offset, offset + limit)` window. The walk over the skipped prefix keeps
/// the cache-hit path warm — cached prefixes are resolved from SQLite without
/// `find_commit` — while only the page's commits are transferred/parsed.
pub(crate) fn load_commit_history_page(
    conn: &mut rusqlite::Connection,
    repo_path: &Path,
    offset: usize,
    limit: usize,
) -> AppResult<Vec<CommitInfo>> {
    let repo_path_str = repo_path.to_string_lossy().to_string();
    let repo_id = dao::get_repository_id_by_path(conn, &repo_path_str)?;

    // Walk HEAD for the newest-first OID order (lazy heap walk; bounded by
    // `offset + limit`, does not touch the rest of the history).
    let oids = graph::revwalk_oids(repo_path, offset.saturating_add(limit))?;

    // Open the repo once for ref resolution + uncached commit parsing.
    let repo = git2::Repository::open(repo_path)?;
    let ref_map = graph::ref_map(&repo);

    let page_len = oids.len().saturating_sub(offset).min(limit);
    let mut result: Vec<CommitInfo> = Vec::with_capacity(page_len);
    let mut to_store: Vec<CommitRecord> = Vec::new();

    for oid_str in oids.iter().skip(offset) {
        if result.len() >= limit {
            break;
        }
        let oid = git2::Oid::from_str(oid_str)?;
        let refs = ref_map.get(oid_str).cloned().unwrap_or_default();

        // Cache hit: reconstruct from DB, skipping `find_commit`.
        let cached = match repo_id {
            Some(id) => dao::get_commit_record(conn, id, oid_str)?,
            None => None,
        };

        match cached {
            Some(record) => result.push(graph::commit_info_from_record(&record, refs)),
            None => {
                let record = graph::commit_record_from_oid(&repo, &oid)
                    .ok_or_else(|| AppError::Other(format!("Commit {} not found", oid_str)))?;
                result.push(graph::commit_info_from_record(&record, refs));
                to_store.push(record);
            }
        }
    }

    // Persist any uncached commits for the next load.
    if let Some(id) = repo_id {
        dao::upsert_commits_batch(conn, id, &to_store)?;
    }

    Ok(result)
}

/// Get all branches (local and remote) for a repository.
#[tauri::command]
pub fn get_branches(repo_path: String) -> AppResult<Vec<BranchInfo>> {
    graph::get_branches(Path::new(&repo_path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::repository::ScannedRepo;

    /// Build a linear repo of `n` commits (distinct times, newest last) and
    /// return (dir, messages newest-first).
    fn linear_repo(tag: &str, n: usize) -> (std::path::PathBuf, Vec<String>) {
        let dir = std::env::temp_dir().join(format!(
            "gw_graph_page_{}_{}",
            tag,
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let repo = git2::Repository::init(&dir).unwrap();

        let mut parent: Option<git2::Commit> = None;
        for i in 0..n {
            std::fs::write(dir.join("f.txt"), format!("content {i}")).unwrap();
            let mut index = repo.index().unwrap();
            index.add_path(Path::new("f.txt")).unwrap();
            index.write().unwrap();
            let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
            // Distinct commit times so the newest-first walk order is fixed.
            let sig =
                git2::Signature::new("t", "t@e.c", &git2::Time::new(1_700_000_000 + i as i64, 0))
                    .unwrap();
            let parents: Vec<&git2::Commit> = parent.iter().collect();
            let oid = repo
                .commit(Some("HEAD"), &sig, &sig, &format!("msg {i}"), &tree, &parents)
                .unwrap();
            parent = Some(repo.find_commit(oid).unwrap());
        }

        let newest_first: Vec<String> = (0..n).rev().map(|i| format!("msg {i}")).collect();
        (dir, newest_first)
    }

    /// Register the fixture repo in an in-memory DB so the metadata-cache path
    /// (repo_id lookup + commit upsert) is exercised.
    fn db_with_repo(dir: &Path) -> rusqlite::Connection {
        let mut conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::db::init_db(&mut conn).unwrap();
        conn.execute(
            "INSERT INTO workspaces (name, path, created_at, updated_at) VALUES ('w', 'dummy', 't', 't')",
            [],
        )
        .unwrap();
        let ws_id = conn.last_insert_rowid();
        dao::upsert_repositories_batch(
            &mut conn,
            ws_id,
            &[ScannedRepo {
                path: dir.to_string_lossy().to_string(),
                name: "page_repo".into(),
                relative_path: "page_repo".into(),
                git_dir_mtime: None,
            }],
        )
        .unwrap();
        conn
    }

    fn messages(commits: &[CommitInfo]) -> Vec<String> {
        commits.iter().map(|c| c.message.clone()).collect()
    }

    #[test]
    fn resolve_page_maps_legacy_and_new_params() {
        // Legacy: maxCount alone, no offset → first N from HEAD.
        assert_eq!(resolve_page(Some(50), None, None), (0, 50));
        // Both omitted → legacy default of 100.
        assert_eq!(resolve_page(None, None, None), (0, 100));
        // New page params win; explicit limit overrides the legacy alias.
        assert_eq!(resolve_page(Some(50), Some(300), Some(100)), (300, 100));
        // offset with only the legacy alias → paging on maxCount.
        assert_eq!(resolve_page(Some(100), Some(200), None), (200, 100));
        // offset without any page size → legacy default.
        assert_eq!(resolve_page(None, Some(200), None), (200, 100));
        // maxCount 0 stays 0 (explicit empty page), not the default.
        assert_eq!(resolve_page(Some(0), None, None), (0, 0));
    }

    #[test]
    fn page_returns_window_without_overlap() {
        let (dir, expected) = linear_repo("window", 10);
        let mut conn = db_with_repo(&dir);

        let p1 = load_commit_history_page(&mut conn, &dir, 0, 4).unwrap();
        let p2 = load_commit_history_page(&mut conn, &dir, 4, 4).unwrap();
        let p3 = load_commit_history_page(&mut conn, &dir, 8, 4).unwrap();

        assert_eq!(messages(&p1), expected[0..4]);
        assert_eq!(messages(&p2), expected[4..8]);
        // The last window is short: only 2 of the 4 requested commits remain.
        assert_eq!(messages(&p3), expected[8..10]);
        // Concatenated pages reproduce the full history, newest first.
        let all: Vec<String> = p1
            .iter()
            .chain(&p2)
            .chain(&p3)
            .map(|c| c.message.clone())
            .collect();
        assert_eq!(all, expected);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn page_boundaries_past_end_return_empty_or_truncated() {
        let (dir, _) = linear_repo("edges", 6);
        let mut conn = db_with_repo(&dir);

        // limit overshooting the window returns only what exists.
        let tail = load_commit_history_page(&mut conn, &dir, 4, 10).unwrap();
        assert_eq!(tail.len(), 2);

        // offset exactly at the end → empty.
        let at_end = load_commit_history_page(&mut conn, &dir, 6, 4).unwrap();
        assert!(at_end.is_empty());

        // offset past the end → still empty, no error.
        let past_end = load_commit_history_page(&mut conn, &dir, 99, 4).unwrap();
        assert!(past_end.is_empty());

        // limit 0 → explicit empty page.
        let empty = load_commit_history_page(&mut conn, &dir, 0, 0).unwrap();
        assert!(empty.is_empty());

        // Full-history page equals the walk order.
        let full = load_commit_history_page(&mut conn, &dir, 0, 6).unwrap();
        assert_eq!(full.len(), 6);
        assert_eq!(full[0].message, "msg 5");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn paged_loads_hit_the_sqlite_cache_and_stay_consistent() {
        let (dir, expected) = linear_repo("cache", 8);
        let mut conn = db_with_repo(&dir);
        let repo_id = dao::get_repository_id_by_path(&conn, dir.to_string_lossy().as_ref())
            .unwrap()
            .unwrap();

        // First pass populates the cache.
        let p1 = load_commit_history_page(&mut conn, &dir, 0, 4).unwrap();
        for c in &p1 {
            assert!(
                dao::get_commit_record(&conn, repo_id, &c.oid)
                    .unwrap()
                    .is_some(),
                "page commits must be persisted to the cache"
            );
        }

        // Second pass resolves from the cache: identical order and payload.
        let p1_again = load_commit_history_page(&mut conn, &dir, 0, 4).unwrap();
        assert_eq!(messages(&p1), messages(&p1_again));
        assert_eq!(p1[0].parents, p1_again[0].parents);

        // The delegated legacy entry point (benchmark path) is offset-0.
        let legacy = load_commit_history_cached(&mut conn, &dir, 4).unwrap();
        assert_eq!(messages(&legacy), expected[0..4]);

        // Cold-loading the second page after the first is cached must still
        // yield the next window in walk order.
        let p2 = load_commit_history_page(&mut conn, &dir, 4, 4).unwrap();
        assert_eq!(messages(&p2), expected[4..8]);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
