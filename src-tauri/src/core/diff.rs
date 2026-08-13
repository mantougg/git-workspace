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

/// Diff rendering options (Roadmap §9 diff settings).
#[derive(Debug, Clone, Default)]
pub struct DiffConfig {
    pub ignore_whitespace: bool,
    pub ignore_whitespace_eol: bool,
    pub ignore_case: bool,
}

/// Maximum lines rendered for a single "entire file added" hunk, to avoid
/// transferring MB-sized files over IPC and freezing the UI.
const MAX_FULL_FILE_LINES: usize = 2000;

/// Compute the diff between the HEAD tree and the working directory (with index).
///
/// For repositories with no commits (unborn HEAD), all files appear as "added".
/// Binary files are included but with an empty hunk list.
pub fn get_workdir_diff(repo_path: &Path) -> AppResult<Vec<FileDiff>> {
    get_workdir_diff_with_config(repo_path, &DiffConfig::default())
}

/// Like [`get_workdir_diff`], but with explicit diff rendering options.
pub fn get_workdir_diff_with_config(
    repo_path: &Path,
    config: &DiffConfig,
) -> AppResult<Vec<FileDiff>> {
    let repo = git2::Repository::open(repo_path)?;

    // Get HEAD tree (or None if no commits exist)
    let head_tree = match repo.head() {
        Ok(head) => Some(head.peel_to_tree()?),
        Err(_) => None,
    };

    let mut diff_opts = git2::DiffOptions::new();
    diff_opts.include_untracked(true);
    diff_opts.include_unmodified(false);
    diff_opts.ignore_whitespace(config.ignore_whitespace);
    diff_opts.ignore_whitespace_eol(config.ignore_whitespace_eol);
    // Note: `DiffOptions::ignore_case` only makes *filename* comparison
    // case-insensitive (GIT_DIFF_IGNORE_CASE). Content-level "Ignore Case"
    // (Roadmap §9) has no libgit2 equivalent, so it is applied as a
    // post-processing pass after the diff is computed.

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

    if config.ignore_case {
        apply_ignore_case_to_files(&mut files);
    }

    Ok(files)
}

/// Apply content-level "Ignore Case" (Roadmap §9) to the computed diffs.
///
/// libgit2 has no content case-insensitive diff option, so we post-process each
/// hunk: consecutive add/delete line pairs whose content differs only in case
/// are dropped, and surviving lines are re-numbered. Files left with no hunks
/// are removed entirely, matching how `ignore_whitespace` makes whitespace-only
/// deltas disappear.
fn apply_ignore_case_to_files(files: &mut Vec<FileDiff>) {
    files.retain_mut(|file| {
        let had_hunks = !file.hunks.is_empty();
        for hunk in file.hunks.iter_mut() {
            apply_ignore_case_to_hunk(hunk);
        }
        // Keep only hunks that still contain add/delete lines; a hunk left with
        // only context lines (case-only change fully ignored) is meaningless.
        file.hunks.retain(|h| {
            h.lines
                .iter()
                .any(|l| l.line_type == "add" || l.line_type == "delete")
        });
        // Files that already had no hunks (e.g. binary) stay untouched; files
        // whose diffs were fully filtered out disappear, matching how
        // `ignore_whitespace` removes whitespace-only deltas.
        !had_hunks || !file.hunks.is_empty()
    });
}

