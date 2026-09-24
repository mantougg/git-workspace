//! Operation log 单元测试（同父模块 tests.rs，可访问私有成员）。
//! 覆盖：记录/查询全维度 roundtrip、Checkout All / Delete Branch All /
//! Reset / Rebase 的 Undo 闭环、状态漂移与脏工作区拒绝、快照助手。

use super::*;
use std::path::Path;

use chrono::Utc;
use rusqlite::{params, Connection};

use super::record::{insert_operation_log, resolve_workspace_id};
use crate::core::branch;
use crate::core::history::PickOutcome;
use crate::core::merge::MergeOutcome;

fn tmpdir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "gw_oplog_{}_{}",
        tag,
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn open_db() -> Connection {
    let mut conn = Connection::open_in_memory().unwrap();
    crate::db::init_db(&mut conn).unwrap();
    conn
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
    repo.commit(Some("HEAD"), &sig, &sig, msg, &tree, &parents).unwrap();
}

fn init_repo(dir: &Path) -> git2::Repository {
    let repo = git2::Repository::init(dir).unwrap();
    commit_file(&repo, dir, "a.txt", "one\n", "c1");
    repo
}

fn head_tip(dir: &Path) -> String {
    git2::Repository::open(dir)
        .unwrap()
        .head()
        .unwrap()
        .target()
        .unwrap()
        .to_string()
}

fn item(repo: &Path, ref_name: &str, before: &str, after: Option<&str>) -> NewOperationLogItem {
    NewOperationLogItem {
        repo_path: repo.to_string_lossy().to_string(),
        ref_name: ref_name.to_string(),
        before_oid: before.to_string(),
        after_oid: after.map(String::from),
        detail: None,
    }
}

/// Item with an explicit `detail` (op-specific snapshot).
fn item_detail(repo: &Path, ref_name: &str, before: &str, after: Option<&str>, detail: &str) -> NewOperationLogItem {
    NewOperationLogItem {
        detail: Some(detail.to_string()),
        ..item(repo, ref_name, before, after)
    }
}

/// A repo with a git identity configured (stash_save needs a stasher
/// signature, which falls back to repo config).
fn init_repo_with_identity(dir: &Path) -> git2::Repository {
    let repo = init_repo(dir);
    repo.config().unwrap().set_str("user.name", "tester").unwrap();
    repo.config().unwrap().set_str("user.email", "t@example.com").unwrap();
    repo
}

