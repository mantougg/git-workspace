//! Branch Manager core (T-09): local/remote branch and tag listing with
//! upstream ahead/behind, branch lifecycle operations, and branch compare.
//!
//! Everything here is strictly local (Roadmap §46): ahead/behind comes from
//! local remote-tracking refs only — no network fetch is ever triggered.
//! `Repository` handles are opened and dropped per call, never shared across
//! threads (libgit2 thread-safety boundary).

use std::path::Path;

use serde::Serialize;

use crate::core::{diff, graph};
use crate::error::{AppError, AppResult};

/// A local branch with its upstream tracking state.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BranchEntry {
    pub name: String,
    pub is_current: bool,
    pub last_commit_oid: String,
    pub last_commit_message: String,
    /// Upstream remote branch, e.g. "origin/main"; None if not tracking.
    pub upstream: Option<String>,
    /// Commits ahead of the upstream (local-only).
    pub ahead: usize,
    /// Commits behind the upstream (local-only).
    pub behind: usize,
}

/// A remote-tracking branch (local snapshot of `refs/remotes/*`).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteBranchEntry {
    pub name: String,
    pub last_commit_oid: String,
    pub last_commit_message: String,
}

/// A tag with its (peeled) target commit.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TagEntry {
    pub name: String,
    pub target_oid: String,
    /// Annotated tag message, if any.
    pub message: Option<String>,
}

/// The three branch-manager sections of one repository.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BranchOverview {
    /// Current branch name; None when HEAD is detached.
    pub current: Option<String>,
    pub locals: Vec<BranchEntry>,
    pub remotes: Vec<RemoteBranchEntry>,
    pub tags: Vec<TagEntry>,
}

/// Result of comparing two revisions (Branch Compare).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompareResult {
    pub base: String,
    pub other: String,
    /// Commits reachable from `other` but not from `base`.
    pub ahead: Vec<graph::CommitInfo>,
    /// Commits reachable from `base` but not from `other`.
    pub behind: Vec<graph::CommitInfo>,
    /// File diff from `base` to `other`.
    pub files: Vec<diff::FileDiff>,
}

/// Upper bound for the commit差集 of a compare, keeping IPC payloads bounded.
const COMPARE_MAX_COMMITS: usize = 200;

/// List local branches (with upstream ahead/behind), remote-tracking branches
/// and tags of a repository. Purely local: no network access.
pub fn list_branches(repo_path: &Path) -> AppResult<BranchOverview> {
    let repo = git2::Repository::open(repo_path)?;

    let current = repo
        .head()
        .ok()
        .and_then(|h| {
            if h.is_branch() {
                h.shorthand().map(String::from)
            } else {
                None
            }
        });

    let mut locals = Vec::new();
    let mut remotes = Vec::new();

    for item in repo.branches(None)? {
        let (branch, btype) = item?;
        let name = match branch.name()? {
            Some(n) if !n.is_empty() => n.to_string(),
            _ => continue,
        };
        let (oid, message) = last_commit(&repo, &branch);
        match btype {
            git2::BranchType::Local => {
                let (upstream, ahead, behind) = upstream_info(&repo, &branch);
                locals.push(BranchEntry {
                    name,
                    is_current: branch.is_head(),
                    last_commit_oid: oid,
                    last_commit_message: message,
                    upstream,
                    ahead,
                    behind,
                });
            }
            git2::BranchType::Remote => {
                // Skip symbolic refs such as `origin/HEAD`.
                if branch.get().symbolic_target().is_some() {
                    continue;
                }
                remotes.push(RemoteBranchEntry {
                    name,
                    last_commit_oid: oid,
                    last_commit_message: message,
                });
            }
        }
    }

    locals.sort_by(|a, b| a.name.cmp(&b.name));
    remotes.sort_by(|a, b| a.name.cmp(&b.name));

    let mut tags = Vec::new();
    let tag_names = repo.tag_names(None)?;
    for name in tag_names.iter().flatten() {
        let reference = format!("refs/tags/{}", name);
        if let Ok(obj) = repo.revparse_single(&reference) {
            let message = obj
                .as_tag()
                .and_then(|t| t.message().map(String::from));
            let target_oid = obj
                .peel_to_commit()
                .map(|c| c.id().to_string())
                .unwrap_or_else(|_| obj.id().to_string());
            tags.push(TagEntry {
                name: name.to_string(),
                target_oid,
                message,
            });
        }
    }
    tags.sort_by(|a, b| a.name.cmp(&b.name));

    Ok(BranchOverview {
        current,
        locals,
        remotes,
        tags,
    })
}

