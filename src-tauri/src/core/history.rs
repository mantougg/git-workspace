//! History operations (T-13): cherry-pick, revert, reset, and abort of an
//! in-progress pick/revert. All local libgit2 work (global constraint §3);
//! conflict outcomes keep the repository in a recoverable state
//! (CHERRY_PICK_HEAD / REVERT_HEAD preserved until abort, abort = hard reset
//! to the pre-operation HEAD).

use std::path::Path;

use serde::Serialize;

use crate::error::{AppError, AppResult};

/// Outcome of a cherry-pick / revert operation.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum PickOutcome {
    /// All commits applied and committed.
    Success { picked: usize },
    /// A commit conflicted; the repo is left in cherry-pick/revert state
    /// with conflict markers, recoverable via `abort_pick`.
    #[serde(rename_all = "camelCase")]
    Conflict {
        /// Conflicted file paths.
        files: Vec<String>,
        /// The commit being applied when the conflict occurred.
        current: String,
        /// How many commits were already applied before the conflict.
        done: usize,
        total: usize,
        /// HEAD before the operation started (abort target).
        base_oid: Option<String>,
    },
}

/// Result of a reset operation.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResetResult {
    /// HEAD before the reset (recovery hint; reflog comes with T-14).
    pub previous_head: Option<String>,
    /// Resolved target oid the HEAD/index/worktree now points at.
    pub target: String,
    pub mode: String,
}

/// Cherry-pick one or more commits onto HEAD, in order.
/// Each successfully applied commit is committed immediately (original author
/// and message preserved, committer = repo signature). On conflict the repo
/// keeps CHERRY_PICK_HEAD and conflict markers so the user can resolve or
/// abort (T-16 wires the resolver UI).
pub fn cherry_pick(repo_path: &Path, oids: &[String]) -> AppResult<PickOutcome> {
    let repo = git2::Repository::open(repo_path)?;
    let base_oid = head_oid(&repo);
    let total = oids.len();

    for (idx, oid_str) in oids.iter().enumerate() {
        let oid = git2::Oid::from_str(oid_str)
            .map_err(|_| AppError::NotFound(format!("commit '{}' not found", oid_str)))?;
        let commit = repo.find_commit(oid)?;
        repo.cherrypick(&commit, None)?;

        let mut index = repo.index()?;
        if index.has_conflicts() {
            return Ok(PickOutcome::Conflict {
                files: conflict_paths(&index)?,
                current: oid_str.clone(),
                done: idx,
                total,
                base_oid: base_oid.clone(),
            });
        }

        // No conflicts: commit immediately, mirroring `git cherry-pick`.
        let tree_oid = index.write_tree()?;
        let tree = repo.find_tree(tree_oid)?;
        let sig = repo.signature()?;
        let parent = repo.head()?.peel_to_commit()?;
        repo.commit(
            Some("HEAD"),
            &commit.author(),
            &sig,
            commit.message().unwrap_or_default(),
            &tree,
            &[&parent],
        )?;
        repo.cleanup_state()?;
    }

    Ok(PickOutcome::Success { picked: total })
}

/// Revert a single commit, creating a revert commit on success.
/// On conflict the repo keeps REVERT_HEAD and conflict markers.
pub fn revert(repo_path: &Path, oid_str: &str) -> AppResult<PickOutcome> {
    let repo = git2::Repository::open(repo_path)?;
    let base_oid = head_oid(&repo);
    let oid = git2::Oid::from_str(oid_str)
        .map_err(|_| AppError::NotFound(format!("commit '{}' not found", oid_str)))?;
    let commit = repo.find_commit(oid)?;
    repo.revert(&commit, None)?;

    let mut index = repo.index()?;
    if index.has_conflicts() {
        return Ok(PickOutcome::Conflict {
            files: conflict_paths(&index)?,
            current: oid_str.to_string(),
            done: 0,
            total: 1,
            base_oid,
        });
    }

    let tree_oid = index.write_tree()?;
    let tree = repo.find_tree(tree_oid)?;
    let sig = repo.signature()?;
    let parent = repo.head()?.peel_to_commit()?;
    let message = format!(
        "Revert \"{}\"\n\nThis reverts commit {}.\n",
        commit.summary().unwrap_or_default(),
        oid
    );
    repo.commit(Some("HEAD"), &sig, &sig, &message, &tree, &[&parent])?;
    repo.cleanup_state()?;

    Ok(PickOutcome::Success { picked: 1 })
}

