//! Stash operations (T-10): save / apply / pop / drop / clear / show diff /
//! branch-from-stash. All local libgit2 work (global constraint §3).
//! Workspace-level stash is T-21; this is single-repo only.

use std::path::Path;

use serde::Serialize;

use crate::core::{branch, diff, graph};
use crate::error::{AppError, AppResult};

/// A single stash entry (index 0 = most recent).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StashEntry {
    /// Position in the stash stack (the N in `stash@{N}`).
    pub index: usize,
    /// Stash commit oid.
    pub oid: String,
    /// Full reflog-style message, e.g. "On master: my message" / "WIP on ...".
    pub message: String,
    /// Stash creation time (formatted like commit times in the graph view).
    pub time: String,
}

/// Save the working-tree changes as a stash. When `include_untracked`,
/// untracked files are stashed too. Returns the stash commit oid.
pub fn stash_save(repo_path: &Path, message: Option<&str>, include_untracked: bool) -> AppResult<String> {
    let mut repo = git2::Repository::open(repo_path)?;
    let sig = crate::core::signature_or_default(&repo)?;
    let flags = if include_untracked {
        git2::StashFlags::INCLUDE_UNTRACKED
    } else {
        git2::StashFlags::DEFAULT
    };
    let oid = repo.stash_save2(&sig, message, Some(flags))?;
    Ok(oid.to_string())
}

/// List the stash stack, newest first.
pub fn list_stashes(repo_path: &Path) -> AppResult<Vec<StashEntry>> {
    let mut repo = git2::Repository::open(repo_path)?;
    // Collect raw fields first (the closure cannot also borrow `repo`).
    let mut raw: Vec<(usize, String, git2::Oid)> = Vec::new();
    repo.stash_foreach(|index, message, oid| {
        raw.push((index, message.to_string(), *oid));
        true
    })?;
    let entries = raw
        .into_iter()
        .map(|(index, message, oid)| {
            // Time comes from the stash commit; fall back silently if unreadable.
            let time = repo
                .find_commit(oid)
                .map(|c| {
                    let when = c.committer().when();
                    graph::format_commit_time(when.seconds(), when.offset_minutes())
                })
                .unwrap_or_default();
            StashEntry {
                index,
                message,
                oid: oid.to_string(),
                time,
            }
        })
        .collect();
    Ok(entries)
}

/// Apply a stash entry without removing it from the stack.
pub fn stash_apply(repo_path: &Path, index: usize) -> AppResult<()> {
    let mut repo = git2::Repository::open(repo_path)?;
    repo.stash_apply(index, None)?;
    Ok(())
}

/// Apply a stash entry and drop it from the stack.
pub fn stash_pop(repo_path: &Path, index: usize) -> AppResult<()> {
    let mut repo = git2::Repository::open(repo_path)?;
    repo.stash_pop(index, None)?;
    Ok(())
}

/// Drop a single stash entry.
pub fn stash_drop(repo_path: &Path, index: usize) -> AppResult<()> {
    let mut repo = git2::Repository::open(repo_path)?;
    repo.stash_drop(index)?;
    Ok(())
}

/// Clear the whole stash stack (drops from the top so indices stay valid).
pub fn stash_clear(repo_path: &Path) -> AppResult<usize> {
    let mut repo = git2::Repository::open(repo_path)?;
    let mut count = 0usize;
    repo.stash_foreach(|_, _, _| {
        count += 1;
        true
    })?;
    for _ in 0..count {
        repo.stash_drop(0)?;
    }
    Ok(count)
}

/// Diff of a stash entry against its base commit (`stash@{N}^1` vs
/// `stash@{N}`), i.e. the tracked changes the stash would apply. Untracked
/// files of an include-untracked stash live in a separate commit
/// (`stash@{N}^3`) and are not part of this diff.
pub fn stash_diff(repo_path: &Path, index: usize) -> AppResult<Vec<diff::FileDiff>> {
    let repo = git2::Repository::open(repo_path)?;
    let stash_spec = format!("stash@{{{}}}", index);
    // Validate the stash exists for a clean NotFound error.
    repo.revparse_single(&stash_spec)
        .map_err(|_| AppError::NotFound(format!("stash@{{{}}} not found", index)))?;
    let base_spec = format!("{}^1", stash_spec);
    diff::diff_revisions(&repo, &base_spec, &stash_spec)
}

