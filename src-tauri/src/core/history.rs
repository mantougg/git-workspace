//! History operations (T-13): cherry-pick, revert, reset, and abort of an
//! in-progress pick/revert. All local libgit2 work (global constraint §3);
//! conflict outcomes keep the repository in a recoverable state
//! (CHERRY_PICK_HEAD / REVERT_HEAD preserved until abort, abort = hard reset
//! to the pre-operation HEAD).

use std::path::Path;

use serde::Serialize;

use crate::error::{AppError, AppResult};

/// PAF-10：cherry-pick / revert 冲突时持久化的操作前 HEAD
/// （`.git/gitworkspace-pick-base.json`）。ConflictResolver（含批量模式与
/// 应用重启后进入）拿不到 PickOutcome.baseOid，abort 时由后端兜底读取。
const PICK_BASE_FILE: &str = "gitworkspace-pick-base.json";

fn pick_base_path(repo: &git2::Repository) -> std::path::PathBuf {
    repo.path().join(PICK_BASE_FILE)
}

fn save_pick_base(repo: &git2::Repository, base_oid: &str) -> AppResult<()> {
    let raw = serde_json::json!({ "baseOid": base_oid }).to_string();
    std::fs::write(pick_base_path(repo), raw)?;
    Ok(())
}

/// 读取并清除持久化的 pick base（存在时）。
fn take_pick_base(repo: &git2::Repository) -> Option<String> {
    let path = pick_base_path(repo);
    let raw = std::fs::read_to_string(&path).ok()?;
    let oid = serde_json::from_str::<serde_json::Value>(&raw)
        .ok()?
        .get("baseOid")?
        .as_str()?
        .to_string();
    let _ = std::fs::remove_file(&path);
    Some(oid)
}

fn clear_pick_base(repo: &git2::Repository) {
    let _ = std::fs::remove_file(pick_base_path(repo));
}

/// PAF-10：脏工作区前置校验——已暂存 / 已跟踪文件的未提交变更会被随后的
/// hard reset / merge 静默吞掉，先拒绝并给出可行动提示。未跟踪新文件
/// （WT_NEW）不拦截，与 git 语义一致。
pub fn ensure_clean_worktree(repo: &git2::Repository, action: &str) -> AppResult<()> {
    let statuses = repo.statuses(None)?;
    let dirty: Vec<String> = statuses
        .iter()
        .filter(|e| {
            let s = e.status();
            s != git2::Status::CURRENT && !s.contains(git2::Status::WT_NEW)
        })
        .filter_map(|e| e.path().map(String::from))
        .collect();
    if dirty.is_empty() {
        return Ok(());
    }
    let preview = dirty.iter().take(5).cloned().collect::<Vec<_>>().join("、");
    Err(AppError::Conflict(format!(
        "{action} 前工作区存在未提交变更（{} 个文件：{}{}）。\
         请先提交，或 stash / 放弃这些改动后再试",
        dirty.len(),
        preview,
        if dirty.len() > 5 { " 等" } else { "" }
    )))
}

/// Outcome of a cherry-pick / revert operation.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum PickOutcome {
    /// All commits applied and committed.
    Success { picked: usize },
    /// A commit conflicted; the repo is left in cherry-pick/revert state
    /// with conflict markers, recoverable via `abort_pick`.
    #[serde(rename_all = "camelCase")]
    Conflict {
        /// Conflicted file paths.
        files: Vec<String>,
        /// The commit being applied when the conflict occurred.
        current: String,
        /// How many commits were already applied before the conflict.
        done: usize,
        total: usize,
        /// HEAD before the operation started (abort target).
        base_oid: Option<String>,
    },
}

/// Result of a reset operation.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResetResult {
    /// HEAD before the reset (recovery hint; reflog comes with T-14).
    pub previous_head: Option<String>,
    /// Resolved target oid the HEAD/index/worktree now points at.
    pub target: String,
    pub mode: String,
}

