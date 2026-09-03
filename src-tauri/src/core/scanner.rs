use std::collections::HashMap;
use std::ffi::OsStr;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::UNIX_EPOCH;

use rayon::prelude::*;
use walkdir::{DirEntry, WalkDir};

use crate::models::repository::ScannedRepo;

/// Multi-threaded Git repository scanner.
/// Recursively traverses directories to find `.git` markers, using rayon
/// for parallel validation of candidate repositories. A `.git` marker is a
/// directory for a normal repository, or a *file* (gitdir pointer) for a
/// linked worktree / submodule checkout (T-17).
///
/// Traversal rules:
/// - Descends up to `scan_depth` levels deep
/// - Never follows symlinks (avoids cycles / escaping the workspace)
/// - Skips known non-repo directories (node_modules, target, ...)
/// - Skips directories matched by the workspace-level `.gitworkspaceignore`
/// - Validates candidates as real Git repositories via libgit2
pub struct RepoScanner {
    scan_depth: usize,
}

impl RepoScanner {
    /// Create a scanner with the given maximum recursion depth.
    pub fn new(scan_depth: usize) -> Self {
        Self {
            scan_depth: scan_depth.max(1),
        }
    }

    /// Scan the given root directory for Git repositories (full validation).
    pub fn scan(&self, root: &Path) -> Vec<ScannedRepo> {
        self.scan_internal(root, None, None)
    }

    /// Scan with an optional cancellation flag. When the flag is set,
    /// traversal stops early and returns the repositories found so far.
    ///
    /// Public API reserved for the UI cancel button (T-01 cancellation wiring);
    /// exercised by unit tests today, hence the allow.
    #[allow(dead_code)]
    pub fn scan_cancellable(&self, root: &Path, cancel: Option<&AtomicBool>) -> Vec<ScannedRepo> {
        self.scan_internal(root, cancel, None)
    }

    /// Incremental scan.
    ///
    /// `known` maps a previously discovered repository path to the `.git`
    /// directory mtime (unix millis) recorded at the last successful scan. A
    /// candidate whose path is known and whose mtime is unchanged skips the
    /// `git2::Repository::open` validation — the dominant per-repo cost — so a
    /// rescan that finds no new or removed repositories only walks the
    /// filesystem instead of re-opening every repository.
    pub fn scan_incremental(
        &self,
        root: &Path,
        cancel: Option<&AtomicBool>,
        known: &HashMap<String, Option<i64>>,
    ) -> Vec<ScannedRepo> {
        self.scan_internal(root, cancel, Some(known))
    }

    fn scan_internal(
        &self,
        root: &Path,
        cancel: Option<&AtomicBool>,
        known: Option<&HashMap<String, Option<i64>>>,
    ) -> Vec<ScannedRepo> {
        log::debug!("Scanning {:?} with depth {}", root, self.scan_depth);

        let ignore = IgnoreRules::load(root);

        let mut walker = WalkDir::new(root)
            .max_depth(self.scan_depth)
            .follow_links(false)
            .into_iter();

        let mut entries: Vec<DirEntry> = Vec::new();

        loop {
            if cancel.map_or(false, |c| c.load(Ordering::Relaxed)) {
                log::info!("Scan cancelled for {:?}", root);
                return Vec::new();
            }

            match walker.next() {
                Some(Ok(entry)) => {
                    if entry.file_type().is_dir() {
                        let name = entry.file_name();

                        // Found a .git directory - keep it but don't descend
                        if name == OsStr::new(".git") {
                            entries.push(entry);
                            walker.skip_current_dir();
                            continue;
                        }

                        // Skip default non-repo dirs and .gitworkspaceignore matches
                        let rel = entry
                            .path()
                            .strip_prefix(root)
                            .map(|p| p.to_string_lossy().to_string())
                            .unwrap_or_default();
                        let name_str = name.to_string_lossy();
                        if is_skip_dir(name) || ignore.is_ignored(&name_str, &rel) {
                            walker.skip_current_dir();
                            continue;
                        }
                    }
                    entries.push(entry);
                }
                Some(Err(e)) => {
                    log::warn!("WalkDir error: {}", e);
                    continue;
                }
                None => break,
            }
        }

        log::debug!("Collected {} directory entries", entries.len());

        // Parallel: filter for .git markers and validate as Git repos.
        // A `.git` entry is a directory for a normal repository and a *file*
        // (gitdir pointer) for a linked worktree / submodule checkout (T-17).
        let repos: Vec<ScannedRepo> = entries
            .par_iter()
            .filter_map(|entry| {
                if cancel.map_or(false, |c| c.load(Ordering::Relaxed)) {
                    return None;
                }
                let path = entry.path();
                if !(entry.file_type().is_dir() || entry.file_type().is_file()) {
                    return None;
                }
                if entry.file_name() != OsStr::new(".git") {
                    return None;
                }

                let parent = path.parent()?;
                let relative = parent.strip_prefix(root).ok()?;
                let parent_str = parent.to_string_lossy().to_string();

                // Record the `.git` directory mtime as the incremental-scan key.
                let mtime = dir_mtime_millis(path);

                // Incremental fast-path: a repository we have seen before whose
                // `.git` mtime is unchanged skips libgit2 validation entirely.
                let known_mtime = known.and_then(|k| k.get(&parent_str)).copied().flatten();
                let skip_validation = known_mtime.is_some() && known_mtime == mtime;

                // Validate: can we open this as a Git repository?
                // (libgit2 Repository is opened and dropped per-thread; never
                // shared across threads.)
                if !skip_validation && git2::Repository::open(parent).is_err() {
                    return None;
                }

                let name = parent
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("unknown")
                    .to_string();

                Some(ScannedRepo {
                    path: parent_str,
                    name,
                    relative_path: relative.to_string_lossy().to_string(),
                    git_dir_mtime: mtime,
                })
            })
            .collect();

        log::info!("Found {} Git repositories under {:?}", repos.len(), root);
        repos
    }
}