/// Tip commit oid + summary of a branch (empty strings when unborn/symbolic).
fn last_commit(repo: &git2::Repository, branch: &git2::Branch) -> (String, String) {
    match branch.get().target() {
        Some(oid) => {
            let message = repo
                .find_commit(oid)
                .ok()
                .and_then(|c| c.summary().map(String::from))
                .unwrap_or_default();
            (oid.to_string(), message)
        }
        None => (String::new(), String::new()),
    }
}

/// Upstream name and local ahead/behind counts for a local branch.
/// Uses only the local remote-tracking ref — never fetches.
fn upstream_info(
    repo: &git2::Repository,
    branch: &git2::Branch,
) -> (Option<String>, usize, usize) {
    let upstream = match branch.upstream() {
        Ok(u) => u,
        Err(_) => return (None, 0, 0),
    };
    let name = upstream.name().ok().flatten().map(String::from);
    let local_oid = branch.get().target();
    let upstream_oid = upstream.get().target();
    let (ahead, behind) = match (local_oid, upstream_oid) {
        (Some(l), Some(u)) => repo.graph_ahead_behind(l, u).unwrap_or((0, 0)),
        _ => (0, 0),
    };
    (name, ahead, behind)
}

/// Create a local branch at HEAD (or at `target` when given: branch/tag/oid).
pub fn create_branch(repo_path: &Path, name: &str, target: Option<&str>) -> AppResult<()> {
    let repo = git2::Repository::open(repo_path)?;
    let spec = target.unwrap_or("HEAD");
    let commit = repo
        .revparse_single(spec)
        .and_then(|o| o.peel_to_commit())
        .map_err(|_| AppError::NotFound(format!("target '{}' not found", spec)))?;
    // force = false: error out if the branch already exists.
    repo.branch(name, &commit, false)?;
    Ok(())
}

/// Checkout a local branch. A dirty worktree that conflicts with the target
/// tree makes libgit2's safe checkout fail with a structured error.
pub fn checkout_branch(repo_path: &Path, name: &str) -> AppResult<()> {
    let repo = git2::Repository::open(repo_path)?;
    let refname = format!("refs/heads/{}", name);
    let obj = repo
        .revparse_single(&refname)
        .map_err(|_| AppError::NotFound(format!("branch '{}' not found", name)))?;
    // checkout_tree BEFORE set_head: the checkout baseline defaults to the
    // current HEAD tree, so files absent from the target tree (tracked on the
    // old branch) are removed from the worktree and index.
    repo.checkout_tree(&obj, None)?;
    repo.set_head(&refname)?;
    Ok(())
}

/// Delete a local branch. Refuses to delete the current branch, and refuses
/// to delete a branch whose tip is not merged into HEAD unless `force`.
pub fn delete_branch(repo_path: &Path, name: &str, force: bool) -> AppResult<()> {
    let repo = git2::Repository::open(repo_path)?;
    let mut branch = repo
        .find_branch(name, git2::BranchType::Local)
        .map_err(|_| AppError::NotFound(format!("branch '{}' not found", name)))?;

    if branch.is_head() {
        return Err(AppError::Conflict(format!(
            "cannot delete the current branch '{}'",
            name
        )));
    }

    if !force {
        let tip = branch
            .get()
            .target()
            .ok_or_else(|| AppError::Other(format!("branch '{}' has no target", name)))?;
        let head = repo
            .head()
            .and_then(|h| h.target().ok_or(git2::Error::from_str("HEAD has no target")))?;
        // tip reachable from HEAD => the branch is fully merged. Note:
        // graph_descendant_of treats a commit as NOT its own descendant, so
        // an identical tip (branch at HEAD) must be allowed explicitly.
        let merged =
            tip == head || repo.graph_descendant_of(head, tip).unwrap_or(false);
        if !merged {
            return Err(AppError::Conflict(format!(
                "branch '{}' is not fully merged; deleting it may lose commits — retry with force",
                name
            )));
        }
    }

    branch.delete()?;
    Ok(())
}

/// Rename a local branch (force = false: error if the new name exists).
pub fn rename_branch(repo_path: &Path, old_name: &str, new_name: &str) -> AppResult<()> {
    let repo = git2::Repository::open(repo_path)?;
    let mut branch = repo
        .find_branch(old_name, git2::BranchType::Local)
        .map_err(|_| AppError::NotFound(format!("branch '{}' not found", old_name)))?;
    branch.rename(new_name, false)?;
    Ok(())
}

