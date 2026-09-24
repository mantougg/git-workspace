//! Batch operation commands (T-20): selector queries, bulk branch ops and
//! dry-run impact reports. Multi-repo work always goes through the T-05 task
//! queue (per-repo sub-results + Partial Success aggregation).

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rayon::prelude::*;
use rusqlite::Connection;
use serde::Serialize;
use tauri::State;

use crate::core::git_ops::GitOps;
use crate::core::git_status;
use crate::core::merge::{self, MergeOutcome};
use crate::core::operation_log::{self, NewOperationLogItem};
use crate::core::rebase::{self, RebaseOutcome};
use crate::core::selector::{self, RepoFacet};
use crate::db::dao;
use crate::error::{AppError, AppResult};
use crate::models::task::{BranchOpKind, TaskRequest, TaskStatus, TaskType};
use crate::process::OutputStream;
use crate::state::AppState;
use crate::task::console::{emit_git_op_finished, emit_git_op_started, ConsoleStreamer};
use crate::task::manager::TaskManager;

/// GF-07 同步网络命令超时预算（单仓 fetch/pull 同一量级，逐仓沿用）。
use super::git_ops::SYNC_GIT_TIMEOUT;

/// Query repositories of a workspace with the selector syntax (T-20 §52):
/// `@group:` / `@tag:` / `@status:` tokens and plain text, ANDed. Filtering
/// happens in memory over the repo list + status cache — no DB scan per
/// keystroke, no git processes spawned.
#[tauri::command]
pub fn select_repos(workspace_id: i64, query: String, state: State<'_, AppState>) -> AppResult<Vec<String>> {
    let conn = state
        .db
        .lock()
        .map_err(|e| AppError::Other(format!("DB lock error: {}", e)))?;
    facet_repo_paths(&conn, &state.status_cache, workspace_id, &query)
}

/// `select_repos` 的核心：选择器 → 仓库路径列表。
/// T-28 符号搜索的 `@group:` / `@status:` 过滤复用同一 facet 引擎。
pub(crate) fn facet_repo_paths(
    conn: &rusqlite::Connection,
    status_cache: &std::sync::Arc<moka::sync::Cache<String, crate::models::repository::RepoStatus>>,
    workspace_id: i64,
    query: &str,
) -> AppResult<Vec<String>> {
    let repos = dao::list_repositories_by_workspace(conn, workspace_id)?;
    let groups = dao::list_groups(conn, workspace_id)?;
    let group_names: std::collections::HashMap<i64, String> = groups.into_iter().map(|g| (g.id, g.name)).collect();

    let facets: Vec<RepoFacet> = repos
        .into_par_iter()
        .map(|r| {
            // T-02 status cache first; on miss, one live read that also
            // backfills the cache (same pattern as change_set/health). This
            // keeps @status:* selectors working even when the page was
            // entered directly without a prior list_repositories/scan.
            let status = match status_cache.get(&r.path) {
                Some(s) => Some(s),
                None => git_status::get_repo_status(Path::new(&r.path)).ok().map(|s| {
                    status_cache.insert(r.path.clone(), s.clone());
                    s
                }),
            };
            let (ahead, behind, dirty, detached) = match &status {
                Some(s) => (s.ahead > 0, s.behind > 0, !s.is_clean, s.is_detached),
                None => (false, false, false, false),
            };
            RepoFacet {
                path: r.path.clone(),
                name: r.name.clone(),
                group: r.group_id.and_then(|id| group_names.get(&id).cloned()),
                tags: r.tags.clone(),
                dirty,
                conflicted: has_conflict_marker(Path::new(&r.path)),
                ahead,
                behind,
                detached,
                favorite: r.is_favorite,
            }
        })
        .collect();

    Ok(selector::select_paths(query, &facets))
}

/// Cheap conflict probe: in-progress merge/rebase/cherry-pick/revert markers
/// under the repo's git dir (handles both `.git` dir and worktree `.git`
/// file forms). No libgit2 involved.
fn has_conflict_marker(repo_path: &Path) -> bool {
    let git_dir = resolve_git_dir(repo_path);
    let Some(git_dir) = git_dir else {
        return false;
    };
    ["MERGE_HEAD", "CHERRY_PICK_HEAD", "REVERT_HEAD", "REBASE_HEAD"]
        .iter()
        .any(|m| git_dir.join(m).exists())
        || git_dir.join("rebase-merge").exists()
        || git_dir.join("rebase-apply").exists()
}

/// Resolve the real git dir: `.git` directory, or the `gitdir:` target of a
/// `.git` file (worktree form, T-17).
fn resolve_git_dir(repo_path: &Path) -> Option<std::path::PathBuf> {
    let dotgit = repo_path.join(".git");
    if dotgit.is_dir() {
        return Some(dotgit);
    }
    let content = std::fs::read_to_string(&dotgit).ok()?;
    let target = content.trim().strip_prefix("gitdir:")?.trim();
    let p = Path::new(target);
    if p.is_absolute() {
        Some(p.to_path_buf())
    } else {
        Some(repo_path.join(p))
    }
}

