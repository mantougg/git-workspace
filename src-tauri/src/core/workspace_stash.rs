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
use std::sync::atomic::{AtomicBool, Ordering};

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::core::stash;
use crate::error::{AppError, AppResult};
use crate::models::task::TaskType;

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

/// Which half of the workspace stash lifecycle a queued run performs (GF-10).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WorkspaceStashKind {
    Save,
    Restore,
}

/// Rollup of one save / restore run (GF-10). Pure data: derived from the
/// per-repo outcomes and mapped onto the task status (unit-tested below).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceStashRunSummary {
    pub total: usize,
    /// Save: repos stashed. Restore: repos applied.
    pub stashed: usize,
    /// Save: clean repos skipped. Restore: repos skipped by the safety check.
    pub skipped: usize,
    pub failed: usize,
    /// Repos the run never touched because it was cancelled — the
    /// "re-process these" half of the partial-completion contract.
    pub cancelled: usize,
}

impl WorkspaceStashRunSummary {
    /// Count the per-repo outcomes by status (unknown statuses count as
    /// failures so the rollup can never silently hide a repo).
    pub fn from_outcomes(items: &[WorkspaceStashRepoOutcome]) -> Self {
        let mut s = Self {
            total: items.len(),
            ..Self::default()
        };
        for item in items {
            match item.status.as_str() {
                "stashed" | "applied" => s.stashed += 1,
                "skipped_clean" | "skipped" => s.skipped += 1,
                "failed" => s.failed += 1,
                "cancelled" => s.cancelled += 1,
                _ => s.failed += 1,
            }
        }
        s
    }

    /// Task status the worker reports for the run. A cancelled run reports
    /// Cancelled even when some repos already succeeded: the completed subset
    /// is recoverable (see [`WorkspaceStashRunResult`]) but the run as a
    /// whole did not finish. Otherwise the usual rollup: clean → Success,
    /// everything failed → Failed, mixed → PartialSuccess.
    pub fn to_task_status(&self) -> crate::models::task::TaskStatus {
        use crate::models::task::TaskStatus;
        if self.cancelled > 0 {
            TaskStatus::Cancelled
        } else if self.failed == 0 {
            TaskStatus::Success
        } else if self.stashed + self.skipped == 0 {
            TaskStatus::Failed {
                error: format!("{} 个仓库全部失败", self.failed),
            }
        } else {
            TaskStatus::PartialSuccess {
                succeeded: self.stashed + self.skipped,
                failed: self.failed,
            }
        }
    }

    /// One-line summary for the Git Console mirror / task output.
    pub fn summary_line(&self) -> String {
        let mut parts = vec![format!("完成 {} 个仓库", self.stashed)];
        if self.skipped > 0 {
            parts.push(format!("跳过 {}", self.skipped));
        }
        if self.failed > 0 {
            parts.push(format!("失败 {}", self.failed));
        }
        if self.cancelled > 0 {
            parts.push(format!("取消（未处理）{}", self.cancelled));
        }
        parts.join("，")
    }
}

/// Per-repo progress of a queued run (GF-10), emitted after every repo so the
/// TaskPanel can render逐仓进度 while the serial run proceeds.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceStashProgress {
    pub task_id: String,
    pub kind: WorkspaceStashKind,
    pub record_name: String,
    /// 1-based position of the repo inside the run.
    pub index: usize,
    pub total: usize,
    pub repo_path: String,
    pub repo_name: String,
    /// Save: "stashed" | "skipped_clean" | "failed" | "cancelled".
    /// Restore: "applied" | "skipped" | "failed" | "cancelled".
    pub status: String,
    pub detail: String,
}