/// Set (or clear, when `upstream` is None) the upstream of a local branch.
/// The upstream must be an existing remote-tracking branch, e.g. "origin/main".
pub fn set_upstream(
    repo_path: &Path,
    branch_name: &str,
    upstream: Option<&str>,
) -> AppResult<()> {
    let repo = git2::Repository::open(repo_path)?;
    let mut branch = repo
        .find_branch(branch_name, git2::BranchType::Local)
        .map_err(|_| AppError::NotFound(format!("branch '{}' not found", branch_name)))?;

    match upstream {
        Some(u) => {
            repo.find_branch(u, git2::BranchType::Remote)
                .map_err(|_| {
                    AppError::NotFound(format!("remote branch '{}' not found", u))
                })?;
            branch.set_upstream(Some(u))?;
        }
        // git2 0.19 has no dedicated unset; None clears the upstream config.
        None => branch.set_upstream(None)?,
    }
    Ok(())
}

/// Create a local branch tracking the given remote branch (e.g. pass
/// "origin/feature" to create local "feature" with its upstream set).
pub fn track_remote_branch(repo_path: &Path, remote_name: &str) -> AppResult<()> {
    let repo = git2::Repository::open(repo_path)?;
    let remote = repo
        .find_branch(remote_name, git2::BranchType::Remote)
        .map_err(|_| AppError::NotFound(format!("remote branch '{}' not found", remote_name)))?;

    // Local name defaults to the part after the first '/' (remote name).
    let local_name = remote_name
        .splitn(2, '/')
        .nth(1)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            AppError::Other(format!("invalid remote branch name '{}'", remote_name))
        })?;

    let commit = remote
        .get()
        .peel_to_commit()
        .map_err(|_| AppError::NotFound(format!("remote branch '{}' has no commit", remote_name)))?;
    // force = false: error out if the local branch already exists.
    let mut branch = repo.branch(local_name, &commit, false)?;
    branch.set_upstream(Some(remote_name))?;
    Ok(())
}

/// Compare two revisions (branch / tag / oid specs): commit差集 in both
/// directions plus the tree diff from `base` to `other`.
pub fn compare_branches(
    repo_path: &Path,
    base: &str,
    other: &str,
) -> AppResult<CompareResult> {
    let repo = git2::Repository::open(repo_path)?;
    let base_commit = repo
        .revparse_single(base)
        .and_then(|o| o.peel_to_commit())
        .map_err(|_| AppError::NotFound(format!("revision '{}' not found", base)))?;
    let other_commit = repo
        .revparse_single(other)
        .and_then(|o| o.peel_to_commit())
        .map_err(|_| AppError::NotFound(format!("revision '{}' not found", other)))?;

    let ahead = revwalk_commit_infos(&repo, other_commit.id(), Some(base_commit.id()))?;
    let behind = revwalk_commit_infos(&repo, base_commit.id(), Some(other_commit.id()))?;
    let files = diff::diff_revisions(&repo, base, other)?;

    Ok(CompareResult {
        base: base.to_string(),
        other: other.to_string(),
        ahead,
        behind,
        files,
    })
}