/// Submit a bulk branch operation (T-20): checkout / create / delete the
/// named branch in each repo, through the task queue. `force` only applies
/// to delete (unmerged branches).
///
/// T-34: for the reversible ops (checkout / delete / create) a per-repo ref
/// snapshot is captured BEFORE submission (queued tasks may start
/// immediately), and the operation log is written after the queue accepts the
/// batch — a rejected submission leaves no fake record. GF-16: the async
/// tasks' results are unknown at submit time, so `after_oid` is backfilled
/// per repo once its task finishes (see `backfill_batch_after_oids`).
#[tauri::command]
pub fn batch_branch_op(
    repo_paths: Vec<String>,
    op: BranchOpKind,
    name: String,
    force: bool,
    state: State<'_, AppState>,
) -> AppResult<Vec<String>> {
    // T-34 ref snapshots (pure data; repos without a loggable ref — unborn
    // HEAD or missing branch — are simply not undoable and skipped).
    let snapshots: Vec<NewOperationLogItem> = match op {
        BranchOpKind::Checkout | BranchOpKind::Create => repo_paths
            .iter()
            .filter_map(|p| {
                operation_log::snapshot_head(Path::new(p)).map(|(head_ref, oid)| NewOperationLogItem {
                    repo_path: p.clone(),
                    // Checkout: the pre-op branch is the undo target (switch
                    // back). Create: the new branch is the undo target
                    // (delete it again at the recorded tip).
                    ref_name: if matches!(op, BranchOpKind::Create) {
                        name.clone()
                    } else {
                        head_ref
                    },
                    before_oid: oid,
                    // Executed asynchronously by the task queue: the after
                    // state is unknown at submit time and stays NULL until
                    // the backfill poller below fills it in.
                    after_oid: None,
                    detail: Some(if matches!(op, BranchOpKind::Create) {
                        format!("创建目标分支：{name}")
                    } else {
                        format!("检出目标分支：{name}")
                    }),
                })
            })
            .collect(),
        BranchOpKind::Delete => repo_paths
            .iter()
            .filter_map(|p| {
                operation_log::snapshot_branch(Path::new(p), &name).map(|(ref_name, oid)| NewOperationLogItem {
                    repo_path: p.clone(),
                    ref_name,
                    before_oid: oid,
                    after_oid: None,
                    detail: Some(if force {
                        "force 删除".to_string()
                    } else {
                        "删除已合入分支".to_string()
                    }),
                })
            })
            .collect(),
    };

    let requests: Vec<TaskRequest> = repo_paths
        .iter()
        .map(|p| {
            let name_repo = Path::new(p)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown")
                .to_string();
            TaskRequest {
                task_type: TaskType::BranchOp {
                    op,
                    name: name.clone(),
                    force,
                },
                repo_path: p.clone(),
                repo_name: name_repo,
            }
        })
        .collect();
    let task_ids = state.task_manager.submit(&requests)?;

    // R-21 §48：批量 Checkout 提交后通知 Git 联动引擎复核 POM 变化
    //（提交时刻记录 pre-fingerprint；重试窗口覆盖「重算先于切换完成」竞态）。
    if matches!(op, BranchOpKind::Checkout) {
        for p in &repo_paths {
            state.git_link.notify_branch_switched(p);
        }
    }

    // T-34: record the accepted batch (best-effort; a log failure must not
    // fail the already-queued operation). The returned log id is needed for
    // the after-oid backfill.
    let mut log_id = None;
    if !snapshots.is_empty() {
        let (op_type, summary) = match op {
            BranchOpKind::Checkout => (
                operation_log::OP_CHECKOUT_ALL,
                format!("批量检出分支 '{name}'（{} 个仓库）", snapshots.len()),
            ),
            BranchOpKind::Create => (
                operation_log::OP_CREATE_BRANCH_ALL,
                format!("批量创建分支 '{name}'（{} 个仓库）", snapshots.len()),
            ),
            BranchOpKind::Delete => (
                operation_log::OP_DELETE_BRANCH_ALL,
                format!("批量删除分支 '{name}'（{} 个仓库）", snapshots.len()),
            ),
        };
        if let Some(first) = repo_paths.first() {
            log_id = operation_log::record_operation_log(&state.db, first, op_type, &summary, snapshots);
        }
    }

    // GF-16: backfill after-oids once the queued tasks finish, so the undo
    // preview shows the full before → after state. Design notes: a task
    // completion callback is not reachable from a command without touching
    // the GF-10 task module (off-limits), and pre-computing the after state
    // would record a *speculative* value for tasks that may fail — so the
    // backfill polls `TaskManager::get_status` from a detached blocking task
    // and only writes the state of repos whose task actually succeeded.
    // F-43: sync command => detach via `tauri::async_runtime::spawn`, never a
    // bare `tokio::spawn`.
    if let Some(log_id) = log_id {
        if matches!(op, BranchOpKind::Checkout | BranchOpKind::Create) {
            let db = Arc::clone(&state.db);
            let task_manager = Arc::clone(&state.task_manager);
            let pairs: Vec<(String, String)> = task_ids
                .iter()
                .zip(repo_paths.iter())
                .map(|(t, p)| (t.clone(), p.clone()))
                .collect();
            let branch_name = name.clone();
            tauri::async_runtime::spawn(async move {
                let _ = tokio::task::spawn_blocking(move || {
                    backfill_batch_after_oids(&db, &task_manager, log_id, &pairs, &branch_name, op);
                })
                .await;
            });
        }
    }

    Ok(task_ids)
}

/// Poll interval / cap for the batch after-oid backfill. Branch ops are local
/// and fast; the cap only bounds the poller when tasks are stuck or the
/// manager lost track of them (the last poll's result is either way a
/// best-effort snapshot — undo re-checks the live state).
const BACKFILL_POLL_INTERVAL: Duration = Duration::from_millis(500);
const BACKFILL_MAX_WAIT: Duration = Duration::from_secs(15 * 60);

/// Terminal task statuses (children never become PartialSuccess; the
/// synthetic batch row does, so it is included for safety).
fn is_terminal(status: &TaskStatus) -> bool {
    matches!(
        status,
        TaskStatus::Success | TaskStatus::Failed { .. } | TaskStatus::Cancelled | TaskStatus::PartialSuccess { .. }
    )
}

/// Wait for the batch's per-repo tasks to finish, then write each successful
/// repo's current ref state into the operation log's `after_oid` (only rows
/// still NULL — a known snapshot is never overwritten). Repos whose task
/// failed or was cancelled keep NULL: the undo preview then refuses them,
/// which is the honest outcome.
fn backfill_batch_after_oids(
    db: &Arc<Mutex<Connection>>,
    task_manager: &TaskManager,
    log_id: i64,
    pairs: &[(String, String)],
    branch_name: &str,
    op: BranchOpKind,
) {
    let task_ids: Vec<String> = pairs.iter().map(|(t, _)| t.clone()).collect();
    let deadline = Instant::now() + BACKFILL_MAX_WAIT;
    let statuses = loop {
        let statuses = task_manager.get_status(&task_ids);
        // A task id the manager no longer tracks has finished (the worker
        // drops entries ~30s after completion; its last status was terminal
        // and the poller saw it before the drop).
        let pending = task_ids.iter().any(|id| {
            statuses
                .iter()
                .find(|t| &t.id == id)
                .map(|t| !is_terminal(&t.status))
                .unwrap_or(false)
        });
        if !pending || Instant::now() >= deadline {
            break statuses;
        }
        std::thread::sleep(BACKFILL_POLL_INTERVAL);
    };

    let updates: Vec<(String, String)> = pairs
        .iter()
        .filter_map(|(task_id, repo_path)| {
            // Unknown outcome (manager dropped the entry) is treated as
            // success for the backfill: reading the live state is harmless
            // because undo re-checks it, and a failed op leaves the repo at
            // its before state (which the undo plan then reports as
            // "无需撤销").
            let succeeded = statuses
                .iter()
                .find(|t| &t.id == task_id)
                .map(|t| matches!(t.status, TaskStatus::Success))
                .unwrap_or(true);
            if !succeeded {
                return None;
            }
            let after = match op {
                BranchOpKind::Checkout => operation_log::snapshot_head(Path::new(repo_path)).map(|(_, oid)| oid),
                BranchOpKind::Create => operation_log::snapshot_branch(Path::new(repo_path), branch_name)
                    .map(|(_, oid)| oid),
                // The branch is gone after a delete — there is no after ref
                // to record (the UI renders NULL as "已删除").
                BranchOpKind::Delete => None,
            }?;
            Some((repo_path.clone(), after))
        })
        .collect();

    if updates.is_empty() {
        return;
    }
    match db.lock() {
        Ok(mut conn) => {
            if let Err(e) = operation_log::backfill_after_oids(&mut conn, log_id, &updates) {
                log::warn!("T-34: batch after-oid backfill failed: {}", e);
            }
        }
        Err(e) => log::warn!("T-34: batch after-oid backfill DB lock failed: {}", e),
    }
}

