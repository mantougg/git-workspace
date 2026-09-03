//! Rebase (T-15): basic / --onto / interactive (pick / reword / squash /
//! drop) via a libgit2 step sequencer.
//!
//! Deviation from the task spec's "interactive rebase via git CLI" note
//! (documented in T-15-merge-rebase.md): the CLI's editor semantics are
//! unreliable on Windows (GIT_SEQUENCE_EDITOR shell injection) and cannot
//! carry reword/squash messages cleanly. Instead each op is applied with
//! libgit2 `cherrypick` and committed explicitly — fully local, offline and
//! testable. Progress is persisted after every step to
//! `.git/gitworkspace-rebase.json`, so an interrupted rebase is detected and
//! resumable across app restarts (acceptance 4).

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::core::history;
use crate::error::{AppError, AppResult};

/// One interactive-rebase todo entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RebaseOp {
    /// "pick" | "reword" | "squash" | "drop"
    pub action: String,
    pub oid: String,
    /// Replacement message for reword; None keeps the original.
    pub message: Option<String>,
    /// Original first line (display + state file readability).
    pub subject: String,
}

/// Persisted rebase progress (`.git/gitworkspace-rebase.json`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RebaseState {
    /// HEAD before the rebase started (abort target).
    pub original_head: String,
    /// Revision the branch is being replayed onto.
    pub onto: String,
    pub ops: Vec<RebaseOp>,
    /// Index of the op currently being applied (or next to apply).
    pub position: usize,
    /// Last commit of the new chain (squash melds into it).
    pub prev_commit: String,
}

/// Outcome of a rebase run (start / continue / skip).
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum RebaseOutcome {
    /// All ops applied.
    #[serde(rename_all = "camelCase")]
    Success { rewritten: usize },
    /// An op conflicted; progress is persisted — resolve and continue,
    /// skip, or abort.
    #[serde(rename_all = "camelCase")]
    Conflict {
        files: Vec<String>,
        position: usize,
        total: usize,
        /// Oid of the op being applied when the conflict occurred.
        current: String,
    },
}

const STATE_FILE: &str = "gitworkspace-rebase.json";

fn state_path(repo: &git2::Repository) -> std::path::PathBuf {
    repo.path().join(STATE_FILE)
}

/// Current rebase progress, if a rebase is in progress (restart detection).
pub fn get_rebase_state(repo_path: &Path) -> AppResult<Option<RebaseState>> {
    let repo = git2::Repository::open(repo_path)?;
    let path = state_path(&repo);
    if !path.exists() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(&path)?;
    let state: RebaseState =
        serde_json::from_str(&raw).map_err(|e| AppError::Other(format!("corrupt rebase state: {}", e)))?;
    Ok(Some(state))
}

/// Commits of `branch` (default HEAD) not in `upstream`, oldest first, as
/// default pick ops — the interactive todo's starting point.
pub fn list_rebase_commits(repo_path: &Path, upstream: &str, branch: Option<&str>) -> AppResult<Vec<RebaseOp>> {
    let repo = git2::Repository::open(repo_path)?;
    let upstream_oid = repo
        .revparse_single(upstream)
        .and_then(|o| o.peel_to_commit())
        .map_err(|_| AppError::NotFound(format!("revision '{}' not found", upstream)))?
        .id();
    let branch_oid = repo
        .revparse_single(branch.unwrap_or("HEAD"))
        .and_then(|o| o.peel_to_commit())
        .map_err(|_| AppError::NotFound(format!("branch not found")))?
        .id();

    let mut walk = repo.revwalk()?;
    walk.push(branch_oid)?;
    walk.hide(upstream_oid)?;
    let mut oids: Vec<git2::Oid> = walk.flatten().collect();
    oids.reverse(); // oldest first

    let mut ops = Vec::new();
    for oid in oids {
        let commit = repo.find_commit(oid)?;
        ops.push(RebaseOp {
            action: "pick".to_string(),
            oid: oid.to_string(),
            message: None,
            subject: commit.summary().unwrap_or_default().to_string(),
        });
    }
    Ok(ops)
}

