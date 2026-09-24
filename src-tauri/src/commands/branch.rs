//! Branch Manager commands (T-09). Single-repo operations run immediately;
//! the batch/multi-repo variant goes through the task queue (T-05) later.

use std::path::Path;

use tauri::State;

use crate::core::branch::{self, BranchOverview, CompareResult};
use crate::core::git_ops::GitOps;
use crate::db::dao;
use crate::error::AppResult;
use crate::state::AppState;
use crate::task::console::{emit_git_op_started, finish_streaming, ConsoleStreamer};

use super::git_ops::{emit_op_finished, register_single_op, repo_display_name, SYNC_GIT_TIMEOUT};

/// List local branches (with upstream ahead/behind), remote-tracking branches
/// and tags, persisting a snapshot into the branches / remote_branches / tags
/// tables (T-03). Purely local — never triggers a network fetch.
#[tauri::command]
pub fn list_branches(repo_path: String, state: State<'_, AppState>) -> AppResult<BranchOverview> {
    let overview = branch::list_branches(Path::new(&repo_path))?;

    // Persist the snapshot when the repository is registered in the DB.
    let mut conn = state
        .db
        .lock()
        .map_err(|e| crate::error::AppError::Other(format!("DB lock error: {}", e)))?;
    if let Some(repo_id) = dao::get_repository_id_by_path(&conn, &repo_path)? {
        let locals: Vec<(String, bool, usize, usize)> = overview
            .locals
            .iter()
            .map(|b| (b.name.clone(), b.is_current, b.ahead, b.behind))
            .collect();
        dao::replace_branches(&mut conn, repo_id, &locals)?;

        let remote_names: Vec<String> = overview.remotes.iter().map(|r| r.name.clone()).collect();
        dao::replace_remote_branches(&mut conn, repo_id, &remote_names)?;

        let tags: Vec<(String, Option<String>)> = overview
            .tags
            .iter()
            .map(|t| (t.name.clone(), Some(t.target_oid.clone())))
            .collect();
        dao::replace_tags(&mut conn, repo_id, &tags)?;
    }

    Ok(overview)
}

/// Create a local branch at HEAD (or at `target`: branch / tag / oid).
#[tauri::command]
pub fn create_branch(repo_path: String, name: String, target: Option<String>) -> AppResult<()> {
    branch::create_branch(Path::new(&repo_path), &name, target.as_deref())
}

/// Checkout a local branch (safe checkout; dirty-conflict fails with an error).
/// R-21 §48：成功后通知 Runtime Git 联动引擎做依赖模型重算与 POM 变化复核
/// （不阻塞 checkout 本身，通知开销为一次 DB 读 + 一次任务提交）。
#[tauri::command]
pub fn checkout_branch(repo_path: String, name: String, state: State<'_, AppState>) -> AppResult<()> {
    branch::checkout_branch(Path::new(&repo_path), &name)?;
    state.git_link.notify_branch_switched(&repo_path);
    Ok(())
}

/// Delete a local branch. Unmerged branches are refused unless `force`.
#[tauri::command]
pub fn delete_branch(repo_path: String, name: String, force: Option<bool>) -> AppResult<()> {
    branch::delete_branch(Path::new(&repo_path), &name, force.unwrap_or(false))
}

/// Rename a local branch.
#[tauri::command]
pub fn rename_branch(repo_path: String, old_name: String, new_name: String) -> AppResult<()> {
    branch::rename_branch(Path::new(&repo_path), &old_name, &new_name)
}

/// Set (or clear) the upstream of a local branch. `upstream` is an existing
/// remote-tracking branch name like "origin/main"; None clears it.
#[tauri::command]
pub fn set_upstream(repo_path: String, branch_name: String, upstream: Option<String>) -> AppResult<()> {
    branch::set_upstream(Path::new(&repo_path), &branch_name, upstream.as_deref())
}

/// Create a local branch tracking the given remote branch (e.g. "origin/feature").
#[tauri::command]
pub fn track_remote_branch(repo_path: String, remote_branch: String) -> AppResult<()> {
    branch::track_remote_branch(Path::new(&repo_path), &remote_branch)
}