/// Workspace-level ignore rules loaded from `.gitworkspaceignore`.
///
/// Each non-empty, non-comment line is a pattern:
/// - `vendor/`        → bare directory name, ignored anywhere
/// - `third_party/`   → same
/// - `generated/`     → same
/// - `sub/dir/`       → relative path prefix, ignored under that prefix
pub(crate) struct IgnoreRules {
    /// Bare directory names (no `/`), matched against the entry's file name.
    names: Vec<String>,
    /// Relative path prefixes (contain `/`), stored with a trailing `/`.
    path_prefixes: Vec<String>,
}

impl IgnoreRules {
    pub(crate) fn load(root: &Path) -> Self {
        let mut names = Vec::new();
        let mut path_prefixes = Vec::new();
        if let Ok(content) = std::fs::read_to_string(root.join(".gitworkspaceignore")) {
            for line in content.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                let pat = line.trim_end_matches('/');
                if pat.contains('/') {
                    let mut prefix = pat.to_string();
                    prefix.push('/');
                    path_prefixes.push(prefix);
                } else {
                    names.push(pat.to_string());
                }
            }
        }
        IgnoreRules { names, path_prefixes }
    }

    pub(crate) fn is_ignored(&self, name: &str, relative: &str) -> bool {
        if self.names.iter().any(|n| n == name) {
            return true;
        }
        self.path_prefixes.iter().any(|p| relative.starts_with(p.as_str()))
    }
}

/// Check if a directory name should be skipped during traversal.
/// These are common build output / dependency directories that
/// will never contain useful Git repositories and slow down scanning.
pub(crate) fn is_skip_dir(name: &OsStr) -> bool {
    matches!(
        name.to_str(),
        Some("node_modules")
            | Some("target")
            | Some("dist")
            | Some("build")
            | Some(".next")
            | Some(".nuxt")
            | Some(".cache")
            | Some("__pycache__")
            | Some(".gradle")
            | Some(".m2")
            | Some("venv")
            | Some(".venv")
    )
}