/// Commit infos reachable from `from`, excluding anything reachable from
/// `hide`, newest first, capped at COMPARE_MAX_COMMITS.
fn revwalk_commit_infos(
    repo: &git2::Repository,
    from: git2::Oid,
    hide: Option<git2::Oid>,
) -> AppResult<Vec<graph::CommitInfo>> {
    let mut walk = repo.revwalk()?;
    walk.set_sorting(git2::Sort::TOPOLOGICAL | git2::Sort::TIME)?;
    walk.push(from)?;
    if let Some(h) = hide {
        walk.hide(h)?;
    }
    let ref_map = graph::ref_map(repo);
    let mut out = Vec::new();
    for oid in walk.take(COMPARE_MAX_COMMITS).flatten() {
        if let Some(record) = graph::commit_record_from_oid(repo, &oid) {
            let refs = ref_map.get(&oid.to_string()).cloned().unwrap_or_default();
            out.push(graph::commit_info_from_record(&record, refs));
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "gw_branch_{}_{}",
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
        repo.commit(Some("HEAD"), &sig, &sig, msg, &tree, &parents)
            .unwrap();
    }

    fn init_repo(dir: &Path) -> git2::Repository {
        let repo = git2::Repository::init(dir).unwrap();
        commit_file(&repo, dir, "a.txt", "one\n", "init");
        repo
    }

    /// Branch lifecycle: create / list grouping / rename / delete rules.
    #[test]
    fn branch_lifecycle() {
        let dir = tmpdir("lifecycle");
        {
            let repo = init_repo(&dir);
            drop(repo);
        }

        create_branch(&dir, "feature", None).unwrap();
        // Duplicate create must fail (force = false).
        assert!(create_branch(&dir, "feature", None).is_err());

        let overview = list_branches(&dir).unwrap();
        assert_eq!(overview.current.as_deref(), Some("master"));
        let names: Vec<&str> = overview.locals.iter().map(|b| b.name.as_str()).collect();
        assert!(names.contains(&"master"));
        assert!(names.contains(&"feature"));
        let master = overview.locals.iter().find(|b| b.name == "master").unwrap();
        assert!(master.is_current);
        assert_eq!(master.upstream, None);

        rename_branch(&dir, "feature", "feature-2").unwrap();
        let overview = list_branches(&dir).unwrap();
        let names: Vec<&str> = overview.locals.iter().map(|b| b.name.as_str()).collect();
        assert!(names.contains(&"feature-2"));
        assert!(!names.contains(&"feature"));

        // Current branch cannot be deleted.
        assert!(delete_branch(&dir, "master", false).is_err());
        // Merged branch deletes fine without force.
        delete_branch(&dir, "feature-2", false).unwrap();
        let overview = list_branches(&dir).unwrap();
        assert_eq!(overview.locals.len(), 1);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// An unmerged branch requires force to delete.
    #[test]
    fn unmerged_branch_delete_requires_force() {
        let dir = tmpdir("unmerged");
        {
            let repo = init_repo(&dir);
            drop(repo);
        }

        create_branch(&dir, "work", None).unwrap();
        checkout_branch(&dir, "work").unwrap();
        {
            let repo = git2::Repository::open(&dir).unwrap();
            commit_file(&repo, &dir, "b.txt", "two\n", "work commit");
            drop(repo);
        }
        checkout_branch(&dir, "master").unwrap();

        // Not merged into master: refused without force, allowed with force.
        assert!(delete_branch(&dir, "work", false).is_err());
        delete_branch(&dir, "work", true).unwrap();

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Checkout switches HEAD and materializes the target tree.
    #[test]
    fn checkout_switches_tree() {
        let dir = tmpdir("checkout");
        {
            let repo = init_repo(&dir);
            drop(repo);
        }

        create_branch(&dir, "alt", None).unwrap();
        checkout_branch(&dir, "alt").unwrap();
        {
            let repo = git2::Repository::open(&dir).unwrap();
            commit_file(&repo, &dir, "alt.txt", "alt\n", "alt commit");
            drop(repo);
        }
        checkout_branch(&dir, "master").unwrap();
        assert!(!dir.join("alt.txt").exists());

        let overview = list_branches(&dir).unwrap();
        assert_eq!(overview.current.as_deref(), Some("master"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Compare reports the commit差集 in both directions plus the tree diff.
    #[test]
    fn compare_reports_commit_sets_and_files() {
        let dir = tmpdir("compare");
        {
            let repo = init_repo(&dir);
            drop(repo);
        }

        create_branch(&dir, "feature", None).unwrap();
        checkout_branch(&dir, "feature").unwrap();
        {
            let repo = git2::Repository::open(&dir).unwrap();
            commit_file(&repo, &dir, "feat.txt", "feat\n", "feature commit");
            drop(repo);
        }
        checkout_branch(&dir, "master").unwrap();
        {
            let repo = git2::Repository::open(&dir).unwrap();
            commit_file(&repo, &dir, "master.txt", "master\n", "master commit");
            drop(repo);
        }

        let result = compare_branches(&dir, "master", "feature").unwrap();
        assert_eq!(result.ahead.len(), 1);
        assert_eq!(result.ahead[0].message, "feature commit");
        assert_eq!(result.behind.len(), 1);
        assert_eq!(result.behind[0].message, "master commit");

        let paths: Vec<&str> = result.files.iter().map(|f| f.new_path.as_str()).collect();
        assert!(paths.contains(&"feat.txt"));
        assert!(paths.contains(&"master.txt"));

        // Identical revisions: empty差集 and no file diffs.
        let same = compare_branches(&dir, "master", "master").unwrap();
        assert!(same.ahead.is_empty());
        assert!(same.behind.is_empty());
        assert!(same.files.is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Tags are listed with their target commit oid.
    #[test]
    fn tags_are_listed() {
        let dir = tmpdir("tags");
        {
            let repo = init_repo(&dir);
            let head = repo.head().unwrap().target().unwrap();
            let obj = repo.find_object(head, None).unwrap();
            repo.tag_lightweight("v1.0", &obj, false).unwrap();
            drop(obj);
            drop(repo);
        }

        let overview = list_branches(&dir).unwrap();
        assert_eq!(overview.tags.len(), 1);
        assert_eq!(overview.tags[0].name, "v1.0");
        assert!(!overview.tags[0].target_oid.is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