/// One repo's dry-run outcome (T-20, Roadmap 评审增量: 批量预演影响报告).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DryRunItem {
    pub repo_path: String,
    pub repo_name: String,
    /// "up_to_date" | "fast_forward" | "diverged" | "conflict" |
    /// "no_upstream" | "error"
    pub category: String,
    pub ahead: u32,
    pub behind: u32,
    pub detail: String,
}

/// Dry-run Pull/Push over many repos (T-20): computes the predicted outcome
/// from *local* remote-tracking refs only — no network fetch, no repo
/// mutation (global constraints §3). Diverged pulls are conflict-predicted
/// via an in-memory `merge_commits`.
///
/// CPU-bound per-repo work is parallelized with rayon (no git CLI processes
/// are forked, so §45 process limits do not apply here).
#[tauri::command]
pub fn batch_dry_run(repo_paths: Vec<String>, op: String) -> AppResult<Vec<DryRunItem>> {
    if op != "pull" && op != "push" {
        return Err(AppError::Other(format!("不支持的 dry-run 类型：{op}")));
    }
    let items: Vec<DryRunItem> = repo_paths.par_iter().map(|p| dry_run_repo(p, &op)).collect();
    Ok(items)
}

fn dry_run_repo(repo_path: &str, op: &str) -> DryRunItem {
    let name = Path::new(repo_path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string();
    let mut item = DryRunItem {
        repo_path: repo_path.to_string(),
        repo_name: name,
        category: "error".to_string(),
        ahead: 0,
        behind: 0,
        detail: String::new(),
    };

    let result = (|| -> AppResult<()> {
        let repo = git2::Repository::open(repo_path)?;
        let head = repo.head()?;
        let local_oid = head
            .target()
            .ok_or_else(|| AppError::Other("HEAD 未指向提交".to_string()))?;
        let branch_name = head
            .shorthand()
            .ok_or_else(|| AppError::Other("HEAD 异常".to_string()))?
            .to_string();
        let branch = repo.find_branch(&branch_name, git2::BranchType::Local)?;
        let upstream = match branch.upstream() {
            Ok(u) => u,
            Err(_) => {
                item.category = "no_upstream".to_string();
                item.detail = "没有上游分支".to_string();
                return Ok(());
            }
        };
        let upstream_oid = upstream
            .get()
            .target()
            .ok_or_else(|| AppError::Other("上游引用异常".to_string()))?;

        let (ahead, behind) = repo.graph_ahead_behind(local_oid, upstream_oid)?;
        item.ahead = ahead as u32;
        item.behind = behind as u32;

        match op {
            "pull" => {
                if behind == 0 {
                    item.category = "up_to_date".to_string();
                    item.detail = "无需拉取".to_string();
                } else if ahead == 0 {
                    item.category = "fast_forward".to_string();
                    item.detail = format!("可快进 {} 个提交", behind);
                } else {
                    // Diverged: predict conflicts with an in-memory merge.
                    let local_commit = repo.find_commit(local_oid)?;
                    let their_commit = repo.find_commit(upstream_oid)?;
                    let merge_index = repo.merge_commits(&local_commit, &their_commit, None)?;
                    if merge_index.has_conflicts() {
                        item.category = "conflict".to_string();
                        item.detail = format!("分叉（前 {ahead} / 后 {behind}），预计冲突");
                    } else {
                        item.category = "diverged".to_string();
                        item.detail = format!("分叉（前 {ahead} / 后 {behind}），可合并无冲突");
                    }
                }
            }
            _ => {
                // push
                if ahead == 0 && behind == 0 {
                    item.category = "up_to_date".to_string();
                    item.detail = "与上游一致".to_string();
                } else if behind > 0 {
                    item.category = "diverged".to_string();
                    item.detail = format!("落后上游 {behind} 个提交，推送将被拒（需先 Pull）");
                } else {
                    item.category = "fast_forward".to_string();
                    item.detail = format!("可推送 {ahead} 个提交");
                }
            }
        }
        Ok(())
    })();

    if let Err(e) = result {
        item.category = "error".to_string();
        item.detail = e.to_string();
    }
    item
}

// ---------------------------------------------------------------------------
// GF-15：批量 Pull 分叉跟进（diverged follow-up）
//
// dry-run 已能识别分叉仓库（`dry_run_repo` 的 ahead/behind 判定），缺的是
// 「分叉 → 选 merge / rebase / --ff-only 重试策略批量执行」的闭环。本节补齐：
// 执行前逐仓 fetch（remote-tracking 刷新，避免拿过期 ref 合并）+ 复判分叉，
// 再按策略落地；冲突仓库留在 merge/rebase 状态进前端 smartMergeQueue，
// 其余仓库不受影响（部分完成语义）。
// ---------------------------------------------------------------------------

/// Follow-up strategy for diverged repos (GF-15).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DivergedStrategy {
    /// Create a merge commit (`core::merge::merge` normal mode; a repo that
    /// turned out behind-only fast-forwards instead).
    Merge,
    /// Replay the local commits onto the upstream tip
    /// (`core::rebase::start_rebase` with the default pick todo — the
    /// interactive batch variant is GF-20 scope, not here).
    Rebase,
    /// Retry `git pull --ff-only` (the remote may have moved or been
    /// force-pushed since the dry-run; a still-diverged repo keeps failing).
    FfOnly,
}

impl DivergedStrategy {
    /// Human label for logs / UI copy.
    pub fn label(self) -> &'static str {
        match self {
            DivergedStrategy::Merge => "Merge",
            DivergedStrategy::Rebase => "Rebase",
            DivergedStrategy::FfOnly => "--ff-only 重试",
        }
    }

    /// Git Console command title line.
    fn console_command(self) -> &'static str {
        match self {
            DivergedStrategy::Merge => "git merge <upstream>",
            DivergedStrategy::Rebase => "git rebase <upstream>",
            DivergedStrategy::FfOnly => "git pull --ff-only",
        }
    }
}

