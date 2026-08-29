use std::path::Path;

use crate::error::{AppError, AppResult};

impl super::GitOps {
    /// Clone a repository from `url` into `dest` (T-33 batch clone). The
    /// destination directory itself must not exist yet (git errors
    /// otherwise); its parent is created when missing. Credentials / SSH
    /// follow the user's git installation (system CLI).
    pub(super) fn clone_repo(
        &self,
        dest: &Path,
        url: &str,
        branch: Option<&str>,
    ) -> AppResult<String> {
        let parent = dest
            .parent()
            .ok_or_else(|| AppError::Other(format!("clone 目标 {:?} 没有父目录", dest)))?;
        std::fs::create_dir_all(parent)?;
        let name = dest
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| AppError::Other(format!("clone 目标路径无效: {:?}", dest)))?;
        let mut args: Vec<&str> = vec!["clone"];
        if let Some(b) = branch {
            args.extend(["--branch", b]);
        }
        args.extend([url, name]);
        log::info!("Cloning {} into {:?}", url, dest);
        let out = run_git(parent, &args)?;
        log::info!("Clone completed for {:?}", dest);
        Ok(out)
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

    /// Push a specific local branch (T-09). Uses the branch's configured
    /// upstream remote + remote branch name when set; otherwise pushes to the
    /// default remote under the same branch name.
    pub fn push_branch(&self, repo_path: &Path, branch: &str) -> AppResult<String> {
        let repo = git2::Repository::open(repo_path)?;
        let upstream = repo
            .find_branch(branch, git2::BranchType::Local)
            .ok()
            .and_then(|b| b.upstream().ok())
            .and_then(|u| u.name().ok().flatten().map(String::from));

        let (remote_name, refspec) = match upstream.as_deref() {
            // "origin/feature-x" -> push to origin as local:feature-x
            Some(u) => {
                let mut parts = u.splitn(2, '/');
                let remote = parts.next().unwrap_or("origin").to_string();
                let remote_branch = parts.next().unwrap_or(branch).to_string();
                (remote, format!("{}:{}", branch, remote_branch))
            }
            None => (self.find_default_remote_name(&repo)?, branch.to_string()),
        };

        log::info!(
            "Pushing branch '{}' to '{}' for {:?}",
            branch,
            remote_name,
            repo_path
        );
        let out = run_git(repo_path, &["push", &remote_name, &refspec])?;
        log::info!("Push branch completed for {:?}", repo_path);
        Ok(out)
    }

    /// Push the current branch to its upstream remote.
    pub fn push(&self, repo_path: &Path) -> AppResult<String> {
        log::info!("Pushing for {:?}", repo_path);
        let out = run_git(repo_path, &["push"])?;
        log::info!("Push completed for {:?}", repo_path);
        Ok(out)
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

    let output = cmd
        .output()
        .map_err(|e| AppError::Git(git2::Error::from_str(&format!("failed to run git: {}", e))))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if output.status.success() {
        log::info!("git {} -> {}", args.join(" "), stdout.trim());
        let combined = [stdout.trim(), stderr.trim()].join("\n").trim().to_string();
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