/// Cherry-pick one or more commits onto HEAD, in order.
/// Each successfully applied commit is committed immediately (original author
/// and message preserved, committer = repo signature). On conflict the repo
/// keeps CHERRY_PICK_HEAD and conflict markers so the user can resolve or
/// abort (T-16 wires the resolver UI).
pub fn cherry_pick(repo_path: &Path, oids: &[String]) -> AppResult<PickOutcome> {
    let repo = git2::Repository::open(repo_path)?;
    let base_oid = head_oid(&repo);
    let total = oids.len();

    for (idx, oid_str) in oids.iter().enumerate() {
        let oid =
            git2::Oid::from_str(oid_str).map_err(|_| AppError::NotFound(format!("commit '{}' not found", oid_str)))?;
        let commit = repo.find_commit(oid)?;
        repo.cherrypick(&commit, None)?;

        let mut index = repo.index()?;
        if index.has_conflicts() {
            // PAF-10：持久化操作前 HEAD，供 ConflictResolver 的 Abort（不传
            // baseOid 的路径）恢复到操作前状态。
            if let Some(base) = &base_oid {
                save_pick_base(&repo, base)?;
            }
            return Ok(PickOutcome::Conflict {
                files: conflict_paths(&index)?,
                current: oid_str.clone(),
                done: idx,
                total,
                base_oid: base_oid.clone(),
            });
        }

        // No conflicts: commit immediately, mirroring `git cherry-pick`.
        let tree_oid = index.write_tree()?;
        let tree = repo.find_tree(tree_oid)?;
        let sig = crate::core::signature_or_default(&repo)?;
        let parent = repo.head()?.peel_to_commit()?;
        repo.commit(
            Some("HEAD"),
            &commit.author(),
            &sig,
            commit.message().unwrap_or_default(),
            &tree,
            &[&parent],
        )?;
        repo.cleanup_state()?;
    }

    clear_pick_base(&repo);
    Ok(PickOutcome::Success { picked: total })
}

/// Revert a single commit, creating a revert commit on success./// On conflict the repo keeps REVERT_HEAD and conflict markers.
pub fn revert(repo_path: &Path, oid_str: &str) -> AppResult<PickOutcome> {
    let repo = git2::Repository::open(repo_path)?;
    let base_oid = head_oid(&repo);
    let oid =
        git2::Oid::from_str(oid_str).map_err(|_| AppError::NotFound(format!("commit '{}' not found", oid_str)))?;
    let commit = repo.find_commit(oid)?;
    repo.revert(&commit, None)?;

    let mut index = repo.index()?;
    if index.has_conflicts() {
        // PAF-10：持久化操作前 HEAD（同 cherry_pick）。
        if let Some(base) = &base_oid {
            save_pick_base(&repo, base)?;
        }
        return Ok(PickOutcome::Conflict {
            files: conflict_paths(&index)?,
            current: oid_str.to_string(),
            done: 0,
            total: 1,
            base_oid,
        });
    }

    let tree_oid = index.write_tree()?;
    let tree = repo.find_tree(tree_oid)?;
    let sig = crate::core::signature_or_default(&repo)?;
    let parent = repo.head()?.peel_to_commit()?;
    let message = format!(
        "Revert \"{}\"\n\nThis reverts commit {}.\n",
        commit.summary().unwrap_or_default(),
        oid
    );
    repo.commit(Some("HEAD"), &sig, &sig, &message, &tree, &[&parent])?;
    repo.cleanup_state()?;
    clear_pick_base(&repo);

    Ok(PickOutcome::Success { picked: 1 })
}

