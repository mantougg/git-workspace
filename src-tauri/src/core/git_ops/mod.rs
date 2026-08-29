use std::path::Path;
use std::time::Duration;

use crate::core::ssh::SshCredentials;
use crate::error::{AppError, AppResult};
use crate::models::task::TaskType;

mod commit;
mod remote;
mod safety;
mod shell;

pub use commit::CommitOptions;
pub use safety::pre_commit_scan;

use shell::run_shell_command;

/// Push attempts inside a Commit & Push task. The retry never re-runs the
/// commit — only the network push is retried (T-11 中间态语义).
const PUSH_MAX_ATTEMPTS: usize = 3;

/// Performs Git remote operations (fetch/pull/push) and local commits.
///
/// Network operations (fetch/pull/push) are executed through the `git` CLI
/// (`run_git`) so that the user's credential manager / SSH configuration is
/// used; libgit2 cannot access the Windows Git Credential Manager and fails
/// on HTTP redirects that require authentication. Local operations
/// (commit/add/restore/status/diff) use libgit2 (git2-rs).
pub struct GitOps {
    /// Kept for future per-repo SSH credential customization.
    #[allow(dead_code)]
    ssh: SshCredentials,
}

impl GitOps {
    /// Create a new GitOps with default SSH credential detection.
    pub fn new(ssh: SshCredentials) -> Self {
        GitOps { ssh }
    }

    /// Create with default credentials.
    pub fn with_default_ssh() -> Self {
        Self::new(SshCredentials::new())
    }

    /// Execute a Git operation based on the task type.
    /// This is the main entry point called by task workers.
    /// Returns the command output for network operations (fetch/pull/push).
    pub fn execute(&self, task_type: &TaskType, repo_path: &Path) -> AppResult<Option<String>> {
        match task_type {
            TaskType::Fetch => self.fetch(repo_path).map(Some),
            TaskType::Pull => self.pull(repo_path).map(Some),
            TaskType::Push => self.push(repo_path).map(Some),
            // Bulk branch operation (T-20): local libgit2 ops, one repo per task.
            TaskType::BranchOp { op, name, force } => {
                match op {
                    crate::models::task::BranchOpKind::Checkout => {
                        crate::core::branch::checkout_branch(repo_path, name)?
                    }
                    crate::models::task::BranchOpKind::Create => {
                        crate::core::branch::create_branch(repo_path, name, None)?
                    }
                    crate::models::task::BranchOpKind::Delete => {
                        crate::core::branch::delete_branch(repo_path, name, *force)?
                    }
                }
                Ok(None)
            }
            // Clone into a new directory (T-33 batch clone): network op via
            // the system git CLI (credentials/SSH follow the user's git).
            TaskType::Clone { url, branch } => {
                self.clone_repo(repo_path, url, branch.as_deref()).map(Some)
            }
            // Pipeline Build/Test shell step (T-23): run in the repo dir,
            // killed after `timeout_secs` (worker timeout is the hard bound).
            TaskType::ShellCommand {
                command,
                timeout_secs,
            } => run_shell_command(repo_path, command, *timeout_secs).map(Some),
            TaskType::Commit { then_push, .. } => {
                let outcome = self.commit(repo_path, &CommitOptions::from_task(task_type))?;
                if !then_push {
                    return Ok(None);
                } // Commit & Push (T-11): only the push is retried — the commit
                  // must never be re-run. A push failure surfaces the
                  // intermediate state "committed but push failed".
                let mut attempt = 0usize;
                loop {
                    match self.push(repo_path) {
                        Ok(out) => return Ok(Some(out)),
                        Err(e) => {
                            attempt += 1;
                            if attempt >= PUSH_MAX_ATTEMPTS {
                                return Err(AppError::Other(format!(
                                    "提交成功但推送失败（提交 {} 已保留，可稍后单独 Push）：{}",
                                    outcome.oid, e
                                )));
                            }
                            let backoff = Duration::from_millis(500 * 2u64.pow(attempt as u32));
                            log::warn!(
                                "Push after commit failed (attempt {}), retrying in {:?}",
                                attempt,
                                backoff
                            );
                            std::thread::sleep(backoff);
                        }
                    }
                }
            }
            // Runtime 任务（R-12）由 worker 直接分发给 RuntimeTaskHandler，
            // 正常不会走到这里；防御性报错而非 panic。
            TaskType::Runtime { .. } => Err(AppError::Task(
                "Runtime 任务不应由 GitOps 执行（worker 分发错误）".into(),
            )),
        }
    }
}

#[cfg(test)]
mod tests;
