//! Batch operation commands (T-20): selector queries, bulk branch ops and
//! dry-run impact reports. Multi-repo work always goes through the T-05 task
//! queue (per-repo sub-results + Partial Success aggregation).

use std::path::Path;

use rayon::prelude::*;
use serde::Serialize;
use tauri::State;

use crate::core::selector::{self, RepoFacet};
use crate::db::dao;
use crate::error::{AppError, AppResult};
use crate::models::task::{BranchOpKind, TaskRequest, TaskType};
use crate::state::AppState;

/// Query repositories of a workspace with the selector syntax (T-20 §52):
/// `@group:` / `@tag:` / `@status:` tokens and plain text, ANDed. Filtering
/// happens in memory over the repo list + status cache — no DB scan per
/// keystroke, no git processes spawned.
#[tauri::command]
pub fn select_repos(
    workspace_id: i64,
    query: String,
    state: State<'_, AppState>,
) -> AppResult<Vec<String>> {
    let conn = state
        .db
        .lock()
        .map_err(|e| AppError::Other(format!("DB lock error: {}", e)))?;
    let repos = dao::list_repositories_by_workspace(&conn, workspace_id)?;
    let groups = dao::list_groups(&conn, workspace_id)?;
    let group_names: std::collections::HashMap<i64, String> =
        groups.into_iter().map(|g| (g.id, g.name)).collect();

    let facets: Vec<RepoFacet> = repos
        .into_iter()
        .map(|r| {
            let status = state.status_cache.get(&r.path);
            let (ahead, behind, dirty) = match &status {
                Some(s) => (s.ahead > 0, s.behind > 0, !s.is_clean),
                None => (false, false, false),
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
                favorite: r.is_favorite,
            }
        })
        .collect();

    Ok(selector::select_paths(&query, &facets))
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
#[tauri::command]
pub fn batch_branch_op(
    repo_paths: Vec<String>,
    op: BranchOpKind,
    name: String,
    force: bool,
    state: State<'_, AppState>,
) -> AppResult<Vec<String>> {
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
    state.task_manager.submit(&requests)
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
    let items: Vec<DryRunItem> = repo_paths
        .par_iter()
        .map(|p| dry_run_repo(p, &op))
        .collect();
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
        let local_oid = head.target().ok_or_else(|| {
            AppError::Other("HEAD 未指向提交".to_string())
        })?;
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
        let upstream_oid = upstream.get().target().ok_or_else(|| {
            AppError::Other("上游引用异常".to_string())
        })?;

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
                    let mut merge_index =
                        repo.merge_commits(&local_commit, &their_commit, None)?;
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
        let out = Command::new("git")
            .current_dir(dir)
            .args(args)
            .output()
            .unwrap();
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
        let bare = dir.join("remote.git");
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
        let bare = dir.join("remote.git");
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
        assert_eq!(r.category, "fast_forward", "{}", r.detail);
        assert_eq!(r.ahead, 1);
        assert_eq!(r.behind, 0);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