/// Insert + query roundtrip with every filter dimension, workspace
/// resolution, and detail loading (acceptance: 日志可按仓库/时间/类型查询).
#[test]
fn log_insert_and_query_roundtrip() {
    let mut conn = open_db();
    conn.execute(
        "INSERT INTO workspaces (name, path, created_at, updated_at) VALUES ('w', 'D:/w', 't', 't')",
        [],
    )
    .unwrap();
    let ws_id = conn.last_insert_rowid();
    conn.execute(
        "INSERT INTO repositories (workspace_id, path, name, relative_path, created_at, updated_at)
         VALUES (?1, 'D:/w/repo-a', 'repo-a', 'repo-a', 't', 't')",
        params![ws_id],
    )
    .unwrap();
    assert_eq!(resolve_workspace_id(&conn, "D:/w/repo-a"), Some(ws_id));
    assert_eq!(resolve_workspace_id(&conn, "D:/nowhere"), None);

    let items = vec![
        NewOperationLogItem {
            repo_path: "D:/w/repo-a".into(),
            ref_name: "main".into(),
            before_oid: "a".repeat(40),
            after_oid: Some("b".repeat(40)),
            detail: Some("mode:hard".into()),
        },
        NewOperationLogItem {
            repo_path: "D:/w/repo-b".into(),
            ref_name: "dev".into(),
            before_oid: "c".repeat(40),
            after_oid: None,
            detail: None,
        },
    ];
    let log_id = insert_operation_log(&mut conn, Some(ws_id), OP_RESET, "reset --hard", &items).unwrap();

    // Unfiltered page.
    let page = query_operation_logs(&conn, &LogFilter::default(), 50, 0).unwrap();
    assert_eq!(page.total, 1);
    assert_eq!(page.logs[0].repo_count, 2);
    assert_eq!(page.logs[0].undone_count, 0);
    assert_eq!(page.logs[0].workspace_id, Some(ws_id));

    // Filter by workspace / op_type / repo substring.
    let f = LogFilter {
        workspace_id: Some(ws_id),
        ..Default::default()
    };
    assert_eq!(query_operation_logs(&conn, &f, 50, 0).unwrap().total, 1);
    let f = LogFilter {
        workspace_id: Some(ws_id + 9),
        ..Default::default()
    };
    assert_eq!(query_operation_logs(&conn, &f, 50, 0).unwrap().total, 0);
    let f = LogFilter {
        op_type: Some(OP_RESET),
        ..Default::default()
    };
    assert_eq!(query_operation_logs(&conn, &f, 50, 0).unwrap().total, 1);
    let f = LogFilter {
        op_type: Some(OP_REBASE),
        ..Default::default()
    };
    assert_eq!(query_operation_logs(&conn, &f, 50, 0).unwrap().total, 0);
    let f = LogFilter {
        repo_path: Some("repo-b"),
        ..Default::default()
    };
    assert_eq!(query_operation_logs(&conn, &f, 50, 0).unwrap().total, 1);
    let f = LogFilter {
        repo_path: Some("nope"),
        ..Default::default()
    };
    assert_eq!(query_operation_logs(&conn, &f, 50, 0).unwrap().total, 0);

    // Date filter against the UTC date part of created_at.
    let today = Utc::now().format("%Y-%m-%d").to_string();
    let f = LogFilter {
        date_from: Some(&today),
        date_to: Some(&today),
        ..Default::default()
    };
    assert_eq!(query_operation_logs(&conn, &f, 50, 0).unwrap().total, 1);
    let f = LogFilter {
        date_from: Some("1999-01-01"),
        date_to: Some("1999-01-02"),
        ..Default::default()
    };
    assert_eq!(query_operation_logs(&conn, &f, 50, 0).unwrap().total, 0);

    // Detail loads all items in order.
    let detail = get_operation_log(&conn, log_id).unwrap();
    assert_eq!(detail.items.len(), 2);
    assert_eq!(detail.items[0].ref_name, "main");
    assert_eq!(detail.items[1].after_oid, None);
    assert!(get_operation_log(&conn, log_id + 99).is_err());

    // Empty items are rejected.
    assert!(insert_operation_log(&mut conn, None, OP_RESET, "x", &[]).is_err());
}

/// Checkout All undo: HEAD returns to the recorded branch, and the
/// undo marks items + log as undone (acceptance: 一键撤销恢复操作前状态).
#[test]
fn undo_checkout_restores_original_branch() {
    let dir = tmpdir("checkout");
    let master_tip = {
        let repo = init_repo(&dir);
        let tip = repo.head().unwrap().target().unwrap().to_string();
        drop(repo);
        branch::create_branch(&dir, "feature", None).unwrap();
        tip
    };
    // Simulate the batch op: repo was on master, now checked out to feature.
    branch::checkout_branch(&dir, "feature").unwrap();

    let mut conn = open_db();
    let log_id = insert_operation_log(
        &mut conn,
        None,
        OP_CHECKOUT_ALL,
        "批量检出 'feature'",
        &[item(&dir, "master", &master_tip, None)],
    )
    .unwrap();
    let detail = get_operation_log(&conn, log_id).unwrap();

    // Preview says the reverse op is safe.
    let preview = preview_undo(&detail);
    assert_eq!(preview.len(), 1);
    assert!(preview[0].ok, "{}", preview[0].message);
    assert!(preview[0].action.contains("master"));

    let results = run_undo(&detail);
    assert!(results[0].success, "{}", results[0].message);
    let repo = git2::Repository::open(&dir).unwrap();
    let head = repo.head().unwrap();
    assert_eq!(head.shorthand(), Some("master"));
    drop(head);
    drop(repo);

    // Persisting marks the item and, with nothing pending, the whole log.
    let fully = persist_undo_results(&mut conn, log_id, &results).unwrap();
    assert!(fully);
    let detail = get_operation_log(&conn, log_id).unwrap();
    assert!(detail.undone_at.is_some());
    assert!(detail.items.iter().all(|i| i.undone_at.is_some()));

    // A second run is a graceful no-op.
    let results = run_undo(&detail);
    assert!(results[0].success);
    assert!(results[0].message.contains("此前已撤销"));

    let _ = std::fs::remove_dir_all(&dir);
}

