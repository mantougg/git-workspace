//! GF-17: structured previews for destructive single-repo operations
//! (reset / merge).
//!
//! Roadmap §46 requires a Dangerous operation to state Repository / Branch /
//! Files / Potential Data Loss *before* it runs; until GF-17 the UI only had
//! prose confirms ("未提交的更改将丢失") with no facts behind them. This module
//! computes those facts from **local** git data only, extending the GF-15
//! `batch_dry_run` pattern to single-repo destructive ops.
//!
//! Read-only by construction:
//! * no CLI git processes, no network;
//! * only read APIs (`revwalk`, `diff_tree_to_tree`, `statuses`,
//!   `merge_analysis`, in-memory `merge_commits`);
//! * no ref / index / worktree writes and no objects written to the ODB —
//!   the unit tests assert HEAD + branch oids are unchanged across a call.
//!
//! Callers show the result *before* executing the operation; nothing here
//! mutates the repository, and no “preview-then-edit-strategy” flow exists.

use std::path::Path;

use serde::Serialize;

use crate::core::graph;
use crate::core::history::conflict_paths;
use crate::error::{AppError, AppResult};

/// Display cap for preview commit lists (IPC payload budget, global
/// constraint §2: large payloads must be bounded). The authoritative total
/// lives in the sibling `*_count` field; the vector is a newest-first
/// sample.
pub const PREVIEW_COMMIT_LIMIT: usize = 50;

/// Display cap for preview file lists (same rationale as above).
pub const PREVIEW_FILE_LIMIT: usize = 200;

/// One commit in a preview list.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewCommit {
    pub oid: String,
    pub short_oid: String,
    /// First line of the commit message.
    pub summary: String,
    pub author: String,
    /// Formatted like graph-view commit times.
    pub time: String,
}

/// One tracked change a hard reset would discard from the index (and, for
/// hard mode, from the worktree too).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewFileChange {
    pub path: String,
    /// "conflicted" | "staged" | "unstaged" | "staged+unstaged".
    pub status: String,
}

/// What a `reset` will do (GF-17).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResetPreview {
    pub repo_path: String,
    /// Current branch shorthand; "detached HEAD" when HEAD is detached.
    pub branch: String,
    pub detached: bool,
    pub head_oid: String,
    /// Resolved target oid the reset will point HEAD at.
    pub target_oid: String,
    pub target_summary: String,
    /// "soft" | "mixed" | "hard".
    pub mode: String,
    /// Commits the branch will drop (`target..HEAD`), newest first, capped at
    /// [`PREVIEW_COMMIT_LIMIT`].
    pub discarded_commits: Vec<PreviewCommit>,
    /// Authoritative count of discarded commits (the list may be capped).
    pub discarded_count: usize,
    /// Tracked uncommitted changes the reset drops (index for every mode,
    /// worktree too for hard). Untracked files survive any reset (git
    /// semantics) and are excluded.
    pub lost_file_changes: Vec<PreviewFileChange>,
    /// Authoritative count of lost changes (the list may be capped).
    pub lost_changes_count: usize,
    /// hard + uncommitted changes: content no reflog entry can restore —
    /// the UI marks these unrecoverable (Roadmap §46 “Potential Data Loss”).
    pub unrecoverable: bool,
}

