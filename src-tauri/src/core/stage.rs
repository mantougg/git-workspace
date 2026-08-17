//! Hunk- and line-level staging (T-12, Roadmap §9).
//!
//! Implements `git add -p` semantics via libgit2 patch/apply:
//! - **Stage** operates on the unstaged diff (index → workdir), applying the
//!   selected hunk/lines to the index.
//! - **Unstage** operates on the staged diff (HEAD tree → index), applying the
//!   *reversed* selection to the index.
//!
//! Whole-hunk (un)staging uses `ApplyOptions::hunk_callback` filtering, so no
//! patch text is reconstructed. Line-level (un)staging and all reverse
//! (unstage) operations rebuild a single-hunk patch buffer and re-parse it
//! with `git2::Diff::from_buffer`.
//!
//! Contract with the UI: hunk/line indices refer to the diffs returned by
//! `get_unstaged_diff` / `get_staged_diff` with **default** diff options
//! (staging is disabled in the UI while any Ignore option is active, because
//! those options renumber/filter hunks and lines).

use std::path::Path;

use crate::error::{AppError, AppResult};

/// Stage one hunk of a file's unstaged changes into the index.
pub fn stage_hunk(repo_path: &Path, file_path: &str, hunk_index: usize) -> AppResult<()> {
    let repo = git2::Repository::open(repo_path)?;
    let diff = workdir_file_diff(&repo, file_path)?;
    apply_hunk_forward(&repo, &diff, file_path, hunk_index)
}

/// Unstage one hunk of a file's staged changes (index returns towards HEAD).
pub fn unstage_hunk(repo_path: &Path, file_path: &str, hunk_index: usize) -> AppResult<()> {
    let repo = git2::Repository::open(repo_path)?;
    let diff = staged_file_diff(&repo, file_path)?;
    let patch = build_single_hunk_patch(&diff, hunk_index, None, true)?;
    apply_patch_text(&repo, &patch)
}

/// Stage only the selected lines (indices into the hunk's line list) of one
/// hunk of a file's unstaged changes.
pub fn stage_lines(
    repo_path: &Path,
    file_path: &str,
    hunk_index: usize,
    line_indices: &[u32],
) -> AppResult<()> {
    let repo = git2::Repository::open(repo_path)?;
    let diff = workdir_file_diff(&repo, file_path)?;
    let patch = build_single_hunk_patch(&diff, hunk_index, Some(line_indices), false)?;
    apply_patch_text(&repo, &patch)
}

/// Unstage only the selected lines of one hunk of a file's staged changes.
pub fn unstage_lines(
    repo_path: &Path,
    file_path: &str,
    hunk_index: usize,
    line_indices: &[u32],
) -> AppResult<()> {
    let repo = git2::Repository::open(repo_path)?;
    let diff = staged_file_diff(&repo, file_path)?;
    let patch = build_single_hunk_patch(&diff, hunk_index, Some(line_indices), true)?;
    apply_patch_text(&repo, &patch)
}

/// Unstaged diff (index → workdir) restricted to a single file.
fn workdir_file_diff<'r>(repo: &'r git2::Repository, file_path: &str) -> AppResult<git2::Diff<'r>> {
    let mut opts = git2::DiffOptions::new();
    opts.pathspec(file_path);
    Ok(repo.diff_index_to_workdir(None, Some(&mut opts))?)
}

/// Staged diff (HEAD tree → index) restricted to a single file.
fn staged_file_diff<'r>(repo: &'r git2::Repository, file_path: &str) -> AppResult<git2::Diff<'r>> {
    let head_tree = crate::core::diff::head_or_empty_tree(repo)?;
    let mut opts = git2::DiffOptions::new();
    opts.pathspec(file_path);
    Ok(repo.diff_tree_to_index(Some(&head_tree), None, Some(&mut opts))?)
}

/// Apply exactly one hunk of a single-file diff to the index (forward
/// direction), using libgit2's apply-time hunk filter.
fn apply_hunk_forward(
    repo: &git2::Repository,
    diff: &git2::Diff,
    file_path: &str,
    hunk_index: usize,
) -> AppResult<()> {
    let num_hunks = count_hunks(diff)?;
    if num_hunks == 0 {
        return Err(AppError::Other(format!(
            "{file_path}: 没有可暂存的 hunk（未跟踪或二进制文件请整体暂存）"
        )));
    }
    if hunk_index >= num_hunks {
        return Err(AppError::Other(format!(
            "{file_path}: hunk 索引 {hunk_index} 越界（共 {num_hunks} 个）"
        )));
    }

    let mut seen = 0usize;
    let mut apply_opts = git2::ApplyOptions::new();
    apply_opts.hunk_callback(move |_| {
        let idx = seen;
        seen += 1;
        idx == hunk_index
    });
    repo.apply(diff, git2::ApplyLocation::Index, Some(&mut apply_opts))?;
    Ok(())
}