/// Delete Branch All undo: the branch is recreated at its recorded tip.
#[test]
fn undo_delete_recreates_branch_at_tip() {
    let dir = tmpdir("delete");
    {
        let repo = init_repo(&dir);
        drop(repo);
    }
    branch::create_branch(&dir, "work", None).unwrap();
    branch::checkout_branch(&dir, "work").unwrap();
    {
        let repo = git2::Repository::open(&dir).unwrap();
        commit_file(&repo, &dir, "b.txt", "two\n", "work commit");
        drop(repo);
    }
    let work_tip = head_tip(&dir);
    branch::checkout_branch(&dir, "master").unwrap();
    branch::delete_branch(&dir, "work", true).unwrap();

    let mut conn = open_db();
    let log_id = insert_operation_log(
        &mut conn,
        None,
        OP_DELETE_BRANCH_ALL,
        "批量删除分支 'work'",
        &[item(&dir, "work", &work_tip, None)],
    )
    .unwrap();
    let detail = get_operation_log(&conn, log_id).unwrap();

    let preview = preview_undo(&detail);
    assert!(preview[0].ok, "{}", preview[0].message);

    let results = run_undo(&detail);
    assert!(results[0].success, "{}", results[0].message);
    {
        let repo = git2::Repository::open(&dir).unwrap();
        let b = repo.find_branch("work", git2::BranchType::Local).unwrap();
        assert_eq!(b.get().target().unwrap().to_string(), work_tip);
    }

    // Undo again (results not yet persisted): the branch now exists, so
    // the safety check refuses to clobber it.
    let results = run_undo(&get_operation_log(&conn, log_id).unwrap());
    assert!(!results[0].success);
    assert!(results[0].message.contains("已存在"), "{}", results[0].message);

    let _ = std::fs::remove_dir_all(&dir);
}

/// Read a text file with CRLF normalized — the Windows CI git config
/// (core.autocrlf) rewrites checked-out files.
fn read_norm(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap().replace("\r\n", "\n")
}

/// Reset --hard undo: the branch ref rolls back and the worktree files
/// are restored to the before-state (acceptance: ref 回退可验证).
#[test]
fn undo_hard_reset_restores_tip_and_files() {
    let dir = tmpdir("reset");
    let (c1, c2) = {
        let repo = init_repo(&dir);
        let c1 = repo.head().unwrap().target().unwrap().to_string();
        commit_file(&repo, &dir, "a.txt", "two\n", "c2");
        let c2 = repo.head().unwrap().target().unwrap().to_string();
        drop(repo);
        (c1, c2)
    };
    // The op: reset --hard to c1 (before = c2, after = c1).
    crate::core::history::reset_to(&dir, Some(&c1), "hard").unwrap();
    assert_eq!(read_norm(&dir.join("a.txt")), "one\n");

    let mut conn = open_db();
    let mut it = item(&dir, "master", &c2, Some(&c1));
    it.detail = Some("mode:hard".into());
    let log_id = insert_operation_log(&mut conn, None, OP_RESET, "reset --hard", &[it]).unwrap();
    let detail = get_operation_log(&conn, log_id).unwrap();

    let results = run_undo(&detail);
    assert!(results[0].success, "{}", results[0].message);
    assert_eq!(head_tip(&dir), c2);
    assert_eq!(read_norm(&dir.join("a.txt")), "two\n");

    let fully = persist_undo_results(&mut conn, log_id, &results).unwrap();
    assert!(fully);

    let _ = std::fs::remove_dir_all(&dir);
}

/// Safety (§46): undo refuses repos whose ref moved on after the op, and
/// hard rollbacks refuse a dirty worktree; the log stays un-undone.
#[test]
fn undo_refuses_when_state_moved_on() {
    let dir = tmpdir("moved");
    let (c1, c2) = {
        let repo = init_repo(&dir);
        let c1 = repo.head().unwrap().target().unwrap().to_string();
        commit_file(&repo, &dir, "a.txt", "two\n", "c2");
        let c2 = repo.head().unwrap().target().unwrap().to_string();
        drop(repo);
        (c1, c2)
    };
    crate::core::history::reset_to(&dir, Some(&c1), "hard").unwrap();

    let mut conn = open_db();
    let mut it = item(&dir, "master", &c2, Some(&c1));
    it.detail = Some("mode:hard".into());
    let log_id = insert_operation_log(&mut conn, None, OP_RESET, "reset --hard", &[it]).unwrap();

    // Case 1: a later commit on top of the reset target -> refuse.
    {
        let repo = git2::Repository::open(&dir).unwrap();
        commit_file(&repo, &dir, "a.txt", "three\n", "c3");
        drop(repo);
    }
    let detail = get_operation_log(&conn, log_id).unwrap();
    let preview = preview_undo(&detail);
    assert!(!preview[0].ok);
    assert!(preview[0].message.contains("后续变更"), "{}", preview[0].message);
    let results = run_undo(&detail);
    assert!(!results[0].success);
    let fully = persist_undo_results(&mut conn, log_id, &results).unwrap();
    assert!(!fully, "failed items keep the log un-undone");
    assert!(get_operation_log(&conn, log_id).unwrap().undone_at.is_none());

    // Case 2: back at the after-oid but with a dirty worktree (hard undo
    // would lose those edits) -> refuse.
    crate::core::history::reset_to(&dir, Some(&c1), "hard").unwrap();
    std::fs::write(dir.join("dirty.txt"), "x\n").unwrap();
    let detail = get_operation_log(&conn, log_id).unwrap();
    let preview = preview_undo(&detail);
    assert!(!preview[0].ok);
    assert!(preview[0].message.contains("未提交变更"), "{}", preview[0].message);

    let _ = std::fs::remove_dir_all(&dir);
}