/// What a `merge` will do (GF-17).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MergePreview {
    pub repo_path: String,
    /// Current branch (merge target).
    pub branch: String,
    /// Source ref being merged in.
    pub source: String,
    pub head_oid: String,
    pub source_oid: String,
    /// "up_to_date" | "fast_forward" | "merge" (a forced `no-ff` on a
    /// fast-forwardable pair reports "merge": a merge commit is created).
    pub kind: String,
    /// "normal" | "no-ff" | "squash".
    pub mode: String,
    /// Commits the merge brings in (reachable from `source`, not from HEAD),
    /// newest first, capped at [`PREVIEW_COMMIT_LIMIT`].
    pub incoming_commits: Vec<PreviewCommit>,
    /// Authoritative count of incoming commits (the list may be capped).
    pub incoming_count: usize,
    /// Files the merge can change in the worktree: their side's changes
    /// against the merge base. Both-sides overlap (conflict candidates) is a
    /// subset of this set.
    pub affected_files: Vec<String>,
    /// Authoritative count of affected files (the list may be capped).
    pub affected_files_count: usize,
    /// Predicted via an in-memory `merge_commits` (same call shape as GF-15's
    /// dry-run); always false for up_to_date / fast_forward kinds.
    pub conflict_predicted: bool,
    /// Conflicted file paths when `conflict_predicted`.
    pub conflict_files: Vec<String>,
    /// Uncommitted tracked changes: `merge` itself would refuse (PAF-10
    /// dirty-worktree guard), so the UI warns instead of letting the user
    /// hit the executor's error.
    pub dirty_blocked: bool,
    /// Dirty tracked files (capped at [`PREVIEW_FILE_LIMIT`]).
    pub dirty_files: Vec<String>,
    /// Authoritative count of dirty files.
    pub dirty_files_count: usize,
}

/// Display label for a dirty index/worktree entry. A conflicted entry always
/// wins (it is the most severe state).
fn dirty_status_label(status: git2::Status) -> &'static str {
    if status.contains(git2::Status::CONFLICTED) {
        return "conflicted";
    }
    let staged = status.intersects(
        git2::Status::INDEX_NEW
            | git2::Status::INDEX_MODIFIED
            | git2::Status::INDEX_DELETED
            | git2::Status::INDEX_RENAMED
            | git2::Status::INDEX_TYPECHANGE,
    );
    let unstaged = status.intersects(
        git2::Status::WT_MODIFIED
            | git2::Status::WT_DELETED
            | git2::Status::WT_TYPECHANGE
            | git2::Status::WT_RENAMED,
    );
    match (staged, unstaged) {
        (true, true) => "staged+unstaged",
        (true, false) => "staged",
        (false, true) => "unstaged",
        (false, false) => "other",
    }
}

/// True for entries that would be swallowed by a reset / block a merge —
/// same filter as `history::ensure_clean_worktree` (untracked new files,
/// WT_NEW, are intentionally excluded: every git reset mode keeps them).
fn is_dirty_entry(status: git2::Status) -> bool {
    status != git2::Status::CURRENT && !status.contains(git2::Status::WT_NEW)
}

/// Collect the repo's dirty tracked files (path + state label), capped.
fn dirty_entries(repo: &git2::Repository) -> AppResult<(Vec<PreviewFileChange>, usize)> {
    let mut out: Vec<PreviewFileChange> = Vec::new();
    for entry in repo.statuses(None)?.iter() {
        let status = entry.status();
        if !is_dirty_entry(status) {
            continue;
        }
        if let Some(path) = entry.path() {
            out.push(PreviewFileChange {
                path: path.to_string(),
                status: dirty_status_label(status).to_string(),
            });
        }
    }
    let count = out.len();
    out.truncate(PREVIEW_FILE_LIMIT);
    Ok((out, count))
}

fn preview_commit(commit: &git2::Commit) -> PreviewCommit {
    let oid = commit.id().to_string();
    let short_oid = oid[..7.min(oid.len())].to_string();
    let summary = commit
        .message()
        .unwrap_or("")
        .lines()
        .next()
        .unwrap_or("")
        .trim()
        .to_string();
    let author = commit.author();
    PreviewCommit {
        oid,
        short_oid,
        summary,
        author: author.name().unwrap_or("").to_string(),
        time: graph::format_commit_time(commit.time().seconds(), commit.time().offset_minutes()),
    }
}