/// Parse the strategy token from the frontend ("merge" | "rebase" | "ff_only").
fn parse_strategy(s: &str) -> AppResult<DivergedStrategy> {
    match s {
        "merge" => Ok(DivergedStrategy::Merge),
        "rebase" => Ok(DivergedStrategy::Rebase),
        "ff_only" | "ff-only" | "ffonly" => Ok(DivergedStrategy::FfOnly),
        other => Err(AppError::Other(format!(
            "不支持的分叉跟进策略：{other}（merge / rebase / ff_only）"
        ))),
    }
}

/// Whether the pull follow-up still applies to a repo *after* the
/// pre-execution fetch — the dry-run classification was computed from
/// remote-tracking refs that may have been stale (pure; unit-tested).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DivergenceClass {
    /// Nothing incoming (behind == 0): no pull work left, strategy irrelevant.
    NothingToPull,
    /// Only incoming commits (ahead == 0): a plain fast-forward suffices —
    /// the strategy choice does not change the outcome.
    BehindOnly,
    /// Local + remote both moved: the case merge / rebase / --ff-only
    /// disagree on — the strategies target exactly this.
    Diverged,
}

fn classify_divergence(ahead: usize, behind: usize) -> DivergenceClass {
    if behind == 0 {
        DivergenceClass::NothingToPull
    } else if ahead == 0 {
        DivergenceClass::BehindOnly
    } else {
        DivergenceClass::Diverged
    }
}

/// One repo's follow-up outcome (GF-15). Partial completion is the norm: a
/// conflicted or failed repo never stops the rest of the batch.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DivergedFollowupItem {
    pub repo_path: String,
    pub repo_name: String,
    /// "merged" | "rebased" | "up_to_date" | "conflict" | "failed" |
    /// "skipped" | "cancelled"
    pub outcome: String,
    /// outcome == "conflict"：驱动冲突队列走哪套 continue/abort——
    /// "merge"（SmartMergeDialog）| "rebase"（skip/abort/冲突解决器）。
    pub conflict_op: Option<String>,
    /// Conflicted files (conflict outcome).
    pub files: Vec<String>,
    /// HEAD before the operation (abort target hint for the conflict queue).
    pub base_oid: Option<String>,
    /// Local commits replayed onto the upstream tip (rebase success).
    pub rewritten: u32,
    /// 执行时复判的影响范围：本地领先 / 远程领先提交数（Safety First 展示）。
    pub ahead: u32,
    pub behind: u32,
    pub detail: String,
}

impl DivergedFollowupItem {
    fn new(repo_path: &str) -> Self {
        DivergedFollowupItem {
            repo_path: repo_path.to_string(),
            repo_name: super::git_ops::repo_display_name(repo_path),
            outcome: "failed".to_string(),
            conflict_op: None,
            files: Vec::new(),
            base_oid: None,
            rewritten: 0,
            ahead: 0,
            behind: 0,
            detail: String::new(),
        }
    }
}

/// Aggregate counts of a follow-up batch (pure; unit-tested for the partial
/// completion semantics — conflicts / failures never hide the successes).
#[derive(Debug, Default, PartialEq, Eq)]
struct FollowupSummary {
    merged: usize,
    rebased: usize,
    up_to_date: usize,
    conflicts: usize,
    failed: usize,
    skipped: usize,
    cancelled: usize,
}

impl FollowupSummary {
    fn of(items: &[DivergedFollowupItem]) -> Self {
        let mut s = FollowupSummary::default();
        for item in items {
            match item.outcome.as_str() {
                "merged" => s.merged += 1,
                "rebased" => s.rebased += 1,
                "up_to_date" => s.up_to_date += 1,
                "conflict" => s.conflicts += 1,
                "failed" => s.failed += 1,
                "skipped" => s.skipped += 1,
                "cancelled" => s.cancelled += 1,
                _ => {}
            }
        }
        s
    }

    /// One-line log / finished-event summary.
    fn one_line(&self) -> String {
        let mut parts = Vec::new();
        if self.merged > 0 {
            parts.push(format!("合并 {} 个", self.merged));
        }
        if self.rebased > 0 {
            parts.push(format!("变基 {} 个", self.rebased));
        }
        if self.up_to_date > 0 {
            parts.push(format!("无需跟进 {}", self.up_to_date));
        }
        if self.conflicts > 0 {
            parts.push(format!("冲突 {} 个", self.conflicts));
        }
        if self.failed > 0 {
            parts.push(format!("失败 {} 个", self.failed));
        }
        if self.skipped > 0 {
            parts.push(format!("跳过 {}", self.skipped));
        }
        if self.cancelled > 0 {
            parts.push(format!("取消 {}", self.cancelled));
        }
        if parts.is_empty() {
            "无仓库".to_string()
        } else {
            parts.join("，")
        }
    }
}

/// Resolve the repo's upstream ref + local/remote divergence (GF-15
/// pre-check, run after the fetch). `Ok(None)` = no upstream configured
/// (nothing to follow up).
fn upstream_divergence(repo_path: &str) -> AppResult<Option<(String, u32, u32, Option<String>)>> {
    let repo = git2::Repository::open(repo_path)?;
    let head = repo.head()?;
    let local_oid = head
        .target()
        .ok_or_else(|| AppError::Other("HEAD 未指向提交".to_string()))?;
    let base_oid = Some(local_oid.to_string());
    let branch_name = head
        .shorthand()
        .ok_or_else(|| AppError::Other("HEAD 异常".to_string()))?
        .to_string();
    let branch = repo.find_branch(&branch_name, git2::BranchType::Local)?;
    let upstream = match branch.upstream() {
        Ok(u) => u,
        Err(_) => return Ok(None),
    };
    let upstream_name = upstream
        .name()?
        .ok_or_else(|| AppError::Other("上游引用名异常".to_string()))?
        .to_string();
    let upstream_oid = upstream
        .get()
        .target()
        .ok_or_else(|| AppError::Other("上游引用异常".to_string()))?;
    let (ahead, behind) = repo.graph_ahead_behind(local_oid, upstream_oid)?;
    Ok(Some((upstream_name, ahead as u32, behind as u32, base_oid)))
}