/// Reset HEAD to `target` (default HEAD) with soft / mixed / hard semantics.
/// Returns the previous HEAD oid so the UI can show a recovery hint.
pub fn reset_to(repo_path: &Path, target: Option<&str>, mode: &str) -> AppResult<ResetResult> {
    let repo = git2::Repository::open(repo_path)?;
    let previous_head = head_oid(&repo);

    let spec = target.unwrap_or("HEAD");
    let obj = repo
        .revparse_single(spec)
        .map_err(|_| AppError::NotFound(format!("revision '{}' not found", spec)))?;

    match mode {
        "soft" => repo.reset(&obj, git2::ResetType::Soft, None)?,
        "mixed" => repo.reset(&obj, git2::ResetType::Mixed, None)?,
        "hard" => {
            let mut co = git2::build::CheckoutBuilder::new();
            co.force();
            repo.reset(&obj, git2::ResetType::Hard, Some(&mut co))?;
        }
        other => {
            return Err(AppError::Other(format!(
                "invalid reset mode '{}' (soft | mixed | hard)",
                other
            )))
        }
    }

    Ok(ResetResult {
        previous_head,
        target: obj.id().to_string(),
        mode: mode.to_string(),
    })
}

/// Abort an in-progress cherry-pick / revert: hard reset to `base_oid`
/// (the pre-operation HEAD captured by the caller) or, without it, to the
/// current HEAD, then clear CHERRY_PICK_HEAD / REVERT_HEAD state.
pub fn abort_pick(repo_path: &Path, base_oid: Option<&str>) -> AppResult<()> {
    let repo = git2::Repository::open(repo_path)?;
    let target = match base_oid {
        Some(o) => git2::Oid::from_str(o)
            .map_err(|_| AppError::NotFound(format!("commit '{}' not found", o)))?,
        None => repo
            .head()
            .and_then(|h| h.target().ok_or(git2::Error::from_str("HEAD has no target")))?,
    };
    let obj = repo.find_object(target, Some(git2::ObjectType::Commit))?;
    let mut co = git2::build::CheckoutBuilder::new();
    co.force();
    repo.reset(&obj, git2::ResetType::Hard, Some(&mut co))?;
    repo.cleanup_state()?;
    Ok(())
}

/// Currently conflicted files (empty when the repo is clean of conflicts).
/// Used by the UI to surface an in-progress conflict after reload/restart.
pub fn conflict_files(repo_path: &Path) -> AppResult<Vec<String>> {
    let repo = git2::Repository::open(repo_path)?;
    let index = repo.index()?;
    conflict_paths(&index)
}

fn head_oid(repo: &git2::Repository) -> Option<String> {
    repo.head()
        .ok()
        .and_then(|h| h.target())
        .map(|o| o.to_string())
}

