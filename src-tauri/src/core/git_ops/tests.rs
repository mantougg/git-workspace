//! GitOps 单元测试（同父模块 tests.rs，可访问私有成员）。
//! 覆盖：commit/amend/index-only/identity、安全扫描拦截与放行、
//! Commit & Push 中间态、BranchOp 任务分发。

use super::*;
use crate::models::task::BranchOpKind;

fn tmpdir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "gw_commit_{}_{}",
        tag,
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Init a repo with one committed file.
fn init_repo(dir: &Path, name: &str, content: &str) {
    let repo = git2::Repository::init(dir).unwrap();
    std::fs::write(dir.join(name), content).unwrap();
    let mut index = repo.index().unwrap();
    index.add_path(Path::new(name)).unwrap();
    index.write().unwrap();
    let tree_oid = index.write_tree().unwrap();
    let tree = repo.find_tree(tree_oid).unwrap();
    let sig = git2::Signature::now("tester", "t@example.com").unwrap();
    repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[]).unwrap();
}

fn head_commit(dir: &Path) -> git2::Commit<'static> {
    // Leak the repository handle for test convenience: the process exits
    // after the test run, so the leaked handle is never a real leak.
    let repo = Box::leak(Box::new(git2::Repository::open(dir).unwrap()));
    repo.head().unwrap().peel_to_commit().unwrap()
}

fn head_tree_file(dir: &Path, name: &str) -> String {
    let commit = head_commit(dir);
    let tree = commit.tree().unwrap();
    let entry = tree.get_path(Path::new(name)).unwrap();
    let repo = git2::Repository::open(dir).unwrap();
    let blob = repo.find_blob(entry.id()).unwrap();
    String::from_utf8(blob.content().to_vec()).unwrap()
}

/// Amend with a new message replaces the message and the tree (T-11).
#[test]
fn amend_replaces_message_and_tree() {
    let dir = tmpdir("amend");
    init_repo(&dir, "a.txt", "one\n");
    std::fs::write(dir.join("a.txt"), "one\ntwo\n").unwrap();

    let ops = GitOps::with_default_ssh();
    let out = ops
        .commit(
            &dir,
            &CommitOptions {
                message: "amended".to_string(),
                amend: true,
                ..Default::default()
            },
        )
        .unwrap();

    let head = head_commit(&dir);
    assert_eq!(head.message().unwrap(), "amended");
    assert_eq!(head.id().to_string(), out.oid);
    assert_eq!(head.parent_count(), 0, "amend keeps parentage (root)");
    assert_eq!(head_tree_file(&dir, "a.txt"), "one\ntwo\n");

    let _ = std::fs::remove_dir_all(&dir);
}

/// Amend --no-edit keeps the original message while taking the new tree.
#[test]
fn amend_no_edit_keeps_message() {
    let dir = tmpdir("noedit");
    init_repo(&dir, "a.txt", "one\n");
    std::fs::write(dir.join("a.txt"), "one\ntwo\n").unwrap();

    let ops = GitOps::with_default_ssh();
    ops.commit(
        &dir,
        &CommitOptions {
            message: String::new(),
            amend: true,
            no_edit: true,
            ..Default::default()
        },
    )
    .unwrap();

    let head = head_commit(&dir);
    assert_eq!(head.message().unwrap(), "init");
    assert_eq!(head_tree_file(&dir, "a.txt"), "one\ntwo\n");

    let _ = std::fs::remove_dir_all(&dir);
}

/// Committing with index_only preserves hunk/line staging (T-12 联动):
/// only the staged line reaches HEAD; the rest stays modified in the
/// working tree.
#[test]
fn index_only_commit_preserves_partial_staging() {
    let dir = tmpdir("indexonly");
    init_repo(&dir, "a.txt", "one\ntwo\nthree\nfour\nfive\n");
    std::fs::write(dir.join("a.txt"), "one\nTWO\nthree\nFOUR\nfive\n").unwrap();

    // Stage only the first change (hunk 0 lines 1,2 = -two/+TWO).
    crate::core::stage::stage_lines(&dir, "a.txt", 0, &[1, 2]).unwrap();

    let ops = GitOps::with_default_ssh();
    ops.commit(
        &dir,
        &CommitOptions {
            message: "partial".to_string(),
            index_only: true,
            ..Default::default()
        },
    )
    .unwrap();

    assert_eq!(
        head_tree_file(&dir, "a.txt"),
        "one\nTWO\nthree\nfour\nfive\n",
        "only the staged line may be committed"
    );
    // The remaining change is still unstaged afterwards.
    let unstaged =
        crate::core::diff::get_unstaged_diff_with_config(&dir, &crate::core::diff::DiffConfig::default()).unwrap();
    let adds: Vec<_> = unstaged[0]
        .hunks
        .iter()
        .flat_map(|h| h.lines.iter())
        .filter(|l| l.line_type == "add")
        .map(|l| l.content.clone())
        .collect();
    assert_eq!(adds, vec!["FOUR"]);

    let _ = std::fs::remove_dir_all(&dir);
}

