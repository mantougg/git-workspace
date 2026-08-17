//! Workspace Change Set (T-22, Roadmap §17): cross-repository feature
//! grouping.
//!
//! A change set is lightweight metadata — a named set of repositories plus an
//! optional per-repo target branch — persisted in the `change_sets` /
//! `change_set_repositories` tables (Roadmap §41, pre-existing v1 schema).
//! Summary statistics reuse the T-02 status cache (branch / ahead / behind)
//! and a per-repo libgit2 diff pass; no workspace-wide rescan happens here.
//!
//! DAO helpers deliberately live in this module (not `db/dao.rs`) while
//! parallel task agents share the tree. All batch writes are transactional
//! (single-writer model, global constraint §6). No libgit2 handles are
//! cached: every git read opens and drops its own `Repository` inside the
//! calling thread (global constraint §3).

use std::path::Path;

use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::core::git_status;
use crate::error::{AppError, AppResult};

// ---------------------------------------------------------------------------
// IPC data model (serde structs are the single source of truth)
// ---------------------------------------------------------------------------

/// A workspace change set row (`change_sets` table).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangeSet {
    pub id: i64,
    pub workspace_id: i64,
    pub name: String,
    pub description: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// One repository associated with a change set, joined with the repository's
/// path / name / relative path for direct UI rendering.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangeSetRepo {
    pub change_set_id: i64,
    pub repo_id: i64,
    pub repo_path: String,
    pub repo_name: String,
    pub relative_path: String,
    /// Branch this repo's part of the feature is meant to land on
    /// (informational; Commit/Push operate on the checked-out branch).
    pub target_branch: Option<String>,
}

/// Repo association input (create / add / target-branch update).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangeSetRepoInput {
    pub repo_id: i64,
    pub target_branch: Option<String>,
}

/// Create payload. `repos` may be empty — repos can be attached later.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateChangeSetRequest {
    pub workspace_id: i64,
    pub name: String,
    pub description: Option<String>,
    #[serde(default)]
    pub repos: Vec<ChangeSetRepoInput>,
}

/// Update payload; `None` fields keep their current value.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateChangeSetRequest {
    pub id: i64,
    pub name: Option<String>,
    pub description: Option<String>,
}

/// Per-repo summary row: workdir diff stats plus branch / ahead / behind from
/// the T-02 status cache (or a single live fallback read when uncached).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangeSetRepoSummary {
    pub repo: ChangeSetRepo,
    pub current_branch: Option<String>,
    /// Unpushed commits on the current branch (status-cache `ahead`).
    pub ahead: usize,
    pub behind: usize,
    /// Changed files in the workdir (untracked runtime dirs excluded).
    pub files: usize,
    /// Added / deleted lines across the workdir diff (binary files: 0).
    pub added: usize,
    pub deleted: usize,
    /// Human-readable error when the repo could not be read.
    pub error: Option<String>,
}

/// Aggregate summary for one change set (the "统一汇总" view).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangeSetSummary {
    pub change_set: ChangeSet,
    /// Member repository count.
    pub repositories: usize,
    pub files: usize,
    pub added: usize,
    pub deleted: usize,
    /// Total unpushed commits across member repos (status-cache `ahead`).
    pub commits: usize,
    pub repos: Vec<ChangeSetRepoSummary>,
}

/// Workdir change statistics for one repository (pure data).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct ChangeStats {
    pub files: usize,
    pub added: usize,
    pub deleted: usize,
}

// ---------------------------------------------------------------------------
// DAO helpers (module-local by design; see module docs)
// ---------------------------------------------------------------------------

fn row_to_change_set(row: &rusqlite::Row<'_>) -> rusqlite::Result<ChangeSet> {
    Ok(ChangeSet {
        id: row.get(0)?,
        workspace_id: row.get(1)?,
        name: row.get(2)?,
        description: row.get(3)?,
        created_at: row.get(4)?,
        updated_at: row.get(5)?,
    })
}