/// Push a specific local branch (network op via the git CLI, so the user's
/// credential manager / SSH setup applies). Returns the command output.
///
/// GF-07：原为**同步命令**——远程挂起时直接阻塞 WebView IPC 回调线程，整个
/// 界面失去响应，且无超时。改 async + `spawn_blocking` 并走
/// `push_branch_streaming`：执行移出 IPC 线程；输出逐行（100ms 聚合）镜像到
/// Git Console；`op_id` + `cancel_git_op` 提供取消入口；300s 硬超时杀 git
/// 进程树（`SYNC_GIT_TIMEOUT`）。错误经 stderr 尾部还原后原样冒泡（GF-08
/// 认证错误分类的语义不变）。
#[tauri::command]
pub async fn push_branch(
    repo_path: String,
    branch: String,
    op_id: Option<String>,
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> AppResult<String> {
    let (op_id, cancel, _guard) = register_single_op(&state, op_id);
    let repo_name = repo_display_name(&repo_path);
    let command = format!("git push {}", branch);
    emit_git_op_started(&app, &op_id, &repo_path, &repo_name, &command);
    let app_for_finish = app.clone();

    let result = tauri::async_runtime::spawn_blocking(move || {
        let ops = GitOps::with_default_ssh();
        let mut streamer = ConsoleStreamer::new(app, repo_path.clone(), repo_name, command);
        streamer.emit_meta_header();
        let r = ops.push_branch_streaming(
            Path::new(&repo_path),
            &branch,
            Some(cancel.as_ref()),
            Some(SYNC_GIT_TIMEOUT),
            &mut |s, l| streamer.on_line(s, l),
        );
        streamer.flush();
        finish_streaming(r, &streamer, SYNC_GIT_TIMEOUT)
    })
    .await
    .map_err(|e| crate::error::AppError::Other(format!("push_branch join error: {e}")))?;

    emit_op_finished(&app_for_finish, &op_id, &result);
    result
}

/// Compare two revisions (branch / tag / oid): commit差集 in both directions
/// plus the tree diff from `base` to `other`.
#[tauri::command]
pub fn compare_branches(repo_path: String, base: String, other: String) -> AppResult<CompareResult> {
    branch::compare_branches(Path::new(&repo_path), &base, &other)
}

// ---------------------------------------------------------------------------
// GF-04: tags. create / delete are local libgit2 operations (no network, so
// they stay synchronous like the branch variants); push_tag and the "already
// on the remote?" probe hit the network and therefore follow the GF-07
// async + spawn_blocking + streaming pattern (never block the IPC thread).
// ---------------------------------------------------------------------------

/// Create a tag at `target` (branch / tag / oid; defaults to HEAD). With a
/// `message` the tag is annotated, otherwise lightweight. An existing name is
/// refused. Purely local — no network access.
#[tauri::command]
pub fn create_tag(
    repo_path: String,
    name: String,
    message: Option<String>,
    target: Option<String>,
) -> AppResult<()> {
    branch::create_tag(
        Path::new(&repo_path),
        &name,
        message.as_deref(),
        target.as_deref(),
    )
}

/// Delete a local tag (`refs/tags/<name>`). Only the local ref is removed; a
/// remote copy is untouched — the UI states that in the confirmation.
#[tauri::command]
pub fn delete_tag(repo_path: String, name: String) -> AppResult<()> {
    branch::delete_tag(Path::new(&repo_path), &name)
}

/// Push a tag to the default remote (origin, else the first configured one).
/// Returns the command output.
///
/// GF-04：网络操作走 git CLI（用户凭据 / SSH 生效），async + `spawn_blocking`
/// + 流式镜像 + 300s 硬超时，与 GF-07 的 `push_branch` 同一模式。
///
/// Roadmap §47：`force` / `force_with_lease` **默认 false**——git 本身拒绝覆盖
/// 远程已有标签，force 只在用户显式开启时追加；`--force-with-lease` 为推荐方案
/// （远端被他人更新时安全失败）。
#[tauri::command]
pub async fn push_tag(
    repo_path: String,
    name: String,
    force: Option<bool>,
    force_with_lease: Option<bool>,
    op_id: Option<String>,
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> AppResult<String> {
    let (op_id, cancel, _guard) = register_single_op(&state, op_id);
    let repo_name = repo_display_name(&repo_path);
    let force = force.unwrap_or(false);
    let force_with_lease = force_with_lease.unwrap_or(false);
    let flags = if force_with_lease {
        " --force-with-lease"
    } else if force {
        " --force"
    } else {
        ""
    };
    let command = format!("git push <remote>{} {}", flags, name);
    emit_git_op_started(&app, &op_id, &repo_path, &repo_name, &command);
    let app_for_finish = app.clone();

    let result = tauri::async_runtime::spawn_blocking(move || {
        let ops = GitOps::with_default_ssh();
        let mut streamer = ConsoleStreamer::new(app, repo_path.clone(), repo_name, command);
        streamer.emit_meta_header();
        let r = ops.push_tag_streaming(
            Path::new(&repo_path),
            &name,
            force,
            force_with_lease,
            Some(cancel.as_ref()),
            Some(SYNC_GIT_TIMEOUT),
            &mut |s, l| streamer.on_line(s, l),
        );
        streamer.flush();
        finish_streaming(r, &streamer, SYNC_GIT_TIMEOUT)
    })
    .await
    .map_err(|e| crate::error::AppError::Other(format!("push_tag join error: {e}")))?;

    emit_op_finished(&app_for_finish, &op_id, &result);
    result
}

/// Whether a tag already exists on the remote — the Dangerous 删除确认 needs to
/// say so explicitly (Roadmap §46).
///
/// GF-04：网络查询 `git ls-remote --tags <remote>`（async + `spawn_blocking` +
/// 流式 + 300s 超时，同 `push_tag`）。查询失败（无远程 / 离线 / 凭据问题）时
/// 退回本地 remote-tracking refs 判定（`refs/remotes/*/tags/<name>`）：只可能
/// 漏报（本机推送的标签不产生 remote-tracking ref），不会误报。
#[tauri::command]
pub async fn tag_pushed_to_remote(
    repo_path: String,
    name: String,
    op_id: Option<String>,
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> AppResult<bool> {
    let (op_id, cancel, _guard) = register_single_op(&state, op_id);
    let repo_name = repo_display_name(&repo_path);
    let command = "git ls-remote --tags <remote>".to_string();
    emit_git_op_started(&app, &op_id, &repo_path, &repo_name, &command);
    let app_for_finish = app.clone();

    let result = tauri::async_runtime::spawn_blocking(move || {
        let ops = GitOps::with_default_ssh();
        let mut streamer = ConsoleStreamer::new(app, repo_path.clone(), repo_name, command);
        streamer.emit_meta_header();
        let r = ops.remote_tags_streaming(
            Path::new(&repo_path),
            Some(cancel.as_ref()),
            Some(SYNC_GIT_TIMEOUT),
            &mut |s, l| streamer.on_line(s, l),
        );
        streamer.flush();
        match finish_streaming(r, &streamer, SYNC_GIT_TIMEOUT) {
            Ok(out) => Ok(parse_remote_tag_names(&out).iter().any(|t| *t == name)),
            // 网络不可用 / 未配置远程：退回本地 remote-tracking refs 判定。
            Err(_) => Ok(branch::tag_in_remote_tracking_refs(
                Path::new(&repo_path),
                &name,
            )),
        }
    })
    .await
    .map_err(|e| crate::error::AppError::Other(format!("tag_pushed_to_remote join error: {e}")))?;

    emit_op_finished(&app_for_finish, &op_id, &result);
    result
}

/// Parse `git ls-remote --tags` output into tag names.
/// Lines look like `<oid>\trefs/tags/<name>`, plus one `<oid>\trefs/tags/<name>^{}`
/// peeled line per annotated tag (skipped here).
fn parse_remote_tag_names(output: &str) -> Vec<String> {
    output
        .lines()
        .filter_map(|line| line.split_once('\t').map(|(_, r)| r.trim()))
        .filter(|r| r.starts_with("refs/tags/") && !r.ends_with("^{}"))
        .map(|r| r.trim_start_matches("refs/tags/").to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::git_ops::GitOps;

    /// ls-remote output: annotated tags carry an extra peeled `^{}` line that
    /// must not become a second tag name.
    #[test]
    fn parse_remote_tag_names_skips_peeled_refs() {
        let out = "\
3f7a1c2b\trefs/tags/v1\n\
3f7a1c2b\trefs/tags/v1^{}\n\
9c8d7e6f\trefs/tags/rc/2026-09\n";
        assert_eq!(
            parse_remote_tag_names(out),
            vec!["v1".to_string(), "rc/2026-09".to_string()]
        );
        assert!(parse_remote_tag_names("").is_empty());
        assert!(parse_remote_tag_names("\trefs/heads/master\n").is_empty());
    }

    fn tmpdir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "gw_cmdbranch_{}_{}",
            tag,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Append one commit (new file) and return its oid.
    fn commit_file(dir: &Path, name: &str) -> String {
        let repo = git2::Repository::open(dir).unwrap();
        std::fs::write(dir.join(name), "x\n").unwrap();
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
        repo.commit(Some("HEAD"), &sig, &sig, name, &tree, &parents)
            .unwrap()
            .to_string()
    }

    /// GF-04：tag push / remote tag listing round-trip against a local bare
    /// "remote". The push path deliberately runs the system git CLI
    /// (credentials / SSH), so this test is skipped when `git` is missing.
    ///
    /// Verifies: push of a new tag succeeds, a missing local tag is refused
    /// before spawning git, `ls-remote --tags` lists what was pushed, and the
    /// Roadmap §47 force semantics (plain push and `--force-with-lease` reject
    /// a stale tag, `--force` overwrites). Also documents why the
    /// "already on the remote?" check needs ls-remote: a locally pushed tag
    /// leaves no remote-tracking ref behind.
    #[test]
    fn tag_push_round_trip_against_local_remote() {
        if std::process::Command::new("git").arg("--version").output().is_err() {
            eprintln!("skip: git CLI not available on PATH");
            return;
        }

        let dir = tmpdir("tagpush");
        let remote_dir = tmpdir("tagpush_remote");
        {
            let repo = git2::Repository::init(&dir).unwrap();
            std::fs::write(dir.join("a.txt"), "one\n").unwrap();
            let mut index = repo.index().unwrap();
            index.add_path(Path::new("a.txt")).unwrap();
            index.write().unwrap();
            let tree_oid = index.write_tree().unwrap();
            let tree = repo.find_tree(tree_oid).unwrap();
            let sig = git2::Signature::now("tester", "t@example.com").unwrap();
            repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[]).unwrap();
        }
        git2::Repository::init_bare(&remote_dir).unwrap();
        {
            let repo = git2::Repository::open(&dir).unwrap();
            // 平台兼容：URL 用正斜杠，避免 Windows 反斜杠被 git 解析异常。
            let url = remote_dir.to_string_lossy().replace('\\', "/");
            repo.remote("origin", &url).unwrap();
        }

        // v1 指向第一个提交，v2 指向第二个（后续把 v1 改指向第二个来触发 force）。
        let second_oid = commit_file(&dir, "b.txt");
        branch::create_tag(&dir, "v1", Some("release one"), None).unwrap();
        branch::create_tag(&dir, "v2", None, Some(&second_oid)).unwrap();

        let ops = GitOps::with_default_ssh();
        let no_cancel = None;
        let no_timeout = None;

        // 本地没有该标签：直接 NotFound，不 spawn git。
        let missing = ops.push_tag_streaming(&dir, "nope", false, false, no_cancel, no_timeout, &mut |_, _| {});
        assert!(missing.is_err(), "pushing a missing local tag must fail fast");

        // 1) 新标签：普通 push 成功（force 类参数均未传）。
        let exit = ops
            .push_tag_streaming(&dir, "v1", false, false, no_cancel, no_timeout, &mut |_, _| {})
            .expect("plain push of a new tag should succeed");
        assert_eq!(exit.exit_code, Some(0));
        let exit = ops
            .push_tag_streaming(&dir, "v2", false, false, no_cancel, no_timeout, &mut |_, _| {})
            .expect("second tag push should succeed");
        assert_eq!(exit.exit_code, Some(0));

        // 2) ls-remote 能看到两个标签（`^{}` peeled 行被 parse 跳过）。
        let listing = run_capture(&ops, &dir);
        let names = parse_remote_tag_names(&listing);
        assert!(names.contains(&"v1".to_string()), "listing was: {listing}");
        assert!(names.contains(&"v2".to_string()), "listing was: {listing}");
        assert!(
            !names.iter().any(|n| n.ends_with("^{}")),
            "peeled refs must not become tag names: {names:?}"
        );

        // 3) 把本地 v1 改指向第二个提交 —— 远程已有 v1，force 语义登场。
        {
            let repo = git2::Repository::open(&dir).unwrap();
            repo.reference("refs/tags/v1", second_oid.parse().unwrap(), true, "retarget")
                .unwrap();
        }
        assert!(
            ops.push_tag_streaming(&dir, "v1", false, false, no_cancel, no_timeout, &mut |_, _| {})
                .is_err(),
            "git refuses to overwrite an existing remote tag without force"
        );
        assert!(
            ops.push_tag_streaming(&dir, "v1", false, true, no_cancel, no_timeout, &mut |_, _| {})
                .is_err(),
            "--force-with-lease must reject a stale tag (Roadmap §47)"
        );
        let exit = ops
            .push_tag_streaming(&dir, "v1", true, false, no_cancel, no_timeout, &mut |_, _| {})
            .expect("explicit --force should overwrite the remote tag");
        assert_eq!(exit.exit_code, Some(0));

        // 4) 本机推送不会留下 remote-tracking ref —— 这正是「是否已推送」
        //    判定需要 ls-remote、本地 refs 只能当回退的原因。
        assert!(!branch::tag_in_remote_tracking_refs(&dir, "v1"));

        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&remote_dir);
    }

    /// Run `remote_tags_streaming` and return its accumulated output.
    fn run_capture(ops: &GitOps, dir: &Path) -> String {
        let mut out = String::new();
        ops.remote_tags_streaming(dir, None, None, &mut |_, line| {
            out.push_str(line);
            out.push('\n');
        })
        .expect("ls-remote --tags should succeed");
        out
    }
}