/// index_only with an empty index is an explicit error, not an empty commit.
#[test]
fn index_only_empty_staged_errors() {
    let dir = tmpdir("emptystaged");
    init_repo(&dir, "a.txt", "one\n");

    let ops = GitOps::with_default_ssh();
    let err = ops
        .commit(
            &dir,
            &CommitOptions {
                message: "x".to_string(),
                index_only: true,
                ..Default::default()
            },
        )
        .unwrap_err();
    assert!(err.to_string().contains("暂存区为空"), "unexpected: {err}");

    let _ = std::fs::remove_dir_all(&dir);
}

/// Safety scan: forbidden files (.env) and secrets are flagged; committing
/// is refused unless explicitly allowed (T-11, §5 拦截并可放行).
#[test]
fn safety_scan_blocks_and_override_allows() {
    let dir = tmpdir("scan");
    init_repo(&dir, "a.txt", "one\n");
    std::fs::write(dir.join(".env"), "TOKEN=x\n").unwrap();
    std::fs::write(dir.join("key.txt"), "const k = \"AKIAIOSFODNN7EXAMPLE\";\n").unwrap();

    // Pre-flight scan sees both the forbidden file and the secret.
    let findings = pre_commit_scan(&dir, &[".env".to_string(), "key.txt".to_string()], false).unwrap();
    assert!(findings.iter().any(|f| f.kind == "forbidden" && f.path == ".env"));
    assert!(findings.iter().any(|f| f.kind == "secret" && f.path == "key.txt"));

    let ops = GitOps::with_default_ssh();
    let err = ops
        .commit(
            &dir,
            &CommitOptions {
                message: "nope".to_string(),
                ..Default::default()
            },
        )
        .unwrap_err();
    assert!(err.to_string().contains("安全拦截"), "unexpected: {err}");

    // Explicit override commits anyway.
    ops.commit(
        &dir,
        &CommitOptions {
            message: "override".to_string(),
            allow_unsafe: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(head_commit(&dir).message().unwrap(), "override");

    let _ = std::fs::remove_dir_all(&dir);
}

/// Large files (> 5 MiB) are flagged by the scan.
#[test]
fn safety_scan_flags_large_files() {
    let dir = tmpdir("large");
    init_repo(&dir, "a.txt", "one\n");
    let big = "x".repeat(6 * 1024 * 1024);
    std::fs::write(dir.join("big.bin"), &big).unwrap();

    let findings = pre_commit_scan(&dir, &["big.bin".to_string()], false).unwrap();
    assert!(
        findings.iter().any(|f| f.kind == "large_file" && f.path == "big.bin"),
        "large file must be flagged: {findings:?}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// Per-repo identity override is used as author/committer (T-11 §54).
#[test]
fn identity_override_is_used_for_commit() {
    let dir = tmpdir("identity");
    init_repo(&dir, "a.txt", "one\n");
    std::fs::write(dir.join("a.txt"), "one\ntwo\n").unwrap();

    let ops = GitOps::with_default_ssh();
    ops.commit(
        &dir,
        &CommitOptions {
            message: "whoami".to_string(),
            author_name: Some("Repo Bot".to_string()),
            author_email: Some("bot@repo.local".to_string()),
            ..Default::default()
        },
    )
    .unwrap();

    let head = head_commit(&dir);
    assert_eq!(head.author().name().unwrap(), "Repo Bot");
    assert_eq!(head.author().email().unwrap(), "bot@repo.local");
    assert_eq!(head.committer().name().unwrap(), "Repo Bot");

    let _ = std::fs::remove_dir_all(&dir);
}

/// Commit & Push with a failing push reports the intermediate state
/// "提交成功但推送失败" and keeps the commit (T-11 acceptance).
#[test]
fn commit_then_push_failure_keeps_commit_and_marks_state() {
    let dir = tmpdir("commitpush");
    init_repo(&dir, "a.txt", "one\n");
    // An unreachable remote makes the push phase fail.
    {
        let repo = git2::Repository::open(&dir).unwrap();
        repo.remote("origin", "file:///nonexistent/nowhere.git").unwrap();
    }
    std::fs::write(dir.join("a.txt"), "one\ntwo\n").unwrap();

    let ops = GitOps::with_default_ssh();
    let task = TaskType::Commit {
        message: "cp".to_string(),
        files: vec![],
        amend: false,
        no_edit: false,
        index_only: false,
        then_push: true,
        allow_unsafe: false,
        author_name: None,
        author_email: None,
    };
    let err = ops.execute(&task, &dir).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("提交成功但推送失败"),
        "middle state must be explicit, got: {msg}"
    );
    // The commit itself survived the failed push.
    assert_eq!(head_commit(&dir).message().unwrap(), "cp");

    let _ = std::fs::remove_dir_all(&dir);
}

/// Bulk branch ops execute per repo through the task path (T-20):
/// create, then checkout, then delete a branch via GitOps::execute.
#[test]
fn branch_op_task_create_checkout_delete() {
    let dir = tmpdir("branchop");
    init_repo(&dir, "a.txt", "one\n");
    let ops = GitOps::with_default_ssh();

    let mk = |op: BranchOpKind, name: &str, force: bool| TaskType::BranchOp {
        op,
        name: name.to_string(),
        force,
    };

    ops.execute(&mk(BranchOpKind::Create, "feature", false), &dir).unwrap();
    {
        let repo = git2::Repository::open(&dir).unwrap();
        assert!(
            repo.find_branch("feature", git2::BranchType::Local).is_ok(),
            "branch must be created"
        );
    }

    ops.execute(&mk(BranchOpKind::Checkout, "feature", false), &dir)
        .unwrap();
    {
        let repo = git2::Repository::open(&dir).unwrap();
        let head = repo.head().unwrap();
        assert_eq!(head.shorthand().unwrap(), "feature");
    }

    // Checked-out branch cannot be deleted (git refuses); go back first.
    ops.execute(&mk(BranchOpKind::Checkout, "master", false), &dir)
        .or_else(|_| ops.execute(&mk(BranchOpKind::Checkout, "main", false), &dir))
        .unwrap();
    ops.execute(&mk(BranchOpKind::Delete, "feature", false), &dir).unwrap();
    {
        let repo = git2::Repository::open(&dir).unwrap();
        assert!(repo.find_branch("feature", git2::BranchType::Local).is_err());
    }

    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// PAF-11：stage_files / restore_files（自同步命令收编入队列路径后的行为回归）
// ---------------------------------------------------------------------------

#[test]
fn stage_files_tracks_modification_and_deletion() {
    let dir = tmpdir("stage_files");
    init_repo(&dir, "a.txt", "one\n");
    let ops = GitOps::with_default_ssh();

    // 修改 + 新文件 + 磁盘删除三种形态。
    std::fs::write(dir.join("a.txt"), "two\n").unwrap();
    std::fs::write(dir.join("new.txt"), "new\n").unwrap();
    ops.stage_files(&dir, &["a.txt".into(), "new.txt".into()])
        .unwrap();
    std::fs::remove_file(dir.join("a.txt")).unwrap();
    ops.stage_files(&dir, &["a.txt".into()]).unwrap();

    let repo = git2::Repository::open(&dir).unwrap();
    let index = repo.index().unwrap();
    assert!(
        index.get_path(Path::new("a.txt"), 0).is_none(),
        "deleted file must be removed from the index"
    );
    assert!(
        index.get_path(Path::new("new.txt"), 0).is_some(),
        "new file must be staged"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn restore_files_reverts_tracked_and_removes_untracked() {
    let dir = tmpdir("restore_files");
    init_repo(&dir, "a.txt", "one\n");
    let ops = GitOps::with_default_ssh();

    // tracked 文件修改（含暂存）→ 还原到 HEAD；
    // staged-new 文件 → 取消暂存并删除。
    std::fs::write(dir.join("a.txt"), "dirty\n").unwrap();
    ops.stage_files(&dir, &["a.txt".into()]).unwrap();
    std::fs::write(dir.join("b.txt"), "staged new\n").unwrap();
    ops.stage_files(&dir, &["b.txt".into()]).unwrap();

    ops.restore_files(&dir, &["a.txt".into(), "b.txt".into()])
        .unwrap();

    assert_eq!(
        std::fs::read_to_string(dir.join("a.txt")).unwrap().replace("\r\n", "\n"),
        "one\n",
        "tracked file must be restored from HEAD"
    );
    assert!(!dir.join("b.txt").exists(), "staged-new file must be deleted");
    let repo = git2::Repository::open(&dir).unwrap();
    let statuses = repo.statuses(None).unwrap();
    assert!(
        statuses.iter().all(|e| e.status() == git2::Status::CURRENT),
        "worktree must be clean after restore: {:?}",
        statuses
            .iter()
            .map(|e| (e.path().map(|p| p.to_string()), e.status().bits()))
            .collect::<Vec<_>>()
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn execute_dispatches_stage_and_restore_task_types() {
    let dir = tmpdir("dispatch_stage_restore");
    init_repo(&dir, "a.txt", "one\n");
    let ops = GitOps::with_default_ssh();

    std::fs::write(dir.join("a.txt"), "dirty\n").unwrap();
    ops.execute(
        &crate::models::task::TaskType::StageFiles { files: vec!["a.txt".into()] },
        &dir,
    )
    .unwrap();
    ops.execute(
        &crate::models::task::TaskType::RestoreFiles { files: vec!["a.txt".into()] },
        &dir,
    )
    .unwrap();

    assert_eq!(
        std::fs::read_to_string(dir.join("a.txt")).unwrap().replace("\r\n", "\n"),
        "one\n"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
