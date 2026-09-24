//! Merge (T-15): normal / --no-ff / --squash via libgit2 (local op, global
//! constraint §3). Conflicts keep MERGE_HEAD so the user can resolve and
//! `merge_continue` (or `merge_abort` to restore).

use std::path::Path;

use serde::Serialize;

use crate::core::history;
use crate::error::{AppError, AppResult};

/// Outcome of a merge operation.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum MergeOutcome {
    /// HEAD already contains the branch; nothing to do.
    UpToDate,
    /// Branch fast-forwarded (normal mode only).
    #[serde(rename_all = "camelCase")]
    FastForward { to: String },
    /// A merge commit was created.
    #[serde(rename_all = "camelCase")]
    Merged { commit_oid: String },
    /// Squash merge staged (no commit; user commits the staged tree).
    Squashed,
    /// Conflicts: repo is in merge state (MERGE_HEAD set), resolve then
    /// `merge_continue`, or `merge_abort` to restore.
    #[serde(rename_all = "camelCase")]
    Conflict {
        files: Vec<String>,
        /// HEAD before the merge (abort target hint).
        base_oid: Option<String>,
    },
}

/// Merge `branch` into the current HEAD. `mode`: "normal" | "no-ff" | "squash".
pub fn merge(repo_path: &Path, branch: &str, mode: &str) -> AppResult<MergeOutcome> {
    if !matches!(mode, "normal" | "no-ff" | "squash") {
        return Err(AppError::Other(format!(
            "invalid merge mode '{}' (normal | no-ff | squash)",
            mode
        )));
    }

    let repo = git2::Repository::open(repo_path)?;
    // PAF-10：互斥与脏区前置校验。MERGE_HEAD 已存在（merge_in_progress 此前
    // 定义但未被调用）时再 merge 会覆盖冲突状态；rebase 进行中同理；脏工作区
    // 会被 merge/checkout 吞掉。
    if merge_in_progress(repo_path)? {
        return Err(AppError::Conflict(
            "已有 merge 进行中（请先 resolve + continue，或 abort 后再试）".into(),
        ));
    }
    if crate::core::rebase::get_rebase_state(repo_path)?.is_some() {
        return Err(AppError::Conflict(
            "rebase 进行中，请先完成或中止该 rebase 再 merge".into(),
        ));
    }
    history::ensure_clean_worktree(&repo, "Merge")?;
    let base_oid = repo.head().ok().and_then(|h| h.target()).map(|o| o.to_string());
    let their_commit = repo
        .revparse_single(branch)
        .and_then(|o| o.peel_to_commit())
        .map_err(|_| AppError::NotFound(format!("branch '{}' not found", branch)))?;
    let their_annotated = repo.find_annotated_commit(their_commit.id())?;

    let (analysis, _preference) = repo.merge_analysis(&[&their_annotated])?;

    if analysis.is_up_to_date() {
        return Ok(MergeOutcome::UpToDate);
    }

    // Fast-forward: only in normal mode when the analysis allows it.
    if analysis.is_fast_forward() && mode == "normal" {
        let head_ref = repo.head()?.name().unwrap_or("HEAD").to_string();
        // Checkout BEFORE moving the ref: the checkout baseline defaults to
        // the current HEAD tree, so new files materialize in the worktree.
        repo.checkout_tree(their_commit.as_object(), None)?;
        repo.find_reference(&head_ref)?
            .set_target(their_commit.id(), "merge: fast-forward")?;
        return Ok(MergeOutcome::FastForward {
            to: their_commit.id().to_string(),
        });
    }

    // Full merge (no-ff / squash / non-ff normal).
    repo.merge(&[&their_annotated], None, None)?;

    let mut index = repo.index()?;
    if index.has_conflicts() {
        // MERGE_HEAD stays set for resolve/continue or abort.
        return Ok(MergeOutcome::Conflict {
            files: history::conflict_paths(&index)?,
            base_oid,
        });
    }

    let tree_oid = index.write_tree()?;
    let tree = repo.find_tree(tree_oid)?;

    if mode == "squash" {
        // Squash: staged tree only, no commit, and clear MERGE_HEAD so a
        // later commit does not become a merge commit.
        repo.cleanup_state()?;
        return Ok(MergeOutcome::Squashed);
    }

    let sig = crate::core::signature_or_default(&repo)?;
    let head_commit = repo.head()?.peel_to_commit()?;
    let message = format!("Merge branch '{}'", branch);
    let oid = repo.commit(
        Some("HEAD"),
        &sig,
        &sig,
        &message,
        &tree,
        &[&head_commit, &their_commit],
    )?;
    repo.cleanup_state()?;
    Ok(MergeOutcome::Merged {
        commit_oid: oid.to_string(),
    })
}