/// Start a rebase: move the current branch to `onto` and replay `ops`.
/// Basic rebase = pick ops from `list_rebase_commits`; --onto = custom onto;
/// interactive = user-arranged ops (pick/reword/squash/drop).
pub fn start_rebase(repo_path: &Path, onto: &str, ops: Vec<RebaseOp>) -> AppResult<RebaseOutcome> {
    let repo = git2::Repository::open(repo_path)?;
    if state_path(&repo).exists() {
        return Err(AppError::Conflict(
            "a rebase is already in progress (continue / skip / abort it first)".into(),
        ));
    }

    // Validate: oids exist; the first non-drop op cannot be a squash.
    for op in &ops {
        git2::Oid::from_str(&op.oid)
            .ok()
            .and_then(|oid| repo.find_commit(oid).ok())
            .ok_or_else(|| AppError::NotFound(format!("commit '{}' not found", op.oid)))?;
        if !matches!(op.action.as_str(), "pick" | "reword" | "squash" | "drop") {
            return Err(AppError::Other(format!("invalid rebase action '{}'", op.action)));
        }
    }
    if let Some(first) = ops.iter().find(|o| o.action != "drop") {
        if first.action == "squash" {
            return Err(AppError::Other(
                "第一个有效提交不能是 squash（没有可并入的前驱）".into(),
            ));
        }
    }

    let onto_oid = repo
        .revparse_single(onto)
        .and_then(|o| o.peel_to_commit())
        .map_err(|_| AppError::NotFound(format!("revision '{}' not found", onto)))?
        .id()
        .to_string();
    let original_head = repo
        .head()
        .and_then(|h| h.target().ok_or(git2::Error::from_str("HEAD has no target")))?
        .to_string();

    if ops.is_empty() {
        return Ok(RebaseOutcome::Success { rewritten: 0 });
    }

    // Move the branch to onto (hard reset), then replay the ops forward.
    history::reset_to(repo_path, Some(onto), "hard")?;
    let repo = git2::Repository::open(repo_path)?;
    let state = RebaseState {
        original_head,
        onto: onto.to_string(),
        ops,
        position: 0,
        prev_commit: onto_oid,
    };
    save_state(&repo, &state)?;
    drop(repo);

    run_remaining(repo_path)
}

/// Continue after the user resolved the conflicted op (index must be clean):
/// commits the staged resolution for the current op and replays the rest.
pub fn rebase_continue(repo_path: &Path) -> AppResult<RebaseOutcome> {
    let repo = git2::Repository::open(repo_path)?;
    let mut state = load_state(&repo)?;

    let mut index = repo.index()?;
    if index.has_conflicts() {
        return Err(AppError::Conflict(
            "仍有未解决的冲突，请先解决（或选择 Skip / Abort）".into(),
        ));
    }
    if state.position >= state.ops.len() {
        // Nothing pending: just finish.
        finish(&repo)?;
        return Ok(RebaseOutcome::Success { rewritten: 0 });
    }

    let tree_oid = index.write_tree()?;
    let tree = repo.find_tree(tree_oid)?;
    let op = state.ops[state.position].clone();
    let new_prev = commit_op(&repo, &state, &op, &tree)?;
    drop(tree);
    drop(index);
    repo.cleanup_state()?;
    state.prev_commit = new_prev;
    state.position += 1;
    save_state(&repo, &state)?;
    drop(repo);

    run_remaining(repo_path)
}