/// Create a branch from a stash entry (mirrors `git stash branch`):
/// branch at the stash's base commit, check it out, apply the stash, and
/// drop it on success.
pub fn branch_from_stash(repo_path: &Path, branch_name: &str, index: usize) -> AppResult<()> {
    let repo = git2::Repository::open(repo_path)?;
    let stash_spec = format!("stash@{{{}}}", index);
    let stash_commit = repo
        .revparse_single(&stash_spec)
        .and_then(|o| o.peel_to_commit())
        .map_err(|_| AppError::NotFound(format!("stash@{{{}}} not found", index)))?;
    let base_oid = stash_commit
        .parent_id(0)
        .map_err(|_| AppError::Other("stash entry has no base commit".into()))?;
    drop(stash_commit);
    drop(repo);

    branch::create_branch(repo_path, branch_name, Some(&base_oid.to_string()))?;
    branch::checkout_branch(repo_path, branch_name)?;
    stash_apply(repo_path, index)?;
    stash_drop(repo_path, index)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "gw_stash_{}_{}",
            tag,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn commit_file(repo: &git2::Repository, dir: &Path, name: &str, content: &str, msg: &str) {
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
        repo.commit(Some("HEAD"), &sig, &sig, msg, &tree, &parents).unwrap();
    }

    fn init_repo(dir: &Path) -> git2::Repository {
        let repo = git2::Repository::init(dir).unwrap();
        commit_file(&repo, dir, "a.txt", "one\n", "init");
        // Test commits never set a user identity; libgit2 stash_save needs a
        // stasher signature, which falls back to repo config — provide one.
        repo.config().unwrap().set_str("user.name", "tester").unwrap();
        repo.config().unwrap().set_str("user.email", "t@example.com").unwrap();
        repo
    }

    /// Save (tracked + untracked variants), list with metadata, drop, clear.
    #[test]
    fn save_list_drop_clear() {
        let dir = tmpdir("basic");
        {
            let repo = init_repo(&dir);
            drop(repo);
        }
        std::fs::write(dir.join("a.txt"), "one\nmodified\n").unwrap();

        let oid = stash_save(&dir, Some("work in progress"), false).unwrap();
        assert!(!oid.is_empty());
        // Tracked modification stashed: worktree restored.
        assert_eq!(
            std::fs::read_to_string(dir.join("a.txt"))
                .unwrap()
                .replace("\r\n", "\n"),
            "one\n"
        );

        // Untracked file stashed only with the flag.
        std::fs::write(dir.join("new.txt"), "new\n").unwrap();
        stash_save(&dir, Some("with untracked"), true).unwrap();
        assert!(!dir.join("new.txt").exists());

        let entries = list_stashes(&dir).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].index, 0);
        assert!(entries[0].message.contains("with untracked"));
        assert!(entries[1].message.contains("work in progress"));
        assert!(!entries[0].time.is_empty());

        stash_drop(&dir, 0).unwrap();
        let entries = list_stashes(&dir).unwrap();
        assert_eq!(entries.len(), 1);
        assert!(entries[0].message.contains("work in progress"));

        let cleared = stash_clear(&dir).unwrap();
        assert_eq!(cleared, 1);
        assert_eq!(list_stashes(&dir).unwrap().len(), 0);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Apply keeps the entry; pop applies and drops it.
    #[test]
    fn apply_and_pop() {
        let dir = tmpdir("apply");
        {
            let repo = init_repo(&dir);
            drop(repo);
        }
        std::fs::write(dir.join("a.txt"), "one\nchanged\n").unwrap();
        stash_save(&dir, Some("c"), false).unwrap();

        // Apply twice (entry kept).
        stash_apply(&dir, 0).unwrap();
        assert!(std::fs::read_to_string(dir.join("a.txt")).unwrap().contains("changed"));
        assert_eq!(list_stashes(&dir).unwrap().len(), 1);

        // Reset worktree, then pop: applied + dropped.
        let repo = git2::Repository::open(&dir).unwrap();
        repo.checkout_head(Some(git2::build::CheckoutBuilder::new().force()))
            .unwrap();
        drop(repo);
        assert!(!std::fs::read_to_string(dir.join("a.txt")).unwrap().contains("changed"));

        stash_pop(&dir, 0).unwrap();
        assert!(std::fs::read_to_string(dir.join("a.txt")).unwrap().contains("changed"));
        assert_eq!(list_stashes(&dir).unwrap().len(), 0);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Show Diff returns the stashed changes relative to the base commit.
    #[test]
    fn stash_diff_shows_changes() {
        let dir = tmpdir("diff");
        {
            let repo = init_repo(&dir);
            drop(repo);
        }
        std::fs::write(dir.join("a.txt"), "one\nstashed line\n").unwrap();
        stash_save(&dir, Some("diff me"), false).unwrap();

        let files = stash_diff(&dir, 0).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].new_path, "a.txt");
        assert_eq!(files[0].status, "modified");
        let added: Vec<&str> = files[0]
            .hunks
            .iter()
            .flat_map(|h| h.lines.iter())
            .filter(|l| l.line_type == "add")
            .map(|l| l.content.as_str())
            .collect();
        assert!(added.contains(&"stashed line"));

        // Unknown index -> structured NotFound.
        assert!(stash_diff(&dir, 9).is_err());

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Branch-from-stash: branch created at the base commit, checked out,
    /// stash applied and dropped.
    #[test]
    fn branch_from_stash_restores_changes() {
        let dir = tmpdir("branch");
        {
            let repo = init_repo(&dir);
            drop(repo);
        }
        std::fs::write(dir.join("a.txt"), "one\nstash work\n").unwrap();
        stash_save(&dir, Some("to branch"), false).unwrap();

        branch_from_stash(&dir, "rescue", 0).unwrap();

        let repo = git2::Repository::open(&dir).unwrap();
        assert_eq!(repo.head().unwrap().shorthand(), Some("rescue"));
        drop(repo);
        assert!(std::fs::read_to_string(dir.join("a.txt"))
            .unwrap()
            .contains("stash work"));
        assert_eq!(list_stashes(&dir).unwrap().len(), 0);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