/// Whether a merge is in progress (MERGE_HEAD exists).
pub fn merge_in_progress(repo_path: &Path) -> AppResult<bool> {
    let repo = git2::Repository::open(repo_path)?;
    Ok(repo.path().join("MERGE_HEAD").exists())
}

/// The in-progress merge's target oid (MERGE_HEAD), None when no merge is in
/// progress. GF-16: the merge-abort command snapshots it so Undo can re-run
/// the exact same merge.
pub fn merge_head_oid(repo_path: &Path) -> Option<String> {
    let repo = git2::Repository::open(repo_path).ok()?;
    let raw = std::fs::read_to_string(repo.path().join("MERGE_HEAD")).ok()?;
    let oid = raw.trim();
    if oid.is_empty() {
        None
    } else {
        Some(oid.to_string())
    }
}

/// Finalize a conflicted merge after the user resolved the index:
/// creates the merge commit with [HEAD, MERGE_HEAD] as parents.
pub fn merge_continue(repo_path: &Path, message: Option<&str>) -> AppResult<String> {
    let repo = git2::Repository::open(repo_path)?;

    let merge_head_file = repo.path().join("MERGE_HEAD");
    let merge_head_raw =
        std::fs::read_to_string(&merge_head_file).map_err(|_| AppError::Conflict("no merge in progress".into()))?;
    let merge_head_oid =
        git2::Oid::from_str(merge_head_raw.trim()).map_err(|_| AppError::Other("invalid MERGE_HEAD".into()))?;

    let mut index = repo.index()?;
    if index.has_conflicts() {
        return Err(AppError::Conflict("仍有未解决的冲突，请先解决后再继续".into()));
    }

    let tree_oid = index.write_tree()?;
    let tree = repo.find_tree(tree_oid)?;
    let sig = crate::core::signature_or_default(&repo)?;
    let head_commit = repo.head()?.peel_to_commit()?;
    let merge_commit = repo.find_commit(merge_head_oid)?;

    let default_msg = std::fs::read_to_string(repo.path().join("MERGE_MSG"))
        .ok()
        .and_then(|m| m.lines().next().map(String::from))
        .unwrap_or_else(|| "Merge".to_string());
    let msg = message.unwrap_or(&default_msg);

    let oid = repo.commit(Some("HEAD"), &sig, &sig, msg, &tree, &[&head_commit, &merge_commit])?;
    repo.cleanup_state()?;
    Ok(oid.to_string())
}

/// Abort a conflicted merge: hard reset to HEAD and clear merge state.
pub fn merge_abort(repo_path: &Path) -> AppResult<()> {
    if !merge_in_progress(repo_path)? {
        return Err(AppError::Conflict("no merge in progress".into()));
    }
    history::reset_to(repo_path, None, "hard")?;
    let repo = git2::Repository::open(repo_path)?;
    repo.cleanup_state()?;
    Ok(())
}

