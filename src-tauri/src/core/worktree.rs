//! Git worktree management (T-17, Roadmap §14).
//!
//! Worktrees share one `.git` object store across multiple working
//! directories; a linked worktree carries a `.git` *file* pointing at
//! `<main>/.git/worktrees/<name>`. libgit2 handles the metadata
//! (`Repository::worktree` / `Worktree::prune`); the working directory itself
//! is removed with plain fs and then pruned.

use std::path::Path;

use serde::Serialize;

use crate::error::{AppError, AppResult};

/// One worktree of a repository (the main working tree included).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeInfo {
    /// Worktree name (`.git/worktrees/<name>`); the main tree uses the repo
    /// directory name.
    pub name: String,
    pub path: String,
    /// Branch checked out in this worktree (None = detached HEAD).
    pub branch: Option<String>,
    pub is_main: bool,
    pub is_locked: bool,
    /// Uncommitted changes present (drives the §46 Warning on remove).
    pub is_dirty: bool,
}

/// List the main worktree plus all linked worktrees of a repository.
pub fn list_worktrees(repo_path: &Path) -> AppResult<Vec<WorktreeInfo>> {
    let repo = git2::Repository::open(repo_path)?;
    let mut out = Vec::new();

    // Main working tree.
    let workdir = repo
        .workdir()
        .ok_or_else(|| AppError::Other("裸仓库没有工作目录".to_string()))?;
    out.push(WorktreeInfo {
        name: workdir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("main")
            .to_string(),
        path: workdir.to_string_lossy().to_string(),
        branch: head_branch(&repo),
        is_main: true,
        is_locked: false,
        is_dirty: worktree_is_dirty(workdir),
    });

    // Linked worktrees. A missing working directory (deleted externally) is
    // still listed — dirty/branch fall back to defaults — so the user can
    // prune it from the UI.
    let names = repo.worktrees()?;
    for name in names.iter().flatten() {
        let wt = repo.find_worktree(name)?;
        let path = wt.path().to_path_buf();
        let (branch, is_dirty) = if path.exists() {
            match git2::Repository::open(&path) {
                Ok(wt_repo) => (head_branch(&wt_repo), worktree_is_dirty(&path)),
                Err(_) => (None, false),
            }
        } else {
            (None, false)
        };
        let is_locked = matches!(wt.is_locked(), Ok(git2::WorktreeLockStatus::Locked(_)));
        out.push(WorktreeInfo {
            name: name.to_string(),
            path: path.to_string_lossy().to_string(),
            branch,
            is_main: false,
            is_locked,
            is_dirty,
        });
    }

    Ok(out)
}

/// Snapshot a linked worktree's identity + checked-out position — the GF-16
/// undo record for `remove_worktree` (which cannot be reconstructed from a
/// branch ref alone). Returns the encoded-ready snapshot plus the worktree's
/// HEAD oid (the log item's before-oid). None when the worktree is unknown or
/// its directory is already gone.
pub fn snapshot_worktree(repo_path: &Path, name: &str) -> Option<(crate::core::operation_log::WorktreeSnapshot, String)> {
    let info = list_worktrees(repo_path)
        .ok()?
        .into_iter()
        .find(|w| !w.is_main && w.name == name)?;
    let repo = git2::Repository::open(&info.path).ok()?;
    let head = repo.head().ok()?;
    let head_oid = head.target()?.to_string();
    let detached = repo.head_detached().unwrap_or(false);
    let snap = crate::core::operation_log::WorktreeSnapshot {
        name: info.name,
        path: info.path,
        branch: info.branch,
        oid: if detached { Some(head_oid.clone()) } else { None },
    };
    Some((snap, head_oid))
}