/// Rebase undo: hard rollback to the pre-rebase head; refused while a
/// rebase is still in progress.
#[test]
fn undo_rebase_rolls_back_and_respects_in_progress() {
    let dir = tmpdir("rebase");
    let (c1, c2) = {
        let repo = init_repo(&dir);
        let c1 = repo.head().unwrap().target().unwrap().to_string();
        commit_file(&repo, &dir, "a.txt", "two\n", "c2");
        let c2 = repo.head().unwrap().target().unwrap().to_string();
        drop(repo);
        (c1, c2)
    };

    let mut conn = open_db();
    let mut it = item(&dir, "master", &c1, Some(&c2));
    it.detail = Some("onto:upstream".into());
    let log_id = insert_operation_log(&mut conn, None, OP_REBASE, "rebase → upstream", &[it]).unwrap();

    // In-progress rebase state blocks the undo (rebase_abort owns recovery).
    let state = serde_json::json!({
        "originalHead": c1, "onto": "upstream", "ops": [], "position": 0, "prevCommit": c1
    });
    std::fs::write(dir.join(".git").join("gitworkspace-rebase.json"), state.to_string()).unwrap();
    let detail = get_operation_log(&conn, log_id).unwrap();
    let preview = preview_undo(&detail);
    assert!(!preview[0].ok);
    assert!(preview[0].message.contains("rebase"), "{}", preview[0].message);
    std::fs::remove_file(dir.join(".git").join("gitworkspace-rebase.json")).unwrap();

    // Clean state at the after-oid: rollback to c1 succeeds.
    let detail = get_operation_log(&conn, log_id).unwrap();
    let results = run_undo(&detail);
    assert!(results[0].success, "{}", results[0].message);
    assert_eq!(head_tip(&dir), c1);
    assert_eq!(read_norm(&dir.join("a.txt")), "one\n");

    let _ = std::fs::remove_dir_all(&dir);
}

    /// Snapshot helpers: branch tips and HEAD (branch + detached forms).
    #[test]
    fn snapshots_capture_ref_and_oid() {
        let dir = tmpdir("snapshot");
    let tip = {
        let repo = init_repo(&dir);
        let tip = repo.head().unwrap().target().unwrap().to_string();
        drop(repo);
        tip
    };
    assert_eq!(snapshot_head(&dir), Some(("master".to_string(), tip.clone())));
    assert_eq!(
        snapshot_branch(&dir, "master"),
        Some(("master".to_string(), tip.clone()))
    );
    assert_eq!(snapshot_branch(&dir, "missing"), None);

    // Detached HEAD: empty ref_name, oid preserved.
    {
        let repo = git2::Repository::open(&dir).unwrap();
        let oid = git2::Oid::from_str(&tip).unwrap();
        repo.set_head_detached(oid).unwrap();
        drop(repo);
    }
    assert_eq!(snapshot_head(&dir), Some((String::new(), tip)));

    // Unborn HEAD: not loggable.
    let empty = tmpdir("unborn");
    {
        let repo = git2::Repository::init(&empty).unwrap();
        drop(repo);
    }
    assert_eq!(snapshot_head(&empty), None);

    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&empty);
}

    /// GF-16: cherry-pick undo hard-rolls the branch back to the pre-pick oid;
    /// an in-progress pick/revert refuses the rollback (abort_pick owns it).
    #[test]
    fn undo_cherry_pick_rolls_back_head() {
        let dir = tmpdir("pick");
        let (c1, c2) = {
            let repo = init_repo(&dir);
            let c1 = repo.head().unwrap().target().unwrap().to_string();
            commit_file(&repo, &dir, "b.txt", "two\n", "c2");
            let c2 = repo.head().unwrap().target().unwrap().to_string();
            drop(repo);
            (c1, c2)
        };
        branch::create_branch(&dir, "side", Some(&c1)).unwrap();
        branch::checkout_branch(&dir, "side").unwrap();
        let outcome = crate::core::history::cherry_pick(&dir, &[c2.clone()]).unwrap();
        assert!(matches!(outcome, PickOutcome::Success { .. }), "{outcome:?}");
        let after = head_tip(&dir);
        assert_ne!(after, c1);
        assert!(dir.join("b.txt").exists());

        let mut conn = open_db();
        let log_id = insert_operation_log(
            &mut conn,
            None,
            OP_CHERRY_PICK,
            "cherry-pick c2",
            &[item_detail(&dir, "side", &c1, Some(&after), "picked:1")],
        )
        .unwrap();
        let detail = get_operation_log(&conn, log_id).unwrap();

        let preview = preview_undo(&detail);
        assert!(preview[0].ok, "{}", preview[0].message);
        assert!(preview[0].action.contains("cherry-pick"), "{}", preview[0].action);

        let results = run_undo(&detail);
        assert!(results[0].success, "{}", results[0].message);
        assert_eq!(head_tip(&dir), c1);
        assert!(!dir.join("b.txt").exists(), "hard rollback removes the picked file");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Safety: a cherry-pick in progress refuses the rollback.
    #[test]
    fn undo_cherry_pick_refuses_while_pick_in_progress() {
        let dir = tmpdir("pick_inprogress");
        let (c1, after) = {
            let repo = init_repo(&dir);
            let c1 = repo.head().unwrap().target().unwrap().to_string();
            commit_file(&repo, &dir, "b.txt", "two\n", "c2");
            let after = repo.head().unwrap().target().unwrap().to_string();
            drop(repo);
            (c1, after)
        };
        std::fs::write(dir.join(".git").join("CHERRY_PICK_HEAD"), format!("{after}\n")).unwrap();

        let mut conn = open_db();
        let log_id = insert_operation_log(
            &mut conn,
            None,
            OP_CHERRY_PICK,
            "cherry-pick c2",
            &[item(&dir, "master", &c1, Some(&after))],
        )
        .unwrap();
        let detail = get_operation_log(&conn, log_id).unwrap();
        let preview = preview_undo(&detail);
        assert!(!preview[0].ok);
        assert!(preview[0].message.contains("cherry-pick"), "{}", preview[0].message);
        std::fs::remove_file(dir.join(".git").join("CHERRY_PICK_HEAD")).unwrap();

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// GF-16: merge-abort undo re-runs the merge and restores the exact
    /// conflict state; a HEAD that moved on since the abort is refused.
    #[test]
    fn undo_merge_abort_reruns_merge_and_guards_state() {
        let dir = tmpdir("mergeabort");
        {
            let repo = init_repo(&dir);
            let head = repo.head().unwrap().peel_to_commit().unwrap();
            repo.branch("side", &head, false).unwrap();
            drop(head);
            commit_file(&repo, &dir, "a.txt", "ours\n", "master change");
            drop(repo);
        };
        branch::checkout_branch(&dir, "side").unwrap();
        {
            let repo = git2::Repository::open(&dir).unwrap();
            commit_file(&repo, &dir, "a.txt", "theirs\n", "side change");
            drop(repo);
        }
        branch::checkout_branch(&dir, "master").unwrap();
        let outcome = crate::core::merge::merge(&dir, "side", "normal").unwrap();
        assert!(matches!(outcome, MergeOutcome::Conflict { .. }));
        let merge_head = crate::core::merge::merge_head_oid(&dir).expect("MERGE_HEAD readable");
        let head_oid = head_tip(&dir);

        crate::core::merge::merge_abort(&dir).unwrap();
        assert!(!dir.join(".git").join("MERGE_HEAD").exists());

        let mut conn = open_db();
        let log_id = insert_operation_log(
            &mut conn,
            None,
            OP_MERGE_ABORT,
            "中止 merge",
            &[item_detail(
                &dir,
                "master",
                &head_oid,
                Some(&head_oid),
                &format!("mergehead:{merge_head}"),
            )],
        )
        .unwrap();
        let detail = get_operation_log(&conn, log_id).unwrap();

        let preview = preview_undo(&detail);
        assert!(preview[0].ok, "{}", preview[0].message);
        assert!(preview[0].action.contains("重新合并"), "{}", preview[0].action);

        let results = run_undo(&detail);
        assert!(results[0].success, "{}", results[0].message);
        assert!(dir.join(".git").join("MERGE_HEAD").exists(), "merge state restored");
        let markers = std::fs::read_to_string(dir.join("a.txt")).unwrap();
        assert!(markers.contains("<<<<<<<") && markers.contains(">>>>>>>"), "{markers}");
        assert_eq!(head_tip(&dir), head_oid, "HEAD stays where the abort left it");

        // Move on (abort + new commit) -> the re-merge is refused.
        crate::core::merge::merge_abort(&dir).unwrap();
        {
            let repo = git2::Repository::open(&dir).unwrap();
            commit_file(&repo, &dir, "a.txt", "moved on\n", "c3");
            drop(repo);
        }
        let detail = get_operation_log(&conn, log_id).unwrap();
        let preview = preview_undo(&detail);
        assert!(!preview[0].ok);
        assert!(preview[0].message.contains("后续变更"), "{}", preview[0].message);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// GF-16: stash drop / clear undo restores the recorded stack exactly.
    #[test]
    fn undo_stash_drop_and_clear_restore_stack() {
        let dir = tmpdir("stashundo");
        {
            let repo = init_repo_with_identity(&dir);
            drop(repo);
        }
        std::fs::write(dir.join("a.txt"), "one\nfirst\n").unwrap();
        crate::core::stash::stash_save(&dir, Some("first"), false).unwrap();
        std::fs::write(dir.join("b.txt"), "b\nsecond\n").unwrap();
        crate::core::stash::stash_save(&dir, Some("second"), true).unwrap();
        let stack = crate::core::stash::snapshot_stash_stack(&dir);
        assert_eq!(stack.len(), 2);

        let mut conn = open_db();

        // Drop the newest entry, undo it.
        crate::core::stash::stash_drop(&dir, 0).unwrap();
        let log_id = insert_operation_log(
            &mut conn,
            None,
            OP_STASH_DROP,
            "丢弃 stash@{0}",
            &[item_detail(
                &dir,
                "stash",
                &stack[0].0,
                None,
                &encode_stash_snapshot(&stack),
            )],
        )
        .unwrap();
        let detail = get_operation_log(&conn, log_id).unwrap();
        let preview = preview_undo(&detail);
        assert!(preview[0].ok, "{}", preview[0].message);
        assert!(preview[0].action.contains("恢复 2 条 stash 记录"), "{}", preview[0].action);
        let results = run_undo(&detail);
        assert!(results[0].success, "{}", results[0].message);
        assert_eq!(
            crate::core::stash::snapshot_stash_stack(&dir),
            stack,
            "drop+undo restores the exact stack"
        );

        // Clear the whole stack, undo it.
        crate::core::stash::stash_clear(&dir).unwrap();
        let log_id = insert_operation_log(
            &mut conn,
            None,
            OP_STASH_CLEAR,
            "清空 stash 栈（2 条记录）",
            &[item_detail(
                &dir,
                "stash",
                &stack[0].0,
                None,
                &encode_stash_snapshot(&stack),
            )],
        )
        .unwrap();
        let detail = get_operation_log(&conn, log_id).unwrap();
        let results = run_undo(&detail);
        assert!(results[0].success, "{}", results[0].message);
        assert_eq!(
            crate::core::stash::snapshot_stash_stack(&dir),
            stack,
            "clear+undo restores the exact stack"
        );

        // A GC-pruned entry is reported, not silently lost.
        let gone = item_detail(
            &dir,
            "stash",
            &stack[0].0,
            None,
            &encode_stash_snapshot(&[("1".repeat(40), "WIP: gone".to_string())]),
        );
        let log_id = insert_operation_log(&mut conn, None, OP_STASH_DROP, "x", &[gone]).unwrap();
        let detail = get_operation_log(&conn, log_id).unwrap();
        assert!(!preview_undo(&detail)[0].ok);
        assert!(preview_undo(&detail)[0].message.contains("已不存在"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// GF-16: worktree-remove undo recreates the worktree at the recorded
    /// position; a missing branch is refused.
    #[test]
    fn undo_worktree_remove_recreates_worktree() {
        let dir = tmpdir("wtundo");
        {
            let repo = init_repo(&dir);
            drop(repo);
        }
        let wt_path = dir
            .parent()
            .unwrap()
            .join(format!("{}-wtundo-wt", dir.file_name().unwrap().to_string_lossy()));
        // new_branch creates and checks out "wt-branch" in the worktree.
        crate::core::worktree::add_worktree(&dir, &wt_path, None, Some("wt-branch")).unwrap();
        let wt_name = wt_path.file_name().unwrap().to_string_lossy().to_string();
        let (snap, head_oid) = crate::core::worktree::snapshot_worktree(&dir, &wt_name).expect("snapshot");

        crate::core::worktree::remove_worktree(&dir, &wt_name, true).unwrap();
        assert!(!wt_path.exists());

        let mut conn = open_db();
        let log_id = insert_operation_log(
            &mut conn,
            None,
            OP_WORKTREE_REMOVE,
            "移除 worktree",
            &[item_detail(
                &dir,
                "wt-branch",
                &head_oid,
                None,
                &encode_worktree_snapshot(&snap),
            )],
        )
        .unwrap();
        let detail = get_operation_log(&conn, log_id).unwrap();
        let preview = preview_undo(&detail);
        assert!(preview[0].ok, "{}", preview[0].message);
        assert!(preview[0].action.contains("重建 worktree"), "{}", preview[0].action);

        let results = run_undo(&detail);
        assert!(results[0].success, "{}", results[0].message);
        assert!(wt_path.join(".git").is_file(), "worktree recreated with a .git file");
        let list = crate::core::worktree::list_worktrees(&dir).unwrap();
        let wt = list.iter().find(|w| !w.is_main).unwrap();
        assert_eq!(wt.branch.as_deref(), Some("wt-branch"));

        // A branch that vanished since the removal is refused (use a free
        // path so the check reaches the branch test, not the path test).
        let gone = item_detail(
            &dir,
            "wt-branch",
            &head_oid,
            None,
            &encode_worktree_snapshot(&WorktreeSnapshot {
                name: format!("{wt_name}-gone"),
                path: dir
                    .parent()
                    .unwrap()
                    .join(format!("{}-gone-wt", dir.file_name().unwrap().to_string_lossy()))
                    .to_string_lossy()
                    .to_string(),
                branch: Some("deleted-branch".to_string()),
                oid: None,
            }),
        );
        let log_id = insert_operation_log(&mut conn, None, OP_WORKTREE_REMOVE, "x", &[gone]).unwrap();
        let preview = preview_undo(&get_operation_log(&conn, log_id).unwrap());
        assert!(!preview[0].ok);
        assert!(preview[0].message.contains("已不存在"), "{}", preview[0].message);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// GF-16: batch create undo deletes the created branch at its recorded
    /// tip, and refuses once the branch has moved on.
    #[test]
    fn undo_batch_create_deletes_created_branch() {
        let dir = tmpdir("createundo");
        let base = {
            let repo = init_repo(&dir);
            let base = repo.head().unwrap().target().unwrap().to_string();
            drop(repo);
            base
        };
        branch::create_branch(&dir, "feature-x", None).unwrap();

        let mut conn = open_db();
        let log_id = insert_operation_log(
            &mut conn,
            None,
            OP_CREATE_BRANCH_ALL,
            "批量创建分支 'feature-x'",
            &[item_detail(&dir, "feature-x", &base, Some(&base), "创建目标分支：feature-x")],
        )
        .unwrap();
        let detail = get_operation_log(&conn, log_id).unwrap();
        let preview = preview_undo(&detail);
        assert!(preview[0].ok, "{}", preview[0].message);
        assert!(preview[0].action.contains("删除新建分支"), "{}", preview[0].action);

        let results = run_undo(&detail);
        assert!(results[0].success, "{}", results[0].message);
        assert!(git2::Repository::open(&dir)
            .unwrap()
            .find_branch("feature-x", git2::BranchType::Local)
            .is_err());

        // Recreate, commit ON TOP OF THE BRANCH, undo again -> refused.
        branch::create_branch(&dir, "feature-x", None).unwrap();
        branch::checkout_branch(&dir, "feature-x").unwrap();
        {
            let repo = git2::Repository::open(&dir).unwrap();
            commit_file(&repo, &dir, "c.txt", "c\n", "c4");
            drop(repo);
        }
        branch::checkout_branch(&dir, "master").unwrap();
        let preview = preview_undo(&get_operation_log(&conn, log_id).unwrap());
        assert!(!preview[0].ok);
        assert!(preview[0].message.contains("新提交"), "{}", preview[0].message);

        // Backfill still pending (after_oid NULL) -> refuse with an
        // actionable message rather than guessing.
        let null_after = insert_operation_log(
            &mut conn,
            None,
            OP_CREATE_BRANCH_ALL,
            "batch",
            &[item(&dir, "feature-x", &base, None)],
        )
        .unwrap();
        let preview = preview_undo(&get_operation_log(&conn, null_after).unwrap());
        assert!(!preview[0].ok);
        assert!(preview[0].message.contains("操作后快照"), "{}", preview[0].message);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// GF-16 (checklist 4): ops that cannot be covered by ref snapshots are
    /// marked 不可撤销 with a reason + recovery hint, not "unsupported".
    #[test]
    fn undo_marks_non_recoverable_ops_explicitly() {
        let dir = tmpdir("nonrecoverable");
        {
            let repo = init_repo(&dir);
            drop(repo);
        }
        let mut conn = open_db();
        let log_id = insert_operation_log(
            &mut conn,
            None,
            OP_CONFLICT_RESOLUTION,
            "解决 1 个文件冲突：a.txt",
            &[item(&dir, "master", "a".repeat(40).as_str(), None)],
        )
        .unwrap();
        let preview = preview_undo(&get_operation_log(&conn, log_id).unwrap());
        assert!(!preview[0].ok);
        assert!(preview[0].action.contains("不可自动撤销"), "{}", preview[0].action);
        assert!(preview[0].message.contains("不可自动撤销"), "{}", preview[0].message);
        assert!(preview[0].message.contains("Abort"), "{}", preview[0].message);

        let log_id = insert_operation_log(
            &mut conn,
            None,
            OP_RESTORE_FILES,
            "batch restore working-tree changes",
            &[item(&dir, "master", "a".repeat(40).as_str(), None)],
        )
        .unwrap();
        let preview = preview_undo(&get_operation_log(&conn, log_id).unwrap());
        assert!(!preview[0].ok);
        assert!(preview[0].message.contains("reflog / stash 保底"), "{}", preview[0].message);

        // The execute path refuses the same way (no half-applied reverse op).
        let results = run_undo(&get_operation_log(&conn, log_id).unwrap());
        assert!(!results[0].success);
        assert!(results[0].message.contains("不支持撤销") || results[0].message.contains("不可自动撤销"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// GF-16: the async batch backfill writes only still-NULL after-oids and
    /// never overwrites a known snapshot.
    #[test]
    fn backfill_after_oids_skips_known_values() {
        let dir_a = tmpdir("backfill_a");
        let dir_b = tmpdir("backfill_b");
        {
            let repo = init_repo(&dir_a);
            drop(repo);
        }
        {
            let repo = init_repo(&dir_b);
            drop(repo);
        }
        let (a1, a2) = {
            let repo = git2::Repository::open(&dir_a).unwrap();
            let a1 = repo.head().unwrap().target().unwrap().to_string();
            commit_file(&repo, &dir_a, "b.txt", "two\n", "c2");
            let a2 = repo.head().unwrap().target().unwrap().to_string();
            drop(repo);
            (a1, a2)
        };
        let b_known = "c".repeat(40);

        let mut conn = open_db();
        let log_id = insert_operation_log(
            &mut conn,
            None,
            OP_CHECKOUT_ALL,
            "批量检出",
            &[
                item(&dir_a, "master", &a1, None),
                item(&dir_b, "master", "d".repeat(40).as_str(), Some(&b_known)),
            ],
        )
        .unwrap();
        let updated = backfill_after_oids(
            &mut conn,
            log_id,
            &[
                (dir_a.to_string_lossy().to_string(), a2.clone()),
                (dir_b.to_string_lossy().to_string(), "e".repeat(40)),
            ],
        )
        .unwrap();
        assert_eq!(updated, 1, "only the NULL row is written");

        let detail = get_operation_log(&conn, log_id).unwrap();
        assert_eq!(detail.items[0].after_oid.as_deref(), Some(a2.as_str()));
        assert_eq!(detail.items[1].after_oid.as_deref(), Some(b_known.as_str()));

        // Idempotent: a second backfill is a no-op.
        let again = backfill_after_oids(&mut conn, log_id, &[(dir_a.to_string_lossy().to_string(), a1.clone())]).unwrap();
        assert_eq!(again, 0);
        assert_eq!(get_operation_log(&conn, log_id).unwrap().items[0].after_oid.as_deref(), Some(a2.as_str()));

        // Unknown repo paths simply match nothing.
        let none = backfill_after_oids(&mut conn, log_id, &[("D:/nowhere".to_string(), a1.clone())]).unwrap();
        assert_eq!(none, 0);

        let _ = std::fs::remove_dir_all(&dir_a);
        let _ = std::fs::remove_dir_all(&dir_b);
    }
