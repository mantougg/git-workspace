//! Conflict Resolver core (T-16): conflict detection with stage details,
//! three-way content loading, and resolution operations (ours / theirs /
//! both / manual content). Resolution ends with `git add` semantics, so the
//! result matches the CLI exactly (acceptance 2).

use std::path::Path;

use serde::Serialize;

use crate::core::rebase::RebaseState;
use crate::error::{AppError, AppResult};

/// A conflicted file with its conflict shape.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConflictFile {
    pub path: String,
    /// "both-modified" | "both-added" | "deleted-by-us" | "deleted-by-them"
    pub conflict_type: String,
}

/// The operation currently driving the repo's conflict state (for routing
/// Continue / Abort to the right state machine).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationState {
    pub merge: bool,
    pub cherry_pick: bool,
    pub revert: bool,
    pub rebase: Option<RebaseState>,
    pub conflicts: Vec<ConflictFile>,
}

/// Three-way + worktree content of one conflicted file (loaded on demand;
/// large files are capped per side).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConflictContent {
    /// BASE (common ancestor) content; None when the file has no ancestor.
    pub base: Option<String>,
    /// OURS (current HEAD side); None when deleted on our side.
    pub ours: Option<String>,
    /// THEIRS (incoming side); None when deleted on their side.
    pub theirs: Option<String>,
    /// Current worktree content (with conflict markers), if readable.
    pub worktree: Option<String>,
    /// True when any side was truncated.
    pub truncated: bool,
}

/// Per-side content cap for the resolver panes (IPC + render budget).
const MAX_SIDE_CHARS: usize = 500_000;

/// The repo's current operation + conflict state (CONFLICT detection).
pub fn operation_state(repo_path: &Path) -> AppResult<OperationState> {
    let repo = git2::Repository::open(repo_path)?;
    let rebase = crate::core::rebase::get_rebase_state(repo_path)?;
    Ok(OperationState {
        merge: repo.path().join("MERGE_HEAD").exists(),
        cherry_pick: repo.path().join("CHERRY_PICK_HEAD").exists(),
        revert: repo.path().join("REVERT_HEAD").exists(),
        rebase,
        conflicts: conflict_files(&repo)?,
    })
}

/// Conflicted files with conflict shape, from the index stage entries.
fn conflict_files(repo: &git2::Repository) -> AppResult<Vec<ConflictFile>> {
    let index = repo.index()?;
    let mut out: Vec<ConflictFile> = Vec::new();
    for conflict in index.conflicts()? {
        let conflict = conflict?;
        let path = conflict
            .our
            .as_ref()
            .or(conflict.their.as_ref())
            .or(conflict.ancestor.as_ref())
            .map(|e| String::from_utf8_lossy(&e.path).to_string())
            .unwrap_or_default();
        if path.is_empty() || out.iter().any(|c| c.path == path) {
            continue;
        }
        let conflict_type = match (
            conflict.ancestor.is_some(),
            conflict.our.is_some(),
            conflict.their.is_some(),
        ) {
            (true, true, true) => "both-modified",
            (false, true, true) => "both-added",
            (true, false, true) => "deleted-by-us",
            (true, true, false) => "deleted-by-them",
            _ => "both-modified",
        };
        out.push(ConflictFile {
            path,
            conflict_type: conflict_type.to_string(),
        });
    }
    Ok(out)
}

/// Load BASE / OURS / THEIRS stage contents + worktree content of one
/// conflicted file. Sides absent from the index come back as None.
pub fn conflict_content(repo_path: &Path, path: &str) -> AppResult<ConflictContent> {
    let repo = git2::Repository::open(repo_path)?;
    let index = repo.index()?;

    let mut base = None;
    let mut ours = None;
    let mut theirs = None;
    let mut truncated = false;

    for conflict in index.conflicts()? {
        let conflict = conflict?;
        let matches = conflict
            .our
            .as_ref()
            .or(conflict.their.as_ref())
            .or(conflict.ancestor.as_ref())
            .map(|e| e.path == path.as_bytes())
            .unwrap_or(false);
        if !matches {
            continue;
        }
        for (stage_entry, slot) in [
            (conflict.ancestor, &mut base),
            (conflict.our, &mut ours),
            (conflict.their, &mut theirs),
        ] {
            if let Some(entry) = stage_entry {
                let (text, was_truncated) = read_blob_capped(&repo, entry.id);
                *slot = text;
                truncated |= was_truncated;
            }
        }
        break;
    }

    let worktree_path = repo_path.join(path);
    let (worktree, wt_truncated) = match std::fs::read_to_string(&worktree_path) {
        Ok(content) => {
            let (t, tr) = cap(content);
            (Some(t), tr)
        }
        Err(_) => (None, false),
    };
    truncated |= wt_truncated;

    Ok(ConflictContent {
        base,
        ours,
        theirs,
        worktree,
        truncated,
    })
}

