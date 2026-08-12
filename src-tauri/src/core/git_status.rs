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
fn is_runtime_path(path: &str) -> bool {
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

    for entry in statuses.iter() {
        let s = entry.status();

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