/// Number of real patch hunks in a single-file diff (0 for binary/untracked/
/// absent files — untracked files are not in an index→workdir diff at all).
fn count_hunks(diff: &git2::Diff) -> AppResult<usize> {
    if diff.deltas().len() == 0 {
        return Ok(0);
    }
    match git2::Patch::from_diff(diff, 0)? {
        Some(patch) => Ok(patch.num_hunks()),
        None => Ok(0),
    }
}

/// Apply a reconstructed patch buffer to the index.
fn apply_patch_text(repo: &git2::Repository, patch: &[u8]) -> AppResult<()> {
    let diff = git2::Diff::from_buffer(patch)?;
    repo.apply(&diff, git2::ApplyLocation::Index, None)?;
    Ok(())
}

/// One parsed patch line: change kind plus content (without the trailing
/// newline). `no_newline` marks libgit2's EOF-without-newline lines (origins
/// `=` / `>` / `<`), whose `\ No newline at end of file` marker is re-emitted
/// at serialization time.
#[derive(Debug, Clone, PartialEq, Eq)]
struct RawLine {
    kind: RawKind,
    content: Vec<u8>,
    no_newline: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RawKind {
    Context,
    Add,
    Del,
}

impl RawLine {
    fn is_change(&self) -> bool {
        self.kind != RawKind::Context
    }
}

/// Build a single-hunk unified-diff patch for the (single-file) diff.
///
/// - `selected`: when given, only these line indices keep their change kind;
///   unselected deletions become context, unselected additions are dropped.
/// - `reverse`: swap old/new sides (used for unstage, where the index must
///   move back towards HEAD).
///
/// Hunk header counts are always recomputed from the surviving lines, which
/// covers both filtering and reversal. `\ No newline at end of file` markers
/// are preserved for lines whose content lacks a trailing newline.
fn build_single_hunk_patch(
    diff: &git2::Diff,
    hunk_index: usize,
    selected: Option<&[u32]>,
    reverse: bool,
) -> AppResult<Vec<u8>> {
    let delta = diff
        .deltas()
        .next()
        .ok_or_else(|| AppError::Other("文件不在 diff 中".to_string()))?;
    let patch = git2::Patch::from_diff(diff, 0)?
        .ok_or_else(|| AppError::Other("二进制文件不支持 hunk/行级暂存".to_string()))?;
    if hunk_index >= patch.num_hunks() {
        return Err(AppError::Other(format!(
            "hunk 索引 {hunk_index} 越界（共 {} 个）",
            patch.num_hunks()
        )));
    }
    let (hunk, _) = patch.hunk(hunk_index)?;

    let num_lines = patch.num_lines_in_hunk(hunk_index)?;
    let mut lines: Vec<RawLine> = Vec::with_capacity(num_lines);
    for i in 0..num_lines {
        let line = patch.line_in_hunk(hunk_index, i)?;
        let content = line.content();
        // Strip the trailing newline; its absence marks "no newline at EOF".
        let (body, had_newline) = match content.strip_suffix(b"\n") {
            Some(b) => (b.to_vec(), true),
            None => (content.to_vec(), false),
        };
        let kind = match line.origin() {
            '+' => RawKind::Add,
            '-' => RawKind::Del,
            ' ' => RawKind::Context,
            // '>' / '<' / '=' are the `\ No newline at end of file` marker
            // lines libgit2 emits *after* an EOF-no-newline content line; the
            // marker is re-emitted at serialization time from `no_newline`.
            _ => continue,
        };
        lines.push(RawLine {
            kind,
            content: body,
            no_newline: !had_newline,
        });
    }

    // Reverse *before* filtering: for unstage, the patch preimage is the
    // index (the diff's new side), so selection semantics must be evaluated
    // on the reversed lines. (Unselected deletions→context / additions→drop
    // is correct only in the apply direction.)
    if reverse {
        lines = lines
            .into_iter()
            .map(|mut l| {
                l.kind = match l.kind {
                    RawKind::Add => RawKind::Del,
                    RawKind::Del => RawKind::Add,
                    RawKind::Context => RawKind::Context,
                };
                l
            })
            .collect();
    }

    // Line filter (line-level staging): indices into the hunk's line list.
    if let Some(sel) = selected {
        for &i in sel {
            if i as usize >= lines.len() {
                return Err(AppError::Other(format!(
                    "行索引 {i} 越界（hunk 共 {} 行）",
                    lines.len()
                )));
            }
        }
        let keep = |i: usize| sel.contains(&(i as u32));
        let mut filtered: Vec<RawLine> = Vec::with_capacity(lines.len());
        for (i, line) in lines.into_iter().enumerate() {
            let kind = match line.kind {
                RawKind::Context => RawKind::Context,
                RawKind::Del => {
                    if keep(i) {
                        RawKind::Del
                    } else {
                        RawKind::Context
                    }
                }
                RawKind::Add => {
                    if keep(i) {
                        RawKind::Add
                    } else {
                        // Unselected additions are dropped entirely.
                        continue;
                    }
                }
            };
            filtered.push(RawLine { kind, ..line });
        }
        lines = filtered;
    }

    if !lines.iter().any(RawLine::is_change) {
        return Err(AppError::Other(
            "选中的行不包含任何变更（无 +/- 行）".to_string(),
        ));
    }

    let old_lines = lines.iter().filter(|l| l.kind != RawKind::Add).count();
    let new_lines = lines.iter().filter(|l| l.kind != RawKind::Del).count();
    let (old_start, new_start) = if reverse {
        (hunk.new_start(), hunk.old_start())
    } else {
        (hunk.old_start(), hunk.new_start())
    };

    // File header. old/new sides swap when reversing; a staged-new file
    // (Added) reversed becomes a deletion from the index, and vice versa.
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
    let status = delta.status();
    let (old_mode, new_mode) = (
        i32::from(delta.old_file().mode()),
        i32::from(delta.new_file().mode()),
    );

    let (hdr_old, hdr_new, hdr_status) = if reverse {
        (new_path.clone(), old_path.clone(), reverse_status(status))
    } else {
        (old_path.clone(), new_path.clone(), status)
    };

    let mut out: Vec<u8> = Vec::new();
    out.extend_from_slice(format!("diff --git a/{hdr_old} b/{hdr_new}\n").as_bytes());
    match hdr_status {
        git2::Delta::Added => {
            // Mode of the file being created: forward Added => new side;
            // reversed Deleted => original old side.
            let mode = if reverse { old_mode } else { new_mode };
            out.extend_from_slice(format!("new file mode {mode:o}\n").as_bytes());
        }
        git2::Delta::Deleted => {
            // Mode of the file being removed: forward Deleted => old side;
            // reversed Added => original new side.
            let mode = if reverse { new_mode } else { old_mode };
            out.extend_from_slice(format!("deleted file mode {mode:o}\n").as_bytes());
        }
        _ => {}
    }
    let minus = if hdr_status == git2::Delta::Added {
        "/dev/null".to_string()
    } else {
        format!("a/{hdr_old}")
    };
    let plus = if hdr_status == git2::Delta::Deleted {
        "/dev/null".to_string()
    } else {
        format!("b/{hdr_new}")
    };
    out.extend_from_slice(format!("--- {minus}\n+++ {plus}\n").as_bytes());
    out.extend_from_slice(
        format!("@@ -{old_start},{old_lines} +{new_start},{new_lines} @@\n").as_bytes(),
    );

    for line in &lines {
        let origin = match line.kind {
            RawKind::Context => b' ',
            RawKind::Add => b'+',
            RawKind::Del => b'-',
        };
        out.push(origin);
        out.extend_from_slice(&line.content);
        out.push(b'\n');
        if line.no_newline {
            out.extend_from_slice(b"\\ No newline at end of file\n");
        }
    }

    Ok(out)
}

/// Swap the direction of a delta status for reverse (unstage) patches.
fn reverse_status(status: git2::Delta) -> git2::Delta {
    match status {
        git2::Delta::Added => git2::Delta::Deleted,
        git2::Delta::Deleted => git2::Delta::Added,
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "gw_stage_{}_{}",
            tag,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Init a repo with one committed file of `lines` "line N" entries.
    fn init_repo(dir: &Path, name: &str, content: &str) -> git2::Repository {
        let repo = git2::Repository::init(dir).unwrap();
        std::fs::write(dir.join(name), content).unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new(name)).unwrap();
        index.write().unwrap();
        let tree_oid = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_oid).unwrap();
        let sig = git2::Signature::now("tester", "t@example.com").unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[])
            .unwrap();
        drop(tree);
        repo
    }

    /// Current index content of a file (None if absent from the index).
    fn index_content(repo: &git2::Repository, path: &str) -> Option<String> {
        let index = repo.index().unwrap();
        let entry = index.get_path(Path::new(path), 0)?;
        let blob = repo.find_blob(entry.id).ok()?;
        Some(String::from_utf8(blob.content().to_vec()).unwrap())
    }

    fn lines_content(n: usize) -> String {
        (1..=n).map(|i| format!("line {i}\n")).collect()
    }

    /// Staging hunk 0 of a two-hunk modification puts only that hunk's change
    /// into the index; the rest stays unstaged (`git add -p` semantics).
    #[test]
    fn stage_hunk_applies_only_that_hunk_to_index() {
        let dir = tmpdir("hunk");
        // Two changes far apart => two hunks (context = 3 lines).
        let mut content = lines_content(30);
        let repo = init_repo(&dir, "a.txt", &content);
        drop(repo);
        content = content.replacen("line 2\n", "line 2 changed\n", 1);
        content = content.replacen("line 28\n", "line 28 changed\n", 1);
        std::fs::write(dir.join("a.txt"), &content).unwrap();

        stage_hunk(&dir, "a.txt", 0).unwrap();

        let repo = git2::Repository::open(&dir).unwrap();
        let staged = index_content(&repo, "a.txt").unwrap();
        assert!(staged.contains("line 2 changed"), "hunk 0 must be staged");
        assert!(
            !staged.contains("line 28 changed"),
            "hunk 1 must stay unstaged"
        );

        // The remaining unstaged diff has exactly one hunk.
        let unstaged = crate::core::diff::get_unstaged_diff_with_config(
            &dir,
            &crate::core::diff::DiffConfig::default(),
        )
        .unwrap();
        assert_eq!(unstaged.len(), 1);
        assert_eq!(unstaged[0].hunks.len(), 1);
        assert!(
            unstaged[0]
                .hunks
                .iter()
                .flat_map(|h| h.lines.iter())
                .any(|l| l.line_type == "add" && l.content == "line 28 changed")
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Unstaging a hunk moves the index back towards HEAD for that hunk only.
    #[test]
    fn unstage_hunk_reverses_only_that_hunk_in_index() {
        let dir = tmpdir("unhunk");
        let mut content = lines_content(30);
        let repo = init_repo(&dir, "a.txt", &content);
        drop(repo);
        content = content.replacen("line 2\n", "line 2 changed\n", 1);
        content = content.replacen("line 28\n", "line 28 changed\n", 1);
        std::fs::write(dir.join("a.txt"), &content).unwrap();

        // Stage everything, then unstage the first hunk.
        {
            let repo = git2::Repository::open(&dir).unwrap();
            let mut index = repo.index().unwrap();
            index.add_path(Path::new("a.txt")).unwrap();
            index.write().unwrap();
        }
        unstage_hunk(&dir, "a.txt", 0).unwrap();

        let repo = git2::Repository::open(&dir).unwrap();
        let staged = index_content(&repo, "a.txt").unwrap();
        assert!(
            !staged.contains("line 2 changed"),
            "hunk 0 must be unstaged"
        );
        assert!(staged.contains("line 28 changed"), "hunk 1 stays staged");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Line-level staging: within one hunk, only the selected add/delete lines
    /// reach the index; the result must match `git diff --cached` semantics
    /// (index content equality is the semantic check).
    #[test]
    fn stage_lines_stages_only_selected_lines() {
        let dir = tmpdir("lines");
        let repo = init_repo(&dir, "a.txt", "one\ntwo\nthree\nfour\nfive\n");
        drop(repo);
        std::fs::write(dir.join("a.txt"), "one\nTWO\nthree\nFOUR\nfive\n")
            .unwrap();

        // Single hunk; line list: " one","-two","+TWO"," three","-four","+FOUR"," five"
        // Stage only the first change (delete idx 1 + add idx 2).
        stage_lines(&dir, "a.txt", 0, &[1, 2]).unwrap();

        let repo = git2::Repository::open(&dir).unwrap();
        let staged = index_content(&repo, "a.txt").unwrap();
        assert_eq!(staged, "one\nTWO\nthree\nfour\nfive\n");

        // Remaining unstaged change: only the FOUR line.
        let unstaged = crate::core::diff::get_unstaged_diff_with_config(
            &dir,
            &crate::core::diff::DiffConfig::default(),
        )
        .unwrap();
        let adds: Vec<_> = unstaged[0]
            .hunks
            .iter()
            .flat_map(|h| h.lines.iter())
            .filter(|l| l.line_type == "add")
            .collect();
        assert_eq!(adds.len(), 1);
        assert_eq!(adds[0].content, "FOUR");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Staging a single delete line (no add counterpart) works and the index
    /// loses exactly that line.
    #[test]
    fn stage_lines_single_delete() {
        let dir = tmpdir("del");
        let repo = init_repo(&dir, "a.txt", "one\ntwo\nthree\n");
        drop(repo);
        std::fs::write(dir.join("a.txt"), "one\nthree\n").unwrap();

        // Hunk lines: " one","-two"," three" -> stage only the delete (idx 1).
        stage_lines(&dir, "a.txt", 0, &[1]).unwrap();

        let repo = git2::Repository::open(&dir).unwrap();
        assert_eq!(index_content(&repo, "a.txt").unwrap(), "one\nthree\n");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Line-level unstaging: only the selected staged change returns to
    /// unstaged.
    #[test]
    fn unstage_lines_reverses_only_selected_lines() {
        let dir = tmpdir("unlines");
        let repo = init_repo(&dir, "a.txt", "one\ntwo\nthree\nfour\nfive\n");
        drop(repo);
        std::fs::write(dir.join("a.txt"), "one\nTWO\nthree\nFOUR\nfive\n")
            .unwrap();
        {
            let repo = git2::Repository::open(&dir).unwrap();
            let mut index = repo.index().unwrap();
            index.add_path(Path::new("a.txt")).unwrap();
            index.write().unwrap();
        }

        // Staged hunk lines: " one","-two","+TWO"," three","-four","+FOUR"," five"
        // Unstage only the first change.
        unstage_lines(&dir, "a.txt", 0, &[1, 2]).unwrap();

        let repo = git2::Repository::open(&dir).unwrap();
        let staged = index_content(&repo, "a.txt").unwrap();
        assert_eq!(staged, "one\ntwo\nthree\nFOUR\nfive\n");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Files without a trailing newline keep their `\ No newline at end of
    /// file` semantics through line staging.
    #[test]
    fn stage_lines_preserves_no_newline_at_eof() {
        let dir = tmpdir("nonewline");
        let repo = init_repo(&dir, "a.txt", "one\ntwo"); // no trailing newline
        drop(repo);
        std::fs::write(dir.join("a.txt"), "one\nTWO").unwrap();

        // Hunk lines: " one","-two","+TWO" -> stage both change lines.
        stage_lines(&dir, "a.txt", 0, &[1, 2]).unwrap();

        let repo = git2::Repository::open(&dir).unwrap();
        assert_eq!(index_content(&repo, "a.txt").unwrap(), "one\nTWO");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Untracked files have no patch hunks: hunk staging must fail with a
    /// clear error (the UI offers whole-file staging instead).
    #[test]
    fn stage_hunk_rejects_untracked_file() {
        let dir = tmpdir("untracked");
        let repo = git2::Repository::init(&dir).unwrap();
        drop(repo);
        std::fs::write(dir.join("new.txt"), "hello\n").unwrap();

        let err = stage_hunk(&dir, "new.txt", 0).unwrap_err();
        assert!(
            err.to_string().contains("没有可暂存的 hunk"),
            "unexpected error: {err}"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Out-of-range hunk indices are rejected, not silently ignored.
    #[test]
    fn stage_hunk_rejects_out_of_range_index() {
        let dir = tmpdir("oob");
        let repo = init_repo(&dir, "a.txt", "one\ntwo\n");
        drop(repo);
        std::fs::write(dir.join("a.txt"), "one\nTWO\n").unwrap();

        let err = stage_hunk(&dir, "a.txt", 5).unwrap_err();
        assert!(err.to_string().contains("越界"), "unexpected error: {err}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Staging then unstaging the same lines returns the index to HEAD.
    #[test]
    fn stage_then_unstage_lines_roundtrips_to_head() {
        let dir = tmpdir("roundtrip");
        let repo = init_repo(&dir, "a.txt", "one\ntwo\nthree\n");
        drop(repo);
        std::fs::write(dir.join("a.txt"), "one\nTWO\nthree\n").unwrap();

        stage_lines(&dir, "a.txt", 0, &[1, 2]).unwrap();
        unstage_lines(&dir, "a.txt", 0, &[1, 2]).unwrap();

        let repo = git2::Repository::open(&dir).unwrap();
        assert_eq!(index_content(&repo, "a.txt").unwrap(), "one\ntwo\nthree\n");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