fn read_blob_capped(repo: &git2::Repository, oid: git2::Oid) -> (Option<String>, bool) {
    match repo.find_blob(oid) {
        Ok(blob) => {
            let content = String::from_utf8_lossy(blob.content()).to_string();
            let (t, tr) = cap(content);
            (Some(t), tr)
        }
        Err(_) => (None, false),
    }
}

fn cap(content: String) -> (String, bool) {
    if content.chars().count() > MAX_SIDE_CHARS {
        let truncated: String = content.chars().take(MAX_SIDE_CHARS).collect();
        (format!("{}\n... (content truncated)", truncated), true)
    } else {
        (content, false)
    }
}

/// Resolve one conflicted file with a strategy: "ours" | "theirs" | "both".
/// Writes the chosen content to the worktree and stages the file (conflict
/// cleared), exactly like editing + `git add`. When the chosen side is absent
/// (deletion conflict), the file is removed instead.
pub fn resolve_conflict(repo_path: &Path, path: &str, strategy: &str) -> AppResult<()> {
    let content = conflict_content(repo_path, path)?;
    let chosen: Option<String> = match strategy {
        "ours" => content.ours,
        "theirs" => content.theirs,
        "both" => match (content.ours, content.theirs) {
            (Some(o), Some(t)) => Some(format!("{}\n{}", o.trim_end(), t)),
            (o, t) => o.or(t),
        },
        other => {
            return Err(AppError::Other(format!(
                "invalid resolve strategy '{}' (ours | theirs | both)",
                other
            )))
        }
    };
    apply_resolution(repo_path, path, chosen)
}

/// Resolve one conflicted file with manually edited content (None = delete).
pub fn resolve_conflict_with_content(repo_path: &Path, path: &str, content: Option<&str>) -> AppResult<()> {
    apply_resolution(repo_path, path, content.map(String::from))
}