const CHANGE_SET_COLS: &str =
    "SELECT id, workspace_id, name, description, created_at, updated_at FROM change_sets";

/// Insert a new change set together with its initial member repositories in
/// one transaction (a foreign/unknown repo rolls back the create, so no
/// orphan half-empty set can be left behind).
pub(crate) fn create_change_set(
    conn: &mut Connection,
    workspace_id: i64,
    name: &str,
    description: Option<&str>,
    repos: &[ChangeSetRepoInput],
) -> AppResult<ChangeSet> {
    let now = Utc::now().to_rfc3339();
    let tx = conn.transaction()?;
    tx.execute(
        "INSERT INTO change_sets (workspace_id, name, description, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![workspace_id, name, description, now, now],
    )?;
    let id = tx.last_insert_rowid();
    insert_members(&tx, id, workspace_id, repos)?;
    tx.commit()?;
    Ok(ChangeSet {
        id,
        workspace_id,
        name: name.to_string(),
        description: description.map(|d| d.to_string()),
        created_at: now.clone(),
        updated_at: now,
    })
}

/// Load one change set by id (`AppError::NotFound` when missing).
pub(crate) fn get_change_set(conn: &Connection, id: i64) -> AppResult<ChangeSet> {
    conn.query_row(
        &format!("{} WHERE id = ?1", CHANGE_SET_COLS),
        params![id],
        row_to_change_set,
    )
    .map_err(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => {
            AppError::NotFound(format!("Change set {} not found", id))
        }
        other => AppError::Db(other),
    })
}

