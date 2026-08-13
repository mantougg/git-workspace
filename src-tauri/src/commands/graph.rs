use std::path::Path;

use tauri::State;

use crate::core::graph::{self, BranchInfo, CommitInfo};
use crate::db::dao;
use crate::error::{AppError, AppResult};
use crate::models::repository::CommitRecord;
use crate::state::AppState;

/// Get commit history for a repository, starting from HEAD.
/// Returns up to `max_count` commits, sorted topologically.
///
/// Commit metadata is cached in SQLite (`commits` / `commit_parents`): repeat
/// loads only parse commits that are not yet cached, avoiding re-reading every
/// commit object from the repository.
#[tauri::command]
pub fn get_commit_history(
    repo_path: String,
    max_count: Option<usize>,
    state: State<'_, AppState>,
) -> AppResult<Vec<CommitInfo>> {
    let max = max_count.unwrap_or(100);
    let mut conn = state
        .db
        .lock()
        .map_err(|e| AppError::Other(format!("DB lock error: {}", e)))?;

    let repo_id = dao::get_repository_id_by_path(&conn, &repo_path)?;

    // Walk HEAD for the topological OID order (cheap; no commit parsing).
    let oids = graph::revwalk_oids(Path::new(&repo_path), max)?;

    // Open the repo once for ref resolution + uncached commit parsing.
    let repo = git2::Repository::open(Path::new(&repo_path))?;
    let ref_map = graph::ref_map(&repo);

    let mut result: Vec<CommitInfo> = Vec::with_capacity(oids.len());
    let mut to_store: Vec<CommitRecord> = Vec::new();

    for oid_str in &oids {
        let oid = git2::Oid::from_str(oid_str)?;
        let refs = ref_map.get(oid_str).cloned().unwrap_or_default();

        // Cache hit: reconstruct from DB, skipping `find_commit`.
        let cached = match repo_id {
            Some(id) => dao::get_commit_record(&conn, id, oid_str)?,
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
        dao::upsert_commits_batch(&mut conn, id, &to_store)?;
    }

    Ok(result)
}

/// Get all branches (local and remote) for a repository.
#[tauri::command]
pub fn get_branches(repo_path: String) -> AppResult<Vec<BranchInfo>> {
    graph::get_branches(Path::new(&repo_path))
}
