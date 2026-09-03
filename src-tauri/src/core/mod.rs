pub mod branch;
pub mod change_set;
pub mod conflict;
pub mod diff;
pub mod git_ops;
pub mod git_status;
pub mod graph;
pub mod health;
pub mod heatmap;
pub mod history;
pub mod logger;
pub mod manifest;
pub mod merge;
pub mod operation_log;
pub mod pipeline;
pub mod rebase;
pub mod reflog;
pub mod scanner;
pub mod secret;
pub mod selector;
pub mod ssh;
pub mod stage;
pub mod stash;
pub mod watcher;
pub mod workspace_stash;
pub mod worktree;

/// Get the repository's default signature, falling back to a generic
/// identity when git config (`user.name` / `user.email`) is not set.
///
/// In production this avoids a hard crash for users who haven't configured
/// git identity yet; a warning is logged so the user knows to set it.
pub(crate) fn signature_or_default(repo: &git2::Repository) -> crate::error::AppResult<git2::Signature<'static>> {
    repo.signature().or_else(|_| {
        log::warn!("git config user.name/user.email not set; using fallback identity");
        git2::Signature::now("Git Multi", "git-multi@localhost")
            .map_err(|e| crate::error::AppError::Other(format!("无法创建签名: {}", e)))
    })
}