/// Execute the follow-up for one repo (GF-15): fetch (streaming via
/// `on_line`), re-check divergence, then apply the strategy. All git work is
/// local libgit2 plus the one CLI fetch; `cancel` is polled before/after and
/// passed into the fetch so an in-flight network read aborts promptly.
///
/// AppHandle-free on purpose (console mirror is wired by the caller through
/// `on_line`) so the decision logic stays unit-testable.
fn followup_repo(
    repo_path: &str,
    strategy: DivergedStrategy,
    cancel: &AtomicBool,
    ops: &GitOps,
    on_line: &mut dyn FnMut(OutputStream, &str),
) -> DivergedFollowupItem {
    let mut item = DivergedFollowupItem::new(repo_path);
    let path = Path::new(repo_path);

    if cancel.load(Ordering::Relaxed) {
        item.outcome = "cancelled".to_string();
        item.detail = "已取消".to_string();
        return item;
    }

    // 1. Fetch first: merging against stale remote-tracking refs could report
    //    "up to date" (or merge the wrong tip). Streaming output is mirrored
    //    to the Git Console by the caller's `on_line`.
    if let Err(e) = ops.fetch_streaming(path, Some(cancel), Some(SYNC_GIT_TIMEOUT), on_line) {
        item.outcome = "failed".to_string();
        item.detail = format!("fetch 失败：{e}");
        return item;
    }
    if cancel.load(Ordering::Relaxed) {
        item.outcome = "cancelled".to_string();
        item.detail = "已取消".to_string();
        return item;
    }

    // 2. Re-check divergence after the fetch (the dry-run snapshot may be old).
    let (upstream, ahead, behind, base_oid) = match upstream_divergence(repo_path) {
        Ok(Some(v)) => v,
        Ok(None) => {
            item.outcome = "skipped".to_string();
            item.detail = "没有上游分支，无法跟进".to_string();
            return item;
        }
        Err(e) => {
            item.outcome = "failed".to_string();
            item.detail = e.to_string();
            return item;
        }
    };
    item.ahead = ahead;
    item.behind = behind;
    item.base_oid = base_oid;

    match classify_divergence(ahead as usize, behind as usize) {
        // 远程无新提交：策略与影响范围都不再适用（不可达输入的安全网）。
        DivergenceClass::NothingToPull => {
            item.outcome = "up_to_date".to_string();
            item.detail = "远程无新提交，无需跟进".to_string();
            return item;
        }
        // 仅落后：任何策略都等价于一次快进，merge normal 即走 FF 路径；
        // rebase 同样复用 FF（空 pick todo 不会移动分支）。
        DivergenceClass::BehindOnly => match merge::merge(path, &upstream, "normal") {
            Ok(MergeOutcome::FastForward { to }) => {
                item.outcome = "merged".to_string();
                item.detail = format!("仅落后，已快进 {} 个提交（{}）", behind, short_oid(&to));
            }
            Ok(MergeOutcome::UpToDate) => {
                item.outcome = "up_to_date".to_string();
                item.detail = "远程无新提交，无需跟进".to_string();
            }
            Ok(other) => {
                item.outcome = "failed".to_string();
                item.detail = format!("快进失败：{other:?}");
            }
            Err(e) => {
                item.outcome = "failed".to_string();
                item.detail = e.to_string();
            }
        },
        DivergenceClass::Diverged => match strategy {
            DivergedStrategy::Merge => match merge::merge(path, &upstream, "normal") {
                Ok(MergeOutcome::Merged { commit_oid }) => {
                    item.outcome = "merged".to_string();
                    item.detail = format!(
                        "已创建合并提交（{}），并入远程 {} 个提交",
                        short_oid(&commit_oid),
                        behind
                    );
                }
                Ok(MergeOutcome::FastForward { to }) => {
                    item.outcome = "merged".to_string();
                    item.detail = format!("已快进 {} 个提交（{}）", behind, short_oid(&to));
                }
                Ok(MergeOutcome::UpToDate) => {
                    item.outcome = "up_to_date".to_string();
                    item.detail = "远程无新提交，无需跟进".to_string();
                }
                Ok(MergeOutcome::Conflict { files, base_oid }) => {
                    item.outcome = "conflict".to_string();
                    item.conflict_op = Some("merge".to_string());
                    item.files = files.clone();
                    item.base_oid = base_oid;
                    item.detail = format!("合并冲突（{} 个文件），待解决后可继续或中止", files.len());
                }
                // normal 模式不会产生 squash 结果；兜底按失败处理。
                Ok(_) => {
                    item.outcome = "failed".to_string();
                    item.detail = "merge 返回了非预期结果".to_string();
                }
                Err(e) => {
                    item.outcome = "failed".to_string();
                    item.detail = e.to_string();
                }
            },
            DivergedStrategy::Rebase => {
                // 普通 rebase：默认 pick todo（interactive 批量版不在本任务范围）。
                let todo = match rebase::list_rebase_commits(path, &upstream, None) {
                    Ok(t) => t,
                    Err(e) => {
                        item.outcome = "failed".to_string();
                        item.detail = e.to_string();
                        return item;
                    }
                };
                if todo.is_empty() {
                    item.outcome = "up_to_date".to_string();
                    item.detail = "本地无待变基提交，无需跟进".to_string();
                    return item;
                }
                match rebase::start_rebase(path, &upstream, todo) {
                    Ok(RebaseOutcome::Success { rewritten }) => {
                        item.outcome = "rebased".to_string();
                        item.rewritten = rewritten as u32;
                        item.detail = format!("已变基 {rewritten} 个提交到 {upstream}");
                    }
                    Ok(RebaseOutcome::Conflict { files, .. }) => {
                        item.outcome = "conflict".to_string();
                        item.conflict_op = Some("rebase".to_string());
                        item.files = files.clone();
                        item.detail = format!("变基冲突（{} 个文件），待解决后可继续/跳过/中止", files.len());
                    }
                    Err(e) => {
                        item.outcome = "failed".to_string();
                        item.detail = e.to_string();
                    }
                }
            }
            DivergedStrategy::FfOnly => {
                match ops.pull_streaming(path, Some(cancel), Some(SYNC_GIT_TIMEOUT), on_line) {
                    Ok(_) => {
                        item.outcome = "merged".to_string();
                        item.detail = format!("--ff-only 重试成功（拉取 {behind} 个提交）");
                    }
                    Err(e) => {
                        item.outcome = "failed".to_string();
                        item.detail = format!("仍为分叉，--ff-only 拒绝执行：{e}");
                    }
                }
            }
        },
    }
    item
}

fn short_oid(oid: &str) -> &str {
    &oid[..7.min(oid.len())]
}