/// Turn add/delete line pairs that differ only in case into context lines.
///
/// Each matched delete becomes a context line (keeping its real `old_line` and
/// adopting the add line's `new_line`, with content from the "new" side) and
/// the paired add line is dropped. Line numbers stay aligned with true file
/// positions, which matters for line-number display and later hunk/line staging
/// (T-12). No re-numbering is needed because the surviving lines already carry
/// their correct positions from libgit2.
fn apply_ignore_case_to_hunk(hunk: &mut Hunk) {
    let n = hunk.lines.len();
    let mut drop_add = vec![false; n];
    // Consecutive delete lines awaiting an add counterpart (index, normalized).
    let mut pending: Vec<(usize, String)> = Vec::new();

    for i in 0..n {
        match hunk.lines[i].line_type.as_str() {
            "delete" => {
                let norm = hunk.lines[i].content.to_lowercase();
                pending.push((i, norm));
            }
            "add" => {
                let norm = hunk.lines[i].content.to_lowercase();
                if let Some(pos) = pending.iter().position(|(_, d)| *d == norm) {
                    let (di, _) = pending.remove(pos);
                    // Convert the matched delete into a context line: keep its
                    // real old_line, adopt the add line's new_line + content.
                    let new_line = hunk.lines[i].new_line;
                    let content = hunk.lines[i].content.clone();
                    hunk.lines[di].line_type = "context".to_string();
                    hunk.lines[di].content = content;
                    hunk.lines[di].new_line = new_line;
                    drop_add[i] = true;
                } else {
                    // Unmatched add ends the delete run: prior deletes are real.
                    pending.clear();
                }
            }
            // Context lines break the run.
            _ => pending.clear(),
        }
    }

    if drop_add.iter().any(|&d| d) {
        let mut idx = 0;
        hunk.lines.retain(|_| {
            let keep = !drop_add[idx];
            idx += 1;
            keep
        });
    }
}