/// Collect unique conflicted paths from the index.
fn conflict_paths(index: &git2::Index) -> AppResult<Vec<String>> {
    let mut paths: Vec<String> = Vec::new();
    for conflict in index.conflicts()? {
        let conflict = conflict?;
        let entry = conflict
            .our
            .or(conflict.their)
            .or(conflict.ancestor);
        if let Some(entry) = entry {
            let path = String::from_utf8_lossy(&entry.path).to_string();
            if !paths.contains(&path) {
                paths.push(path);
            }
        }
    }
    Ok(paths)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "gw_history_{}_{}",
            tag,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn commit_file(
        repo: &git2::Repository,
        dir: &Path,
        name: &str,
        content: &str,
        msg: &str,
    ) -> String {
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

    fn init_repo(dir: &Path) -> git2::Repository {
        let repo = git2::Repository::init(dir).unwrap();
        commit_file(&repo, dir, "a.txt", "one\n", "init");
        repo
    }

    fn head_short(repo_path: &Path) -> String {
        let repo = git2::Repository::open(repo_path).unwrap();
        let c = repo.head().unwrap().peel_to_commit().unwrap();
        c.summary().unwrap_or_default().to_string()
    }

    /// Cherry-pick a commit from a side branch onto master: content applied,
    /// message + author preserved.
    #[test]
    fn cherry_pick_applies_commit() {
        let dir = tmpdir("pick");
        let side_oid;
        {
            let repo = init_repo(&dir);
            // Side branch with one extra commit.
            let head = repo.head().unwrap().peel_to_commit().unwrap();
            repo.branch("side", &head, false).unwrap();
            drop(head);
            drop(repo);
        }
        crate::core::branch::checkout_branch(&dir, "side").unwrap();
        {
            let repo = git2::Repository::open(&dir).unwrap();
            side_oid = commit_file(&repo, &dir, "side.txt", "side\n", "side commit");
            drop(repo);
        }
        crate::core::branch::checkout_branch(&dir, "master").unwrap();
        assert!(!dir.join("side.txt").exists());

        let outcome = cherry_pick(&dir, &[side_oid]).unwrap();
        match outcome {
            PickOutcome::Success { picked } => assert_eq!(picked, 1),
            _ => panic!("expected success"),
        }
        assert!(dir.join("side.txt").exists());
        assert_eq!(head_short(&dir), "side commit");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Revert undoes a commit's changes and creates a revert commit.
    #[test]
    fn revert_undoes_commit() {
        let dir = tmpdir("revert");
        let bad_oid;
        {
            let repo = init_repo(&dir);
            bad_oid = commit_file(&repo, &dir, "bad.txt", "bad\n", "bad commit");
            drop(repo);
        }
        assert!(dir.join("bad.txt").exists());

        let outcome = revert(&dir, &bad_oid).unwrap();
        assert!(matches!(outcome, PickOutcome::Success { picked: 1 }));
        assert!(!dir.join("bad.txt").exists());
        assert!(head_short(&dir).starts_with("Revert \"bad commit\""));

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Reset: soft keeps worktree + index; mixed keeps worktree, resets index;
    /// hard drops both and reports the previous HEAD.
    #[test]
    fn reset_modes_behave() {
        let dir = tmpdir("reset");
        let (init_oid, second_oid);
        {
            let repo = init_repo(&dir);
            init_oid = repo.head().unwrap().target().unwrap().to_string();
            second_oid = commit_file(&repo, &dir, "b.txt", "two\n", "second");
            drop(repo);
        }

        // soft back to init: HEAD moves, b.txt stays staged in the index.
        let r = reset_to(&dir, Some(&init_oid), "soft").unwrap();
        assert_eq!(r.previous_head.as_deref(), Some(second_oid.as_str()));
        {
            let repo = git2::Repository::open(&dir).unwrap();
            let mut opts = git2::StatusOptions::new();
            opts.include_untracked(false);
            let statuses = repo.statuses(Some(&mut opts)).unwrap();
            assert!(
                statuses.iter().any(|e| e.status().contains(git2::Status::INDEX_NEW)),
                "soft reset must keep b.txt staged"
            );
            drop(statuses);
            drop(repo);
        }

        // hard back to init: worktree file gone; previous head = init_oid
        // (HEAD already moved to init by the soft reset above).
        let r = reset_to(&dir, Some(&init_oid), "hard").unwrap();
        assert_eq!(r.previous_head.as_deref(), Some(init_oid.as_str()));
        assert!(!dir.join("b.txt").exists());

        // Invalid mode is a structured error.
        assert!(reset_to(&dir, Some(&init_oid), "nuke").is_err());

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A conflicting cherry-pick reports conflicted files and abort restores
    /// the pre-pick state completely.
    #[test]
    fn cherry_pick_conflict_then_abort_restores() {
        let dir = tmpdir("conflict");
        let side_oid;
        let master_head;
        {
            let repo = init_repo(&dir);
            let head = repo.head().unwrap().peel_to_commit().unwrap();
            repo.branch("side", &head, false).unwrap();
            drop(head);
            // Both branches change a.txt differently.
            commit_file(&repo, &dir, "a.txt", "master line\n", "master change");
            drop(repo);
        }
        {
            let repo = git2::Repository::open(&dir).unwrap();
            master_head = repo.head().unwrap().target().unwrap().to_string();
            drop(repo);
        }
        crate::core::branch::checkout_branch(&dir, "side").unwrap();
        {
            let repo = git2::Repository::open(&dir).unwrap();
            side_oid = commit_file(&repo, &dir, "a.txt", "side line\n", "side change");
            drop(repo);
        }
        crate::core::branch::checkout_branch(&dir, "master").unwrap();

        let outcome = cherry_pick(&dir, &[side_oid]).unwrap();
        match outcome {
            PickOutcome::Conflict {
                files,
                base_oid,
                done,
                total,
                ..
            } => {
                assert_eq!(files, vec!["a.txt".to_string()]);
                assert_eq!(base_oid.as_deref(), Some(master_head.as_str()));
                assert_eq!(done, 0);
                assert_eq!(total, 1);
            }
            _ => panic!("expected conflict"),
        }
        // Repo is in cherry-pick state with conflicts visible.
        assert_eq!(conflict_files(&dir).unwrap(), vec!["a.txt".to_string()]);

        // Abort: worktree + HEAD fully restored.
        abort_pick(&dir, Some(&master_head)).unwrap();
        assert_eq!(conflict_files(&dir).unwrap().len(), 0);
        assert_eq!(head_short(&dir), "master change");
        // Normalize EOL: checkout may apply CRLF depending on host autocrlf.
        assert_eq!(
            std::fs::read_to_string(dir.join("a.txt"))
                .unwrap()
                .replace("\r\n", "\n"),
            "master line\n"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
