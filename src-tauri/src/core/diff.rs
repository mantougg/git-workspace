use std::path::Path;

use serde::Serialize;

use crate::error::AppResult;

/// A complete diff for a single file, containing all hunks.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileDiff {
    pub old_path: String,
    pub new_path: String,
    /// "added", "deleted", "modified", "renamed", "copied", "untracked"
    pub status: String,
    pub hunks: Vec<Hunk>,
}

/// A hunk within a file diff - a contiguous region of changes.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Hunk {
    pub old_start: u32,
    pub old_lines: u32,
    pub new_start: u32,
    pub new_lines: u32,
    pub lines: Vec<DiffLine>,
}

/// A single line within a hunk.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffLine {
    /// "context", "add", "delete"
    pub line_type: String,
    pub content: String,
    pub old_line: Option<u32>,
    pub new_line: Option<u32>,
}

/// Compute the diff between the HEAD tree and the working directory (with index).
///
/// For repositories with no commits (unborn HEAD), all files appear as "added".
/// Binary files are included but with an empty hunk list.
pub fn get_workdir_diff(repo_path: &Path) -> AppResult<Vec<FileDiff>> {    let repo = git2::Repository::open(repo_path)?;

    // Get HEAD tree (or None if no commits exist)
    let head_tree = match repo.head() {
        Ok(head) => Some(head.peel_to_tree()?),
        Err(_) => None,
    };

    let mut diff_opts = git2::DiffOptions::new();
    diff_opts.include_untracked(true);
    diff_opts.include_unmodified(false);

    let diff =
        repo.diff_tree_to_workdir_with_index(head_tree.as_ref(), Some(&mut diff_opts))?;

    let mut files = Vec::new();

    for (i, delta) in diff.deltas().enumerate() {
        let old_path = delta
            .old_file()
            .path()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();

        let new_path = delta
            .new_file()
            .path()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();

        let status = match delta.status() {
            git2::Delta::Added => "added",
            git2::Delta::Deleted => "deleted",
            git2::Delta::Modified => "modified",
            git2::Delta::Renamed => "renamed",
            git2::Delta::Copied => "copied",
            git2::Delta::Untracked => "untracked",
            git2::Delta::Ignored => "ignored",
            _ => "modified",
        }
        .to_string();

        let mut hunks = extract_hunks(&diff, i);

        // Untracked/new files usually have no hunks (libgit2 does not diff
        // their content against anything). Fill them with the full file
        // content as added lines so the UI can show what would be added.
        if (status == "untracked" || status == "added") && hunks.is_empty() {
            if let Some(hunk) = full_add_hunk_for_file(repo_path, &new_path) {
                hunks.push(hunk);
            }
        }

        files.push(FileDiff {
            old_path,
            new_path,
            status,
            hunks,
        });
    }

    Ok(files)
}

/// Build a single "entire file added" hunk by reading the working-tree file.
/// Returns None for binary files or non-readable paths (e.g. directories).
fn full_add_hunk_for_file(repo_path: &Path, new_path: &str) -> Option<Hunk> {
    if new_path.is_empty() || new_path.ends_with('/') {
        return None;
    }
    let full = repo_path.join(new_path);
    let content = std::fs::read_to_string(&full).ok()?;

    let mut lines = Vec::new();
    for (i, line) in content.lines().enumerate() {
        lines.push(DiffLine {
            line_type: "add".to_string(),
            content: line.to_string(),
            old_line: None,
            new_line: Some(i as u32 + 1),
        });
    }

    Some(Hunk {
        old_start: 0,
        old_lines: 0,
        new_start: 1,
        new_lines: lines.len() as u32,
        lines,
    })
}

/// Extract hunks and lines from a specific diff delta using Patch API.
/// Returns an empty vector for binary files or patch failures.
fn extract_hunks(diff: &git2::Diff, delta_index: usize) -> Vec<Hunk> {
    let patch = match git2::Patch::from_diff(diff, delta_index) {
        Ok(Some(p)) => p,
        _ => return Vec::new(), // None = binary, Err = unsupported
    };

    let mut hunks = Vec::new();
    let num_hunks = patch.num_hunks();

    for hunk_idx in 0..num_hunks {
        let hunk = match patch.hunk(hunk_idx) {
            Ok((h, _)) => h,
            Err(_) => continue,
        };

        let mut lines = Vec::new();
        let num_lines = match patch.num_lines_in_hunk(hunk_idx) {
            Ok(n) => n,
            Err(_) => continue,
        };

        for line_idx in 0..num_lines {
            if let Ok(line) = patch.line_in_hunk(hunk_idx, line_idx) {
                let (line_type, content, old_line, new_line) = match line.origin() {
                    '+' => (
                        "add",
                        line.content(),
                        None,
                        line.new_lineno(),
                    ),
                    '-' => (
                        "delete",
                        line.content(),
                        line.old_lineno(),
                        None,
                    ),
                    ' ' => (
                        "context",
                        line.content(),
                        line.old_lineno(),
                        line.new_lineno(),
                    ),
                    _ => (
                        "context",
                        line.content(),
                        line.old_lineno(),
                        line.new_lineno(),
                    ),
                };

                let content_str = String::from_utf8_lossy(content)
                    .trim_end_matches('\n')
                    .trim_end_matches('\r')
                    .to_string();

                lines.push(DiffLine {
                    line_type: line_type.to_string(),
                    content: content_str,
                    old_line,
                    new_line,
                });
            }
        }

        hunks.push(Hunk {
            old_start: hunk.old_start(),
            old_lines: hunk.old_lines(),
            new_start: hunk.new_start(),
            new_lines: hunk.new_lines(),
            lines,
        });
    }

    hunks
}