/// Skip the current (conflicting) op and replay the rest.
pub fn rebase_skip(repo_path: &Path) -> AppResult<RebaseOutcome> {
    let repo = git2::Repository::open(repo_path)?;
    let mut state = load_state(&repo)?;
    let prev = state.prev_commit.clone();
    drop(repo);

    // Discard the conflicted worktree state, then advance.
    history::reset_to(repo_path, Some(&prev), "hard")?;

    let repo = git2::Repository::open(repo_path)?;
    repo.cleanup_state()?;
    state.position += 1;
    save_state(&repo, &state)?;
    drop(repo);

    run_remaining(repo_path)
}

/// Abort the rebase: restore the branch to its pre-rebase HEAD completely.
pub fn rebase_abort(repo_path: &Path) -> AppResult<()> {
    let repo = git2::Repository::open(repo_path)?;
    let state = load_state(&repo)?;
    let original = state.original_head.clone();
    drop(repo);

    history::reset_to(repo_path, Some(&original), "hard")?;

    let repo = git2::Repository::open(repo_path)?;
    repo.cleanup_state()?;
    let path = state_path(&repo);
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    Ok(())
}

/// Replay the remaining ops from the persisted state.
fn run_remaining(repo_path: &Path) -> AppResult<RebaseOutcome> {
    let repo = git2::Repository::open(repo_path)?;
    let mut state = load_state(&repo)?;
    let total = state.ops.len();
    let mut rewritten = 0usize;

    while state.position < total {
        let op = state.ops[state.position].clone();
        if op.action == "drop" {
            state.position += 1;
            save_state(&repo, &state)?;
            continue;
        }

        let commit = repo.find_commit(git2::Oid::from_str(&op.oid)?)?;
        repo.cherrypick(&commit, None)?;

        let mut index = repo.index()?;
        if index.has_conflicts() {
            save_state(&repo, &state)?;
            return Ok(RebaseOutcome::Conflict {
                files: history::conflict_paths(&index)?,
                position: state.position,
                total,
                current: op.oid.clone(),
            });
        }

        let tree_oid = index.write_tree()?;
        let tree = repo.find_tree(tree_oid)?;
        let new_prev = commit_op(&repo, &state, &op, &tree)?;
        repo.cleanup_state()?;
        state.prev_commit = new_prev;
        state.position += 1;
        rewritten += 1;
        save_state(&repo, &state)?;
    }

    finish(&repo)?;
    Ok(RebaseOutcome::Success { rewritten })
}

/// Commit the applied tree according to the op's action; returns the new
/// chain tip. pick/reword create a commit on top of `prev_commit`; squash
/// melds into it (rewrite with combined message and the new tree).
///
/// The commit is created WITHOUT updating HEAD directly: for squash the new
/// commit's first parent is `prev_commit`'s parent, not the current tip, so
/// `commit(Some("HEAD"))` would fail with "current tip is not the first
/// parent". The branch ref is moved explicitly afterwards (works for
/// detached HEAD too).
fn commit_op(repo: &git2::Repository, state: &RebaseState, op: &RebaseOp, tree: &git2::Tree) -> AppResult<String> {
    let sig = repo.signature()?;
    let orig = repo.find_commit(git2::Oid::from_str(&op.oid)?)?;
    let prev_commit = repo.find_commit(git2::Oid::from_str(&state.prev_commit)?)?;

    let (author, message, parents): (git2::Signature, String, Vec<git2::Commit>) = match op.action.as_str() {
        "pick" => (
            orig.author(),
            orig.message().unwrap_or_default().to_string(),
            vec![prev_commit.clone()],
        ),
        "reword" => (
            orig.author(),
            op.message
                .clone()
                .unwrap_or_else(|| orig.message().unwrap_or_default().to_string()),
            vec![prev_commit.clone()],
        ),
        "squash" => {
            // Meld into the previous chain commit: its parent, its author,
            // combined message, the new tree.
            let prev_parent = prev_commit
                .parent(0)
                .map_err(|_| AppError::Other("squash target has no parent".into()))?;
            (
                prev_commit.author(),
                format!(
                    "{}\n\n{}",
                    prev_commit.message().unwrap_or_default(),
                    orig.message().unwrap_or_default()
                ),
                vec![prev_parent],
            )
        }
        other => return Err(AppError::Other(format!("invalid rebase action '{}'", other))),
    };

    let parent_refs: Vec<&git2::Commit> = parents.iter().collect();
    let oid = repo.commit(None, &author, &sig, &message, tree, &parent_refs)?;

    let head = repo.head()?;
    let mut resolved = head.resolve()?;
    resolved.set_target(oid, "rebase step")?;
    Ok(oid.to_string())
}