/// Final result of one queued run (GF-10). The worker records it in the task
/// manager's run registry on **every** completion path (including a cancel),
/// and the pending IPC command takes it from there — the caller therefore
/// always learns which repos were stashed / applied and which were never
/// touched, and can present the recoverable list.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceStashRunResult {
    /// Save: id of the persisted record — also for a cancelled run's
    /// completed subset, so those repos stay restorable through the normal
    /// workspace restore flow. None when nothing was stashed (all clean /
    /// all failed) or the insert failed.
    pub record_id: Option<i64>,
    pub record_name: String,
    /// One entry per selected repo (save) / recorded item (restore).
    pub items: Vec<WorkspaceStashRepoOutcome>,
    pub summary: WorkspaceStashRunSummary,
    /// True when the user cancelled the run mid-way (or before it started).
    pub cancelled: bool,
}

impl WorkspaceStashRunResult {
    pub fn save(record_name: &str, record_id: Option<i64>, items: Vec<WorkspaceStashRepoOutcome>) -> Self {
        Self::from_parts(record_name, record_id, items)
    }

    pub fn restore(record_name: &str, items: Vec<WorkspaceStashRepoOutcome>) -> Self {
        Self::from_parts(record_name, None, items)
    }

    fn from_parts(record_name: &str, record_id: Option<i64>, items: Vec<WorkspaceStashRepoOutcome>) -> Self {
        let summary = WorkspaceStashRunSummary::from_outcomes(&items);
        WorkspaceStashRunResult {
            record_id,
            record_name: record_name.to_string(),
            cancelled: summary.cancelled > 0,
            items,
            summary,
        }
    }