/// Build a single "entire file added" hunk by reading the working-tree file.
/// Returns None for binary files or non-readable paths (e.g. directories).
fn full_add_hunk_for_file(repo_path: &Path, new_path: &str) -> Option<Hunk> {
    if new_path.is_empty() || new_path.ends_with('/') {
        return None;
    }
    let full = repo_path.join(new_path);
    let content = std::fs::read_to_string(&full).ok()?;

    let total_lines = content.lines().count();
    let mut lines = Vec::new();
    for (i, line) in content.lines().take(MAX_FULL_FILE_LINES).enumerate() {
        lines.push(DiffLine {
            line_type: "add".to_string(),
            content: line.to_string(),
            old_line: None,
            new_line: Some(i as u32 + 1),
        });
    }
    if total_lines > MAX_FULL_FILE_LINES {
        lines.push(DiffLine {
            line_type: "context".to_string(),
            content: format!(
                "... ({} more lines truncated)",
                total_lines - MAX_FULL_FILE_LINES
            ),
            old_line: None,
            new_line: None,
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Init a repo with a single committed file.
    fn init_repo_with_file(dir: &Path, name: &str, content: &str) -> git2::Repository {
        let repo = git2::Repository::init(dir).unwrap();
        std::fs::write(dir.join(name), content).unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new(name)).unwrap();
        index.write().unwrap();
        let tree_oid = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_oid).unwrap();
        let sig = git2::Signature::now("tester", "t@example.com").unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[]).unwrap();
        drop(tree);
        repo
    }

    fn tmpdir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "gw_diff_{}_{}",
            tag,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// `ignore_whitespace` must hide whitespace-only changes (Roadmap §9).
    #[test]
    fn ignore_whitespace_hides_whitespace_only_changes() {
        let dir = tmpdir("ws");
        {
            let repo = init_repo_with_file(&dir, "a.txt", "hello\nworld\n");
            drop(repo);
        }
        // Introduce trailing whitespace only.
        std::fs::write(dir.join("a.txt"), "hello  \nworld\n").unwrap();

        let default = get_workdir_diff_with_config(&dir, &DiffConfig::default()).unwrap();
        assert_eq!(default.len(), 1);
        assert!(
            !default[0].hunks.is_empty(),
            "default config must surface whitespace changes"
        );

        let ignored = get_workdir_diff_with_config(
            &dir,
            &DiffConfig {
                ignore_whitespace: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(
            ignored.is_empty() || ignored.iter().all(|f| f.hunks.is_empty()),
            "ignore_whitespace must hide whitespace-only changes"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `ignore_case` must hide case-only changes (Roadmap §9).
    #[test]
    fn ignore_case_hides_case_only_changes() {
        let dir = tmpdir("case");
        {
            let repo = init_repo_with_file(&dir, "a.txt", "HELLO\n");
            drop(repo);
        }
        std::fs::write(dir.join("a.txt"), "hello\n").unwrap();

        let default = get_workdir_diff_with_config(&dir, &DiffConfig::default()).unwrap();
        assert_eq!(default.len(), 1);
        assert!(!default[0].hunks.is_empty());

        let ignored = get_workdir_diff_with_config(
            &dir,
            &DiffConfig {
                ignore_case: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(
            ignored.is_empty() || ignored.iter().all(|f| f.hunks.is_empty()),
            "ignore_case must hide case-only changes"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A case-only change surrounded by context lines must fully disappear
    /// (no context-only hunk is left behind).
    #[test]
    fn ignore_case_removes_context_surrounded_case_change() {
        let dir = tmpdir("case_ctx");
        {
            let repo = init_repo_with_file(&dir, "a.txt", "line one\nHELLO\nline three\n");
            drop(repo);
        }
        std::fs::write(dir.join("a.txt"), "line one\nhello\nline three\n").unwrap();

        let ignored = get_workdir_diff_with_config(
            &dir,
            &DiffConfig {
                ignore_case: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(
            ignored.is_empty(),
            "case-only change with surrounding context must vanish, got: {:?}",
            ignored.iter().map(|f| f.new_path.clone()).collect::<Vec<_>>()
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Mixed real + case-only changes: the case-only pair becomes a context
    /// line and the real change keeps its true line numbers.
    #[test]
    fn ignore_case_converts_case_pair_to_context_and_keeps_true_lines() {
        let dir = tmpdir("case_mix");
        {
            let repo = init_repo_with_file(&dir, "a.txt", "A\nB\nC\n");
            drop(repo);
        }
        // Line 2 is a case-only change, line 3 is a real content change.
        std::fs::write(dir.join("a.txt"), "A\nb\nX\n").unwrap();

        let ignored = get_workdir_diff_with_config(
            &dir,
            &DiffConfig {
                ignore_case: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(ignored.len(), 1, "one file should remain");
        let hunk = &ignored[0].hunks[0];
        assert_eq!(hunk.lines.len(), 4, "A, b, -C, +X should remain");

        assert_eq!(hunk.lines[0].line_type, "context");
        assert_eq!(hunk.lines[0].content, "A");
        assert_eq!(hunk.lines[0].old_line, Some(1));
        assert_eq!(hunk.lines[0].new_line, Some(1));
        assert_eq!(hunk.lines[1].line_type, "context");
        assert_eq!(hunk.lines[1].content, "b");
        assert_eq!(hunk.lines[1].old_line, Some(2));
        assert_eq!(hunk.lines[1].new_line, Some(2));
        assert_eq!(hunk.lines[2].line_type, "delete");
        assert_eq!(hunk.lines[2].content, "C");
        assert_eq!(hunk.lines[2].old_line, Some(3));
        assert_eq!(hunk.lines[3].line_type, "add");
        assert_eq!(hunk.lines[3].content, "X");
        assert_eq!(hunk.lines[3].new_line, Some(3));

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A huge untracked file must be truncated at MAX_FULL_FILE_LINES with a
    /// truncation marker, so the IPC payload stays bounded (acceptance: no UI
    /// freeze from MB-sized diffs).
    #[test]
    fn oversized_file_diff_is_truncated() {
        let dir = tmpdir("big");
        {
            let repo = git2::Repository::init(&dir).unwrap();
            drop(repo);
        }
        let content: String = (0..3000).map(|i| format!("line {}\n", i)).collect();
        std::fs::write(dir.join("big.txt"), &content).unwrap();

        let files = get_workdir_diff_with_config(&dir, &DiffConfig::default()).unwrap();
        let f = files
            .iter()
            .find(|f| f.new_path == "big.txt")
            .expect("untracked big.txt must appear in the diff");
        assert_eq!(f.status, "untracked");
        assert_eq!(f.hunks.len(), 1);

        let lines = &f.hunks[0].lines;
        assert_eq!(lines.len(), MAX_FULL_FILE_LINES + 1);
        assert!(
            lines[lines.len() - 1].content.contains("truncated"),
            "truncation marker missing: {}",
            lines[lines.len() - 1].content
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
