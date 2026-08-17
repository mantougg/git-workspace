use std::path::Path;

use crate::error::AppResult;
use crate::models::repository::{FileChange, RepoChanges, RepoStatus};

/// Read the file-level change list of a repository.
///
/// Returns every changed file with its change category, suitable for
/// building a selectable change tree in the UI. Files are sorted by path.
pub fn get_repo_changes(repo_path: &Path) -> AppResult<RepoChanges> {    let repo = git2::Repository::open(repo_path)?;

    // 1. Determine current branch
    let (branch, is_detached) = match repo.head() {
        Ok(head) => {
            let is_branch = head.is_branch();
            let name = head.shorthand().unwrap_or("HEAD").to_string();
            (name, !is_branch)
        }
        Err(_) => ("(no commits)".to_string(), true),
    };

    // 2. Read file-level statuses
    let mut opts = git2::StatusOptions::new();
    opts.include_untracked(true);
    // Expand untracked directories into their files so the UI tree shows
    // the actual files, not just the folder.
    opts.recurse_untracked_dirs(true);

    let statuses = repo.statuses(Some(&mut opts))?;

    let mut changes: Vec<FileChange> = Vec::new();
    for entry in statuses.iter() {
        let s = entry.status();
        let category = classify_status(s);
        if category == "clean" {
            continue;
        }

        // For renames, `entry.path()` may be None; fall back to the diff deltas.
        let path = entry
            .path()
            .or_else(|| {
                entry
                    .index_to_workdir()
                    .and_then(|d| d.new_file().path())
                    .and_then(|p| p.to_str())
            })
            .or_else(|| {
                entry
                    .head_to_index()
                    .and_then(|d| d.new_file().path())
                    .and_then(|p| p.to_str())
            })
            .unwrap_or("")
            .replace('\\', "/");

        if path.is_empty() {
            continue;
        }

        // Skip untracked files under common runtime/generated directories
        // (e.g. `.workspaces/`, `.project-store/`, node_modules). Tracked
        // file modifications are never filtered.
        if category == "untracked" && is_runtime_path(&path) {
            continue;
        }

        changes.push(FileChange {
            path,
            status: category.to_string(),
        });
    }
    changes.sort_by(|a, b| a.path.cmp(&b.path));

    // 3. Remote tracking status
    let (ahead, behind) = compute_ahead_behind(&repo, &branch);

    let repo_name = repo_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string();

    Ok(RepoChanges {
        repo_path: repo_path.to_string_lossy().to_string(),
        repo_name,
        // Filled in by get_workspace_changes from the DB record.
        relative_path: String::new(),
        branch,
        is_detached,
        ahead,
        behind,
        changes,
    })
}

/// Whether a path lives under a common runtime/generated directory that
/// should not be shown as untracked (mirrors the scanner's skip list).
pub(crate) fn is_runtime_path(path: &str) -> bool {
    let top = path.split('/').next().unwrap_or("");
    matches!(
        top,
        ".workspaces"
            | ".project-store"
            | ".next"
            | ".nuxt"
            | ".cache"
            | "node_modules"
            | "dist"
            | "build"
            | "target"
            | "out"
            | "coverage"
            | "__pycache__"
            | ".gradle"
            | ".m2"
            | "venv"
            | ".venv"
            | ".idea"
            | ".vscode"
            | "logs"
            | ".git"
    )
}

/// Map a libgit2 status bitmask to a coarse change category.
fn classify_status(s: git2::Status) -> &'static str {    if s.is_index_renamed() {
        "renamed"
    } else if s.is_index_typechange() {
        "typechange"
    } else if s.is_index_new() {
        "added"
    } else if s.is_index_modified() {
        "modified"
    } else if s.is_index_deleted() {
        "deleted"
    } else if s.is_wt_new() {
        "untracked"
    } else if s.is_wt_modified() {
        "modified"
    } else if s.is_wt_deleted() {
        "deleted"
    } else {
        "clean"
    }
}