    /// Result for a run cancelled before any repo was processed (the task's
    /// cancel flag was already set when a worker dequeued it): save lists
    /// every selected repo as untouched, restore has no items (nothing was
    /// applied). No record is written either way.
    pub fn cancelled_before_start(kind: WorkspaceStashKind, record_name: &str, repo_paths: &[String]) -> Self {
        let items = match kind {
            WorkspaceStashKind::Save => repo_paths.iter().map(|p| cancelled_outcome(p)).collect(),
            WorkspaceStashKind::Restore => Vec::new(),
        };
        let summary = WorkspaceStashRunSummary::from_outcomes(&items);
        WorkspaceStashRunResult {
            record_id: None,
            record_name: record_name.to_string(),
            cancelled: true,
            items,
            summary,
        }
    }
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

/// Whether the cooperative cancel flag has been set (`None` = never cancels).
pub fn cancel_requested(cancel: Option<&AtomicBool>) -> bool {
    cancel.map(|c| c.load(Ordering::Relaxed)).unwrap_or(false)
}

/// Outcome for a repo the run never reached (cancelled before its turn).
pub(crate) fn cancelled_outcome(repo_path: &str) -> WorkspaceStashRepoOutcome {
    WorkspaceStashRepoOutcome {
        repo_path: repo_path.to_string(),
        repo_name: repo_name(repo_path),
        status: "cancelled".into(),
        stash_oid: None,
        detail: "已取消，未处理".into(),
    }
}

/// Whether a pre-restore check outcome permits applying the stash (GF-10:
/// shared by the pre-enqueue preflight and the execution-time re-check).
pub fn check_allows_apply(status: &str, allow_branch_mismatch: bool) -> bool {
    status == "ok" || (status == "branch_mismatch" && allow_branch_mismatch)
}

/// (kind, record name, repo paths) of a workspace-stash task payload; `None`
/// for every other task type. The worker uses it to resolve a pending IPC
/// command on the early-cancel path, where the run never started.
pub fn ws_stash_task_info(task_type: &TaskType) -> Option<(WorkspaceStashKind, &str, &[String])> {
    match task_type {
        TaskType::WorkspaceStashSave {
            record_name,
            repo_paths,
            ..
        } => Some((WorkspaceStashKind::Save, record_name, repo_paths)),
        TaskType::WorkspaceStashRestore { record_name, .. } => {
            Some((WorkspaceStashKind::Restore, record_name, &[]))
        }
        _ => None,
    }
}

/// Git phase of a workspace stash save: stash every repo (T-10 semantics,
/// untracked per flag). Clean repos are skipped and failures are collected
/// per repo — one repo never blocks the rest. Returns the per-repo outcomes
/// plus the successfully stashed items (the record's members).
///
/// `cancel` is polled before each repo: on a set flag the loop stops and
/// every remaining repo is reported as "cancelled" (untouched), so the caller
/// can present a recoverable list instead of a black hole. `on_repo` fires
/// once per repo outcome (including the untouched ones) to drive live
/// progress (GF-10: the worker emits a `workspace_stash_progress` event from
/// it). Pass `None` / a no-op for the plain serial behaviour.
pub fn stash_repos_cancellable(
    repo_paths: &[String],
    record_name: &str,
    message: Option<&str>,
    include_untracked: bool,
    cancel: Option<&AtomicBool>,
    mut on_repo: impl FnMut(&WorkspaceStashRepoOutcome),
) -> (Vec<WorkspaceStashRepoOutcome>, Vec<WorkspaceStashItemEntry>) {
    let stash_message = match message {
        Some(m) if !m.trim().is_empty() => format!("[{}] {}", record_name, m.trim()),
        _ => format!("[{}]", record_name),
    };
    let mut outcomes = Vec::with_capacity(repo_paths.len());
    let mut stashed = Vec::new();
    for (idx, path) in repo_paths.iter().enumerate() {
        // Cancel checkpoint: everything not yet processed stays untouched and
        // is reported as such (partial-completion contract).
        if cancel_requested(cancel) {
            for rest in &repo_paths[idx..] {
                let outcome = cancelled_outcome(rest);
                on_repo(&outcome);
                outcomes.push(outcome);
            }
            break;
        }
        let name = repo_name(path);
        let p = Path::new(path);
        let branch = match current_branch(p) {
            Ok(b) => b,
            Err(e) => {
                let outcome = WorkspaceStashRepoOutcome {
                    repo_path: path.clone(),
                    repo_name: name,
                    status: "failed".into(),
                    stash_oid: None,
                    detail: format!("读取分支失败：{e}"),
                };
                on_repo(&outcome);
                outcomes.push(outcome);
                continue;
            }
        };
        let outcome = match stash::stash_save(p, Some(&stash_message), include_untracked) {
            Ok(oid) => {
                stashed.push(WorkspaceStashItemEntry {
                    repo_path: path.clone(),
                    stash_oid: oid.clone(),
                    // A fresh stash always lands at stash@{0}.
                    stash_index: 0,
                    branch,
                });
                WorkspaceStashRepoOutcome {
                    repo_path: path.clone(),
                    repo_name: name,
                    status: "stashed".into(),
                    stash_oid: Some(oid),
                    detail: String::new(),
                }
            }
            // libgit2 reports "there is nothing to stash" as ENOTFOUND.
            Err(AppError::Git(e)) if e.code() == git2::ErrorCode::NotFound => WorkspaceStashRepoOutcome {
                repo_path: path.clone(),
                repo_name: name,
                status: "skipped_clean".into(),
                stash_oid: None,
                detail: "工作区干净，无需暂存".into(),
            },
            Err(e) => WorkspaceStashRepoOutcome {
                repo_path: path.clone(),
                repo_name: name,
                status: "failed".into(),
                stash_oid: None,
                detail: e.to_string(),
            },
        };
        on_repo(&outcome);
        outcomes.push(outcome);
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
/// rest. `cancel` is polled before each item; the not-yet-applied items are
/// then reported as "cancelled" (their stashes stay on the stack, so a later
/// restore simply re-applies them — `check_workspace_stash` is the safety net
/// both times). `on_repo` fires once per item outcome (GF-10 progress).
pub fn restore_items_cancellable(
    items: &[WorkspaceStashItemEntry],
    allow_branch_mismatch: bool,
    cancel: Option<&AtomicBool>,
    mut on_repo: impl FnMut(&WorkspaceStashRepoOutcome),
) -> Vec<WorkspaceStashRepoOutcome> {
    let mut outcomes = Vec::with_capacity(items.len());
    for (idx, item) in items.iter().enumerate() {
        if cancel_requested(cancel) {
            for rest in &items[idx..] {
                let outcome = WorkspaceStashRepoOutcome {
                    repo_path: rest.repo_path.clone(),
                    repo_name: repo_name(&rest.repo_path),
                    status: "cancelled".into(),
                    stash_oid: Some(rest.stash_oid.clone()),
                    detail: "已取消，未处理（stash 仍在栈中，可重新恢复）".into(),
                };
                on_repo(&outcome);
                outcomes.push(outcome);
            }
            break;
        }
        let outcome = restore_one(item, allow_branch_mismatch);
        on_repo(&outcome);
        outcomes.push(outcome);
    }
    outcomes
}

/// Re-check and apply one recorded item.
fn restore_one(item: &WorkspaceStashItemEntry, allow_branch_mismatch: bool) -> WorkspaceStashRepoOutcome {
    let check = check_item(item);
    if !check_allows_apply(&check.status, allow_branch_mismatch) {
        return WorkspaceStashRepoOutcome {
            repo_path: item.repo_path.clone(),
            repo_name: check.repo_name,
            status: "skipped".into(),
            stash_oid: Some(item.stash_oid.clone()),
            detail: check.detail,
        };
    }
    apply_item(item)
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

/// Display name of one record (GF-10: used as the queued restore task's
/// label). Unknown ids surface as NotFound.
pub(crate) fn workspace_stash_name(conn: &Connection, workspace_stash_id: i64) -> AppResult<String> {
    let name: String = conn.query_row(
        "SELECT name FROM workspace_stashes WHERE id = ?1",
        rusqlite::params![workspace_stash_id],
        |r| r.get(0),
    )?;
    Ok(name)
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
    use crate::models::task::TaskStatus;

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
        let (outcomes, stashed) =
            stash_repos_cancellable(&paths, "Workspace Stash #1", Some("sprint work"), true, None, |_| {});
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

        let restored = restore_items_cancellable(&items, false, None, |_| {});
        assert_eq!(restored[0].status, "applied", "{}", restored[0].detail);
        assert!(std::fs::read_to_string(a.join("a.txt")).unwrap().contains("work"));
        // Apply keeps the stash on the stack (T-10 semantics).
        assert_eq!(stash::list_stashes(&a).unwrap().len(), 1);

        // Drop the stash: the preflight must flip to stash_missing and the
        // restore must skip the repo instead of failing the batch.
        stash::stash_drop(&a, 0).unwrap();
        let checks = check_restore(&items);
        assert_eq!(checks[0].status, "stash_missing");
        let restored = restore_items_cancellable(&items, false, None, |_| {});
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
        let (_outcomes, stashed) =
            stash_repos_cancellable(&paths, "Workspace Stash #1", None, true, None, |_| {});
        assert_eq!(stashed.len(), 1);

        crate::core::branch::create_branch(&dir, "other", None).unwrap();
        crate::core::branch::checkout_branch(&dir, "other").unwrap();

        let checks = check_restore(&stashed);
        assert_eq!(checks[0].status, "branch_mismatch");
        assert_eq!(checks[0].current_branch.as_deref(), Some("other"));

        let restored = restore_items_cancellable(&stashed, false, None, |_| {});
        assert_eq!(restored[0].status, "skipped");
        assert!(!std::fs::read_to_string(dir.join("a.txt")).unwrap().contains("work"));

        let restored = restore_items_cancellable(&stashed, true, None, |_| {});
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

    // -----------------------------------------------------------------------
    // GF-10: queued runs (serial + cancel + progress + partial completion)
    // -----------------------------------------------------------------------

    fn outcome(status: &str) -> WorkspaceStashRepoOutcome {
        WorkspaceStashRepoOutcome {
            repo_path: format!("D:/w/{status}"),
            repo_name: status.into(),
            status: status.into(),
            stash_oid: None,
            detail: String::new(),
        }
    }

    /// Rollup → task status mapping: clean → Success, mixed → PartialSuccess,
    /// all failed → Failed, any untouched repo → Cancelled (even when other
    /// repos succeeded — the run as a whole did not finish).
    #[test]
    fn run_summary_maps_to_task_status() {
        let clean = [outcome("stashed"), outcome("skipped_clean")];
        let s = WorkspaceStashRunSummary::from_outcomes(&clean);
        assert_eq!((s.total, s.stashed, s.skipped, s.failed, s.cancelled), (2, 1, 1, 0, 0));
        assert!(matches!(s.to_task_status(), TaskStatus::Success));

        let mixed = [outcome("applied"), outcome("failed"), outcome("skipped")];
        let s = WorkspaceStashRunSummary::from_outcomes(&mixed);
        assert_eq!((s.stashed, s.skipped, s.failed), (1, 1, 1));
        match s.to_task_status() {
            TaskStatus::PartialSuccess { succeeded, failed } => {
                assert_eq!((succeeded, failed), (2, 1));
            }
            other => panic!("expected PartialSuccess, got {:?}", other),
        }

        let all_failed = [outcome("failed"), outcome("failed")];
        let s = WorkspaceStashRunSummary::from_outcomes(&all_failed);
        assert!(matches!(s.to_task_status(), TaskStatus::Failed { .. }));

        let cancelled = [outcome("stashed"), outcome("cancelled")];
        let s = WorkspaceStashRunSummary::from_outcomes(&cancelled);
        assert!(matches!(s.to_task_status(), TaskStatus::Cancelled));
        assert!(s.summary_line().contains("取消（未处理）1"));
        assert!(s.summary_line().starts_with("完成 1 个仓库"));
    }

    /// An unknown status counts as a failure so the rollup cannot silently
    /// drop a repo from every bucket.
    #[test]
    fn run_summary_counts_unknown_status_as_failed() {
        let s = WorkspaceStashRunSummary::from_outcomes(&[outcome("weird_new_status")]);
        assert_eq!((s.failed, s.total), (1, 1));
        assert!(matches!(s.to_task_status(), TaskStatus::Failed { .. }));
    }

    /// Cancel before the run starts: save lists every selected repo as
    /// untouched (the recoverable list), restore has no items; no record id.
    #[test]
    fn cancelled_before_start_result_shape() {
        let paths = vec!["D:/w/a".to_string(), "D:/w/b".to_string()];
        let run =
            WorkspaceStashRunResult::cancelled_before_start(WorkspaceStashKind::Save, "Workspace Stash #1", &paths);
        assert!(run.cancelled);
        assert!(run.record_id.is_none());
        assert_eq!(run.items.len(), 2);
        assert!(run.items.iter().all(|i| i.status == "cancelled"));
        assert_eq!(run.items[1].repo_name, "b");
        assert_eq!(run.summary.cancelled, 2);

        let run = WorkspaceStashRunResult::cancelled_before_start(WorkspaceStashKind::Restore, "Workspace Stash #1", &[]);
        assert!(run.cancelled);
        assert!(run.items.is_empty());
    }

    /// The save constructor keeps a cancelled run's completed subset
    /// restorable: record id + per-repo statuses survive in the result.
    #[test]
    fn save_result_keeps_partial_record() {
        let run = WorkspaceStashRunResult::save(
            "Workspace Stash #1",
            Some(7),
            vec![outcome("stashed"), outcome("cancelled")],
        );
        assert_eq!(run.record_id, Some(7));
        assert!(run.cancelled);
        assert_eq!(run.items.len(), 2);
        assert_eq!(run.record_name, "Workspace Stash #1");
    }

    /// Mid-run cancel: the loop stops before the next repo, the remaining
    /// repos are reported untouched (their worktree change is still there),
    /// and only the processed repo lands in the record members.
    #[test]
    fn cancel_mid_save_reports_untouched_repos() {
        let dir = tmpdir("cancel_save");
        let a = dir.join("a");
        let b = dir.join("b");
        let c = dir.join("c");
        for r in [&a, &b, &c] {
            std::fs::create_dir_all(r).unwrap();
            init_repo(r);
            std::fs::write(r.join("a.txt"), "one\nwork\n").unwrap();
        }
        let paths: Vec<String> = [&a, &b, &c]
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect();

        let flag = AtomicBool::new(false);
        let mut seen = 0usize;
        let (outcomes, stashed) = stash_repos_cancellable(&paths, "Workspace Stash #1", None, true, Some(&flag), |_| {
            seen += 1;
            // Cancel right after the first repo is processed.
            if seen == 1 {
                flag.store(true, Ordering::Relaxed);
            }
        });

        assert_eq!(seen, 3, "every repo must be reported exactly once");
        assert_eq!(outcomes[0].status, "stashed");
        assert_eq!(outcomes[1].status, "cancelled");
        assert_eq!(outcomes[2].status, "cancelled");
        assert_eq!(stashed.len(), 1);
        // Untouched repos keep their working-tree change (recoverable).
        assert!(std::fs::read_to_string(b.join("a.txt")).unwrap().contains("work"));
        assert!(std::fs::read_to_string(c.join("a.txt")).unwrap().contains("work"));
        // The processed repo really was stashed.
        assert!(!std::fs::read_to_string(a.join("a.txt")).unwrap().contains("work"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Mid-run cancel on restore: the not-yet-applied items are reported
    /// cancelled and their stashes stay on the stack (re-restorable).
    #[test]
    fn cancel_mid_restore_leaves_stashes_on_stack() {
        let dir = tmpdir("cancel_restore");
        let a = dir.join("a");
        let b = dir.join("b");
        for r in [&a, &b] {
            std::fs::create_dir_all(r).unwrap();
            init_repo(r);
            std::fs::write(r.join("a.txt"), "one\nwork\n").unwrap();
        }
        let paths: Vec<String> = [&a, &b]
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect();
        let (_outcomes, stashed) =
            stash_repos_cancellable(&paths, "Workspace Stash #1", None, true, None, |_| {});
        assert_eq!(stashed.len(), 2);

        let flag = AtomicBool::new(false);
        let mut seen = 0usize;
        let outcomes = restore_items_cancellable(&stashed, false, Some(&flag), |_| {
            seen += 1;
            if seen == 1 {
                flag.store(true, Ordering::Relaxed);
            }
        });
        assert_eq!(outcomes[0].status, "applied");
        assert_eq!(outcomes[1].status, "cancelled");
        assert!(outcomes[1].detail.contains("栈中"));
        // First repo's change came back, second repo's did not.
        assert!(std::fs::read_to_string(a.join("a.txt")).unwrap().contains("work"));
        assert!(!std::fs::read_to_string(b.join("a.txt")).unwrap().contains("work"));
        // The cancelled repo's stash is still on its stack.
        assert_eq!(stash::list_stashes(&b).unwrap().len(), 1);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Pre-enqueue preflight gate (GF-10 checklist #3): only "ok" (or an
    /// explicitly allowed branch mismatch) permits an apply.
    #[test]
    fn check_allows_apply_gate() {
        assert!(check_allows_apply("ok", false));
        assert!(!check_allows_apply("branch_mismatch", false));
        assert!(check_allows_apply("branch_mismatch", true));
        for blocked in ["stash_missing", "repo_missing", "error"] {
            assert!(!check_allows_apply(blocked, true));
        }
    }

    /// Progress callback fires once per repo in order, including the
    /// untouched tail of a cancelled run.
    #[test]
    fn progress_callback_reports_every_repo_once() {
        let dir = tmpdir("progress");
        let a = dir.join("a");
        let b = dir.join("b");
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        init_repo(&a);
        init_repo(&b); // b stays clean -> skipped_clean
        std::fs::write(a.join("a.txt"), "one\nwork\n").unwrap();
        let paths: Vec<String> = [&a, &b]
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect();

        let mut seen: Vec<(String, String)> = Vec::new();
        let (outcomes, _stashed) = stash_repos_cancellable(&paths, "Workspace Stash #1", None, true, None, |o| {
            seen.push((o.repo_name.clone(), o.status.clone()));
        });
        assert_eq!(seen.len(), outcomes.len());
        assert_eq!(seen[0].1, "stashed");
        assert_eq!(seen[1].1, "skipped_clean");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