/// Reset HEAD to `target` (default HEAD) with soft / mixed / hard semantics.
/// Returns the previous HEAD oid so the UI can show a recovery hint.
pub fn reset_to(repo_path: &Path, target: Option<&str>, mode: &str) -> AppResult<ResetResult> {
    let repo = git2::Repository::open(repo_path)?;
    let previous_head = head_oid(&repo);

    let spec = target.unwrap_or("HEAD");
    let obj = repo
        .revparse_single(spec)
        .map_err(|_| AppError::NotFound(format!("revision '{}' not found", spec)))?;

    match mode {
        "soft" => repo.reset(&obj, git2::ResetType::Soft, None)?,
        "mixed" => repo.reset(&obj, git2::ResetType::Mixed, None)?,
        "hard" => {
            let mut co = git2::build::CheckoutBuilder::new();
            co.force();
            repo.reset(&obj, git2::ResetType::Hard, Some(&mut co))?;
        }
        other => {
            return Err(AppError::Other(format!(
                "invalid reset mode '{}' (soft | mixed | hard)",
                other
            )))
        }
    }

    Ok(ResetResult {
        previous_head,
        target: obj.id().to_string(),
        mode: mode.to_string(),
    })
}

/// Abort an in-progress cherry-pick / revert: hard reset to `base_oid`
/// (the pre-operation HEAD captured by the caller), falling back to the
/// persisted pick base (PAF-10, written when the conflict surfaced) or, as a
/// last resort, the current HEAD, then clear CHERRY_PICK_HEAD / REVERT_HEAD
/// state.
pub fn abort_pick(repo_path: &Path, base_oid: Option<&str>) -> AppResult<()> {
    let repo = git2::Repository::open(repo_path)?;
    let persisted = take_pick_base(&repo);
    let target_oid = match base_oid {
        Some(o) => git2::Oid::from_str(o).map_err(|_| AppError::NotFound(format!("commit '{}' not found", o)))?,
        None => match persisted {
            Some(o) => git2::Oid::from_str(&o).map_err(|_| AppError::Other("invalid persisted pick base".into()))?,
            None => repo
                .head()
                .and_then(|h| h.target().ok_or(git2::Error::from_str("HEAD has no target")))?,
        },
    };
    let obj = repo.find_object(target_oid, Some(git2::ObjectType::Commit))?;
    let mut co = git2::build::CheckoutBuilder::new();
    co.force();
    repo.reset(&obj, git2::ResetType::Hard, Some(&mut co))?;
    repo.cleanup_state()?;
    clear_pick_base(&repo);
    Ok(())
}

/// Currently conflicted files (empty when the repo is clean of conflicts).
/// Used by the UI to surface an in-progress conflict after reload/restart.
pub fn conflict_files(repo_path: &Path) -> AppResult<Vec<String>> {
    let repo = git2::Repository::open(repo_path)?;
    let index = repo.index()?;
    conflict_paths(&index)
}

/// GF-13c：从 `.git/CHERRY_PICK_HEAD` 取回被 pick 的原提交，用其 author
/// 作为 continue 提交的 author（committer 由调用方保持当前用户）——对齐
/// `git cherry-pick --continue` 语义：原作者保留在当前提交上，committer
/// 才是操作者。cherry_pick 的干净路径早已如此（`commit.author()` + 当前
/// committer），本函数补齐冲突恢复路径（此前 `&sig, &sig` 双当前签名，
/// 原作者丢失）。
///
/// CHERRY_PICK_HEAD 由 libgit2 cherrypick 冲突时写入（oid 字符串，见
/// libgit2 `cherrypick.c::write_cherrypick_head`）。文件缺失 / 内容非法 /
/// 提交不存在时返回 None，调用方回退当前签名（修复前行为）并记 warn：
/// 这是数据正确性问题，但不该把用户卡死在冲突状态。
fn cherry_pick_head_author(repo: &git2::Repository) -> Option<git2::Signature<'static>> {
    let raw = match std::fs::read_to_string(repo.path().join("CHERRY_PICK_HEAD")) {
        Ok(raw) => raw,
        Err(e) => {
            log::warn!(
                "pick_continue: 读取 CHERRY_PICK_HEAD 失败（{}），author 回退当前签名",
                e
            );
            return None;
        }
    };
    let oid = match git2::Oid::from_str(raw.trim()) {
        Ok(oid) => oid,
        Err(e) => {
            log::warn!(
                "pick_continue: CHERRY_PICK_HEAD 内容不是合法 oid（{:?}: {}），author 回退当前签名",
                raw.trim(),
                e
            );
            return None;
        }
    };
    let commit = match repo.find_commit(oid) {
        Ok(commit) => commit,
        Err(e) => {
            log::warn!(
                "pick_continue: CHERRY_PICK_HEAD 指向的提交 {} 不存在（{}），author 回退当前签名",
                oid,
                e
            );
            return None;
        }
    };
    let author = commit.author().to_owned();
    log::info!(
        "pick_continue: 保留 cherry-pick 原作者 {} <{}>",
        author.name().unwrap_or("(unnamed)"),
        author.email().unwrap_or("(no email)")
    );
    Some(author)
}