/// Read the current Git status of a repository.
///
/// Returns information about:
/// - Current branch (or detached HEAD state)
/// - File status counts (modified, added, deleted, untracked, staged)
/// - Remote tracking status (ahead/behind upstream)
///
/// Repositories with no commits (unborn HEAD) are handled gracefully:
/// all files appear as untracked.
pub fn get_repo_status(repo_path: &Path) -> AppResult<RepoStatus> {
    let repo = git2::Repository::open(repo_path)?;

    // 1. Determine current branch
    let (branch, is_detached) = match repo.head() {
        Ok(head) => {
            let is_branch = head.is_branch();
            let name = head
                .shorthand()
                .unwrap_or("HEAD")
                .to_string();
            (name, !is_branch)
        }
        Err(_) => {
            // No HEAD (repo with no commits) - everything is untracked
            let mut opts = git2::StatusOptions::new();
            opts.include_untracked(true);
            opts.recurse_untracked_dirs(true);
            let statuses = repo.statuses(Some(&mut opts))?;

            let untracked = statuses
                .iter()
                .filter(|e| {
                    e.status().is_wt_new()
                        && !e.path().map(is_runtime_path).unwrap_or(false)
                })
                .count();

            return Ok(RepoStatus {
                branch: "(no commits)".to_string(),
                is_detached: true,
                ahead: 0,
                behind: 0,
                modified: 0,
                added: 0,
                deleted: 0,
                untracked,
                staged: 0,
                conflicted: 0,
                has_remote: repo.remotes().map(|r| !r.is_empty()).unwrap_or(false),
                is_clean: untracked == 0,
            });
        }
    };

    // 2. Read file statuses
    let mut opts = git2::StatusOptions::new();
    opts.include_untracked(true);
    opts.recurse_untracked_dirs(true);

    let statuses = repo.statuses(Some(&mut opts))?;

    let mut modified = 0;
    let mut added = 0;
    let mut deleted = 0;
    let mut untracked = 0;
    let mut staged = 0;
    let mut conflicted = 0;

    for entry in statuses.iter() {
        let s = entry.status();

        // Conflicts are counted in their own bucket (T-18 Dashboard conflict
        // card): libgit2 gives conflicted paths their own status bit; do not
        // also count them as modified/deleted.
        if s.is_conflicted() {
            conflicted += 1;
            continue;
        }

        // Staged (index) changes
        if s.is_index_new() {
            staged += 1;
            added += 1;
        }
        if s.is_index_modified() {
            staged += 1;
            modified += 1;
        }
        if s.is_index_deleted() {
            staged += 1;
            deleted += 1;
        }
        if s.is_index_renamed() {
            staged += 1;
        }
        if s.is_index_typechange() {
            staged += 1;
        }

        // Working tree changes (unstaged)
        if s.is_wt_modified() && !s.is_index_modified() {
            modified += 1;
        }
        if s.is_wt_new() {
            // Count untracked files, excluding runtime/generated dirs.
            let runtime = entry
                .path()
                .map(is_runtime_path)
                .unwrap_or(false);
            if !runtime {
                untracked += 1;
            }
        }
        if s.is_wt_deleted() && !s.is_index_deleted() {
            deleted += 1;
        }
    }

    // 3. Calculate ahead/behind relative to upstream
    let (ahead, behind) = compute_ahead_behind(&repo, &branch);

    let is_clean = modified == 0
        && added == 0
        && deleted == 0
        && untracked == 0
        && staged == 0
        && conflicted == 0
        && ahead == 0
        && behind == 0;

    Ok(RepoStatus {
        branch,
        is_detached,
        ahead,
        behind,
        modified,
        added,
        deleted,
        untracked,
        staged,
        conflicted,
        has_remote: repo.remotes().map(|r| !r.is_empty()).unwrap_or(false),
        is_clean,
    })
}

/// Compute how many commits the local branch is ahead/behind its upstream.
/// Returns (0, 0) if no upstream is configured or the comparison fails.
fn compute_ahead_behind(repo: &git2::Repository, branch_name: &str) -> (usize, usize) {    let local_branch = match repo.find_branch(branch_name, git2::BranchType::Local) {
        Ok(b) => b,
        Err(_) => return (0, 0),
    };

    let upstream = match local_branch.upstream() {
        Ok(u) => u,
        Err(_) => return (0, 0),
    };

    let local_oid = match local_branch.get().target() {
        Some(oid) => oid,
        None => return (0, 0),
    };

    let upstream_oid = match upstream.get().target() {
        Some(oid) => oid,
        None => return (0, 0),
    };

    match repo.graph_ahead_behind(local_oid, upstream_oid) {
        Ok((a, b)) => (a, b),
        Err(_) => (0, 0),
    }
}