/// List all change sets of a workspace, most recently updated first.
pub(crate) fn list_change_sets(
    conn: &Connection,
    workspace_id: i64,
) -> AppResult<Vec<ChangeSet>> {
    let mut stmt = conn.prepare(&format!(
        "{} WHERE workspace_id = ?1 ORDER BY updated_at DESC, id DESC",
        CHANGE_SET_COLS
    ))?;
    let rows = stmt
        .query_map(params![workspace_id], row_to_change_set)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Update name / description; `None` keeps the current value. Bumps
/// `updated_at`.
pub(crate) fn update_change_set(
    conn: &Connection,
    id: i64,
    name: Option<&str>,
    description: Option<&str>,
) -> AppResult<ChangeSet> {
    let current = get_change_set(conn, id)?;
    let now = Utc::now().to_rfc3339();
    let name = name.unwrap_or(&current.name);
    let description = description.or(current.description.as_deref());
    conn.execute(
        "UPDATE change_sets SET name = ?1, description = ?2, updated_at = ?3 WHERE id = ?4",
        params![name, description, now, id],
    )?;
    Ok(ChangeSet {
        id,
        workspace_id: current.workspace_id,
        name: name.to_string(),
        description: description.map(|d| d.to_string()),
        created_at: current.created_at,
        updated_at: now,
    })
}

/// Delete a change set; membership rows cascade (`ON DELETE CASCADE`).
pub(crate) fn delete_change_set(conn: &Connection, id: i64) -> AppResult<()> {
    conn.execute("DELETE FROM change_sets WHERE id = ?1", params![id])?;
    Ok(())
}

/// List member repositories of a change set (soft-deleted repos excluded),
/// ordered by repo name.
pub(crate) fn list_change_set_repos(
    conn: &Connection,
    change_set_id: i64,
) -> AppResult<Vec<ChangeSetRepo>> {
    let mut stmt = conn.prepare(
        r#"SELECT csr.change_set_id, csr.repo_id, r.path, r.name, r.relative_path, csr.target_branch
           FROM change_set_repositories csr
           JOIN repositories r ON r.id = csr.repo_id
           WHERE csr.change_set_id = ?1 AND r.is_deleted = 0
           ORDER BY r.name"#,
    )?;
    let rows = stmt
        .query_map(params![change_set_id], |row| {
            Ok(ChangeSetRepo {
                change_set_id: row.get(0)?,
                repo_id: row.get(1)?,
                repo_path: row.get(2)?,
                repo_name: row.get(3)?,
                relative_path: row.get(4)?,
                target_branch: row.get(5)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Repo id → workspace it belongs to (excluding soft-deleted rows).
fn repo_workspace(conn: &Connection, repo_id: i64) -> AppResult<i64> {
    conn.query_row(
        "SELECT workspace_id FROM repositories WHERE id = ?1 AND is_deleted = 0",
        params![repo_id],
        |row| row.get(0),
    )
    .optional()?
    .ok_or_else(|| AppError::NotFound(format!("Repository {} not found", repo_id)))
}

/// Normalize a UI-provided target branch: trims, empty → None.
fn normalize_branch(branch: Option<&str>) -> Option<&str> {
    branch.map(str::trim).filter(|b| !b.is_empty())
}

/// Validate + upsert member rows on an existing connection/transaction.
/// Every repo must belong to `workspace_id` (the change set's workspace).
fn insert_members(
    conn: &Connection,
    change_set_id: i64,
    workspace_id: i64,
    repos: &[ChangeSetRepoInput],
) -> AppResult<()> {
    let mut stmt = conn.prepare(
        r#"INSERT INTO change_set_repositories (change_set_id, repo_id, target_branch)
           VALUES (?1, ?2, ?3)
           ON CONFLICT(change_set_id, repo_id) DO UPDATE SET target_branch = ?3"#,
    )?;
    for repo in repos {
        let ws = repo_workspace(conn, repo.repo_id)?;
        if ws != workspace_id {
            return Err(AppError::Other(format!(
                "Repository {} belongs to workspace {}, not change set workspace {}",
                repo.repo_id, ws, workspace_id
            )));
        }
        stmt.execute(params![
            change_set_id,
            repo.repo_id,
            normalize_branch(repo.target_branch.as_deref()),
        ])?;
    }
    Ok(())
}

/// Add (or update the target branch of) member repositories, validating that
/// every repo belongs to the change set's workspace. The whole batch runs in
/// one transaction — one invalid repo rolls back everything (no partial
/// association). Also bumps the change set's `updated_at`.
pub(crate) fn add_change_set_repos(
    conn: &mut Connection,
    change_set_id: i64,
    repos: &[ChangeSetRepoInput],
) -> AppResult<()> {
    if repos.is_empty() {
        return Ok(());
    }
    let cs = get_change_set(conn, change_set_id)?;
    let now = Utc::now().to_rfc3339();
    let tx = conn.transaction()?;
    insert_members(&tx, change_set_id, cs.workspace_id, repos)?;
    tx.execute(
        "UPDATE change_sets SET updated_at = ?1 WHERE id = ?2",
        params![now, change_set_id],
    )?;
    tx.commit()?;
    Ok(())
}

/// Remove one repository from a change set. Bumps `updated_at`.
pub(crate) fn remove_change_set_repo(
    conn: &Connection,
    change_set_id: i64,
    repo_id: i64,
) -> AppResult<()> {
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "DELETE FROM change_set_repositories WHERE change_set_id = ?1 AND repo_id = ?2",
        params![change_set_id, repo_id],
    )?;
    conn.execute(
        "UPDATE change_sets SET updated_at = ?1 WHERE id = ?2",
        params![now, change_set_id],
    )?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Diff statistics (per-repo, libgit2; no handles escape this function)
// ---------------------------------------------------------------------------

/// Workdir change statistics for one repository (HEAD ↔ workdir+index,
/// untracked included, recursing untracked dirs).
///
/// Untracked files under runtime/generated directories (node_modules,
/// target, dist, …) are excluded, matching the change tree (T-03). Binary
/// files count as changed files with zero line stats (mirroring T-04, which
/// emits empty hunk lists for binaries). The libgit2 `Repository` handle is
/// opened and dropped inside this call (global constraint §3). Line stats
/// come from `Patch::line_stats()` — `git_diff_foreach` does not load
/// untracked file content, while `Patch::from_diff` does (the T-04 path).
pub(crate) fn change_stats(repo_path: &Path) -> AppResult<ChangeStats> {
    let repo = git2::Repository::open(repo_path)?;
    // Explicit empty tree on unborn HEAD (T-04 semantics: everything added).
    let head_tree = crate::core::diff::head_or_empty_tree(&repo)?;

    let mut opts = git2::DiffOptions::new();
    opts.include_untracked(true)
        .recurse_untracked_dirs(true)
        .include_unmodified(false);
    let diff = repo.diff_tree_to_workdir_with_index(Some(&head_tree), Some(&mut opts))?;

    let mut stats = ChangeStats::default();
    for (idx, delta) in diff.deltas().enumerate() {
        let is_untracked = delta.status() == git2::Delta::Untracked;
        let path = delta
            .new_file()
            .path()
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .unwrap_or_default();
        // Skip untracked files under runtime/generated directories.
        if is_untracked && git_status::is_runtime_path(&path) {
            continue;
        }
        stats.files += 1;
        if is_untracked {
            // libgit2 does not load untracked content into patches, so the
            // delta has no line stats; count the working-tree file directly
            // (mirrors the T-04 full-add hunk path). Binary/unreadable files
            // count as changed with zero added lines.
            if let Ok(content) = std::fs::read_to_string(repo_path.join(&path)) {
                stats.added += content.lines().count();
            }
            continue;
        }
        if let Some(patch) = git2::Patch::from_diff(&diff, idx)? {
            let (_context, additions, deletions) = patch.line_stats()?;
            stats.added += additions;
            stats.deleted += deletions;
        }
    }
    Ok(stats)
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------
    // DB helpers
    // -----------------------------------------------------------------

    fn open_memory() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        crate::db::init_db(&mut conn).unwrap();
        conn
    }

    fn insert_workspace(conn: &Connection, path: &str) -> i64 {
        conn.execute(
            "INSERT INTO workspaces (name, path, created_at, updated_at) VALUES (?1, ?2, 't', 't')",
            params!["w", path],
        )
        .unwrap();
        conn.last_insert_rowid()
    }

    fn insert_repo(conn: &Connection, ws_id: i64, path: &str, name: &str) -> i64 {
        conn.execute(
            "INSERT INTO repositories (workspace_id, path, name, relative_path, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, 't', 't')",
            params![ws_id, path, name, name],
        )
        .unwrap();
        conn.last_insert_rowid()
    }

    fn input(repo_id: i64, branch: Option<&str>) -> ChangeSetRepoInput {
        ChangeSetRepoInput {
            repo_id,
            target_branch: branch.map(|b| b.to_string()),
        }
    }

    // -----------------------------------------------------------------
    // CRUD
    // -----------------------------------------------------------------

    /// Create → list → update → delete round-trip; delete cascades to the
    /// membership rows (acceptance: 落库 + 重启可恢复 is covered by the rows
    /// living in the real tables).
    #[test]
    fn change_set_crud_roundtrip() {
        let mut conn = open_memory();
        let ws = insert_workspace(&conn, "D:/w");
        let r1 = insert_repo(&conn, ws, "D:/w/a", "a");

        let cs = create_change_set(&mut conn, ws, "Feature: AI Review", Some("desc"), &[]).unwrap();
        assert!(cs.id > 0);

        let listed = list_change_sets(&conn, ws).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].name, "Feature: AI Review");
        assert_eq!(listed[0].description.as_deref(), Some("desc"));

        // Update: only the description; name must be preserved.
        let updated = update_change_set(&conn, cs.id, None, Some("new desc")).unwrap();
        assert_eq!(updated.name, "Feature: AI Review");
        assert_eq!(updated.description.as_deref(), Some("new desc"));

        add_change_set_repos(&mut conn, cs.id, &[input(r1, Some("feature/ai"))]).unwrap();
        assert_eq!(list_change_set_repos(&conn, cs.id).unwrap().len(), 1);

        delete_change_set(&conn, cs.id).unwrap();
        assert!(list_change_sets(&conn, ws).unwrap().is_empty());
        let orphans: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM change_set_repositories WHERE change_set_id = ?1",
                params![cs.id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(orphans, 0, "membership rows must cascade on delete");

        match get_change_set(&conn, cs.id) {
            Err(AppError::NotFound(_)) => {}
            other => panic!("expected NotFound after delete, got {:?}", other),
        }
    }

    /// Repo association: workspace guard, upsert of target_branch, removal,
    /// and empty-branch normalization.
    #[test]
    fn change_set_repo_association_rules() {
        let mut conn = open_memory();
        let ws1 = insert_workspace(&conn, "D:/w1");
        let ws2 = insert_workspace(&conn, "D:/w2");
        let r1 = insert_repo(&conn, ws1, "D:/w1/a", "a");
        let r2 = insert_repo(&conn, ws1, "D:/w1/b", "b");
        let r_foreign = insert_repo(&conn, ws2, "D:/w2/c", "c");

        let cs = create_change_set(&mut conn, ws1, "cs", None, &[]).unwrap();

        // A repo from another workspace is rejected …
        let err = add_change_set_repos(&mut conn, cs.id, &[input(r_foreign, None)]).unwrap_err();
        assert!(matches!(err, AppError::Other(_)));
        // … and the batch is transactional: nothing was partially written.
        assert!(list_change_set_repos(&conn, cs.id).unwrap().is_empty());

        // Mixed batch (valid + foreign) also rolls back the valid part.
        let err = add_change_set_repos(
            &mut conn,
            cs.id,
            &[input(r1, Some("f/a")), input(r_foreign, None)],
        )
        .unwrap_err();
        assert!(matches!(err, AppError::Other(_)));
        assert!(list_change_set_repos(&conn, cs.id).unwrap().is_empty());

        // Unknown repo id → NotFound, nothing written.
        let err = add_change_set_repos(&mut conn, cs.id, &[input(9999, None)]).unwrap_err();
        assert!(matches!(err, AppError::NotFound(_)));

        // Valid batch lands; empty branch string normalizes to None.
        add_change_set_repos(
            &mut conn,
            cs.id,
            &[input(r1, Some("feature/x")), input(r2, Some("  "))],
        )
        .unwrap();
        let repos = list_change_set_repos(&conn, cs.id).unwrap();
        assert_eq!(repos.len(), 2);
        assert_eq!(repos[0].repo_name, "a");
        assert_eq!(repos[0].target_branch.as_deref(), Some("feature/x"));
        assert_eq!(repos[1].target_branch, None);

        // Re-adding the same repo upserts the target branch (no duplicate).
        add_change_set_repos(&mut conn, cs.id, &[input(r1, Some("feature/y"))]).unwrap();
        let repos = list_change_set_repos(&conn, cs.id).unwrap();
        assert_eq!(repos.len(), 2);
        assert_eq!(repos[0].target_branch.as_deref(), Some("feature/y"));

        // Removal works and is scoped to this change set.
        remove_change_set_repo(&conn, cs.id, r1).unwrap();
        let repos = list_change_set_repos(&conn, cs.id).unwrap();
        assert_eq!(repos.len(), 1);
        assert_eq!(repos[0].repo_id, r2);
    }

    /// Create with a foreign repo is atomic: the change set row itself rolls
    /// back (no orphan half-created set).
    #[test]
    fn create_with_foreign_repo_rolls_back() {
        let mut conn = open_memory();
        let ws1 = insert_workspace(&conn, "D:/w1");
        let ws2 = insert_workspace(&conn, "D:/w2");
        let r_foreign = insert_repo(&conn, ws2, "D:/w2/c", "c");

        let err =
            create_change_set(&mut conn, ws1, "cs", None, &[input(r_foreign, None)]).unwrap_err();
        assert!(matches!(err, AppError::Other(_)));
        assert!(
            list_change_sets(&conn, ws1).unwrap().is_empty(),
            "the failed create must not leave a change set behind"
        );
    }

    /// Soft-deleted repositories disappear from membership reads (they are
    /// filtered out), without touching the stored association.
    #[test]
    fn soft_deleted_repo_is_hidden_but_association_kept() {
        let mut conn = open_memory();
        let ws = insert_workspace(&conn, "D:/w");
        let r1 = insert_repo(&conn, ws, "D:/w/a", "a");
        let cs = create_change_set(&mut conn, ws, "cs", None, &[]).unwrap();
        add_change_set_repos(&mut conn, cs.id, &[input(r1, None)]).unwrap();
        assert_eq!(list_change_set_repos(&conn, cs.id).unwrap().len(), 1);

        conn.execute("UPDATE repositories SET is_deleted = 1 WHERE id = ?1", params![r1])
            .unwrap();
        assert!(list_change_set_repos(&conn, cs.id).unwrap().is_empty());

        // Rescan revives the row → the association resurfaces.
        conn.execute("UPDATE repositories SET is_deleted = 0 WHERE id = ?1", params![r1])
            .unwrap();
        assert_eq!(list_change_set_repos(&conn, cs.id).unwrap().len(), 1);
    }

    // -----------------------------------------------------------------
    // change_stats
    // -----------------------------------------------------------------

    fn tmpdir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "gw_changeset_{}_{}",
            tag,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn commit_all(repo: &git2::Repository, msg: &str) {
        let mut index = repo.index().unwrap();
        index
            .add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)
            .unwrap();
        index.write().unwrap();
        let tree_oid = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_oid).unwrap();
        let sig = git2::Signature::now("tester", "t@example.com").unwrap();
        let parent = repo.head().ok().and_then(|h| h.peel_to_commit().ok());
        match &parent {
            Some(p) => repo.commit(Some("HEAD"), &sig, &sig, msg, &tree, &[p]).unwrap(),
            None => repo.commit(Some("HEAD"), &sig, &sig, msg, &tree, &[]).unwrap(),
        };
    }

    /// Diff stats count modified / untracked files and their + / - lines;
    /// untracked runtime directories (node_modules) are excluded like the
    /// change tree does.
    #[test]
    fn change_stats_counts_lines_and_skips_runtime_paths() {
        let dir = tmpdir("stats");
        let repo = git2::Repository::init(&dir).unwrap();
        std::fs::write(dir.join("a.txt"), "one\ntwo\n").unwrap();
        commit_all(&repo, "c1");
        drop(repo);

        // 1 modified (+1/-1), 1 untracked (+1), 1 untracked runtime file (excluded).
        std::fs::write(dir.join("a.txt"), "one\nTWO\nthree\n").unwrap();
        std::fs::write(dir.join("new.txt"), "fresh\n").unwrap();
        std::fs::create_dir_all(dir.join("node_modules/junk")).unwrap();
        std::fs::write(dir.join("node_modules/junk/x.js"), "junk\n").unwrap();

        let stats = change_stats(&dir).unwrap();
        assert_eq!(stats.files, 2, "modified + untracked, runtime dir excluded");
        assert_eq!(stats.added, 3, "TWO + three + fresh");
        assert_eq!(stats.deleted, 1, "two");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Unborn HEAD: every file shows up as added (T-04 semantics).
    #[test]
    fn change_stats_handles_unborn_head() {
        let dir = tmpdir("unborn");
        let repo = git2::Repository::init(&dir).unwrap();
        drop(repo);
        std::fs::write(dir.join("a.txt"), "hello\n").unwrap();

        let stats = change_stats(&dir).unwrap();
        assert_eq!(stats.files, 1);
        assert_eq!(stats.added, 1);
        assert_eq!(stats.deleted, 0);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A clean repository reports zeroes (no error).
    #[test]
    fn change_stats_clean_repo_is_zero() {
        let dir = tmpdir("clean");
        let repo = git2::Repository::init(&dir).unwrap();
        std::fs::write(dir.join("a.txt"), "one\n").unwrap();
        commit_all(&repo, "c1");
        drop(repo);

        let stats = change_stats(&dir).unwrap();
        assert_eq!(stats, ChangeStats::default());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