/// Continue an in-progress cherry-pick / revert after the user resolved the
/// conflicts (T-16): commits the staged resolution (message from MERGE_MSG)
/// and clears CHERRY_PICK_HEAD / REVERT_HEAD. Returns the new commit oid.
///
/// GF-13c: the resulting cherry-pick commit keeps the picked commit's author
/// (committer stays the current user); revert commits stay fully owned by
/// the current user (no original-author concept).
pub fn pick_continue(repo_path: &Path) -> AppResult<String> {
    let repo = git2::Repository::open(repo_path)?;
    let in_pick = repo.path().join("CHERRY_PICK_HEAD").exists();
    let in_revert = repo.path().join("REVERT_HEAD").exists();
    if !in_pick && !in_revert {
        return Err(AppError::Conflict("no cherry-pick / revert in progress".into()));
    }

    let mut index = repo.index()?;
    if index.has_conflicts() {
        return Err(AppError::Conflict("仍有未解决的冲突，请先解决后再继续".into()));
    }

    let tree_oid = index.write_tree()?;
    let tree = repo.find_tree(tree_oid)?;
    let sig = crate::core::signature_or_default(&repo)?;
    let parent = repo.head()?.peel_to_commit()?;
    let default_msg = std::fs::read_to_string(repo.path().join("MERGE_MSG"))
        .ok()
        .and_then(|m| m.lines().next().map(String::from))
        .unwrap_or_else(|| if in_pick { "cherry-pick" } else { "revert" }.to_string());

    // GF-13c：cherry-pick 的 continue 保留原 commit 的 author，committer
    // 保持当前用户（`git cherry-pick --continue` 语义）；revert 的新提交
    // author 即操作者（`git revert` 语义），无原作者概念，维持
    // author = committer = 当前签名。
    let pick_author = if in_pick {
        cherry_pick_head_author(&repo)
    } else {
        None
    };
    let oid = match pick_author {
        Some(author) => repo.commit(Some("HEAD"), &author, &sig, &default_msg, &tree, &[&parent])?,
        None => repo.commit(Some("HEAD"), &sig, &sig, &default_msg, &tree, &[&parent])?,
    };
    repo.cleanup_state()?;
    clear_pick_base(&repo);
    Ok(oid.to_string())
}

fn head_oid(repo: &git2::Repository) -> Option<String> {
    repo.head().ok().and_then(|h| h.target()).map(|o| o.to_string())
}