/// Given a list of changed file paths and candidate repository root paths,
/// return the repository roots that contain at least one changed path.
///
/// Used by the file watcher to refresh only affected repositories instead of
/// rescanning the whole workspace (incremental status, §37).
pub fn find_affected_repos<'a>(
    changed_paths: &[String],
    repo_roots: &'a [String],
) -> Vec<&'a str> {
    repo_roots
        .iter()
        .filter(|root| {
            changed_paths
                .iter()
                .any(|cp| path_under_root(cp, root))
        })
        .map(|r| r.as_str())
        .collect()
}

/// Whether `path` is `root` itself or a descendant of `root` (path-boundary
/// aware, so `/ws/a` does not match `/ws/ab`).
fn path_under_root(path: &str, root: &str) -> bool {
    if !path.starts_with(root) {
        return false;
    }
    if path.len() == root.len() {
        return true;
    }
    matches!(path.as_bytes().get(root.len()), Some(b'/') | Some(b'\\'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_affected_repos_matches_only_prefix_boundary() {
        let repos = vec![
            "D:/ws/a".to_string(),
            "D:/ws/ab".to_string(),
            "D:/ws/b".to_string(),
        ];

        let affected = find_affected_repos(&["D:/ws/a/src/main.rs".to_string()], &repos);
        assert_eq!(affected, vec!["D:/ws/a"]);
    }

    #[test]
    fn find_affected_repos_returns_empty_on_no_match() {
        let repos = vec!["D:/ws/a".to_string()];
        let affected = find_affected_repos(&["D:/other/x.txt".to_string()], &repos);
        assert!(affected.is_empty());
    }

    fn commit_file(repo: &git2::Repository, dir: &Path, name: &str, content: &str, msg: &str) {
        std::fs::write(dir.join(name), content).unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new(name)).unwrap();
        index.write().unwrap();
        let tree_oid = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_oid).unwrap();
        let sig = git2::Signature::now("tester", "t@example.com").unwrap();
        let parent = repo.head().ok().and_then(|h| h.peel_to_commit().ok());
        match &parent {
            Some(p) => repo
                .commit(Some("HEAD"), &sig, &sig, msg, &tree, &[p])
                .unwrap(),
            None => repo
                .commit(Some("HEAD"), &sig, &sig, msg, &tree, &[])
                .unwrap(),
        };
    }

    /// Conflicted files get their own bucket (T-18 Dashboard conflict card):
    /// a repo mid-merge reports conflicted > 0, is not clean, and does not
    /// double-count the conflicted path as modified.
    #[test]
    fn repo_status_counts_conflicts_separately() {
        let dir = std::env::temp_dir().join(format!(
            "gw_status_conflict_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();

        // Divergent edits on master + side, then a merge that conflicts.
        {
            let repo = git2::Repository::init(&dir).unwrap();
            commit_file(&repo, &dir, "a.txt", "base\n", "init");
            let head = repo.head().unwrap().peel_to_commit().unwrap();
            repo.branch("side", &head, false).unwrap();
            commit_file(&repo, &dir, "a.txt", "ours\n", "master change");
        }
        crate::core::branch::checkout_branch(&dir, "side").unwrap();
        {
            let repo = git2::Repository::open(&dir).unwrap();
            commit_file(&repo, &dir, "a.txt", "theirs\n", "side change");
        }
        crate::core::branch::checkout_branch(&dir, "master").unwrap();
        let outcome = crate::core::merge::merge(&dir, "side", "normal").unwrap();
        assert!(matches!(
            outcome,
            crate::core::merge::MergeOutcome::Conflict { .. }
        ));

        let status = get_repo_status(&dir).unwrap();
        assert_eq!(status.conflicted, 1);
        assert!(!status.is_clean, "conflicted repo must not be clean");
        assert_eq!(
            status.modified, 0,
            "conflicted path must not also count as modified"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
