use std::ffi::OsStr;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use rayon::prelude::*;
use walkdir::{DirEntry, WalkDir};

use crate::models::repository::ScannedRepo;

/// Multi-threaded Git repository scanner.
/// Recursively traverses directories to find `.git` folders, using rayon
/// for parallel validation of candidate repositories.
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

    /// Scan the given root directory for Git repositories.
    pub fn scan(&self, root: &Path) -> Vec<ScannedRepo> {
        self.scan_cancellable(root, None)
    }

    /// Scan with an optional cancellation flag. When the flag is set,
    /// traversal stops early and returns the repositories found so far.
    pub fn scan_cancellable(
        &self,
        root: &Path,
        cancel: Option<&AtomicBool>,
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

        // Parallel: filter for .git dirs and validate as Git repos.
        let repos: Vec<ScannedRepo> = entries
            .par_iter()
            .filter_map(|entry| {
                if cancel.map_or(false, |c| c.load(Ordering::Relaxed)) {
                    return None;
                }
                let path = entry.path();
                if !entry.file_type().is_dir() {
                    return None;
                }
                if entry.file_name() != OsStr::new(".git") {
                    return None;
                }

                let parent = path.parent()?;
                let relative = parent.strip_prefix(root).ok()?;

                // Validate: can we open this as a Git repository?
                // (libgit2 Repository is opened and dropped per-thread; never
                // shared across threads.)
                if git2::Repository::open(parent).is_err() {
                    return None;
                }

                let name = parent
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("unknown")
                    .to_string();

                Some(ScannedRepo {
                    path: parent.to_string_lossy().to_string(),
                    name,
                    relative_path: relative.to_string_lossy().to_string(),
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
struct IgnoreRules {
    /// Bare directory names (no `/`), matched against the entry's file name.
    names: Vec<String>,
    /// Relative path prefixes (contain `/`), stored with a trailing `/`.
    path_prefixes: Vec<String>,
}

impl IgnoreRules {
    fn load(root: &Path) -> Self {
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

    fn is_ignored(&self, name: &str, relative: &str) -> bool {
        if self.names.iter().any(|n| n == name) {
            return true;
        }
        self.path_prefixes
            .iter()
            .any(|p| relative.starts_with(p.as_str()))
    }
}

/// Check if a directory name should be skipped during traversal.
/// These are common build output / dependency directories that
/// will never contain useful Git repositories and slow down scanning.
fn is_skip_dir(name: &OsStr) -> bool {
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
}
