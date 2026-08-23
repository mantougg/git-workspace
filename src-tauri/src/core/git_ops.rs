use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use crate::core::secret;
use crate::core::ssh::SshCredentials;
use crate::error::{AppError, AppResult};
use crate::models::commit::CommitScanFinding;
use crate::models::task::TaskType;

/// Files larger than this are flagged by the pre-commit Large File Scan
/// (T-11; global constraint §5 Commit 安全检查).
const MAX_COMMIT_FILE_BYTES: u64 = 5 * 1024 * 1024;

/// Secret Scan reads at most this much text per file (keeps the synchronous
/// pre-commit scan cheap on big-but-allowed files).
const SECRET_SCAN_MAX_BYTES: usize = 1024 * 1024;

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

/// Options for creating a commit (T-11 Commit 增强).
#[derive(Debug, Clone, Default)]
pub struct CommitOptions {
    pub message: String,
    /// Files to stage before committing (Commit Selected). Empty = stage all
    /// changes. Ignored when `index_only` is set.
    pub files: Vec<String>,
    /// Commit the index as-is, preserving hunk/line staging (T-12 联动).
    pub index_only: bool,
    /// Amend the HEAD commit (tree comes from the index after staging).
    pub amend: bool,
    /// With `amend`: keep the original message (`--no-edit`).
    pub no_edit: bool,
    /// Proceed despite pre-commit safety findings (explicit user override).
    pub allow_unsafe: bool,
    /// Per-repo/group identity override (T-11 §54); falls back to git config.
    pub author_name: Option<String>,
    pub author_email: Option<String>,
}

impl CommitOptions {
    /// Build commit options from a queued commit task.
    fn from_task(task_type: &TaskType) -> Self {
        match task_type {
            TaskType::Commit {
                message,
                files,
                amend,
                no_edit,
                index_only,
                allow_unsafe,
                author_name,
                author_email,
                ..
            } => Self {
                message: message.clone(),
                files: files.clone(),
                index_only: *index_only,
                amend: *amend,
                no_edit: *no_edit,
                allow_unsafe: *allow_unsafe,
                author_name: author_name.clone(),
                author_email: author_email.clone(),
            },
            _ => Self::default(),
        }
    }
}