/// Collect unique conflicted paths from the index.
pub(crate) fn conflict_paths(index: &git2::Index) -> AppResult<Vec<String>> {
    let mut paths: Vec<String> = Vec::new();
    for conflict in index.conflicts()? {
        let conflict = conflict?;
        let entry = conflict.our.or(conflict.their).or(conflict.ancestor);
        if let Some(entry) = entry {
            let path = String::from_utf8_lossy(&entry.path).to_string();
            if !paths.contains(&path) {
                paths.push(path);
            }
        }
    }
    Ok(paths)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "gw_history_{}_{}",
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

    /// Same as `commit_file` but with an explicit author/committer signature.
    /// GF-13c 用它构造一个「作者 ≠ 当前用户」的被 pick 提交。
    fn commit_file_as(
        repo: &git2::Repository,
        dir: &Path,
        name: &str,
        content: &str,
        msg: &str,
        sig: &git2::Signature,
    ) -> String {
        std::fs::write(dir.join(name), content).unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new(name)).unwrap();
        index.write().unwrap();
        let tree_oid = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_oid).unwrap();
        let parent = repo
            .head()
            .ok()
            .and_then(|h| h.target())
            .map(|oid| repo.find_commit(oid).unwrap());
        let parents: Vec<&git2::Commit> = parent.iter().collect();
        repo.commit(Some("HEAD"), sig, sig, msg, &tree, &parents)
            .unwrap()
            .to_string()
    }

    fn init_repo(dir: &Path) -> git2::Repository {
        let repo = git2::Repository::init(dir).unwrap();
        commit_file(&repo, dir, "a.txt", "one\n", "init");
        repo
    }

    fn head_short(repo_path: &Path) -> String {
        let repo = git2::Repository::open(repo_path).unwrap();
        let c = repo.head().unwrap().peel_to_commit().unwrap();
        c.summary().unwrap_or_default().to_string()
    }

    /// Cherry-pick a commit from a side branch onto master: content applied,
    /// message + author preserved.
    #[test]
    fn cherry_pick_applies_commit() {
        let dir = tmpdir("pick");
        let side_oid;
        {
            let repo = init_repo(&dir);
            // Side branch with one extra commit.
            let head = repo.head().unwrap().peel_to_commit().unwrap();
            repo.branch("side", &head, false).unwrap();
            drop(head);
            drop(repo);
        }
        crate::core::branch::checkout_branch(&dir, "side").unwrap();
        {
            let repo = git2::Repository::open(&dir).unwrap();
            side_oid = commit_file(&repo, &dir, "side.txt", "side\n", "side commit");
            drop(repo);
        }
        crate::core::branch::checkout_branch(&dir, "master").unwrap();
        assert!(!dir.join("side.txt").exists());

        let outcome = cherry_pick(&dir, &[side_oid]).unwrap();
        match outcome {
            PickOutcome::Success { picked } => assert_eq!(picked, 1),
            _ => panic!("expected success"),
        }
        assert!(dir.join("side.txt").exists());
        assert_eq!(head_short(&dir), "side commit");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Revert undoes a commit's changes and creates a revert commit.
    #[test]
    fn revert_undoes_commit() {
        let dir = tmpdir("revert");
        let bad_oid;
        {
            let repo = init_repo(&dir);
            bad_oid = commit_file(&repo, &dir, "bad.txt", "bad\n", "bad commit");
            drop(repo);
        }
        assert!(dir.join("bad.txt").exists());

        let outcome = revert(&dir, &bad_oid).unwrap();
        assert!(matches!(outcome, PickOutcome::Success { picked: 1 }));
        assert!(!dir.join("bad.txt").exists());
        assert!(head_short(&dir).starts_with("Revert \"bad commit\""));

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Reset: soft keeps worktree + index; mixed keeps worktree, resets index;
    /// hard drops both and reports the previous HEAD.
    #[test]
    fn reset_modes_behave() {
        let dir = tmpdir("reset");
        let (init_oid, second_oid);
        {
            let repo = init_repo(&dir);
            init_oid = repo.head().unwrap().target().unwrap().to_string();
            second_oid = commit_file(&repo, &dir, "b.txt", "two\n", "second");
            drop(repo);
        }

        // soft back to init: HEAD moves, b.txt stays staged in the index.
        let r = reset_to(&dir, Some(&init_oid), "soft").unwrap();
        assert_eq!(r.previous_head.as_deref(), Some(second_oid.as_str()));
        {
            let repo = git2::Repository::open(&dir).unwrap();
            let mut opts = git2::StatusOptions::new();
            opts.include_untracked(false);
            let statuses = repo.statuses(Some(&mut opts)).unwrap();
            assert!(
                statuses.iter().any(|e| e.status().contains(git2::Status::INDEX_NEW)),
                "soft reset must keep b.txt staged"
            );
            drop(statuses);
            drop(repo);
        }

        // hard back to init: worktree file gone; previous head = init_oid
        // (HEAD already moved to init by the soft reset above).
        let r = reset_to(&dir, Some(&init_oid), "hard").unwrap();
        assert_eq!(r.previous_head.as_deref(), Some(init_oid.as_str()));
        assert!(!dir.join("b.txt").exists());

        // Invalid mode is a structured error.
        assert!(reset_to(&dir, Some(&init_oid), "nuke").is_err());

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A conflicting cherry-pick reports conflicted files and abort restores
    /// the pre-pick state completely.
    #[test]
    fn cherry_pick_conflict_then_abort_restores() {
        let dir = tmpdir("conflict");
        let side_oid;
        let master_head;
        {
            let repo = init_repo(&dir);
            let head = repo.head().unwrap().peel_to_commit().unwrap();
            repo.branch("side", &head, false).unwrap();
            drop(head);
            // Both branches change a.txt differently.
            commit_file(&repo, &dir, "a.txt", "master line\n", "master change");
            drop(repo);
        }
        {
            let repo = git2::Repository::open(&dir).unwrap();
            master_head = repo.head().unwrap().target().unwrap().to_string();
            drop(repo);
        }
        crate::core::branch::checkout_branch(&dir, "side").unwrap();
        {
            let repo = git2::Repository::open(&dir).unwrap();
            side_oid = commit_file(&repo, &dir, "a.txt", "side line\n", "side change");
            drop(repo);
        }
        crate::core::branch::checkout_branch(&dir, "master").unwrap();

        let outcome = cherry_pick(&dir, &[side_oid]).unwrap();
        match outcome {
            PickOutcome::Conflict {
                files,
                base_oid,
                done,
                total,
                ..
            } => {
                assert_eq!(files, vec!["a.txt".to_string()]);
                assert_eq!(base_oid.as_deref(), Some(master_head.as_str()));
                assert_eq!(done, 0);
                assert_eq!(total, 1);
            }
            _ => panic!("expected conflict"),
        }
        // Repo is in cherry-pick state with conflicts visible.
        assert_eq!(conflict_files(&dir).unwrap(), vec!["a.txt".to_string()]);

        // Abort: worktree + HEAD fully restored.
        abort_pick(&dir, Some(&master_head)).unwrap();
        assert_eq!(conflict_files(&dir).unwrap().len(), 0);
        assert_eq!(head_short(&dir), "master change");
        // Normalize EOL: checkout may apply CRLF depending on host autocrlf.
        assert_eq!(
            std::fs::read_to_string(dir.join("a.txt"))
                .unwrap()
                .replace("\r\n", "\n"),
            "master line\n"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Cherry-pick conflict -> resolve -> pick_continue creates the pick
    /// commit and clears the state (T-16 resolver Continue path).
    #[test]
    fn cherry_pick_conflict_then_continue_commits() {
        let dir = tmpdir("pick_continue");
        let side_oid;
        {
            let repo = git2::Repository::init(&dir).unwrap();
            commit_file(&repo, &dir, "a.txt", "base\n", "init");
            let head = repo.head().unwrap().peel_to_commit().unwrap();
            repo.branch("side", &head, false).unwrap();
            drop(head);
            commit_file(&repo, &dir, "a.txt", "master\n", "master change");
            drop(repo);
        }
        crate::core::branch::checkout_branch(&dir, "side").unwrap();
        {
            let repo = git2::Repository::open(&dir).unwrap();
            // GF-13c：被 pick 的提交作者与后续 committer（当前用户）刻意区分，
            // 才能验证 continue 后 author/committer 各自归属。
            let original_author = git2::Signature::now("original Author", "original@example.com").unwrap();
            side_oid = commit_file_as(&repo, &dir, "a.txt", "side\n", "side change", &original_author);
            drop(repo);
        }
        crate::core::branch::checkout_branch(&dir, "master").unwrap();

        let outcome = cherry_pick(&dir, &[side_oid]).unwrap();
        assert!(matches!(outcome, PickOutcome::Conflict { .. }));
        // Continue while unresolved -> structured error.
        assert!(pick_continue(&dir).is_err());

        // Resolve + stage, then continue.
        std::fs::write(dir.join("a.txt"), "resolved\n").unwrap();
        {
            let repo = git2::Repository::open(&dir).unwrap();
            let mut index = repo.index().unwrap();
            index.add_path(Path::new("a.txt")).unwrap();
            index.write().unwrap();
            // committer = 当前用户：写本地 git config，让 signature_or_default
            // 的取值与测试机全局配置解耦（确定性断言）。
            let mut config = git2::Config::open(&repo.path().join("config")).unwrap();
            config.set_str("user.name", "current-user").unwrap();
            config.set_str("user.email", "current-user@example.com").unwrap();
            drop(repo);
        }
        let oid = pick_continue(&dir).unwrap();
        assert!(!oid.is_empty());
        assert_eq!(head_short(&dir), "side change");
        assert_eq!(conflict_files(&dir).unwrap().len(), 0);
        // CHERRY_PICK_HEAD cleared.
        let repo = git2::Repository::open(&dir).unwrap();
        assert!(!repo.path().join("CHERRY_PICK_HEAD").exists());

        // GF-13c 验收（`git log --format='%an %cn'` 等价断言）：cherry-pick
        // continue 后 author = 被 pick 提交的原作者，committer = 当前用户。
        let head = repo.head().unwrap().peel_to_commit().unwrap();
        assert_eq!(head.author().name(), Some("original Author"));
        assert_eq!(head.author().email(), Some("original@example.com"));
        assert_eq!(head.committer().name(), Some("current-user"));
        assert_eq!(head.committer().email(), Some("current-user@example.com"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// PAF-10：多 commit cherry-pick 中途冲突 → abort **不带 base_oid**
    /// 也恢复到操作前 HEAD（持久化 pick base 兜底，ConflictResolver /
    /// 重启后场景拿不到 PickOutcome.baseOid）。
    #[test]
    fn multi_pick_abort_without_base_restores_original_head() {
        let dir = tmpdir("pick_base");
        let base_head;
        {
            let repo = git2::Repository::init(&dir).unwrap();
            commit_file(&repo, &dir, "a.txt", "base\n", "init");
            let head = repo.head().unwrap().peel_to_commit().unwrap();
            repo.branch("side", &head, false).unwrap();
            drop(head);
            commit_file(&repo, &dir, "a.txt", "master\n", "master change");
            base_head = repo.head().unwrap().target().unwrap().to_string();
            drop(repo);
        }
        crate::core::branch::checkout_branch(&dir, "side").unwrap();
        let (c1, c2);
        {
            let repo = git2::Repository::open(&dir).unwrap();
            // c1 干净落地（新文件），c2 与 master 的 a.txt 冲突。
            c1 = commit_file(&repo, &dir, "b.txt", "b\n", "side b");
            c2 = commit_file(&repo, &dir, "a.txt", "side\n", "side a");
            drop(repo);
        }
        crate::core::branch::checkout_branch(&dir, "master").unwrap();

        let outcome = cherry_pick(&dir, &[c1, c2]).unwrap();
        match &outcome {
            PickOutcome::Conflict { done, total, base_oid, .. } => {
                assert_eq!(*done, 1);
                assert_eq!(*total, 2);
                assert_eq!(base_oid.as_deref(), Some(base_head.as_str()));
            }
            other => panic!("expected Conflict, got {:?}", other),
        }

        // 不带 base_oid 的 abort（ConflictResolver 路径）：恢复到操作前 HEAD。
        abort_pick(&dir, None).unwrap();
        let repo = git2::Repository::open(&dir).unwrap();
        assert_eq!(repo.head().unwrap().target().unwrap().to_string(), base_head);
        drop(repo);
        // c1 已落地的提交被回退：b.txt 消失，a.txt 回到 master 内容。
        assert!(!dir.join("b.txt").exists());
        assert_eq!(
            std::fs::read_to_string(dir.join("a.txt"))
                .unwrap()
                .replace("\r\n", "\n"),
            "master\n"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
