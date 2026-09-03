use std::path::Path;

use super::safety::{format_findings, paths_for_scan, scan_paths};
use crate::error::{AppError, AppResult};
use crate::models::task::TaskType;

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
    pub(super) fn from_task(task_type: &TaskType) -> Self {
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

impl super::GitOps {
    /// Create a commit (T-11): normal / amend / --no-edit / index-only
    /// (hunk/line staging preserved), with pre-commit safety scan and
    /// per-repo identity override.
    pub(super) fn commit(&self, repo_path: &Path, opts: &CommitOptions) -> AppResult<CommitOutcome> {
        let repo = git2::Repository::open(repo_path)?;

        // 1. Staging (skipped entirely in index_only mode).
        let mut index = repo.index()?;
        if opts.index_only {
            if !opts.amend {
                let head_tree = crate::core::diff::head_or_empty_tree(&repo)?;
                let staged = repo.diff_tree_to_index(Some(&head_tree), None, None)?;
                if staged.deltas().len() == 0 {
                    return Err(AppError::Other("暂存区为空，没有可提交的变更".to_string()));
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
        let default_sig = crate::core::signature_or_default(&repo)?;

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
}