/// Collect `from..to` commits (reachable from `to`, hidden at `from`),
/// newest first, capped — plus the uncapped total.
fn commits_between(
    repo: &git2::Repository,
    from: git2::Oid,
    to: git2::Oid,
) -> AppResult<(Vec<PreviewCommit>, usize)> {
    let mut walk = repo.revwalk()?;
    walk.push(to)?;
    walk.hide(from)?;
    let oids: Vec<git2::Oid> = walk.flatten().collect();
    let total = oids.len();
    let list = oids
        .iter()
        .take(PREVIEW_COMMIT_LIMIT)
        .filter_map(|oid| repo.find_commit(*oid).ok())
        .map(|c| preview_commit(&c))
        .collect();
    Ok((list, total))
}

/// Preview a `reset` (GF-17): which commits the branch drops, which tracked
/// changes are discarded, and whether anything is unrecoverable.
///
/// `target` `None` means HEAD (same default as `history::reset_to`).
pub fn preview_reset(repo_path: &Path, target: Option<&str>, mode: &str) -> AppResult<ResetPreview> {
    if !matches!(mode, "soft" | "mixed" | "hard") {
        return Err(AppError::Other(format!(
            "invalid reset mode '{}' (soft | mixed | hard)",
            mode
        )));
    }

    let repo = git2::Repository::open(repo_path)?;
    let head = repo
        .head()
        .map_err(|_| AppError::Other("HEAD 未指向提交（仓库尚无提交）".to_string()))?;
    let head_oid = head
        .target()
        .ok_or_else(|| AppError::Other("HEAD 未指向提交（仓库尚无提交）".to_string()))?;
    let detached = repo.head_detached().unwrap_or(false);
    let branch = if detached {
        "detached HEAD".to_string()
    } else {
        head.shorthand().unwrap_or("HEAD").to_string()
    };

    let spec = target.unwrap_or("HEAD");
    let target_commit = repo
        .revparse_single(spec)
        .and_then(|o| o.peel_to_commit())
        .map_err(|_| AppError::NotFound(format!("revision '{}' not found", spec)))?;

    let (discarded_commits, discarded_count) = commits_between(&repo, target_commit.id(), head_oid)?;
    let (lost_file_changes, lost_changes_count) = dirty_entries(&repo)?;

    Ok(ResetPreview {
        repo_path: repo_path.display().to_string(),
        branch,
        detached,
        head_oid: head_oid.to_string(),
        target_oid: target_commit.id().to_string(),
        target_summary: target_commit.summary().unwrap_or("").to_string(),
        mode: mode.to_string(),
        discarded_commits,
        discarded_count,
        lost_file_changes,
        lost_changes_count,
        unrecoverable: mode == "hard" && lost_changes_count > 0,
    })
}