/// Re-run a merge toward `merge_head_oid` — the undo of `merge_abort`.
/// Restores the in-conflict merge state (MERGE_HEAD + worktree conflict
/// markers) exactly as the original merge produced it: the merge of a fixed
/// commit pair is deterministic, and the caller's undo safety check has
/// already verified HEAD still sits at the recorded post-abort oid.
///
/// Refused when another operation is in progress or the worktree is dirty —
/// `merge` itself requires a clean tree.
pub fn restart_merge_at(repo_path: &Path, merge_head_oid: &str) -> AppResult<()> {
    if merge_in_progress(repo_path)? {
        return Err(AppError::Conflict("已有 merge 进行中，请先继续或中止".into()));
    }
    if crate::core::rebase::get_rebase_state(repo_path)?.is_some() {
        return Err(AppError::Conflict("rebase 进行中，无法重新合并".into()));
    }
    let repo = git2::Repository::open(repo_path)?;
    history::ensure_clean_worktree(&repo, "重新合并")?;
    let oid = git2::Oid::from_str(merge_head_oid)
        .map_err(|_| AppError::Other(format!("记录的 MERGE_HEAD oid 无效：{merge_head_oid}")))?;
    let their_commit = repo
        .find_commit(oid)
        .map_err(|_| AppError::NotFound(format!("原合并目标提交 {merge_head_oid} 已不存在")))?;
    let their_annotated = repo.find_annotated_commit(their_commit.id())?;
    // Same call shape as `merge`'s full-merge path; the analysis (up-to-date /
    // fast-forward) is irrelevant here — the recorded state was a conflict.
    repo.merge(&[&their_annotated], None, None)?;
    let index = repo.index()?;
    if !index.has_conflicts() {
        // Should not happen for the same commit pair; if it does, leave the
        // staged merge to be continued/aborted by the user rather than
        // committing behind their back.
        repo.cleanup_state()?;
        return Err(AppError::Other("重新合并未产生冲突（仓库状态已变化）".into()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "gw_merge_{}_{}",
            tag,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn commit_file(repo: &git2::Repository, dir: &Path, name: &str, content: &str, msg: &str) -> String {
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
        repo.commit(Some("HEAD"), &sig, &sig, msg, &tree, &parents)
            .unwrap()
            .to_string()
    }

    fn checkout(repo_path: &Path, name: &str) {
        crate::core::branch::checkout_branch(repo_path, name).unwrap();
    }

    fn init_with_side(dir: &Path) -> git2::Repository {
        let repo = git2::Repository::init(dir).unwrap();
        commit_file(&repo, dir, "a.txt", "one\n", "init");
        let head = repo.head().unwrap().peel_to_commit().unwrap();
        repo.branch("side", &head, false).unwrap();
        drop(head);
        repo
    }

    fn head_summary(repo_path: &Path) -> String {
        let repo = git2::Repository::open(repo_path).unwrap();
        let head = repo.head().unwrap();
        let summary = head.peel_to_commit().unwrap().summary().unwrap_or_default().to_string();
        summary
    }

    /// Normal mode fast-forwards when possible.
    #[test]
    fn merge_fast_forward() {
        let dir = tmpdir("ff");
        {
            let repo = init_with_side(&dir);
            drop(repo);
        }
        checkout(&dir, "side");
        {
            let repo = git2::Repository::open(&dir).unwrap();
            commit_file(&repo, &dir, "b.txt", "two\n", "side work");
            drop(repo);
        }
        checkout(&dir, "master");

        let outcome = merge(&dir, "side", "normal").unwrap();
        assert!(matches!(outcome, MergeOutcome::FastForward { .. }));
        assert_eq!(head_summary(&dir), "side work");
        assert!(dir.join("b.txt").exists());

        // Up-to-date now.
        let outcome = merge(&dir, "side", "normal").unwrap();
        assert!(matches!(outcome, MergeOutcome::UpToDate));

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// no-ff creates a merge commit even when a fast-forward is possible.
    #[test]
    fn merge_no_ff_creates_merge_commit() {
        let dir = tmpdir("noff");
        {
            let repo = init_with_side(&dir);
            drop(repo);
        }
        checkout(&dir, "side");
        {
            let repo = git2::Repository::open(&dir).unwrap();
            commit_file(&repo, &dir, "b.txt", "two\n", "side work");
            drop(repo);
        }
        checkout(&dir, "master");

        let outcome = merge(&dir, "side", "no-ff").unwrap();
        match outcome {
            MergeOutcome::Merged { commit_oid } => {
                let repo = git2::Repository::open(&dir).unwrap();
                let c = repo.find_commit(git2::Oid::from_str(&commit_oid).unwrap()).unwrap();
                assert_eq!(c.parent_count(), 2, "merge commit has two parents");
                assert!(c.summary().unwrap_or_default().contains("Merge branch 'side'"));
            }
            other => panic!("expected Merged, got {:?}", other),
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Squash stages the merged tree without committing.
    #[test]
    fn merge_squash_stages_without_commit() {
        let dir = tmpdir("squash");
        {
            let repo = init_with_side(&dir);
            drop(repo);
        }
        checkout(&dir, "side");
        {
            let repo = git2::Repository::open(&dir).unwrap();
            commit_file(&repo, &dir, "b.txt", "two\n", "side work");
            drop(repo);
        }
        checkout(&dir, "master");
        let before = head_summary(&dir);

        let outcome = merge(&dir, "side", "squash").unwrap();
        assert!(matches!(outcome, MergeOutcome::Squashed));
        // No new commit on HEAD...
        assert_eq!(head_summary(&dir), before);
        // ...but the change is staged in the index.
        let repo = git2::Repository::open(&dir).unwrap();
        let statuses = repo.statuses(None).unwrap();
        assert!(statuses
            .iter()
            .any(|e| e.path() == Some("b.txt") && e.status().contains(git2::Status::INDEX_NEW)));
        assert!(!repo.path().join("MERGE_HEAD").exists());

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Conflicting merge: Conflict outcome, abort restores, resolve+continue
    /// creates the merge commit.
    #[test]
    fn merge_conflict_abort_and_continue() {
        let dir = tmpdir("conflict");
        {
            let repo = init_with_side(&dir);
            commit_file(&repo, &dir, "a.txt", "master line\n", "master change");
            drop(repo);
        }
        checkout(&dir, "side");
        {
            let repo = git2::Repository::open(&dir).unwrap();
            commit_file(&repo, &dir, "a.txt", "side line\n", "side change");
            drop(repo);
        }
        checkout(&dir, "master");

        let outcome = merge(&dir, "side", "normal").unwrap();
        match outcome {
            MergeOutcome::Conflict { files, .. } => {
                assert_eq!(files, vec!["a.txt".to_string()]);
            }
            other => panic!("expected Conflict, got {:?}", other),
        }
        assert!(merge_in_progress(&dir).unwrap());

        // Abort restores pre-merge state completely.
        merge_abort(&dir).unwrap();
        assert!(!merge_in_progress(&dir).unwrap());
        assert_eq!(
            std::fs::read_to_string(dir.join("a.txt"))
                .unwrap()
                .replace("\r\n", "\n"),
            "master line\n"
        );

        // Merge again, resolve the conflict manually, then continue.
        let outcome = merge(&dir, "side", "normal").unwrap();
        assert!(matches!(outcome, MergeOutcome::Conflict { .. }));
        std::fs::write(dir.join("a.txt"), "resolved\n").unwrap();
        {
            let repo = git2::Repository::open(&dir).unwrap();
            let mut index = repo.index().unwrap();
            index.add_path(Path::new("a.txt")).unwrap();
            index.write().unwrap();
            drop(repo);
        }
        let oid = merge_continue(&dir, None).unwrap();
        let repo = git2::Repository::open(&dir).unwrap();
        let c = repo.find_commit(git2::Oid::from_str(&oid).unwrap()).unwrap();
        assert_eq!(c.parent_count(), 2);
        assert_eq!(
            std::fs::read_to_string(dir.join("a.txt"))
                .unwrap()
                .replace("\r\n", "\n"),
            "resolved\n"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// PAF-10：脏工作区 merge 被拒绝（未提交修改会被 merge 吞掉）。
    #[test]
    fn merge_rejects_dirty_worktree() {
        let dir = tmpdir("dirty");
        {
            let repo = init_with_side(&dir);
            drop(repo);
        }
        checkout(&dir, "side");
        {
            let repo = git2::Repository::open(&dir).unwrap();
            commit_file(&repo, &dir, "b.txt", "two\n", "side work");
            drop(repo);
        }
        checkout(&dir, "master");
        std::fs::write(dir.join("a.txt"), "dirty\n").unwrap();

        let err = merge(&dir, "side", "normal").unwrap_err();
        assert_eq!(err.code(), "ConflictError");
        assert!(err.to_string().contains("未提交变更"));
        assert!(!merge_in_progress(&dir).unwrap());

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// PAF-10：MERGE_HEAD 已存在（冲突 merge 进行中）时再次 merge 被拒绝。
    #[test]
    fn merge_rejects_when_already_in_progress() {
        let dir = tmpdir("inprogress");
        {
            let repo = init_with_side(&dir);
            commit_file(&repo, &dir, "a.txt", "master line\n", "master change");
            drop(repo);
        }
        checkout(&dir, "side");
        {
            let repo = git2::Repository::open(&dir).unwrap();
            commit_file(&repo, &dir, "a.txt", "side line\n", "side change");
            drop(repo);
        }
        checkout(&dir, "master");

        let outcome = merge(&dir, "side", "normal").unwrap();
        assert!(matches!(outcome, MergeOutcome::Conflict { .. }));
        assert!(merge_in_progress(&dir).unwrap());

        let err = merge(&dir, "side", "no-ff").unwrap_err();
        assert_eq!(err.code(), "ConflictError");
        assert!(err.to_string().contains("merge 进行中"));

        // 收口：abort 清理状态。
        merge_abort(&dir).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// GF-16: undo of a merge abort re-runs the merge and restores the exact
    /// conflict state (MERGE_HEAD + worktree markers).
    #[test]
    fn restart_merge_at_restores_conflict_state() {
        let dir = tmpdir("undo_abort");
        let (side_tip, head_before) = {
            let repo = init_with_side(&dir);
            commit_file(&repo, &dir, "a.txt", "master line\n", "master change");
            let head = repo.head().unwrap().target().unwrap().to_string();
            drop(repo);
            checkout(&dir, "side");
            let repo = git2::Repository::open(&dir).unwrap();
            commit_file(&repo, &dir, "a.txt", "side line\n", "side change");
            let tip = repo.head().unwrap().target().unwrap().to_string();
            drop(repo);
            (tip, head)
        };
        checkout(&dir, "master");

        let outcome = merge(&dir, "side", "normal").unwrap();
        assert!(matches!(outcome, MergeOutcome::Conflict { .. }));
        let merge_head = std::fs::read_to_string(dir.join(".git").join("MERGE_HEAD"))
            .unwrap()
            .trim()
            .to_string();
        assert_eq!(merge_head, side_tip);
        let markers = std::fs::read_to_string(dir.join("a.txt")).unwrap();
        assert!(markers.contains("<<<<<<<"));

        // Abort restores the pre-merge worktree.
        merge_abort(&dir).unwrap();
        assert!(!merge_in_progress(&dir).unwrap());
        assert_eq!(head_summary(&dir), "master change");

        // Undo of the abort: the same conflict is back, HEAD unmoved.
        restart_merge_at(&dir, &merge_head).unwrap();
        assert!(merge_in_progress(&dir).unwrap());
        assert_eq!(
            std::fs::read_to_string(dir.join(".git").join("MERGE_HEAD"))
                .unwrap()
                .trim(),
            merge_head
        );
        let markers = std::fs::read_to_string(dir.join("a.txt")).unwrap();
        assert!(markers.contains("<<<<<<<") && markers.contains(">>>>>>>"), "{markers}");
        assert_eq!(
            git2::Repository::open(&dir).unwrap().head().unwrap().target().unwrap().to_string(),
            head_before
        );

        // A second merge in progress refuses the restart.
        let err = restart_merge_at(&dir, &merge_head).unwrap_err();
        assert!(err.to_string().contains("已有 merge 进行中"), "{err}");
        merge_abort(&dir).unwrap();

        // A dirty worktree refuses the restart.
        std::fs::write(dir.join("a.txt"), "dirty\n").unwrap();
        let err = restart_merge_at(&dir, &merge_head).unwrap_err();
        assert!(err.to_string().contains("未提交变更"), "{err}");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