/// Write the resolution to the worktree + stage it (or remove when deleted).
fn apply_resolution(repo_path: &Path, path: &str, content: Option<String>) -> AppResult<()> {
    let repo = git2::Repository::open(repo_path)?;
    let mut index = repo.index()?;
    match content {
        Some(text) => {
            std::fs::write(repo_path.join(path), text)?;
            index.add_path(Path::new(path))?;
        }
        None => {
            let full = repo_path.join(path);
            if full.exists() {
                std::fs::remove_file(full)?;
            }
            index.remove_path(Path::new(path))?;
        }
    }
    index.write()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "gw_conflict_{}_{}",
            tag,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn commit_file(repo: &git2::Repository, dir: &Path, name: &str, content: &str, msg: &str) -> String {
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

    /// Build a repo with a merge conflict on a.txt: base "base\n", ours
    /// "ours\n", theirs "theirs\n". Returns when the merge is in conflict.
    fn setup_conflict(dir: &Path) {
        {
            let repo = git2::Repository::init(dir).unwrap();
            commit_file(&repo, dir, "a.txt", "base\n", "init");
            let head = repo.head().unwrap().peel_to_commit().unwrap();
            repo.branch("side", &head, false).unwrap();
            drop(head);
            commit_file(&repo, dir, "a.txt", "ours\n", "master change");
            drop(repo);
        }
        crate::core::branch::checkout_branch(dir, "side").unwrap();
        {
            let repo = git2::Repository::open(dir).unwrap();
            commit_file(&repo, dir, "a.txt", "theirs\n", "side change");
            drop(repo);
        }
        crate::core::branch::checkout_branch(dir, "master").unwrap();

        let outcome = crate::core::merge::merge(dir, "side", "normal").unwrap();
        assert!(matches!(outcome, crate::core::merge::MergeOutcome::Conflict { .. }));
    }

    /// Detection: operation_state reports the merge + the conflicted file.
    #[test]
    fn detects_merge_conflict_state() {
        let dir = tmpdir("detect");
        setup_conflict(&dir);

        let state = operation_state(&dir).unwrap();
        assert!(state.merge);
        assert!(!state.cherry_pick);
        assert!(state.rebase.is_none());
        assert_eq!(state.conflicts.len(), 1);
        assert_eq!(state.conflicts[0].path, "a.txt");
        assert_eq!(state.conflicts[0].conflict_type, "both-modified");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Three-way content loads base/ours/theirs + worktree markers.
    #[test]
    fn loads_three_way_content() {
        let dir = tmpdir("content");
        setup_conflict(&dir);

        let content = conflict_content(&dir, "a.txt").unwrap();
        assert_eq!(content.base.as_deref().map(|s| s.trim_end()), Some("base"));
        assert_eq!(content.ours.as_deref().map(|s| s.trim_end()), Some("ours"));
        assert_eq!(content.theirs.as_deref().map(|s| s.trim_end()), Some("theirs"));
        let wt = content.worktree.unwrap_or_default();
        assert!(wt.contains("<<<<<<<") && wt.contains(">>>>>>>"));
        assert!(!content.truncated);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Acceptance 2: ours/theirs/both resolutions match `git add` semantics —
    /// conflict entry cleared, chosen content in worktree + staged.
    #[test]
    fn resolve_strategies_match_git_add() {
        for (strategy, expected) in [("ours", "ours\n"), ("theirs", "theirs\n"), ("both", "ours\ntheirs\n")] {
            let dir = tmpdir(&format!("resolve_{}", strategy));
            setup_conflict(&dir);

            resolve_conflict(&dir, "a.txt", strategy).unwrap();

            // Conflict entry cleared; file staged with chosen content.
            let state = operation_state(&dir).unwrap();
            assert_eq!(state.conflicts.len(), 0, "strategy {}", strategy);
            let worktree = std::fs::read_to_string(dir.join("a.txt"))
                .unwrap()
                .replace("\r\n", "\n");
            assert_eq!(worktree, expected, "strategy {}", strategy);

            // Staged (or identical to HEAD for "ours"): no CONFLICTED status.
            let repo = git2::Repository::open(&dir).unwrap();
            let statuses = repo.statuses(None).unwrap();
            if let Some(entry) = statuses.iter().find(|e| e.path() == Some("a.txt")) {
                assert!(
                    !entry.status().contains(git2::Status::CONFLICTED),
                    "strategy {} must clear the conflicted status",
                    strategy
                );
            }

            // Merge can now be continued (merge_continue succeeds).
            let oid = crate::core::merge::merge_continue(&dir, None).unwrap();
            assert!(!oid.is_empty());

            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    /// Manual edit resolution writes exact content and clears the conflict.
    #[test]
    fn resolve_with_manual_content() {
        let dir = tmpdir("manual");
        setup_conflict(&dir);

        resolve_conflict_with_content(&dir, "a.txt", Some("hand crafted\n")).unwrap();
        let state = operation_state(&dir).unwrap();
        assert_eq!(state.conflicts.len(), 0);
        assert_eq!(
            std::fs::read_to_string(dir.join("a.txt"))
                .unwrap()
                .replace("\r\n", "\n"),
            "hand crafted\n"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// AI-09 acceptance: assembling a bounded Preview hunk is read-only. A
    /// suggestion cannot clear the index or alter the worktree before the
    /// existing T-16 Apply / Mark Resolved command is explicitly invoked.
    #[test]
    fn conflict_hunk_preview_leaves_worktree_and_index_untouched() {
        let dir = tmpdir("ai_preview_read_only");
        setup_conflict(&dir);
        let before = std::fs::read_to_string(dir.join("a.txt")).unwrap();

        let items = crate::ai::context::collect_conflict_hunk(&dir, "a.txt", 0, 1).expect("read-only hunk context");
        assert!(items.iter().any(|item| item.source_id.contains("conflict:ours:")));
        assert!(items.iter().any(|item| item.source_id.contains("conflict:theirs:")));

        assert_eq!(std::fs::read_to_string(dir.join("a.txt")).unwrap(), before);
        assert_eq!(operation_state(&dir).unwrap().conflicts.len(), 1);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