/// Create a linked worktree (Roadmap §14: Create Worktree / Create Branch).
///
/// - `new_branch`: create a branch at HEAD and check it out in the worktree.
/// - `branch`: check out an existing branch (fails if it is checked out
///   elsewhere — git forbids double checkout).
/// - neither: detached HEAD.
pub fn add_worktree(repo_path: &Path, path: &Path, branch: Option<&str>, new_branch: Option<&str>) -> AppResult<()> {
    let repo = git2::Repository::open(repo_path)?;

    if path.exists() && path.read_dir()?.next().is_some() {
        return Err(AppError::Other(format!("目标目录已存在且非空：{}", path.display())));
    }
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| AppError::Other("无效的 worktree 路径".to_string()))?;

    let mut opts = git2::WorktreeAddOptions::new();
    // The reference must outlive the `repo.worktree()` call (git2 stores the
    // raw pointer), so it is bound outside the branch-selection blocks.
    let reference: Option<git2::Reference> = if let Some(nb) = new_branch {
        if repo.find_branch(nb, git2::BranchType::Local).is_ok() {
            return Err(AppError::Other(format!("分支已存在：{nb}")));
        }
        let head = repo.head()?.peel_to_commit()?;
        let b = repo.branch(nb, &head, false)?;
        Some(b.into_reference())
    } else if let Some(br) = branch {
        Some(repo.find_branch(br, git2::BranchType::Local)?.into_reference())
    } else {
        None
    };
    // No reference => detached at HEAD (libgit2 default).
    if let Some(r) = reference.as_ref() {
        opts.reference(Some(r));
    }

    repo.worktree(name, path, Some(&mut opts))?;
    log::info!(
        "Worktree '{}' created at {:?} (branch={:?}, new_branch={:?})",
        name,
        path,
        branch,
        new_branch
    );
    Ok(())
}

/// Remove a linked worktree: delete its working directory, then prune the
/// metadata. With `force = false` a dirty worktree is refused (§46 Warning —
/// the UI confirms and retries with force).
pub fn remove_worktree(repo_path: &Path, name: &str, force: bool) -> AppResult<()> {
    let repo = git2::Repository::open(repo_path)?;
    let wt = repo.find_worktree(name)?;
    let path = wt.path().to_path_buf();

    if !force && path.exists() && worktree_is_dirty(&path) {
        return Err(AppError::Other(format!(
            "worktree 含未提交变更：{}。确认后将删除该目录（不可恢复，可用 reflog/stash 保底）",
            path.display()
        )));
    }

    if path.exists() {
        std::fs::remove_dir_all(&path)?;
    }
    // Prune metadata even if the directory was already gone or locked.
    let mut prune_opts = git2::WorktreePruneOptions::new();
    prune_opts.valid(true).locked(true).working_tree(true);
    wt.prune(Some(&mut prune_opts))?;

    log::info!("Worktree '{}' removed (force={})", name, force);
    Ok(())
}

/// Recreate a removed worktree exactly as recorded (undo of `remove_worktree`).
/// - `branch`: the branch that was checked out — it must still exist.
/// - `detached_oid`: set for a detached-HEAD worktree — the worktree is
///   recreated through a temporary ref at that commit, then detached inside
///   the new worktree and the temp ref dropped.
///
/// Uncommitted changes of the removed worktree are NOT recoverable (they were
/// part of the deletion); only the checked-out position is restored.
pub fn restore_worktree(
    repo_path: &Path,
    name: &str,
    path: &Path,
    branch: Option<&str>,
    detached_oid: Option<&str>,
) -> AppResult<()> {
    if path.exists() && path.read_dir()?.next().is_some() {
        return Err(AppError::Other(format!(
            "原 worktree 路径已存在且非空：{}",
            path.display()
        )));
    }
    let repo = git2::Repository::open(repo_path)?;
    // The name must still be free (a re-created worktree may reuse it).
    if repo.find_worktree(name).is_ok() {
        return Err(AppError::Other(format!("worktree '{name}' 已存在，无法重建")));
    }

    if let Some(branch) = branch {
        if repo.find_branch(branch, git2::BranchType::Local).is_err() {
            return Err(AppError::Other(format!("分支 '{branch}' 已不存在，无法恢复 worktree")));
        }
        add_worktree(repo_path, path, Some(branch), None)?;
        return Ok(());
    }

    let oid_str = detached_oid
        .ok_or_else(|| AppError::Other("worktree 记录缺少分支与 detached oid，无法重建".into()))?;
    let oid = git2::Oid::from_str(oid_str)
        .map_err(|_| AppError::Other(format!("worktree 记录 oid 无效：{oid_str}")))?;
    repo.find_commit(oid)
        .map_err(|_| AppError::Other(format!("worktree 原提交 {oid_str} 已不存在（可能已被 GC）")))?;

    // Detached-at-oid: libgit2 only supports detached-at-HEAD, so stage a
    // temporary branch at the recorded commit, check the worktree out on it,
    // detach inside the new worktree, then drop the temp branch.
    let temp_ref = format!("refs/heads/_gw_wt_restore_{}", &oid_str[..12.min(oid_str.len())]);
    if repo.find_reference(&temp_ref).is_ok() {
        return Err(AppError::Other(format!("临时引用 {temp_ref} 已存在，无法重建")));
    }
    let reference = repo
        .reference(&temp_ref, oid, false, "undo: restore detached worktree")
        .map_err(|e| AppError::Other(format!("创建临时引用失败：{}", e.message())))?;
    let restore = (|| -> AppResult<()> {
        let mut opts = git2::WorktreeAddOptions::new();
        opts.reference(Some(&reference));
        repo.worktree(name, path, Some(&mut opts))?;
        let wt_repo = git2::Repository::open(path)?;
        wt_repo.set_head_detached(oid)?;
        Ok(())
    })();
    // Always drop the temp ref, success or failure.
    let _ = repo.find_reference(&temp_ref).map(|mut r| r.delete());
    restore?;
    log::info!("Worktree '{}' restored at {:?} (detached {})", name, path, oid_str);
    Ok(())
}

