//! Workspace-level stash orchestration (T-21): one `Workspace Stash #N`
//! record ties together a per-repo stash across a selected repo set so the
//! whole group can be restored later. Single-repo stash semantics come from
//! T-10 (`core::stash`); all git work is local libgit2 (global constraint §3).
//!
//! The `workspace_stashes` / `workspace_stash_items` DAO helpers (schema V7)
//! deliberately live in this module — not in `db/dao.rs` — while parallel
//! task agents share the tree. Batch writes happen in one transaction
//! (single-writer model, global constraint §6).

use std::path::Path;

use rusqlite::Connection;
use serde::Serialize;

use crate::core::stash;
use crate::error::{AppError, AppResult};

// ---------- IPC types ----------

/// Per-repo outcome of a save / restore run.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceStashRepoOutcome {
    pub repo_path: String,
    pub repo_name: String,
    /// Save: "stashed" | "skipped_clean" | "failed".
    /// Restore: "applied" | "skipped" | "failed".
    pub status: String,
    /// Stash commit oid (set on save / available on restore).
    pub stash_oid: Option<String>,
    pub detail: String,
}

/// Result of a workspace stash save: `id`/`name` of the association record
/// (`id` is None when nothing was stashed, so no record was written) plus
/// the per-repo outcomes.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveWorkspaceStashResult {
    pub id: Option<i64>,
    pub name: String,
    pub items: Vec<WorkspaceStashRepoOutcome>,
}

/// List row for one workspace stash record.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceStashSummary {
    pub id: i64,
    pub name: String,
    pub message: Option<String>,
    pub created_at: String,
    pub repo_count: i64,
}

/// One repo member of a workspace stash (a `workspace_stash_items` row).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceStashItemEntry {
    pub repo_path: String,
    pub stash_oid: String,
    /// Stash stack index at save time (informational; restore re-resolves by
    /// oid because later stashes shift indices).
    pub stash_index: i64,
    /// Branch the repo was on when stashed.
    pub branch: String,
}

/// Pre-restore safety check for one repo (input of the §46 Warning flow).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceStashCheckItem {
    pub repo_path: String,
    pub repo_name: String,
    /// Branch recorded at stash time.
    pub branch: String,
    /// Current branch (None when HEAD is unreadable).
    pub current_branch: Option<String>,
    /// "ok" | "branch_mismatch" | "stash_missing" | "repo_missing" | "error"
    pub status: String,
    pub detail: String,
}

// ---------- Git phase ----------