/// Read the mtime of a `.git` directory as unix milliseconds.
///
/// Returns `None` when the entry cannot be stat'ed (e.g. a broken symlink is
/// already excluded by `follow_links(false)`, but a race with an external
/// delete could still make this fail — treat it as "unknown", which forces
/// validation rather than trusting a stale cache).
fn dir_mtime_millis(path: &Path) -> Option<i64> {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::Arc;

    #[test]
    fn test_skip_dir_detection() {
        assert!(is_skip_dir(OsStr::new("node_modules")));
        assert!(is_skip_dir(OsStr::new("target")));
        assert!(!is_skip_dir(OsStr::new("src")));
        assert!(!is_skip_dir(OsStr::new(".git")));
    }

    #[test]
    fn ignore_rules_match_names_and_path_prefixes() {
        let rules = IgnoreRules {
            names: vec!["vendor".to_string(), "third_party".to_string()],
            path_prefixes: vec!["generated/".to_string()],
        };

        assert!(rules.is_ignored("vendor", "vendor"));
        assert!(rules.is_ignored("vendor", "a/b/vendor"));
        assert!(rules.is_ignored("third_party", "third_party"));
        assert!(rules.is_ignored("generated", "generated/x"));
        assert!(!rules.is_ignored("src", "src"));
        assert!(!rules.is_ignored("generated2", "generated2"));
    }

    #[test]
    fn ignore_rules_load_from_file() {
        let dir = std::env::temp_dir().join(format!(
            "gw_ignore_test_{}",
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join(".gitworkspaceignore"),
            "# comment\nvendor/\nthird_party/\n\nsub/dir/\n",
        )
        .unwrap();

        let rules = IgnoreRules::load(&dir);
        assert_eq!(rules.names, vec!["vendor", "third_party"]);
        assert_eq!(rules.path_prefixes, vec!["sub/dir/"]);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn cancelled_scan_returns_empty() {
        let flag = Arc::new(AtomicBool::new(true));
        let scanner = RepoScanner::new(3);
        let result = scanner.scan_cancellable(Path::new("."), Some(&flag));
        assert!(result.is_empty());
    }

    /// A known, unchanged repository must skip `git2::Repository::open`
    /// validation on an incremental scan. We prove the skip by registering a
    /// *fake* `.git` directory (not a real repository) as "known": a full scan
    /// filters it out, while an incremental scan admits it because validation
    /// is skipped on mtime match.
    #[test]
    fn incremental_scan_skips_validation_for_known_unchanged_repos() {
        let dir = std::env::temp_dir().join(format!(
            "gw_incr_test_{}",
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        fs::create_dir_all(&dir).unwrap();

        // Real repo "a" + fake `.git` directory "b" (empty, not a repository).
        let repo_a = dir.join("a");
        git2::Repository::init(&repo_a).unwrap();
        let fake = dir.join("b");
        fs::create_dir_all(fake.join(".git")).unwrap();

        let scanner = RepoScanner::new(3);

        // Full scan validates every `.git` and drops the fake one.
        let full = scanner.scan(&dir);
        assert_eq!(full.len(), 1, "full scan must drop non-repo .git dirs");
        assert_eq!(full[0].relative_path, "a");

        // Incremental scan trusts the known path+mtime for "b" and skips open.
        let b_path = fake.to_string_lossy().to_string();
        let b_mtime = dir_mtime_millis(&fake.join(".git"));
        let mut known = HashMap::new();
        known.insert(b_path, b_mtime);

        let inc = scanner.scan_incremental(&dir, None, &known);
        let mut rels: Vec<&str> = inc.iter().map(|r| r.relative_path.as_str()).collect();
        rels.sort_unstable();
        assert_eq!(rels, vec!["a", "b"], "incremental scan must skip validation");

        let _ = fs::remove_dir_all(&dir);
    }

    /// A known path whose mtime changed must be re-validated (not skipped).
    #[test]
    fn incremental_scan_revalidates_changed_mtime() {
        let dir = std::env::temp_dir().join(format!(
            "gw_incr_chg_{}",
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        fs::create_dir_all(&dir).unwrap();

        // Fake `.git` dir; register a stale (wrong) mtime so validation runs.
        let fake = dir.join("b");
        fs::create_dir_all(fake.join(".git")).unwrap();
        let stale_mtime = Some(0i64);

        let mut known = HashMap::new();
        known.insert(fake.to_string_lossy().to_string(), stale_mtime);

        let scanner = RepoScanner::new(3);
        let inc = scanner.scan_incremental(&dir, None, &known);
        assert!(inc.is_empty(), "changed mtime must force re-validation");

        let _ = fs::remove_dir_all(&dir);
    }

    /// A linked worktree (`.git` *file*, gitdir pointer) must be discovered
    /// as a repository (T-17 / T-01 linkage).
    #[test]
    fn scan_discovers_worktree_gitfile_form() {
        let dir = std::env::temp_dir().join(format!(
            "gw_wtscan_{}",
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        fs::create_dir_all(&dir).unwrap();

        // Main repo + a linked worktree alongside it.
        let main = dir.join("main_repo");
        git2::Repository::init(&main).unwrap();
        std::fs::write(
            main.join("a.txt"),
            "one
",
        )
        .unwrap();
        {
            let repo = git2::Repository::open(&main).unwrap();
            let mut index = repo.index().unwrap();
            index.add_path(Path::new("a.txt")).unwrap();
            index.write().unwrap();
            let tree_oid = index.write_tree().unwrap();
            let tree = repo.find_tree(tree_oid).unwrap();
            let sig = git2::Signature::now("t", "t@e.c").unwrap();
            repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[]).unwrap();
        }
        let wt = dir.join("linked_wt");
        crate::core::worktree::add_worktree(&main, &wt, None, Some("wtbr")).unwrap();
        assert!(wt.join(".git").is_file());

        let scanner = RepoScanner::new(3);
        let found = scanner.scan(&dir);
        let mut rels: Vec<&str> = found.iter().map(|r| r.relative_path.as_str()).collect();
        rels.sort_unstable();
        assert_eq!(
            rels,
            vec!["linked_wt", "main_repo"],
            "worktree .git file must be discovered"
        );

        crate::core::worktree::remove_worktree(&main, "linked_wt", true).unwrap();
        let _ = fs::remove_dir_all(&dir);
    }
}
