use std::ffi::OsStr;
use std::path::Path;

use rayon::prelude::*;
use walkdir::{DirEntry, WalkDir};

use crate::models::repository::ScannedRepo;

/// Multi-threaded Git repository scanner.
/// Recursively traverses directories to find `.git` folders, using rayon
/// for parallel validation of candidate repositories.
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
    ///
    /// Traversal rules:
    /// - Descends up to `scan_depth` levels deep
    /// - Skips known non-repo directories (node_modules, target, .next, etc.)
    /// - Finds `.git` directories and validates they are real repos
    /// - Does not descend into `.git` directories
    /// - Uses rayon to parallelize repository validation
    pub fn scan(&self, root: &Path) -> Vec<ScannedRepo> {
        log::debug!("Scanning {:?} with depth {}", root, self.scan_depth);

        let mut walker = WalkDir::new(root)
            .max_depth(self.scan_depth)
            .follow_links(false)
            .into_iter();

        let mut entries: Vec<DirEntry> = Vec::new();

        loop {
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

                        // Skip common non-repo directories entirely
                        if is_skip_dir(name) {
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

        // Parallel: filter for .git dirs and validate as Git repos
        let repos: Vec<ScannedRepo> = entries
            .par_iter()
            .filter_map(|entry| {
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

    #[test]
    fn test_skip_dir_detection() {
        assert!(is_skip_dir(OsStr::new("node_modules")));
        assert!(is_skip_dir(OsStr::new("target")));
        assert!(!is_skip_dir(OsStr::new("src")));
        assert!(!is_skip_dir(OsStr::new(".git")));
    }
}