/// Branch shorthand of a repository's HEAD (None when detached).
fn head_branch(repo: &git2::Repository) -> Option<String> {
    if repo.head_detached().unwrap_or(false) {
        return None;
    }
    repo.head().ok().and_then(|h| h.shorthand().map(str::to_string))
}

/// Whether a working directory has uncommitted changes.
fn worktree_is_dirty(path: &Path) -> bool {
    let Ok(repo) = git2::Repository::open(path) else {
        return false;
    };
    let mut opts = git2::StatusOptions::new();
    opts.include_untracked(true);
    repo.statuses(Some(&mut opts)).map(|s| !s.is_empty()).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "gw_wt_{}_{}",
            tag,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Init a repo with one commit on branch "main".
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
        // Ensure a named branch exists for checkout-based tests.
        let head = repo.head().unwrap().peel_to_commit().unwrap();
        repo.branch("feature", &head, false).unwrap();
    }

    /// List sees the main worktree; adding with a new branch shows up with
    /// that branch; the worktree dir carries a `.git` *file*.
    #[test]
    fn add_and_list_worktrees() {
        let dir = tmpdir("list");
        init_repo(&dir);
        let wt_path = dir
            .parent()
            .unwrap()
            .join(format!("{}-wt", dir.file_name().unwrap().to_string_lossy()));

        add_worktree(&dir, &wt_path, None, Some("wt-branch")).unwrap();
        assert!(wt_path.join(".git").is_file(), "linked worktree uses a .git file");

        let list = list_worktrees(&dir).unwrap();
        assert_eq!(list.len(), 2);
        assert!(list[0].is_main);
        let wt = list.iter().find(|w| !w.is_main).unwrap();
        assert_eq!(wt.branch.as_deref(), Some("wt-branch"));
        assert!(!wt.is_dirty);

        // Cleanup.
        remove_worktree(&dir, wt.name.as_str(), true).unwrap();
        assert!(!wt_path.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A dirty worktree refuses removal without force; force removes it.
    #[test]
    fn remove_dirty_worktree_requires_force() {
        let dir = tmpdir("dirty");
        init_repo(&dir);
        let wt_path = dir
            .parent()
            .unwrap()
            .join(format!("{}-wtd", dir.file_name().unwrap().to_string_lossy()));
        add_worktree(&dir, &wt_path, Some("feature"), None).unwrap();

        // Make the worktree dirty.
        std::fs::write(wt_path.join("a.txt"), "changed\n").unwrap();

        let name = wt_path.file_name().unwrap().to_string_lossy().to_string();
        let err = remove_worktree(&dir, &name, false).unwrap_err();
        assert!(err.to_string().contains("未提交变更"), "dirty check: {err}");

        remove_worktree(&dir, &name, true).unwrap();
        assert!(!wt_path.exists());
        let list = list_worktrees(&dir).unwrap();
        assert_eq!(list.len(), 1, "pruned worktree disappears");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// An externally deleted worktree directory can still be pruned.
    #[test]
    fn remove_prunes_externally_deleted_worktree() {
        let dir = tmpdir("gone");
        init_repo(&dir);
        let wt_path = dir
            .parent()
            .unwrap()
            .join(format!("{}-wtg", dir.file_name().unwrap().to_string_lossy()));
        add_worktree(&dir, &wt_path, None, Some("gone-branch")).unwrap();
        std::fs::remove_dir_all(&wt_path).unwrap();

        let name = wt_path.file_name().unwrap().to_string_lossy().to_string();
        remove_worktree(&dir, &name, false).unwrap();
        let list = list_worktrees(&dir).unwrap();
        assert_eq!(list.len(), 1);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// GF-16: undo of a worktree removal recreates it — at the same path, on
    /// the same branch, with the `.git` file form restored.
    #[test]
    fn restore_worktree_recreates_branch_checkout() {
        let dir = tmpdir("undo_branch");
        init_repo(&dir);
        let wt_path = dir
            .parent()
            .unwrap()
            .join(format!("{}-uwb", dir.file_name().unwrap().to_string_lossy()));
        add_worktree(&dir, &wt_path, None, Some("restore-me")).unwrap();
        let name = wt_path.file_name().unwrap().to_string_lossy().to_string();

        remove_worktree(&dir, &name, true).unwrap();
        assert!(!wt_path.exists());

        restore_worktree(&dir, &name, &wt_path, Some("restore-me"), None).unwrap();
        assert!(wt_path.join(".git").is_file(), "linked worktree uses a .git file");
        let list = list_worktrees(&dir).unwrap();
        let wt = list.iter().find(|w| !w.is_main).unwrap();
        assert_eq!(wt.branch.as_deref(), Some("restore-me"));

        // Cleanup.
        remove_worktree(&dir, &name, true).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A detached-HEAD worktree is recreated detached at the recorded commit.
    #[test]
    fn restore_worktree_recreates_detached_head() {
        let dir = tmpdir("undo_detached");
        init_repo(&dir);
        let expected_oid = {
            let repo = git2::Repository::open(&dir).unwrap();
            let oid = repo.head().unwrap().target().unwrap().to_string();
            drop(repo);
            oid
        };
        let wt_path = dir
            .parent()
            .unwrap()
            .join(format!("{}-uwd", dir.file_name().unwrap().to_string_lossy()));
        add_worktree(&dir, &wt_path, None, None).unwrap();
        let name = wt_path.file_name().unwrap().to_string_lossy().to_string();

        // Record the worktree's HEAD, remove it, undo.
        let recorded_oid = git2::Repository::open(&wt_path)
            .unwrap()
            .head()
            .unwrap()
            .target()
            .unwrap()
            .to_string();
        remove_worktree(&dir, &name, true).unwrap();
        assert!(!wt_path.exists());

        restore_worktree(&dir, &name, &wt_path, None, Some(&recorded_oid)).unwrap();
        let wt_repo = git2::Repository::open(&wt_path).unwrap();
        assert!(wt_repo.head_detached().unwrap(), "restored worktree must be detached");
        assert_eq!(wt_repo.head().unwrap().target().unwrap().to_string(), expected_oid);
        // No temp branch left behind.
        assert!(!list_stash_temp_refs(&dir));

        // Cleanup.
        remove_worktree(&dir, &name, true).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The temp ref used for detached restores must not be observable as a
    /// branch after the restore.
    fn list_stash_temp_refs(dir: &Path) -> bool {
        let repo = git2::Repository::open(dir).unwrap();
        repo.references()
            .map(|refs| {
                refs.filter_map(|r| r.ok())
                    .any(|r| r.name().unwrap_or_default().starts_with("refs/heads/_gw_wt_restore_"))
            })
            .unwrap_or(false)
    }

    /// A missing branch / occupied path are refused with actionable errors.
    #[test]
    fn restore_worktree_refuses_missing_branch_and_occupied_path() {
        let dir = tmpdir("undo_refuse");
        init_repo(&dir);
        let wt_path = dir
            .parent()
            .unwrap()
            .join(format!("{}-uwr", dir.file_name().unwrap().to_string_lossy()));
        let name = wt_path.file_name().unwrap().to_string_lossy().to_string();

        let err = restore_worktree(&dir, &name, &wt_path, Some("nope"), None).unwrap_err();
        assert!(err.to_string().contains("已不存在"), "{err}");

        add_worktree(&dir, &wt_path, Some("feature"), None).unwrap();
        let err = restore_worktree(&dir, &name, &wt_path, Some("feature"), None).unwrap_err();
        assert!(err.to_string().contains("已存在且非空"), "{err}");

        remove_worktree(&dir, &name, true).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }
}