/// Preview a `merge` (GF-17): which commits come in, which files the merge
/// can touch, and whether an in-memory merge predicts conflicts.
///
/// Mirrors `core::merge::merge`'s preconditions (a preview of a merge the
/// executor would refuse only produces a confusing dialog): an in-progress
/// merge / rebase is refused, while a dirty worktree is *reported*
/// (`dirty_blocked`) so the UI can warn instead of erroring.
pub fn preview_merge(repo_path: &Path, source: &str, mode: &str) -> AppResult<MergePreview> {
    if !matches!(mode, "normal" | "no-ff" | "squash") {
        return Err(AppError::Other(format!(
            "invalid merge mode '{}' (normal | no-ff | squash)",
            mode
        )));
    }
    if crate::core::merge::merge_in_progress(repo_path)? {
        return Err(AppError::Conflict(
            "已有 merge 进行中（请先 resolve + continue，或 abort 后再试）".into(),
        ));
    }
    if crate::core::rebase::get_rebase_state(repo_path)?.is_some() {
        return Err(AppError::Conflict(
            "rebase 进行中，请先完成或中止该 rebase 再 merge".into(),
        ));
    }

    let repo = git2::Repository::open(repo_path)?;
    let head = repo
        .head()
        .map_err(|_| AppError::Other("HEAD 未指向提交（仓库尚无提交）".to_string()))?;
    let head_oid = head
        .target()
        .ok_or_else(|| AppError::Other("HEAD 未指向提交（仓库尚无提交）".to_string()))?;
    let branch = head.shorthand().unwrap_or("HEAD").to_string();
    let head_commit = repo.find_commit(head_oid)?;

    let their_commit = repo
        .revparse_single(source)
        .and_then(|o| o.peel_to_commit())
        .map_err(|_| AppError::NotFound(format!("branch '{}' not found", source)))?;
    let their_annotated = repo.find_annotated_commit(their_commit.id())?;

    let (analysis, _preference) = repo.merge_analysis(&[&their_annotated])?;
    let up_to_date = analysis.is_up_to_date();
    let kind = if up_to_date {
        "up_to_date"
    } else if analysis.is_fast_forward() && mode == "normal" {
        "fast_forward"
    } else {
        // A forced --no-ff on a fast-forwardable pair still creates a merge
        // commit, so the "what happens" is a merge, not a fast-forward.
        "merge"
    };

    let (incoming_commits, incoming_count) = commits_between(&repo, head_oid, their_commit.id())?;

    // Merge base (an error means unrelated histories: diff against the
    // empty tree).
    let base_tree = match repo.merge_base(head_oid, their_commit.id()) {
        Ok(base_oid) => Some(repo.find_commit(base_oid)?.tree()?),
        Err(_) => None,
    };
    let their_tree = their_commit.tree()?;

    // Files the merge can change in the worktree: their side's changes
    // against the merge base (both-sides overlap is a subset, so conflict
    // candidates are covered). Nothing to do for an up-to-date merge.
    let mut affected_files: Vec<String> = Vec::new();
    if !up_to_date {
        let diff = repo.diff_tree_to_tree(base_tree.as_ref(), Some(&their_tree), None)?;
        for delta in diff.deltas() {
            let path = delta.new_file().path().or_else(|| delta.old_file().path());
            if let Some(path) = path {
                let path = path.to_string_lossy().to_string();
                if !affected_files.contains(&path) {
                    affected_files.push(path);
                }
            }
        }
    }
    let affected_files_count = affected_files.len();
    affected_files.truncate(PREVIEW_FILE_LIMIT);

    // Conflict prediction: in-memory merge (libgit2 `merge_commits` computes
    // the merge index without touching the worktree or the on-disk index —
    // same call shape as GF-15's dry-run diverged classification).
    let (conflict_predicted, conflict_files) = if kind == "merge" {
        let merge_index = repo.merge_commits(&head_commit, &their_commit, None)?;
        if merge_index.has_conflicts() {
            (true, conflict_paths(&merge_index)?)
        } else {
            (false, Vec::new())
        }
    } else {
        (false, Vec::new())
    };

    let (dirty_changes, dirty_files_count) = dirty_entries(&repo)?;
    let dirty_files: Vec<String> = dirty_changes.into_iter().map(|d| d.path).collect();

    Ok(MergePreview {
        repo_path: repo_path.display().to_string(),
        branch,
        source: source.to_string(),
        head_oid: head_oid.to_string(),
        source_oid: their_commit.id().to_string(),
        kind: kind.to_string(),
        mode: mode.to_string(),
        incoming_commits,
        incoming_count,
        affected_files,
        affected_files_count,
        conflict_predicted,
        conflict_files,
        dirty_blocked: dirty_files_count > 0,
        dirty_files,
        dirty_files_count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn tmpdir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "gw_preview_{}_{}",
            tag,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn git(dir: &Path, args: &[&str]) {
        let out = Command::new("git").current_dir(dir).args(args).output().unwrap();
        assert!(
            out.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// Snapshot of the observable repo state a preview must not change:
    /// HEAD oid, branch tip oid, dirty-entry count, and the absence of
    /// in-progress markers. Any mutation inside a preview call trips this.
    #[derive(PartialEq, Eq, Debug)]
    struct StateSnapshot {
        head: String,
        branch_tip: String,
        dirty: usize,
        merge_head: bool,
        rebase_state: bool,
    }

    fn snapshot(repo_path: &Path) -> StateSnapshot {
        let repo = git2::Repository::open(repo_path).unwrap();
        let head = repo.head().unwrap();
        let head_oid = head.target().unwrap().to_string();
        let branch_tip = repo
            .find_branch(head.shorthand().unwrap(), git2::BranchType::Local)
            .unwrap()
            .get()
            .target()
            .unwrap()
            .to_string();
        let dirty = repo
            .statuses(None)
            .unwrap()
            .iter()
            .filter(|e| is_dirty_entry(e.status()))
            .count();
        StateSnapshot {
            head: head_oid,
            branch_tip,
            dirty,
            merge_head: crate::core::merge::merge_in_progress(repo_path).unwrap(),
            rebase_state: crate::core::rebase::get_rebase_state(repo_path).unwrap().is_some(),
        }
    }

    fn head_oid(repo_path: &Path) -> String {
        git2::Repository::open(repo_path)
            .unwrap()
            .head()
            .unwrap()
            .target()
            .unwrap()
            .to_string()
    }

    /// Fixture: a repo with 3 commits on `main` (f.txt growing) plus a side
    /// branch that diverges, one commit each side; `feature` merges into
    /// `main` cleanly at first, then conflicting variants are created by the
    /// caller.
    fn three_commit_fixture(tag: &str) -> std::path::PathBuf {
        let dir = tmpdir(tag);
        git(&dir, &["init", "-q", "-b", "main", "repo"]);
        let repo = dir.join("repo");
        git(&repo, &["config", "user.name", "t"]);
        git(&repo, &["config", "user.email", "t@e.c"]);
        std::fs::write(repo.join("a.txt"), "a1\n").unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-qm", "c1"]);
        std::fs::write(repo.join("b.txt"), "b1\n").unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-qm", "c2"]);
        std::fs::write(repo.join("c.txt"), "c1\n").unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-qm", "c3"]);
        // Side branch with one clean commit (no overlap with main's files).
        git(&repo, &["checkout", "-q", "-b", "feature"]);
        std::fs::write(repo.join("d.txt"), "d1\n").unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-qm", "feature work"]);
        git(&repo, &["checkout", "-q", "main"]);
        repo
    }

    /// GF-17 acceptance 1: a hard-reset preview lists every discarded commit
    /// (oid + message) and the worktree changes that would be lost; counts
    /// are authoritative and nothing is lost silently.
    #[test]
    fn preview_reset_lists_discarded_commits_and_worktree_changes() {
        let repo = three_commit_fixture("reset");

        // Dirty worktree: one tracked modification + one staged change.
        std::fs::write(repo.join("a.txt"), "a1-dirty\n").unwrap();
        std::fs::write(repo.join("staged.txt"), "s1\n").unwrap();
        git(&repo, &["add", "staged.txt"]);
        // An untracked file must NOT appear in the data-loss list (every
        // reset mode keeps untracked files).
        std::fs::write(repo.join("untracked.txt"), "u1\n").unwrap();
        let before = snapshot(&repo);
        assert_eq!(before.dirty, 2);

        // Reset to c2 (drop c3): 1 commit discarded.
        let c2 = git2::Repository::open(&repo)
            .unwrap()
            .revparse_single("HEAD~1")
            .unwrap()
            .peel_to_commit()
            .unwrap()
            .id()
            .to_string();
        let preview = preview_reset(&repo, Some(&c2), "hard").unwrap();

        assert_eq!(preview.mode, "hard");
        assert_eq!(preview.branch, "main");
        assert!(!preview.detached);
        assert_eq!(preview.discarded_count, 1);
        assert_eq!(preview.discarded_commits.len(), 1);
        assert_eq!(preview.discarded_commits[0].summary, "c3");
        assert!(preview.discarded_commits[0].oid.starts_with(&preview.discarded_commits[0].short_oid));
        assert_eq!(preview.lost_changes_count, 2, "staged + unstaged tracked changes");
        let paths: Vec<&str> = preview.lost_file_changes.iter().map(|f| f.path.as_str()).collect();
        assert!(paths.contains(&"a.txt"), "{paths:?}");
        assert!(paths.contains(&"staged.txt"), "{paths:?}");
        assert!(!paths.contains(&"untracked.txt"), "untracked files survive a reset");
        assert!(preview.unrecoverable, "hard + uncommitted changes is unrecoverable");

        // Reset to HEAD itself drops nothing.
        let noop = preview_reset(&repo, None, "mixed").unwrap();
        assert_eq!(noop.discarded_count, 0);
        assert_eq!(noop.lost_changes_count, 2);
        assert!(!noop.unrecoverable, "mixed keeps the worktree content");

        // GF-17 acceptance 3: the preview changed no repo state at all.
        let after = snapshot(&repo);
        assert_eq!(before.head, after.head);
        assert_eq!(before.branch_tip, after.branch_tip);
        assert_eq!(before.dirty, after.dirty);
        assert_eq!(before.merge_head, after.merge_head);
        assert_eq!(before.rebase_state, after.rebase_state);
        assert_eq!(before, after);

        // Invalid mode is rejected up front (same message family as the
        // executor, so the UI never previews something reset_to refuses).
        let err = preview_reset(&repo, None, "squash").unwrap_err();
        assert!(err.to_string().contains("invalid reset mode"), "{err}");

        let _ = std::fs::remove_dir_all(repo.parent().unwrap());
    }

    /// GF-17 acceptance 2 (merge): up_to_date / fast_forward / merge kinds,
    /// incoming commit counts, affected files and conflict prediction — all
    /// without touching the repo.
    #[test]
    fn preview_merge_reports_incoming_files_and_conflicts() {
        use crate::core::merge::MergeOutcome;

        let repo = three_commit_fixture("merge");
        let before = snapshot(&repo);
        let head_before = head_oid(&repo);

        // 1. Fast-forward: `feature` is `main` + one extra commit (created
        //    off main's tip in the fixture), so merging it into main only
        //    fast-forwards.
        let ff = preview_merge(&repo, "feature", "normal").unwrap();
        assert_eq!(ff.kind, "fast_forward", "{}", ff.incoming_count);
        assert_eq!(ff.incoming_count, 1);
        assert_eq!(ff.incoming_commits[0].summary, "feature work");
        assert_eq!(ff.affected_files_count, 1);
        assert_eq!(ff.affected_files, vec!["d.txt".to_string()]);
        assert!(!ff.conflict_predicted);
        assert!(!ff.dirty_blocked);
        assert_eq!(ff.branch, "main");

        // 2. no-ff on the same pair still previews a merge commit.
        let noff = preview_merge(&repo, "feature", "no-ff").unwrap();
        assert_eq!(noff.kind, "merge");
        assert_eq!(noff.incoming_count, 1);
        assert!(!noff.conflict_predicted);

        // 3. Up-to-date: merge the current branch into itself.
        let utd = preview_merge(&repo, "main", "normal").unwrap();
        assert_eq!(utd.kind, "up_to_date");
        assert_eq!(utd.incoming_count, 0);
        assert_eq!(utd.affected_files_count, 0);

        // Read-only so far: the three previews moved nothing.
        assert_eq!(head_oid(&repo), head_before);
        assert_eq!(snapshot(&repo).head, before.head);

        // 4. Conflicting merge: both sides change a.txt.
        //    main: a.txt = "main line"; feature (off the fixture tip):
        //    a.txt = "feature line". Merge base = fixture tip (c3).
        std::fs::write(repo.join("a.txt"), "main line\n").unwrap();
        git(&repo, &["commit", "-qam", "main change"]);
        let head_after_main_change = head_oid(&repo);
        git(&repo, &["checkout", "-q", "feature"]);
        std::fs::write(repo.join("a.txt"), "feature line\n").unwrap();
        git(&repo, &["commit", "-qam", "feature change"]);
        git(&repo, &["checkout", "-q", "main"]);
        let conflict = preview_merge(&repo, "feature", "normal").unwrap();
        assert_eq!(conflict.kind, "merge");
        assert_eq!(conflict.incoming_count, 2, "feature work + feature change");
        assert!(conflict.conflict_predicted, "both sides changed a.txt");
        assert_eq!(conflict.conflict_files, vec!["a.txt".to_string()]);
        assert_eq!(conflict.affected_files_count, 2, "d.txt + a.txt");
        assert!(conflict.affected_files.contains(&"a.txt".to_string()));
        assert!(conflict.affected_files.contains(&"d.txt".to_string()));

        // 5. Dirty worktree: reported (merge would refuse), not hidden.
        std::fs::write(repo.join("b.txt"), "b1-dirty\n").unwrap();
        let dirty = preview_merge(&repo, "feature", "normal").unwrap();
        assert!(dirty.dirty_blocked);
        assert_eq!(dirty.dirty_files_count, 1);
        assert_eq!(dirty.dirty_files, vec!["b.txt".to_string()]);
        git(&repo, &["checkout", "--", "b.txt"]);
        assert_eq!(head_oid(&repo), head_after_main_change, "previews 4-5 moved nothing");

        // 6. In-progress merge is refused (the executor would refuse too):
        //    create a real conflicted merge, then preview again.
        match crate::core::merge::merge(&repo, "feature", "normal").unwrap() {
            MergeOutcome::Conflict { .. } => {}
            other => panic!("expected a conflicting merge, got {:?}", other),
        }
        let err = preview_merge(&repo, "feature", "normal").unwrap_err();
        assert!(err.to_string().contains("merge 进行中"), "{err}");
        crate::core::merge::merge_abort(&repo).unwrap();
        assert_eq!(head_oid(&repo), head_after_main_change, "abort restored HEAD");

        // Unknown source is a readable NotFound.
        let err = preview_merge(&repo, "nope", "normal").unwrap_err();
        assert!(err.to_string().contains("nope"), "{err}");
        // Invalid mode rejected up front.
        let err = preview_merge(&repo, "feature", "soft").unwrap_err();
        assert!(err.to_string().contains("invalid merge mode"), "{err}");

        // No in-progress state leaked out of the whole test.
        let after = snapshot(&repo);
        assert!(!after.merge_head);
        assert!(!after.rebase_state);
        assert_eq!(after.dirty, before.dirty);

        let _ = std::fs::remove_dir_all(repo.parent().unwrap());
    }

    /// A long divergence is capped for the payload while the counts stay
    /// authoritative.
    #[test]
    fn preview_caps_lists_but_keeps_counts() {
        let dir = tmpdir("cap");
        git(&dir, &["init", "-q", "-b", "main", "repo"]);
        let repo = dir.join("repo");
        git(&repo, &["config", "user.name", "t"]);
        git(&repo, &["config", "user.email", "t@e.c"]);
        // One base commit + 60 commits after it.
        std::fs::write(repo.join("base.txt"), "0\n").unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-qm", "base"]);
        for i in 1..=60 {
            std::fs::write(repo.join("base.txt"), format!("{i}\n")).unwrap();
            git(&repo, &["commit", "-qam", &format!("c{i}")]);
        }
        let base_oid = git2::Repository::open(&repo)
            .unwrap()
            .revparse_single("HEAD~60")
            .unwrap()
            .peel_to_commit()
            .unwrap()
            .id()
            .to_string();
        let preview = preview_reset(&repo, Some(&base_oid), "hard").unwrap();
        assert_eq!(preview.discarded_count, 60);
        assert_eq!(preview.discarded_commits.len(), PREVIEW_COMMIT_LIMIT);
        assert_eq!(preview.discarded_commits[0].summary, "c60", "newest first");
        assert_eq!(preview.lost_changes_count, 0);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