/// Batch diverged follow-up (GF-15): for each repo fetch + re-check, then
/// merge / rebase / retry --ff-only per `strategy`. Runs per repo serially
/// inside one blocking task (like GF-10's whole-workspace model — the worker
/// pool would run repos in parallel and fight over the user's git config /
/// credentials); per-repo output streams to the Git Console, `cancel` stops
/// the batch between repos (and aborts the in-flight fetch).
///
/// Returns one item per input repo: partial completion is explicit — merged /
/// rebased / conflicted / failed / skipped / cancelled, never all-or-nothing.
///
/// F-43: async command (macro spawns the body onto the global runtime) +
/// `tauri::async_runtime::spawn_blocking` for the blocking git work; the
/// single-op registration gives the batch the same cancel channel as the
/// GF-07 single-repo network ops (`cancel_git_op`).
#[tauri::command]
pub async fn batch_followup_diverged(
    repo_paths: Vec<String>,
    strategy: String,
    op_id: Option<String>,
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> AppResult<Vec<DivergedFollowupItem>> {
    let strategy = parse_strategy(&strategy)?;
    if repo_paths.is_empty() {
        return Err(AppError::Other("没有要跟进的分叉仓库".to_string()));
    }
    let (op_id, cancel, _guard) = super::git_ops::register_single_op(&state, op_id);
    let command = strategy.console_command().to_string();
    // 取消入口 + Git Console 面板聚焦（与单仓网络操作同一事件对）。
    emit_git_op_started(&app, &op_id, "", "分叉跟进", &command);

    // T-34：rebase 成功仓落操作日志（与单仓 `start_rebase` 一致，Undo 可回退）。
    let db = Arc::clone(&state.db);
    let app_for_loop = app.clone();
    let cancel_for_loop = cancel.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let ops = GitOps::with_default_ssh();
        let mut items: Vec<DivergedFollowupItem> = Vec::with_capacity(repo_paths.len());
        for repo_path in &repo_paths {
            if cancel_for_loop.load(Ordering::Relaxed) {
                let mut item = DivergedFollowupItem::new(repo_path);
                item.outcome = "cancelled".to_string();
                item.detail = "已取消".to_string();
                items.push(item);
                break;
            }
            let repo_name = super::git_ops::repo_display_name(repo_path);
            let mut streamer = ConsoleStreamer::new(
                app_for_loop.clone(),
                repo_path.clone(),
                repo_name,
                command.clone(),
            );
            streamer.emit_meta_header();
            // rebase 前快照 HEAD：成功即落 T-34 日志（before/after 配对）。
            let before = if strategy == DivergedStrategy::Rebase {
                operation_log::snapshot_head(Path::new(repo_path))
            } else {
                None
            };
            let item = followup_repo(
                repo_path,
                strategy,
                &cancel_for_loop,
                &ops,
                &mut |stream: OutputStream, line: &str| streamer.on_line(stream, line),
            );
            streamer.flush();
            if item.outcome == "rebased" {
                if let Some((ref_name, before_oid)) = before {
                    let after_oid = operation_log::snapshot_head(Path::new(repo_path)).map(|(_, oid)| oid);
                    operation_log::record_operation_best_effort(
                        &db,
                        repo_path,
                        operation_log::OP_REBASE,
                        &format!("批量分叉跟进 rebase（{}）", item.repo_name),
                        vec![NewOperationLogItem {
                            repo_path: repo_path.clone(),
                            ref_name,
                            before_oid,
                            after_oid,
                            detail: Some(format!("onto-followup:{}", item.detail)),
                        }],
                    );
                }
            }
            items.push(item);
        }
        items
    })
    .await
    .map_err(|e| AppError::Other(format!("batch_followup_diverged join error: {e}")))?;

    let summary = FollowupSummary::of(&result);
    log::info!(
        "GF-15 分叉跟进（{}，{} 个仓库）完成：{}",
        strategy.label(),
        result.len(),
        summary.one_line()
    );
    let hard_failed = summary.failed > 0 || summary.cancelled > 0;
    let error = if hard_failed {
        Some(summary.one_line())
    } else {
        None
    };
    emit_git_op_finished(&app, &op_id, !hard_failed, error.as_deref());
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn tmpdir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "gw_dryrun_{}_{}",
            tag,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn git(dir: &Path, args: &[&str]) {
        let out = Command::new("git").current_dir(dir).args(args).output().unwrap();
        assert!(
            out.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// Full dry-run categorization (T-20 acceptance): up_to_date /
    /// fast_forward / diverged / conflict / no_upstream for pull, and the
    /// push-side rejection case.
    #[test]
    fn dry_run_categorizes_pull_and_push() {
        let dir = tmpdir("full");
        let _bare = dir.join("remote.git");
        let a = dir.join("a");
        let b = dir.join("b");

        // Bare remote + two clones sharing it.
        git(&dir, &["init", "--bare", "-b", "main", "remote.git"]);
        git(&dir, &["clone", "-q", "remote.git", "a"]);
        git(&dir, &["clone", "-q", "remote.git", "b"]);
        for repo in [&a, &b] {
            git(repo, &["config", "user.name", "t"]);
            git(repo, &["config", "user.email", "t@e.c"]);
        }
        std::fs::write(a.join("f.txt"), "l1\nl2\nl3\n").unwrap();
        git(&a, &["add", "."]);
        git(&a, &["commit", "-qm", "c1"]);
        git(&a, &["push", "-q", "-u", "origin", "main"]);

        let a_path = a.to_string_lossy().to_string();

        // 1. Everything in sync.
        let r = dry_run_repo(&a_path, "pull");
        assert_eq!(r.category, "up_to_date", "{}", r.detail);

        // 2. B pushes a change to l3; A is behind only -> fast_forward.
        git(&b, &["pull", "-q"]);
        std::fs::write(b.join("f.txt"), "l1\nl2\nl3-b\n").unwrap();
        git(&b, &["commit", "-qam", "c2"]);
        git(&b, &["push", "-q"]);
        git(&a, &["fetch", "-q"]); // update remote-tracking refs locally
        let r = dry_run_repo(&a_path, "pull");
        assert_eq!(r.category, "fast_forward", "{}", r.detail);

        // 3. A commits a local change to l1 (no overlap with B's l3 edit).
        std::fs::write(a.join("f.txt"), "l1-a\nl2\nl3\n").unwrap();
        git(&a, &["commit", "-qam", "c3"]);
        let r = dry_run_repo(&a_path, "pull");
        assert_eq!(r.category, "diverged", "{}", r.detail);

        // 4. B also changes l1 -> merge would conflict.
        std::fs::write(b.join("f.txt"), "l1-b\nl2\nl3-b\n").unwrap();
        git(&b, &["commit", "-qam", "c4"]);
        git(&b, &["push", "-q"]);
        git(&a, &["fetch", "-q"]);
        let r = dry_run_repo(&a_path, "pull");
        assert_eq!(r.category, "conflict", "{}", r.detail);

        // 5. Push while behind -> rejected (diverged on push side).
        let r = dry_run_repo(&a_path, "push");
        assert_eq!(r.category, "diverged", "{}", r.detail);

        // 6. Repo without upstream.
        let solo = dir.join("solo");
        git(&dir, &["init", "-q", "-b", "main", "solo"]);
        git(&solo, &["config", "user.name", "t"]);
        git(&solo, &["config", "user.email", "t@e.c"]);
        std::fs::write(solo.join("x.txt"), "x\n").unwrap();
        git(&solo, &["add", "."]);
        git(&solo, &["commit", "-qm", "c1"]);
        let r = dry_run_repo(&solo.to_string_lossy(), "pull");
        assert_eq!(r.category, "no_upstream", "{}", r.detail);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Push fast-forward case: strictly ahead of upstream.
    #[test]
    fn dry_run_push_fast_forward_when_ahead() {
        let dir = tmpdir("pushff");
        let _bare = dir.join("remote.git");
        let a = dir.join("a");
        git(&dir, &["init", "--bare", "-b", "main", "remote.git"]);
        git(&dir, &["clone", "-q", "remote.git", "a"]);
        git(&a, &["config", "user.name", "t"]);
        git(&a, &["config", "user.email", "t@e.c"]);
        std::fs::write(a.join("f.txt"), "l1\n").unwrap();
        git(&a, &["add", "."]);
        git(&a, &["commit", "-qm", "c1"]);
        git(&a, &["push", "-q", "-u", "origin", "main"]);

        std::fs::write(a.join("f.txt"), "l1\nl2\n").unwrap();
        git(&a, &["commit", "-qam", "c2"]);

        let r = dry_run_repo(&a.to_string_lossy(), "push");
        assert_eq!(r.category, "fast_forward");
        assert_eq!(r.ahead, 1);
        assert_eq!(r.behind, 0);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// GF-16: the after-oid backfill poller's terminal-status classifier —
    /// only final statuses stop the wait, so a running task keeps polling.
    #[test]
    fn backfill_poller_terminal_status_classification() {
        use crate::models::task::TaskStatus;
        assert!(is_terminal(&TaskStatus::Success));
        assert!(is_terminal(&TaskStatus::Failed { error: "x".into() }));
        assert!(is_terminal(&TaskStatus::Cancelled));
        assert!(is_terminal(&TaskStatus::PartialSuccess { succeeded: 1, failed: 1 }));
        assert!(!is_terminal(&TaskStatus::Queued));
        assert!(!is_terminal(&TaskStatus::Running { progress: 0.5 }));
    }

    // -----------------------------------------------------------------------
    // GF-15：批量分叉跟进
    // -----------------------------------------------------------------------

    fn head_oid(repo: &Path) -> String {
        git2::Repository::open(repo)
            .unwrap()
            .head()
            .unwrap()
            .target()
            .unwrap()
            .to_string()
    }

    fn head_parents(repo: &Path) -> Vec<String> {
        let repo = git2::Repository::open(repo).unwrap();
        let commit = repo.head().unwrap().peel_to_commit().unwrap();
        commit.parents().map(|p| p.id().to_string()).collect()
    }

    fn followup_item(repo_path: &Path, strategy: DivergedStrategy) -> DivergedFollowupItem {
        let cancel = AtomicBool::new(false);
        let ops = GitOps::with_default_ssh();
        followup_repo(
            &repo_path.to_string_lossy(),
            strategy,
            &cancel,
            &ops,
            &mut |_s: OutputStream, _l: &str| {},
        )
    }

    /// Diverged-repo fixture: a bare remote + clones `a` / `b` / `c`, each with
    /// one local commit, each behind one remote commit made by clone `d`.
    /// `b`'s local commit conflicts with the remote one (same file, different
    /// line). Clone `e` only follows (behind, no local commits).
    fn diverged_fixture(tag: &str) -> std::path::PathBuf {
        let dir = tmpdir(tag);
        git(&dir, &["init", "--bare", "-b", "main", "remote.git"]);
        for name in ["a", "b", "c", "d", "e"] {
            git(&dir, &["clone", "-q", "remote.git", name]);
            let repo = dir.join(name);
            git(&repo, &["config", "user.name", "t"]);
            git(&repo, &["config", "user.email", "t@e.c"]);
        }
        // Seed commit pushed from `a`; everyone else pulls it.
        let a = dir.join("a");
        std::fs::write(a.join("seed.txt"), "seed\n").unwrap();
        git(&a, &["add", "."]);
        git(&a, &["commit", "-qm", "seed"]);
        git(&a, &["push", "-q", "-u", "origin", "main"]);
        for name in ["b", "c", "d", "e"] {
            git(&dir.join(name), &["pull", "-q"]);
        }
        // Remote moves: `d` writes conflict.txt and pushes.
        let d = dir.join("d");
        std::fs::write(d.join("conflict.txt"), "remote line\n").unwrap();
        git(&d, &["add", "."]);
        git(&d, &["commit", "-qm", "remote change"]);
        git(&d, &["push", "-q"]);
        // Local moves: a / b / c commit their own file (b's collides).
        for (name, file, content) in [
            ("a", "a1.txt", "a local\n"),
            ("b", "conflict.txt", "local line\n"),
            ("c", "c1.txt", "c local\n"),
        ] {
            let repo = dir.join(name);
            std::fs::write(repo.join(file), content).unwrap();
            git(&repo, &["add", "."]);
            git(&repo, &["commit", "-qm", &format!("{name} local")]);
        }
        // Refresh remote-tracking refs so the divergence is visible locally.
        for name in ["a", "b", "c", "e"] {
            git(&dir.join(name), &["fetch", "-q"]);
        }
        dir
    }

    /// GF-15 acceptance 1 + 2 + partial completion: merge follow-up across
    /// diverged repos — clean merges land (merge commit created), a
    /// conflicting repo is reported with its files + abort target and stays
    /// resolvable, and the rest of the batch is unaffected.
    #[test]
    fn followup_merge_batch_partial_completion() {
        let dir = diverged_fixture("gf15_merge");
        let a = dir.join("a");
        let b = dir.join("b");
        let c = dir.join("c");
        let e = dir.join("e");
        let b_head_before = head_oid(&b);

        // Sanity: dry-run sees the divergence the follow-up is about —
        // a/c are truly diverged, b is conflict-predicted (the same diverged
        // family the follow-up targets), e is behind-only.
        for name in ["a", "c"] {
            let repo = dir.join(name);
            let r = dry_run_repo(&repo.to_string_lossy(), "pull");
            assert_eq!(r.category, "diverged", "{name}: {}", r.detail);
            assert_eq!((r.ahead, r.behind), (1, 1), "{name}");
        }
        let rb = dry_run_repo(&b.to_string_lossy(), "pull");
        assert_eq!(rb.category, "conflict", "{}", rb.detail);
        assert_eq!((rb.ahead, rb.behind), (1, 1));
        let r = dry_run_repo(&e.to_string_lossy(), "pull");
        assert_eq!(r.category, "fast_forward", "{}", r.detail);
        assert_eq!((r.ahead, r.behind), (0, 1));

        let items: Vec<DivergedFollowupItem> = [&a, &b, &c, &e]
            .iter()
            .map(|p| followup_item(p, DivergedStrategy::Merge))
            .collect();
        let by_name = |name: &str| items.iter().find(|i| i.repo_name == name).unwrap();

        // 1. Clean diverged repos merged with a merge commit (2 parents).
        for name in ["a", "c"] {
            let item = by_name(name);
            assert_eq!(item.outcome, "merged", "{}: {}", name, item.detail);
            assert_eq!(item.ahead, 1, "{name}");
            assert_eq!(item.behind, 1, "{name}");
            let repo = dir.join(name);
            assert_eq!(head_parents(&repo).len(), 2, "{name} merge commit");
            // 该仓库已与上游一致。
            let r = dry_run_repo(&repo.to_string_lossy(), "pull");
            assert_eq!(r.category, "up_to_date", "{}", r.detail);
        }

        // 2. Conflicting repo: conflict outcome + files + base oid, HEAD
        //    unmoved, merge state in progress (前端 smartMergeQueue 的输入).
        let conflict = by_name("b");
        assert_eq!(conflict.outcome, "conflict", "{}", conflict.detail);
        assert_eq!(conflict.conflict_op.as_deref(), Some("merge"));
        assert_eq!(conflict.files, vec!["conflict.txt".to_string()]);
        assert!(conflict.base_oid.is_some());
        assert_eq!(head_oid(&b), b_head_before, "conflicted repo keeps HEAD");
        assert!(crate::core::merge::merge_in_progress(&b).unwrap());

        // 3. Behind-only repo fast-forwards (strategy irrelevant there).
        let ff = by_name("e");
        assert_eq!(ff.outcome, "merged", "{}", ff.detail);
        assert!(ff.detail.contains("快进"), "{}", ff.detail);
        assert_eq!(dry_run_repo(&e.to_string_lossy(), "pull").category, "up_to_date");

        // Partial completion semantics: 3 merged + 1 conflict, 0 failed.
        assert_eq!(
            FollowupSummary::of(&items),
            FollowupSummary {
                merged: 3,
                conflicts: 1,
                ..Default::default()
            }
        );

        // 4. A repo without any remote fails its fetch (surfaced, batch
        //    continues) — fetch failure is a per-repo failure, not a batch stop.
        let solo = dir.join("solo");
        git(&dir, &["init", "-q", "-b", "main", "solo"]);
        git(&solo, &["config", "user.name", "t"]);
        git(&solo, &["config", "user.email", "t@e.c"]);
        std::fs::write(solo.join("x.txt"), "x\n").unwrap();
        git(&solo, &["add", "."]);
        git(&solo, &["commit", "-qm", "c1"]);
        let solo_item = followup_item(&solo, DivergedStrategy::Merge);
        assert_eq!(solo_item.outcome, "failed");
        assert!(solo_item.detail.contains("fetch"), "{}", solo_item.detail);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// GF-15 acceptance 1 (rebase strategy): the local commit is replayed onto
    /// the upstream tip — HEAD's parent becomes the upstream commit.
    #[test]
    fn followup_rebase_replays_local_commit_onto_upstream() {
        let dir = diverged_fixture("gf15_rebase");
        let a = dir.join("a");
        let upstream_before = {
            let repo = git2::Repository::open(&a).unwrap();
            let id = repo
                .revparse_single("origin/main")
                .unwrap()
                .peel_to_commit()
                .unwrap()
                .id()
                .to_string();
            id
        };

        let item = followup_item(&a, DivergedStrategy::Rebase);
        assert_eq!(item.outcome, "rebased", "{}", item.detail);
        assert_eq!(item.rewritten, 1);
        assert_eq!(head_parents(&a), vec![upstream_before.clone()]);
        // Local work preserved on top of the upstream tip.
        assert!(a.join("a1.txt").exists());
        assert_eq!(dry_run_repo(&a.to_string_lossy(), "pull").category, "up_to_date");

        // Re-running is a no-op (nothing left to bring in).
        let again = followup_item(&a, DivergedStrategy::Rebase);
        assert_eq!(again.outcome, "up_to_date", "{}", again.detail);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// GF-15 acceptance 1 (--ff-only retry): a still-diverged repo is refused
    /// with a readable reason (no half-applied state).
    #[test]
    fn followup_ff_only_refuses_diverged_repo() {
        let dir = diverged_fixture("gf15_ffonly");
        let a = dir.join("a");
        let head_before = head_oid(&a);

        let item = followup_item(&a, DivergedStrategy::FfOnly);
        assert_eq!(item.outcome, "failed", "{}", item.detail);
        assert!(item.detail.contains("--ff-only"), "{}", item.detail);
        assert_eq!(head_oid(&a), head_before, "failed retry leaves HEAD alone");
        assert!(!crate::core::merge::merge_in_progress(&a).unwrap());

        // Behind-only repo: the retry succeeds (plain fast-forward).
        let e = dir.join("e");
        let item = followup_item(&e, DivergedStrategy::FfOnly);
        assert_eq!(item.outcome, "merged", "{}", item.detail);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// GF-15: the pure decision + aggregation layer (divergence classification,
    /// strategy parsing, partial-completion summary).
    #[test]
    fn followup_pure_classification_and_summary() {
        assert_eq!(classify_divergence(0, 0), DivergenceClass::NothingToPull);
        assert_eq!(classify_divergence(2, 0), DivergenceClass::NothingToPull);
        assert_eq!(classify_divergence(0, 3), DivergenceClass::BehindOnly);
        assert_eq!(classify_divergence(1, 3), DivergenceClass::Diverged);

        assert_eq!(parse_strategy("merge").unwrap(), DivergedStrategy::Merge);
        assert_eq!(parse_strategy("rebase").unwrap(), DivergedStrategy::Rebase);
        assert_eq!(parse_strategy("ff_only").unwrap(), DivergedStrategy::FfOnly);
        assert_eq!(parse_strategy("ff-only").unwrap(), DivergedStrategy::FfOnly);
        assert!(parse_strategy("squash").is_err());

        let mut merged = DivergedFollowupItem::new("/w/a");
        merged.outcome = "merged".to_string();
        let mut conflict = DivergedFollowupItem::new("/w/b");
        conflict.outcome = "conflict".to_string();
        let mut failed = DivergedFollowupItem::new("/w/c");
        failed.outcome = "failed".to_string();
        let items = vec![merged, conflict.clone(), failed];
        let summary = FollowupSummary::of(&items);
        assert_eq!(
            summary,
            FollowupSummary {
                merged: 1,
                conflicts: 1,
                failed: 1,
                ..Default::default()
            }
        );
        let line = summary.one_line();
        assert!(line.contains("合并 1 个") && line.contains("冲突 1 个") && line.contains("失败 1 个"), "{line}");

        // A cancelled tail marks every not-yet-processed repo (the batch stops
        // at the cancellation point) — the summary reports it honestly.
        let mut cancelled = DivergedFollowupItem::new("/w/d");
        cancelled.outcome = "cancelled".to_string();
        assert_eq!(FollowupSummary::of(&vec![cancelled]).cancelled, 1);
    }
}
