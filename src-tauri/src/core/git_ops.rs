use std::path::Path;
use std::sync::Arc;

use crate::core::ssh::SshCredentials;
use crate::error::{AppError, AppResult};
use crate::models::task::TaskType;

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
    pub fn execute(
        &self,
        task_type: &TaskType,
        repo_path: &Path,
    ) -> AppResult<Option<String>> {
        match task_type {
            TaskType::Fetch => self.fetch(repo_path).map(Some),
            TaskType::Pull => self.pull(repo_path).map(Some),
            TaskType::Push => self.push(repo_path).map(Some),
            TaskType::Commit { message, files } => {
                self.commit(repo_path, message, files)?;
                Ok(None)
            }
        }
    }

    /// Fetch from the default remote (origin or first available).
    /// Updates remote-tracking branches without modifying the working tree.
    ///
    /// Executed via the `git` CLI so that HTTP authentication (credential
    /// manager), redirects (e.g. GitLab's 301 to `.git/`) and SSH work
    /// exactly as they do in the user's git installation.
    pub fn fetch(&self, repo_path: &Path) -> AppResult<String> {
        let repo = git2::Repository::open(repo_path)?;
        let remote_name = self.find_default_remote_name(&repo)?;

        log::info!("Fetching from remote '{}' for {:?}", remote_name, repo_path);
        let out = run_git(repo_path, &["fetch", &remote_name])?;
        log::info!("Fetch completed for {:?}", repo_path);
        Ok(out)
    }

    /// Pull (fetch + fast-forward merge) from the upstream remote.
    /// Only fast-forward merges are performed; divergent branches are
    /// reported as errors (`git pull --ff-only`).
    pub fn pull(&self, repo_path: &Path) -> AppResult<String> {
        log::info!("Pulling for {:?}", repo_path);
        let out = run_git(repo_path, &["pull", "--ff-only"])?;
        log::info!("Pull completed for {:?}", repo_path);
        Ok(out)
    }

    /// Push the current branch to its upstream remote.
    pub fn push(&self, repo_path: &Path) -> AppResult<String> {
        log::info!("Pushing for {:?}", repo_path);
        let out = run_git(repo_path, &["push"])?;
        log::info!("Push completed for {:?}", repo_path);
        Ok(out)
    }

    /// Stage specified files and commit with the given message.
    /// If no files are specified, stages all changes (`git add -A`).
    pub fn commit(&self, repo_path: &Path, message: &str, files: &[String]) -> AppResult<()> {
        let repo = git2::Repository::open(repo_path)?;
        let mut index = repo.index()?;

        // Stage files
        if files.is_empty() {
            // Stage all changes
            index.add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)?;
        } else {
            for file in files {
                index.add_path(Path::new(file))?;
            }
        }
        index.write()?;

        let tree_oid = index.write_tree()?;
        let tree = repo.find_tree(tree_oid)?;

        // Get parent commit (or create initial commit if no HEAD)
        let sig = repo.signature()?;

        let parents: Vec<git2::Commit> = match repo.head() {
            Ok(head) => {
                let parent = repo.find_commit(head.target().unwrap())?;
                vec![parent]
            }
            Err(_) => {
                // No HEAD - initial commit
                vec![]
            }
        };

        let parent_refs: Vec<&git2::Commit> = parents.iter().collect();

        repo.commit(
            Some("HEAD"),
            &sig,
            &sig,
            message,
            &tree,
            &parent_refs,
        )?;

        log::info!("Commit '{}' created for {:?}", message, repo_path);
        Ok(())
    }

    /// Find the default remote name: try "origin" first, then the first available.
    fn find_default_remote_name(&self, repo: &git2::Repository) -> AppResult<String> {
        if repo.find_remote("origin").is_ok() {
            return Ok("origin".to_string());
        }

        let remotes = repo.remotes()?;
        if let Some(name) = remotes.get(0) {
            return Ok(name.to_string());
        }

        Err(AppError::Git(git2::Error::from_str(
            "No remotes configured for this repository",
        )))
    }
}

/// Convenience function to create a default GitOps wrapped in Arc.
pub fn default_git_ops() -> Arc<GitOps> {
    Arc::new(GitOps::with_default_ssh())
}

/// Run a `git` command inside a repository directory.
///
/// Delegating network operations (fetch/pull/push) to the git CLI lets the
/// user's credential manager, redirect handling and SSH configuration work
/// as they do in their normal git usage (libgit2 cannot use the Windows Git
/// Credential Manager, and fails on HTTP redirects that require auth).
///
/// On Windows the child process is spawned with `CREATE_NO_WINDOW` so no
/// console windows pop up. The combined stdout/stderr is returned so the UI
/// can show the executed command and its output (IDE-style git console).
fn run_git(repo_path: &Path, args: &[&str]) -> AppResult<String> {
    let mut cmd = std::process::Command::new("git");
    cmd.current_dir(repo_path).args(args);

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW (0x08000000): don't spawn a visible console.
        cmd.creation_flags(0x0800_0000);
    }

    let output = cmd.output().map_err(|e| {
        AppError::Git(git2::Error::from_str(&format!(
            "failed to run git: {}",
            e
        )))
    })?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if output.status.success() {
        log::info!("git {} -> {}", args.join(" "), stdout.trim());
        let combined = [stdout.trim(), stderr.trim()]
            .join("\n")
            .trim()
            .to_string();
        Ok(combined)
    } else {
        let msg = if !stderr.trim().is_empty() {
            stderr.trim().to_string()
        } else {
            stdout.trim().to_string()
        };
        log::error!("git {} failed: {}", args.join(" "), msg);
        Err(AppError::Git(git2::Error::from_str(&msg)))
    }
}
