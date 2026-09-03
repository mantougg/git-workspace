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
}