fn load_state(repo: &git2::Repository) -> AppResult<RebaseState> {
    let path = state_path(repo);
    let raw = std::fs::read_to_string(&path).map_err(|_| AppError::Conflict("no rebase in progress".into()))?;
    serde_json::from_str(&raw).map_err(|e| AppError::Other(format!("corrupt rebase state: {}", e)))
}

fn save_state(repo: &git2::Repository, state: &RebaseState) -> AppResult<()> {
    let raw = serde_json::to_string_pretty(state)?;
    std::fs::write(state_path(repo), raw)?;
    Ok(())
}

/// Done: remove the state file and clear any pick state.
fn finish(repo: &git2::Repository) -> AppResult<()> {
    repo.cleanup_state()?;
    let path = state_path(repo);
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "gw_rebase_{}_{}",
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

    fn checkout(repo_path: &Path, name: &str) {
        crate::core::branch::checkout_branch(repo_path, name).unwrap();
    }

    fn head_subjects(repo_path: &Path, count: usize) -> Vec<String> {
        let repo = git2::Repository::open(repo_path).unwrap();
        let mut walk = repo.revwalk().unwrap();
        walk.push_head().unwrap();
        walk.flatten()
            .take(count)
            .map(|oid| repo.find_commit(oid).unwrap().summary().unwrap_or_default().to_string())
            .collect()
    }

    /// Build a repo: master with init + m1; side branched from init with s1, s2.
    fn setup_diverged(dir: &Path) -> (String, String) {
        let s1;
        let s2;
        {
            let repo = git2::Repository::init(dir).unwrap();
            commit_file(&repo, dir, "a.txt", "one\n", "init");
            let head = repo.head().unwrap().peel_to_commit().unwrap();
            repo.branch("side", &head, false).unwrap();
            drop(head);
            commit_file(&repo, dir, "m.txt", "m1\n", "m1");
            drop(repo);
        }
        checkout(dir, "side");
        {
            let repo = git2::Repository::open(dir).unwrap();
            s1 = commit_file(&repo, dir, "s1.txt", "s1\n", "s1");
            s2 = commit_file(&repo, dir, "s2.txt", "s2\n", "s2");
            drop(repo);
        }
        (s1, s2)
    }

    /// Basic rebase: side replays its two commits onto master.
    #[test]
    fn basic_rebase_replays_onto() {
        let dir = tmpdir("basic");
        let (_s1, _s2) = setup_diverged(&dir);

        let ops = list_rebase_commits(&dir, "master", None).unwrap();
        assert_eq!(ops.len(), 2);
        assert_eq!(ops[0].subject, "s1"); // oldest first

        let outcome = start_rebase(&dir, "master", ops).unwrap();
        assert!(matches!(outcome, RebaseOutcome::Success { rewritten: 2 }));

        let subjects = head_subjects(&dir, 4);
        assert_eq!(subjects, vec!["s2", "s1", "m1", "init"]);
        assert!(dir.join("m.txt").exists());
        assert!(dir.join("s1.txt").exists());
        assert!(get_rebase_state(&dir).unwrap().is_none());

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Interactive ops: drop + reword + squash produce the expected chain.
    #[test]
    fn interactive_ops_drop_reword_squash() {
        let dir = tmpdir("interactive");
        let (s1, s2) = setup_diverged(&dir);

        let ops = vec![
            RebaseOp {
                action: "reword".into(),
                oid: s1.clone(),
                message: Some("s1 rewritten".into()),
                subject: "s1".into(),
            },
            RebaseOp {
                action: "squash".into(),
                oid: s2.clone(),
                message: None,
                subject: "s2".into(),
            },
            // A dropped op must not appear.
            RebaseOp {
                action: "drop".into(),
                oid: {
                    let repo = git2::Repository::open(&dir).unwrap();
                    let obj = repo.revparse_single("master").unwrap();
                    let oid = obj.id().to_string();
                    oid
                },
                message: None,
                subject: "m1 (dropped pick would be nonsense; drop just skips)".into(),
            },
        ];

        // First op cannot be squash.
        let bad = vec![RebaseOp {
            action: "squash".into(),
            oid: s1.clone(),
            message: None,
            subject: "s1".into(),
        }];
        assert!(start_rebase(&dir, "master", bad).is_err());

        let outcome = start_rebase(&dir, "master", ops).unwrap();
        assert!(matches!(outcome, RebaseOutcome::Success { .. }));

        // Chain: init <- m1 <- "s1 rewritten + s2" (squashed).
        let subjects = head_subjects(&dir, 3);
        assert_eq!(subjects.len(), 3);
        assert_eq!(subjects[2], "init");
        assert_eq!(subjects[1], "m1");
        assert!(subjects[0].contains("s1 rewritten"));
        // Squashed commit carries both messages.
        let repo = git2::Repository::open(&dir).unwrap();
        let head = repo.head().unwrap();
        let msg = head.peel_to_commit().unwrap().message().unwrap_or_default().to_string();
        assert!(msg.contains("s1 rewritten") && msg.contains("s2"));
        // Both files present (squash melded s2's tree).
        assert!(dir.join("s1.txt").exists() && dir.join("s2.txt").exists());

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Conflict mid-rebase: state persisted (restart-detectable); abort
    /// restores the branch completely.
    #[test]
    fn conflict_persists_state_and_abort_restores() {
        let dir = tmpdir("conflict");
        {
            let repo = git2::Repository::init(&dir).unwrap();
            commit_file(&repo, &dir, "a.txt", "base\n", "init");
            let head = repo.head().unwrap().peel_to_commit().unwrap();
            repo.branch("side", &head, false).unwrap();
            drop(head);
            commit_file(&repo, &dir, "a.txt", "master\n", "master change");
            drop(repo);
        }
        checkout(&dir, "side");
        let side_head;
        {
            let repo = git2::Repository::open(&dir).unwrap();
            commit_file(&repo, &dir, "a.txt", "side\n", "side change");
            side_head = repo.head().unwrap().target().unwrap().to_string();
            drop(repo);
        }

        let ops = list_rebase_commits(&dir, "master", None).unwrap();
        let outcome = start_rebase(&dir, "master", ops).unwrap();
        match outcome {
            RebaseOutcome::Conflict {
                files, position, total, ..
            } => {
                assert_eq!(files, vec!["a.txt".to_string()]);
                assert_eq!(position, 0);
                assert_eq!(total, 1);
            }
            other => panic!("expected Conflict, got {:?}", other),
        }

        // State persisted => restart detection works.
        let state = get_rebase_state(&dir).unwrap().expect("state persisted");
        assert_eq!(state.original_head, side_head);
        assert_eq!(state.position, 0);

        // Abort: branch fully restored.
        rebase_abort(&dir).unwrap();
        let repo = git2::Repository::open(&dir).unwrap();
        assert_eq!(repo.head().unwrap().target().unwrap().to_string(), side_head);
        drop(repo);
        assert_eq!(
            std::fs::read_to_string(dir.join("a.txt"))
                .unwrap()
                .replace("\r\n", "\n"),
            "side\n"
        );
        assert!(get_rebase_state(&dir).unwrap().is_none());

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Conflict -> resolve -> continue finishes the rebase; skip drops the op.
    #[test]
    fn conflict_continue_and_skip() {
        let dir = tmpdir("continue");
        {
            let repo = git2::Repository::init(&dir).unwrap();
            commit_file(&repo, &dir, "a.txt", "base\n", "init");
            let head = repo.head().unwrap().peel_to_commit().unwrap();
            repo.branch("side", &head, false).unwrap();
            drop(head);
            commit_file(&repo, &dir, "a.txt", "master\n", "master change");
            drop(repo);
        }
        checkout(&dir, "side");
        {
            let repo = git2::Repository::open(&dir).unwrap();
            commit_file(&repo, &dir, "a.txt", "side\n", "side change");
            commit_file(&repo, &dir, "s2.txt", "s2\n", "s2");
            drop(repo);
        }

        let ops = list_rebase_commits(&dir, "master", None).unwrap();
        let outcome = start_rebase(&dir, "master", ops).unwrap();
        assert!(matches!(outcome, RebaseOutcome::Conflict { position: 0, .. }));

        // Continue with unresolved conflicts -> structured error.
        assert!(rebase_continue(&dir).is_err());

        // Resolve, stage, continue: s1 commit lands, s2 follows, done.
        std::fs::write(dir.join("a.txt"), "resolved\n").unwrap();
        {
            let repo = git2::Repository::open(&dir).unwrap();
            let mut index = repo.index().unwrap();
            index.add_path(Path::new("a.txt")).unwrap();
            index.write().unwrap();
            drop(repo);
        }
        let outcome = rebase_continue(&dir).unwrap();
        assert!(matches!(outcome, RebaseOutcome::Success { .. }));
        let subjects = head_subjects(&dir, 4);
        assert_eq!(subjects, vec!["s2", "side change", "master change", "init"]);
        assert_eq!(
            std::fs::read_to_string(dir.join("a.txt"))
                .unwrap()
                .replace("\r\n", "\n"),
            "resolved\n"
        );

        // Skip scenario: rebase again onto a conflicting base and skip the op.
        {
            let repo = git2::Repository::open(&dir).unwrap();
            let head = repo.head().unwrap().peel_to_commit().unwrap();
            repo.branch("skipside", &head, false).unwrap();
            drop(head);
            drop(repo);
        }
        checkout(&dir, "skipside");
        {
            let repo = git2::Repository::open(&dir).unwrap();
            commit_file(&repo, &dir, "a.txt", "skip me\n", "skip candidate");
            drop(repo);
        }
        checkout(&dir, "master");
        {
            let repo = git2::Repository::open(&dir).unwrap();
            commit_file(&repo, &dir, "a.txt", "master again\n", "master again");
            drop(repo);
        }
        checkout(&dir, "skipside");

        let ops = list_rebase_commits(&dir, "master", None).unwrap();
        assert_eq!(ops.len(), 3); // side change / s2 / skip candidate
        let outcome = start_rebase(&dir, "master", ops).unwrap();
        assert!(matches!(outcome, RebaseOutcome::Conflict { position: 0, .. }));

        // Skip the first conflict; "s2" applies cleanly; the last op
        // conflicts again and is skipped as well.
        let outcome = rebase_skip(&dir).unwrap();
        assert!(matches!(outcome, RebaseOutcome::Conflict { position: 2, .. }));
        let outcome = rebase_skip(&dir).unwrap();
        assert!(matches!(outcome, RebaseOutcome::Success { .. }));

        // Skipped ops are gone: a.txt carries master's content; s2 applied.
        assert_eq!(
            std::fs::read_to_string(dir.join("a.txt"))
                .unwrap()
                .replace("\r\n", "\n"),
            "master again\n"
        );
        let subjects = head_subjects(&dir, 2);
        assert_eq!(subjects, vec!["s2", "master again"]);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
