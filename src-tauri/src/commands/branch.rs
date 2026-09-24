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