fn repo_name(repo_path: &str) -> String {
    Path::new(repo_path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string()
}

/// Current branch shorthand (a detached HEAD yields its oid shorthand).
fn current_branch(repo_path: &Path) -> AppResult<String> {
    let repo = git2::Repository::open(repo_path)?;
    let head = repo.head()?;
    Ok(head.shorthand().unwrap_or("HEAD").to_string())
}

/// Git phase of a workspace stash save: stash every repo (T-10 semantics,
/// untracked per flag). Clean repos are skipped and failures are collected
/// per repo — one repo never blocks the rest. Returns the per-repo outcomes
/// plus the successfully stashed items (the record's members).
pub fn stash_repos(
    repo_paths: &[String],
    record_name: &str,
    message: Option<&str>,
    include_untracked: bool,
) -> (Vec<WorkspaceStashRepoOutcome>, Vec<WorkspaceStashItemEntry>) {
    let stash_message = match message {
        Some(m) if !m.trim().is_empty() => format!("[{}] {}", record_name, m.trim()),
        _ => format!("[{}]", record_name),
    };
    let mut outcomes = Vec::with_capacity(repo_paths.len());
    let mut stashed = Vec::new();
    for path in repo_paths {
        let name = repo_name(path);
        let p = Path::new(path);
        let branch = match current_branch(p) {
            Ok(b) => b,
            Err(e) => {
                outcomes.push(WorkspaceStashRepoOutcome {
                    repo_path: path.clone(),
                    repo_name: name,
                    status: "failed".into(),
                    stash_oid: None,
                    detail: format!("读取分支失败：{e}"),
                });
                continue;
            }
        };
        match stash::stash_save(p, Some(&stash_message), include_untracked) {
            Ok(oid) => {
                stashed.push(WorkspaceStashItemEntry {
                    repo_path: path.clone(),
                    stash_oid: oid.clone(),
                    // A fresh stash always lands at stash@{0}.
                    stash_index: 0,
                    branch,
                });
                outcomes.push(WorkspaceStashRepoOutcome {
                    repo_path: path.clone(),
                    repo_name: name,
                    status: "stashed".into(),
                    stash_oid: Some(oid),
                    detail: String::new(),
                });
            }
            // libgit2 reports "there is nothing to stash" as ENOTFOUND.
            Err(AppError::Git(e)) if e.code() == git2::ErrorCode::NotFound => {
                outcomes.push(WorkspaceStashRepoOutcome {
                    repo_path: path.clone(),
                    repo_name: name,
                    status: "skipped_clean".into(),
                    stash_oid: None,
                    detail: "工作区干净，无需暂存".into(),
                });
            }
            Err(e) => {
                outcomes.push(WorkspaceStashRepoOutcome {
                    repo_path: path.clone(),
                    repo_name: name,
                    status: "failed".into(),
                    stash_oid: None,
                    detail: e.to_string(),
                });
            }
        }
    }
    (outcomes, stashed)
}

/// Pre-restore safety check for every recorded item.
pub fn check_restore(items: &[WorkspaceStashItemEntry]) -> Vec<WorkspaceStashCheckItem> {
    items.iter().map(check_item).collect()
}

fn check_item(item: &WorkspaceStashItemEntry) -> WorkspaceStashCheckItem {
    let base = WorkspaceStashCheckItem {
        repo_path: item.repo_path.clone(),
        repo_name: repo_name(&item.repo_path),
        branch: item.branch.clone(),
        current_branch: None,
        status: "error".into(),
        detail: String::new(),
    };
    let path = Path::new(&item.repo_path);
    if !path.exists() {
        return WorkspaceStashCheckItem {
            status: "repo_missing".into(),
            detail: "仓库路径不存在".into(),
            ..base
        };
    }
    let current = match current_branch(path) {
        Ok(b) => b,
        Err(e) => {
            return WorkspaceStashCheckItem {
                status: "error".into(),
                detail: format!("读取仓库失败：{e}"),
                ..base
            };
        }
    };
    // The stash must still be on the repo's stack; resolve by oid because
    // later stashes shift the recorded index.
    let exists = match stash::list_stashes(path) {
        Ok(entries) => entries.iter().any(|e| e.oid == item.stash_oid),
        Err(e) => {
            return WorkspaceStashCheckItem {
                status: "error".into(),
                detail: format!("读取 stash 列表失败：{e}"),
                current_branch: Some(current),
                ..base
            };
        }
    };
    if !exists {
        return WorkspaceStashCheckItem {
            status: "stash_missing".into(),
            detail: "对应 stash 已不在该仓库栈中（可能已被 pop/drop）".into(),
            current_branch: Some(current),
            ..base
        };
    }
    if current != item.branch {
        return WorkspaceStashCheckItem {
            status: "branch_mismatch".into(),
            detail: format!("记录于分支「{}」，当前在「{}」", item.branch, current),
            current_branch: Some(current),
            ..base
        };
    }
    WorkspaceStashCheckItem {
        status: "ok".into(),
        current_branch: Some(current),
        ..base
    }
}

/// Restore phase: re-check each item, then apply the stash (kept on the
/// stack). Repos whose check fails are skipped (a branch mismatch applies
/// only with `allow_branch_mismatch`); one repo's failure never blocks the
/// rest.
pub fn restore_items(items: &[WorkspaceStashItemEntry], allow_branch_mismatch: bool) -> Vec<WorkspaceStashRepoOutcome> {
    items
        .iter()
        .map(|item| {
            let check = check_item(item);
            let applicable = check.status == "ok" || (check.status == "branch_mismatch" && allow_branch_mismatch);
            if !applicable {
                return WorkspaceStashRepoOutcome {
                    repo_path: item.repo_path.clone(),
                    repo_name: check.repo_name,
                    status: "skipped".into(),
                    stash_oid: Some(item.stash_oid.clone()),
                    detail: check.detail,
                };
            }
            apply_item(item)
        })
        .collect()
}

fn apply_item(item: &WorkspaceStashItemEntry) -> WorkspaceStashRepoOutcome {
    let name = repo_name(&item.repo_path);
    let path = Path::new(&item.repo_path);
    let result = (|| -> AppResult<()> {
        let entries = stash::list_stashes(path)?;
        let index = entries
            .iter()
            .position(|e| e.oid == item.stash_oid)
            .ok_or_else(|| AppError::NotFound("stash 已不存在".into()))?;
        stash::stash_apply(path, index)
    })();
    match result {
        Ok(()) => WorkspaceStashRepoOutcome {
            repo_path: item.repo_path.clone(),
            repo_name: name,
            status: "applied".into(),
            stash_oid: Some(item.stash_oid.clone()),
            detail: String::new(),
        },
        Err(e) => WorkspaceStashRepoOutcome {
            repo_path: item.repo_path.clone(),
            repo_name: name,
            status: "failed".into(),
            stash_oid: Some(item.stash_oid.clone()),
            detail: e.to_string(),
        },
    }
}

// ---------- DAO helpers (intentionally local to this module, see header) ----------

/// Next display name for the workspace (`Workspace Stash #N`, N = max id + 1
/// so numbering survives deletions).
pub(crate) fn next_workspace_stash_name(conn: &Connection, workspace_id: i64) -> AppResult<String> {
    let next: i64 = conn.query_row(
        "SELECT COALESCE(MAX(id), 0) + 1 FROM workspace_stashes WHERE workspace_id = ?1",
        rusqlite::params![workspace_id],
        |r| r.get(0),
    )?;
    Ok(format!("Workspace Stash #{}", next))
}

/// Persist one workspace stash + its per-repo items in a single transaction
/// (batch write via prepared statement, global constraint §6).
pub(crate) fn insert_workspace_stash(
    conn: &mut Connection,
    workspace_id: i64,
    name: &str,
    message: Option<&str>,
    items: &[WorkspaceStashItemEntry],
) -> AppResult<i64> {
    let now = chrono::Utc::now().to_rfc3339();
    let tx = conn.transaction()?;
    tx.execute(
        "INSERT INTO workspace_stashes (workspace_id, name, message, created_at) VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![workspace_id, name, message, now],
    )?;
    let id = tx.last_insert_rowid();
    {
        let mut stmt = tx.prepare(
            "INSERT INTO workspace_stash_items (workspace_stash_id, repo_path, stash_oid, stash_index, branch) VALUES (?1, ?2, ?3, ?4, ?5)",
        )?;
        for item in items {
            stmt.execute(rusqlite::params![
                id,
                item.repo_path,
                item.stash_oid,
                item.stash_index,
                item.branch
            ])?;
        }
    }
    tx.commit()?;
    Ok(id)
}

/// List the records of a workspace, newest first, with item counts.
pub(crate) fn list_workspace_stashes(conn: &Connection, workspace_id: i64) -> AppResult<Vec<WorkspaceStashSummary>> {
    let mut stmt = conn.prepare(
        "SELECT s.id, s.name, s.message, s.created_at, COUNT(i.id)
         FROM workspace_stashes s
         LEFT JOIN workspace_stash_items i ON i.workspace_stash_id = s.id
         WHERE s.workspace_id = ?1
         GROUP BY s.id
         ORDER BY s.id DESC",
    )?;
    let rows = stmt.query_map(rusqlite::params![workspace_id], |r| {
        Ok(WorkspaceStashSummary {
            id: r.get(0)?,
            name: r.get(1)?,
            message: r.get(2)?,
            created_at: r.get(3)?,
            repo_count: r.get(4)?,
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// Items of one record, in insertion order.
pub(crate) fn list_workspace_stash_items(
    conn: &Connection,
    workspace_stash_id: i64,
) -> AppResult<Vec<WorkspaceStashItemEntry>> {
    let mut stmt = conn.prepare(
        "SELECT repo_path, stash_oid, stash_index, branch FROM workspace_stash_items WHERE workspace_stash_id = ?1 ORDER BY id",
    )?;
    let rows = stmt.query_map(rusqlite::params![workspace_stash_id], |r| {
        Ok(WorkspaceStashItemEntry {
            repo_path: r.get(0)?,
            stash_oid: r.get(1)?,
            stash_index: r.get(2)?,
            branch: r.get(3)?,
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// Delete one record (items cascade via FK). The per-repo stashes stay on
/// each repo's stack — they remain manageable in the single-repo Stash view
/// (T-10).
pub(crate) fn delete_workspace_stash(conn: &Connection, id: i64) -> AppResult<()> {
    let n = conn.execute("DELETE FROM workspace_stashes WHERE id = ?1", rusqlite::params![id])?;
    if n == 0 {
        return Err(AppError::NotFound(format!("workspace stash {} not found", id)));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "gw_ws_stash_{}_{}",
            tag,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn init_repo(dir: &Path) {
        let repo = git2::Repository::init(dir).unwrap();
        std::fs::write(dir.join("a.txt"), "one\n").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("a.txt")).unwrap();
        index.write().unwrap();
        let tree_oid = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_oid).unwrap();
        let sig = git2::Signature::now("tester", "t@example.com").unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[]).unwrap();
        // stash_save needs a stasher signature from repo config.
        repo.config().unwrap().set_str("user.name", "tester").unwrap();
        repo.config().unwrap().set_str("user.email", "t@example.com").unwrap();
    }

    fn mem_db() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        crate::db::init_db(&mut conn).unwrap();
        conn.execute(
            "INSERT INTO workspaces (name, path, created_at, updated_at) VALUES ('w', 'D:/w', 't', 't')",
            [],
        )
        .unwrap();
        conn
    }

    /// Save across two repos (one dirty, one clean) -> record + items
    /// persisted transactionally -> check ok -> restore brings the change
    /// back. After the repo's stash is dropped the check flips to
    /// stash_missing and restore skips the repo.
    #[test]
    fn save_record_and_restore_roundtrip() {
        let dir = tmpdir("roundtrip");
        let a = dir.join("a");
        let b = dir.join("b");
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        init_repo(&a);
        init_repo(&b);
        std::fs::write(a.join("a.txt"), "one\nwork\n").unwrap();

        let paths = vec![a.to_string_lossy().to_string(), b.to_string_lossy().to_string()];
        let (outcomes, stashed) = stash_repos(&paths, "Workspace Stash #1", Some("sprint work"), true);
        assert_eq!(outcomes.len(), 2);
        assert_eq!(outcomes[0].status, "stashed");
        assert_eq!(outcomes[1].status, "skipped_clean");
        assert_eq!(stashed.len(), 1);
        // The stash message carries the record name so it is recognizable in
        // each repo's single-repo stash list (T-10 view).
        let entries = stash::list_stashes(&a).unwrap();
        assert_eq!(entries.len(), 1);
        assert!(entries[0].message.contains("Workspace Stash #1"));
        assert!(entries[0].message.contains("sprint work"));

        let mut conn = mem_db();
        let id = insert_workspace_stash(&mut conn, 1, "Workspace Stash #1", Some("sprint work"), &stashed).unwrap();

        let list = list_workspace_stashes(&conn, 1).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "Workspace Stash #1");
        assert_eq!(list[0].message.as_deref(), Some("sprint work"));
        assert_eq!(list[0].repo_count, 1);

        let items = list_workspace_stash_items(&conn, id).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].repo_path, paths[0]);
        assert!(!items[0].branch.is_empty());

        // The dirty change was stashed away.
        assert!(!std::fs::read_to_string(a.join("a.txt")).unwrap().contains("work"));

        let checks = check_restore(&items);
        assert_eq!(checks.len(), 1);
        assert_eq!(checks[0].status, "ok", "{}", checks[0].detail);

        let restored = restore_items(&items, false);
        assert_eq!(restored[0].status, "applied", "{}", restored[0].detail);
        assert!(std::fs::read_to_string(a.join("a.txt")).unwrap().contains("work"));
        // Apply keeps the stash on the stack (T-10 semantics).
        assert_eq!(stash::list_stashes(&a).unwrap().len(), 1);

        // Drop the stash: the preflight must flip to stash_missing and the
        // restore must skip the repo instead of failing the batch.
        stash::stash_drop(&a, 0).unwrap();
        let checks = check_restore(&items);
        assert_eq!(checks[0].status, "stash_missing");
        let restored = restore_items(&items, false);
        assert_eq!(restored[0].status, "skipped");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Branch mismatch blocks the restore unless explicitly allowed.
    #[test]
    fn branch_mismatch_blocks_restore_unless_allowed() {
        let dir = tmpdir("mismatch");
        init_repo(&dir);
        std::fs::write(dir.join("a.txt"), "one\nwork\n").unwrap();

        let paths = vec![dir.to_string_lossy().to_string()];
        let (_outcomes, stashed) = stash_repos(&paths, "Workspace Stash #1", None, true);
        assert_eq!(stashed.len(), 1);

        crate::core::branch::create_branch(&dir, "other", None).unwrap();
        crate::core::branch::checkout_branch(&dir, "other").unwrap();

        let checks = check_restore(&stashed);
        assert_eq!(checks[0].status, "branch_mismatch");
        assert_eq!(checks[0].current_branch.as_deref(), Some("other"));

        let restored = restore_items(&stashed, false);
        assert_eq!(restored[0].status, "skipped");
        assert!(!std::fs::read_to_string(dir.join("a.txt")).unwrap().contains("work"));

        let restored = restore_items(&stashed, true);
        assert_eq!(restored[0].status, "applied", "{}", restored[0].detail);
        assert!(std::fs::read_to_string(dir.join("a.txt")).unwrap().contains("work"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Name numbering survives deletions; delete cascades items and reports
    /// unknown ids as NotFound.
    #[test]
    fn record_naming_cascade_and_delete() {
        let mut conn = mem_db();
        assert_eq!(next_workspace_stash_name(&conn, 1).unwrap(), "Workspace Stash #1");

        let item = WorkspaceStashItemEntry {
            repo_path: "D:/w/a".into(),
            stash_oid: "abc123".into(),
            stash_index: 0,
            branch: "main".into(),
        };
        let id1 = insert_workspace_stash(&mut conn, 1, "Workspace Stash #1", None, &[item.clone()]).unwrap();
        assert_eq!(next_workspace_stash_name(&conn, 1).unwrap(), "Workspace Stash #2");
        let id2 = insert_workspace_stash(&mut conn, 1, "Workspace Stash #2", None, &[item]).unwrap();

        assert_eq!(list_workspace_stashes(&conn, 1).unwrap().len(), 2);
        // Deleting a non-max record keeps the numbering monotonic.
        delete_workspace_stash(&conn, id1).unwrap();
        assert_eq!(next_workspace_stash_name(&conn, 1).unwrap(), "Workspace Stash #3");
        // Items of the deleted record cascaded away.
        assert!(list_workspace_stash_items(&conn, id1).unwrap().is_empty());
        assert_eq!(list_workspace_stash_items(&conn, id2).unwrap().len(), 1);

        assert!(delete_workspace_stash(&conn, 999).is_err());
    }
}
