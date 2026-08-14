//! Reflog reading (T-14). Local, lightweight, no caching (per the task spec);
//! recovery operations are reuses of T-09 `create_branch` and T-13 `reset_to`
//! and are not duplicated here.

use std::path::Path;

use serde::Serialize;

use crate::error::{AppError, AppResult};
use crate::core::graph;

/// A single reflog line (newest first).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReflogEntry {
    /// 0-based position from the tip (the `N` in `HEAD@{N}`).
    pub index: usize,
    /// Display selector, e.g. `HEAD@{0}` or `main@{2}`.
    pub selector: String,
    pub old_oid: String,
    pub new_oid: String,
    /// Reflog message, e.g. "commit: add x" / "reset: moving to abc".
    pub summary: String,
    /// First line of the commit the ref moved to (empty if unparseable).
    pub commit_message: String,
    /// Entry committer time, formatted like commit times in the graph view.
    pub time: String,
}

/// Read the reflog of `reference` ("HEAD", a full ref like "refs/heads/main",
/// or a shorthand like "main" / "origin/main"), newest first, capped at
/// `max` entries.
pub fn read_reflog(
    repo_path: &Path,
    reference: Option<&str>,
    max: usize,
) -> AppResult<Vec<ReflogEntry>> {
    let repo = git2::Repository::open(repo_path)?;
    let (full_ref, display) = resolve_ref_name(reference);

    // libgit2 returns an empty reflog for unknown refs rather than erroring;
    // check existence first so the UI gets a structured NotFound.
    if full_ref != "HEAD" {
        repo.find_reference(&full_ref)
            .map_err(|_| AppError::NotFound(format!("reference '{}' not found", display)))?;
    }

    let reflog = repo
        .reflog(&full_ref)
        .map_err(|_| AppError::NotFound(format!("no reflog for '{}'", display)))?;

    let mut entries = Vec::new();
    for (i, entry) in reflog.iter().take(max).enumerate() {
        let new_oid = entry.id_new();
        let commit_message = repo
            .find_commit(new_oid)
            .ok()
            .and_then(|c| c.summary().map(String::from))
            .unwrap_or_default();
        let when = entry.committer().when();
        entries.push(ReflogEntry {
            index: i,
            selector: format!("{}@{{{}}}", display, i),
            old_oid: entry.id_old().to_string(),
            new_oid: new_oid.to_string(),
            summary: entry.message().unwrap_or_default().to_string(),
            commit_message,
            time: graph::format_commit_time(when.seconds(), when.offset_minutes()),
        });
    }
    Ok(entries)
}

/// Map a user-facing reference choice to (full ref name, display name).
/// Accepts "HEAD" (default), full refs (refs/heads/...), or shorthands
/// (branch name / origin/branch).
fn resolve_ref_name(reference: Option<&str>) -> (String, String) {
    let r = reference.unwrap_or("HEAD").trim();
    if r.is_empty() || r == "HEAD" {
        return ("HEAD".to_string(), "HEAD".to_string());
    }
    if r.starts_with("refs/") {
        let display = r
            .strip_prefix("refs/heads/")
            .or_else(|| r.strip_prefix("refs/remotes/"))
            .unwrap_or(r);
        return (r.to_string(), display.to_string());
    }
    if r.contains('/') {
        // "origin/main" style shorthand -> remote-tracking ref.
        return (format!("refs/remotes/{}", r), r.to_string());
    }
    (format!("refs/heads/{}", r), r.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "gw_reflog_{}_{}",
            tag,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn commit_file(
        repo: &git2::Repository,
        dir: &Path,
        name: &str,
        content: &str,
        msg: &str,
    ) -> String {
        std::fs::write(dir.join(name), content).unwrap();
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
        repo.commit(Some("HEAD"), &sig, &sig, msg, &tree, &parents)
            .unwrap()
            .to_string()
    }

    /// HEAD and branch reflogs record commits with selectors and messages.
    #[test]
    fn reflog_records_commits() {
        let dir = tmpdir("basic");
        let second_oid;
        {
            let repo = git2::Repository::init(&dir).unwrap();
            commit_file(&repo, &dir, "a.txt", "one\n", "init");
            second_oid = commit_file(&repo, &dir, "b.txt", "two\n", "second");
            drop(repo);
        }

        let head_entries = read_reflog(&dir, None, 50).unwrap();
        assert_eq!(head_entries.len(), 2);
        assert_eq!(head_entries[0].selector, "HEAD@{0}");
        assert_eq!(head_entries[0].new_oid, second_oid);
        assert!(head_entries[0].summary.contains("second"));
        assert_eq!(head_entries[0].commit_message, "second");
        assert_eq!(head_entries[1].commit_message, "init");

        // Branch shorthand resolves to refs/heads/<name>.
        let branch_entries = read_reflog(&dir, Some("master"), 50).unwrap();
        assert_eq!(branch_entries.len(), 2);
        assert_eq!(branch_entries[0].selector, "master@{0}");

        // Unknown ref -> structured NotFound error.
        assert!(read_reflog(&dir, Some("nope"), 50).is_err());

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A remote-tracking ref's reflog is readable via the origin/x shorthand.
    #[test]
    fn remote_reflog_is_listed() {
        let dir = tmpdir("remote");
        let oid;
        {
            let repo = git2::Repository::init(&dir).unwrap();
            oid = commit_file(&repo, &dir, "a.txt", "one\n", "init");
            repo.reference(
                "refs/remotes/origin/master",
                git2::Oid::from_str(&oid).unwrap(),
                true,
                "fetch: fast-forward",
            )
            .unwrap();
            drop(repo);
        }

        let entries = read_reflog(&dir, Some("origin/master"), 50).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].selector, "origin/master@{0}");
        assert_eq!(entries[0].new_oid, oid);
        assert!(entries[0].summary.contains("fast-forward"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Acceptance: a mistaken reset is recoverable through the reflog.
    #[test]
    fn mistaken_reset_is_recoverable_via_reflog() {
        let dir = tmpdir("recover");
        let second_oid;
        {
            let repo = git2::Repository::init(&dir).unwrap();
            let first = commit_file(&repo, &dir, "a.txt", "one\n", "init");
            second_oid = commit_file(&repo, &dir, "b.txt", "two\n", "second");
            drop(repo);
            // Mistaken hard reset back to the first commit.
            crate::core::history::reset_to(&dir, Some(&first), "hard").unwrap();
        }
        assert!(!dir.join("b.txt").exists());

        // Reflog still points at the lost commit; resetting to it restores.
        let entries = read_reflog(&dir, None, 50).unwrap();
        let lost = entries
            .iter()
            .find(|e| e.new_oid == second_oid)
            .expect("reflog must still reference the reset-away commit");
        crate::core::history::reset_to(&dir, Some(&lost.new_oid), "hard").unwrap();

        assert!(dir.join("b.txt").exists());
        let repo = git2::Repository::open(&dir).unwrap();
        assert_eq!(
            repo.head().unwrap().target().unwrap().to_string(),
            second_oid
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
