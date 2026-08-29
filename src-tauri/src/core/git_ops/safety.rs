use std::path::Path;

use crate::core::secret;
use crate::error::AppResult;
use crate::models::commit::CommitScanFinding;

/// Files larger than this are flagged by the pre-commit Large File Scan
/// (T-11; global constraint §5 Commit 安全检查).
const MAX_COMMIT_FILE_BYTES: u64 = 5 * 1024 * 1024;

/// Secret Scan reads at most this much text per file (keeps the synchronous
/// pre-commit scan cheap on big-but-allowed files).
const SECRET_SCAN_MAX_BYTES: usize = 1024 * 1024;

/// Paths whose content will be committed: the staged set for `index_only`,
/// the explicit file list, or every changed path for stage-all.
pub(super) fn paths_for_scan(
    repo: &git2::Repository,
    files: &[String],
    index_only: bool,
) -> AppResult<Vec<String>> {
    if index_only {
        let head_tree = crate::core::diff::head_or_empty_tree(repo)?;
        let staged = repo.diff_tree_to_index(Some(&head_tree), None, None)?;
        return Ok(staged
            .deltas()
            .filter_map(|d| d.new_file().path().map(|p| p.to_string_lossy().to_string()))
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
pub(super) fn scan_paths(repo: &git2::Repository, paths: &[String]) -> Vec<CommitScanFinding> {
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
            .or_else(|| std::fs::read(repo.workdir()?.join(path)).ok());

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
pub(super) fn format_findings(findings: &[CommitScanFinding]) -> String {
    findings
        .iter()
        .take(5)
        .map(|f| format!("{}: {}", f.path, f.detail))
        .collect::<Vec<_>>()
        .join("\n")
}