/// Result of a successful commit operation.
#[derive(Debug)]
pub struct CommitOutcome {
    /// The (new) commit oid — the amended oid when amending.
    pub oid: String,
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
                self.clone_repo(repo_path, url, branch.as_deref())
                    .map(Some)
            }
            // Pipeline Build/Test shell step (T-23): run in the repo dir,
            // killed after `timeout_secs` (worker timeout is the hard bound).
            TaskType::ShellCommand {
                command,
                timeout_secs,
            } => run_shell_command(repo_path, command, *timeout_secs).map(Some),
            TaskType::Commit { then_push, .. } => {                let outcome = self.commit(repo_path, &CommitOptions::from_task(task_type))?;
                if !then_push {
                    return Ok(None);
                }                // Commit & Push (T-11): only the push is retried — the commit
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
                            let backoff =
                                Duration::from_millis(500 * 2u64.pow(attempt as u32));
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

    /// Clone a repository from `url` into `dest` (T-33 batch clone). The
    /// destination directory itself must not exist yet (git errors
    /// otherwise); its parent is created when missing. Credentials / SSH
    /// follow the user's git installation (system CLI).
    pub fn clone_repo(&self, dest: &Path, url: &str, branch: Option<&str>) -> AppResult<String> {
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
    pub fn fetch(&self, repo_path: &Path) -> AppResult<String> {        let repo = git2::Repository::open(repo_path)?;
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

        log::info!("Pushing branch '{}' to '{}' for {:?}", branch, remote_name, repo_path);
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

    /// Create a commit (T-11): normal / amend / --no-edit / index-only
    /// (hunk/line staging preserved), with pre-commit safety scan and
    /// per-repo identity override.
    pub fn commit(&self, repo_path: &Path, opts: &CommitOptions) -> AppResult<CommitOutcome> {
        let repo = git2::Repository::open(repo_path)?;

        // 1. Staging (skipped entirely in index_only mode).
        let mut index = repo.index()?;
        if opts.index_only {
            if !opts.amend {
                let head_tree = crate::core::diff::head_or_empty_tree(&repo)?;
                let staged =
                    repo.diff_tree_to_index(Some(&head_tree), None, None)?;
                if staged.deltas().len() == 0 {
                    return Err(AppError::Other(
                        "暂存区为空，没有可提交的变更".to_string(),
                    ));
                }
            }
        } else if opts.files.is_empty() {
            // Stage all changes.
            index.add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)?;
            index.write()?;
        } else {
            for file in &opts.files {
                index.add_path(Path::new(file))?;
            }
            index.write()?;
        }

        // 2. Pre-commit safety scan (Secret + Large File + Forbidden, §5).
        let paths = paths_for_scan(&repo, &opts.files, opts.index_only)?;
        let findings = scan_paths(&repo, &paths);
        if !findings.is_empty() {
            if !opts.allow_unsafe {
                return Err(AppError::Other(format!(
                    "安全拦截：{}\n如确认无误，请勾选「允许跳过安全检查」后重试。",
                    format_findings(&findings)
                )));
            }
            log::warn!(
                "Commit proceeds despite {} safety finding(s) (user override): {}",
                findings.len(),
                format_findings(&findings)
            );
        }

        // 3. Tree from the index.
        let tree_oid = index.write_tree()?;
        let tree = repo.find_tree(tree_oid)?;

        // 4. Identity: per-repo/group override wins over git config (§54).
        let override_sig = match (&opts.author_name, &opts.author_email) {
            (Some(name), Some(email)) => Some(git2::Signature::now(name, email)?),
            _ => None,
        };
        let default_sig = repo.signature()?;

        // 5. Amend or create.
        let oid = if opts.amend {
            let head = repo.head()?.peel_to_commit()?;
            // None author keeps the original author (git amend semantics);
            // None message keeps the original message (--no-edit).
            let author: Option<&git2::Signature> = override_sig.as_ref();
            let message = if opts.no_edit {
                None
            } else {
                Some(opts.message.as_str())
            };
            head.amend(
                Some("HEAD"),
                author,
                Some(override_sig.as_ref().unwrap_or(&default_sig)),
                None, // message_encoding: default (UTF-8)
                message,
                Some(&tree),
            )?
        } else {
            let sig = override_sig.as_ref().unwrap_or(&default_sig);
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
            repo.commit(Some("HEAD"), sig, sig, &opts.message, &tree, &parent_refs)?
        };

        log::info!(
            "Commit {} created for {:?} (amend={}, index_only={})",
            oid,
            repo_path,
            opts.amend,
            opts.index_only
        );
        Ok(CommitOutcome { oid: oid.to_string() })
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

/// Paths whose content will be committed: the staged set for `index_only`,
/// the explicit file list, or every changed path for stage-all.
fn paths_for_scan(
    repo: &git2::Repository,
    files: &[String],
    index_only: bool,
) -> AppResult<Vec<String>> {
    if index_only {
        let head_tree = crate::core::diff::head_or_empty_tree(repo)?;
        let staged = repo.diff_tree_to_index(Some(&head_tree), None, None)?;
        return Ok(staged
            .deltas()
            .filter_map(|d| {
                d.new_file()
                    .path()
                    .map(|p| p.to_string_lossy().to_string())
            })
            .collect());
    }
    if !files.is_empty() {
        return Ok(files.to_vec());
    }
    // Stage-all: enumerate working tree + index changes.
    let mut opts = git2::StatusOptions::new();
    opts.include_untracked(true);
    opts.include_ignored(false);
    let statuses = repo.statuses(Some(&mut opts))?;
    Ok(statuses
        .iter()
        .filter_map(|e| e.path().map(str::to_string))
        .collect())
}

/// Scan the given paths for commit-blocking findings (T-11; §5):
/// forbidden filenames, oversized files, and secret-looking content.
/// Content is read from the index when staged, else from the working tree.
pub fn scan_paths(repo: &git2::Repository, paths: &[String]) -> Vec<CommitScanFinding> {
    let mut findings = Vec::new();
    let index = repo.index().ok();

    for path in paths {
        if secret::is_forbidden_file(path) {
            findings.push(CommitScanFinding {
                path: path.clone(),
                kind: "forbidden".to_string(),
                detail: "禁止提交的敏感文件（.env / *.pem / *.key / 私钥 / credentials.json）"
                    .to_string(),
            });
            // Forbidden is decisive; skip content scans for this path.
            continue;
        }

        // Content source: index blob when staged, else the workdir file.
        let content: Option<Vec<u8>> = index
            .as_ref()
            .and_then(|idx| idx.get_path(Path::new(path), 0))
            .and_then(|entry| repo.find_blob(entry.id).ok())
            .map(|blob| blob.content().to_vec())
            .or_else(|| {
                std::fs::read(repo.workdir()?.join(path)).ok()
            });

        let Some(content) = content else {
            continue; // deleted file or unreadable: nothing to scan
        };

        if content.len() as u64 > MAX_COMMIT_FILE_BYTES {
            findings.push(CommitScanFinding {
                path: path.clone(),
                kind: "large_file".to_string(),
                detail: format!(
                    "大文件（{} MB > {} MB）",
                    content.len() / (1024 * 1024),
                    MAX_COMMIT_FILE_BYTES / (1024 * 1024)
                ),
            });
            // Skip secret scan on huge files (sync scan must stay cheap).
            continue;
        }

        let text = String::from_utf8_lossy(&content[..content.len().min(SECRET_SCAN_MAX_BYTES)]);
        let secrets = secret::scan_secrets(&text);
        if !secrets.is_empty() {
            let kinds: Vec<&str> = secrets.iter().map(|s| s.kind.label()).collect();
            findings.push(CommitScanFinding {
                path: path.clone(),
                kind: "secret".to_string(),
                detail: format!("疑似 Secret（{}）", kinds.join(", ")),
            });
        }
    }

    findings
}

/// Pre-commit safety scan entry point for the UI pre-flight check (T-11):
/// returns the findings without committing, so the UI can list them and let
/// the user explicitly override (allow_unsafe).
pub fn pre_commit_scan(
    repo_path: &Path,
    files: &[String],
    index_only: bool,
) -> AppResult<Vec<CommitScanFinding>> {
    let repo = git2::Repository::open(repo_path)?;
    let paths = paths_for_scan(&repo, files, index_only)?;
    Ok(scan_paths(&repo, &paths))
}

/// Compact one-line-per-finding summary for error messages / logs.
fn format_findings(findings: &[CommitScanFinding]) -> String {
    findings
        .iter()
        .take(5)
        .map(|f| format!("{}: {}", f.path, f.detail))
        .collect::<Vec<_>>()
        .join("\n")
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
/// Run a user shell command in `cwd`, killed after `timeout_secs` (T-23
/// pipeline Build/Test steps; default 10 min when unset).
///
/// Output is redirected to temp files instead of pipes: a child writing more
/// than the OS pipe buffer would block forever while we poll `try_wait`
/// without reading. Only the tail (256 KB) of each stream is kept.
fn run_shell_command(cwd: &Path, command: &str, timeout_secs: Option<u64>) -> AppResult<String> {
    let timeout = Duration::from_secs(timeout_secs.unwrap_or(600));
    let stamp = format!(
        "gw_shell_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    );
    let out_path = std::env::temp_dir().join(format!("{}.out", stamp));
    let err_path = std::env::temp_dir().join(format!("{}.err", stamp));

    let mut cmd = if cfg!(windows) {
        let mut c = std::process::Command::new("cmd");
        c.args(["/C", command]);
        c
    } else {
        let mut c = std::process::Command::new("sh");
        c.args(["-c", command]);
        c
    };
    cmd.current_dir(cwd)
        .stdout(std::fs::File::create(&out_path)?)
        .stderr(std::fs::File::create(&err_path)?);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW (0x08000000): don't spawn a visible console.
        cmd.creation_flags(0x0800_0000);
    }

    let mut child = cmd
        .spawn()
        .map_err(|e| AppError::Other(format!("命令启动失败: {}", e)))?;
    let start = std::time::Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) => {
                if start.elapsed() > timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = std::fs::remove_file(&out_path);
                    let _ = std::fs::remove_file(&err_path);
                    return Err(AppError::Other(format!(
                        "命令超过 {}s 未结束，已终止",
                        timeout.as_secs()
                    )));
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => {
                let _ = std::fs::remove_file(&out_path);
                let _ = std::fs::remove_file(&err_path);
                return Err(AppError::Other(format!("命令等待失败: {}", e)));
            }
        }
    };

    let stdout = read_tail(&out_path, 256 * 1024);
    let stderr = read_tail(&err_path, 256 * 1024);
    let _ = std::fs::remove_file(&out_path);
    let _ = std::fs::remove_file(&err_path);

    let combined = [stdout.trim(), stderr.trim()]
        .join("\n")
        .trim()
        .to_string();
    if status.success() {
        Ok(combined)
    } else {
        Err(AppError::Other(format!(
            "命令失败（exit {}）: {}",
            status.code().unwrap_or(-1),
            combined
        )))
    }
}

/// Read at most `cap` bytes from the END of a file (bounded memory for
/// potentially huge build logs).
fn read_tail(path: &Path, cap: u64) -> String {
    use std::io::{Read, Seek, SeekFrom};
    let Ok(mut f) = std::fs::File::open(path) else {
        return String::new();
    };
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    let skip = len.saturating_sub(cap);
    let _ = f.seek(SeekFrom::Start(skip));
    let mut buf = Vec::new();
    let _ = f.read_to_end(&mut buf);
    String::from_utf8_lossy(&buf).to_string()
}

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


#[cfg(test)]
mod tests {
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
        repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[])
            .unwrap();
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
        let unstaged = crate::core::diff::get_unstaged_diff_with_config(
            &dir,
            &crate::core::diff::DiffConfig::default(),
        )
        .unwrap();
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
        let findings =
            pre_commit_scan(&dir, &[".env".to_string(), "key.txt".to_string()], false).unwrap();
        assert!(findings.iter().any(|f| f.kind == "forbidden" && f.path == ".env"));
        assert!(
            findings
                .iter()
                .any(|f| f.kind == "secret" && f.path == "key.txt")
        );

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
            findings
                .iter()
                .any(|f| f.kind == "large_file" && f.path == "big.bin"),
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
            repo.remote("origin", "file:///nonexistent/nowhere.git")
                .unwrap();
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

        ops.execute(&mk(BranchOpKind::Create, "feature", false), &dir)
            .unwrap();
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
        ops.execute(&mk(BranchOpKind::Delete, "feature", false), &dir)
            .unwrap();
        {
            let repo = git2::Repository::open(&dir).unwrap();
            assert!(repo.find_branch("feature", git2::BranchType::Local).is_err());
        }

        let _ = std::fs::remove_dir_all(&dir);
    }
}
