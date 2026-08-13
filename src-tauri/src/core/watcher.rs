use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use dashmap::DashMap;
use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher};
use tauri::{AppHandle, Emitter};

use crate::core::git_status;
use crate::error::AppResult;
use crate::models::repository::{RepoStatus, RepoStatusUpdate};

/// File watcher that monitors repository directories for changes
/// and triggers incremental status refreshes.
///
/// Uses the `notify` crate for cross-platform filesystem watching.
/// Implements its own debouncing (500ms) to batch rapid changes.
pub struct FileWatcher {
    watcher: Option<RecommendedWatcher>,
}

impl FileWatcher {
    pub fn new() -> Self {
        FileWatcher { watcher: None }
    }

    /// Start watching a list of repository directories.
    ///
    /// When files change in a watched repo, the status is refreshed
    /// and a `repo_status_changed` event is emitted to the frontend.
    pub fn watch_repositories(
        &mut self,
        repo_paths: Vec<PathBuf>,
        status_cache: Arc<DashMap<String, RepoStatus>>,
        app_handle: AppHandle,
    ) -> AppResult<()> {
        // Stop any existing watcher
        if self.watcher.is_some() {
            self.watcher.take();
        }

        let (tx, mut rx) = tokio::sync::mpsc::channel::<Vec<PathBuf>>(256);

        // Use the OS-native watcher (ReadDirectoryChangesW / inotify / FSEvents);
        // `RecommendedWatcher` picks the best backend for the current platform.
        let mut watcher = RecommendedWatcher::new(
            move |res: Result<notify::Event, notify::Error>| {
                if let Ok(event) = res {
                    let paths: Vec<PathBuf> = event.paths;
                    let _ = tx.blocking_send(paths);
                }
            },
            Config::default(),
        )?;

        // Watch each repository's working directory and .git directory
        let repo_paths_clone = repo_paths.clone();
        for path in &repo_paths {
            if let Err(e) = watcher.watch(path, RecursiveMode::NonRecursive) {
                log::warn!("Failed to watch {:?}: {}", path, e);
            }
            let git_dir = path.join(".git");
            if git_dir.exists() {
                if let Err(e) = watcher.watch(&git_dir, RecursiveMode::NonRecursive) {
                    log::warn!("Failed to watch {:?}: {}", git_dir, e);
                }
            }
        }

        self.watcher = Some(watcher);

        // Spawn the event processor with debouncing
        let cache = Arc::clone(&status_cache);
        let handle = app_handle;
        let watched = repo_paths_clone;

        tauri::async_runtime::spawn(async move {
            // Track last refresh time per repo for debouncing
            let mut last_refresh: std::collections::HashMap<PathBuf, Instant> =
                std::collections::HashMap::new();
            let debounce_interval = Duration::from_millis(500);

            while let Some(changed_paths) = rx.recv().await {
                // Determine which repositories were affected
                let affected: HashSet<PathBuf> = changed_paths
                    .iter()
                    .filter_map(|p| find_repo_root(p, &watched).map(|r| r.to_path_buf()))
                    .collect();

                let now = Instant::now();

                for repo_path in affected {
                    // Debounce: skip if we refreshed this repo recently
                    if let Some(&last) = last_refresh.get(&repo_path) {
                        if now.duration_since(last) < debounce_interval {
                            continue;
                        }
                    }

                    last_refresh.insert(repo_path.clone(), now);

                    // Incremental refresh: only update this repo's status
                    match git_status::get_repo_status(&repo_path) {
                        Ok(new_status) => {
                            let path_str = repo_path.to_string_lossy().to_string();
                            cache.insert(path_str.clone(), new_status.clone());

                            let update = RepoStatusUpdate {
                                repo_path: path_str,
                                status: new_status,
                            };

                            if let Err(e) = handle.emit("repo_status_changed", &update) {
                                log::warn!("Failed to emit repo_status_changed: {}", e);
                            }
                        }
                        Err(e) => {
                            log::warn!(
                                "Failed to refresh status for {:?}: {}",
                                repo_path,
                                e
                            );
                        }
                    }
                }
            }
        });

        log::info!("File watcher started for {} repositories", repo_paths.len());
        Ok(())
    }

    /// Stop all watching.
    pub fn stop(&mut self) {
        if self.watcher.take().is_some() {
            log::info!("File watcher stopped");
        }
    }
}

impl Default for FileWatcher {
    fn default() -> Self {
        Self::new()
    }
}

/// Given a changed file path and a list of watched repo roots,
/// find which repo root the path belongs to.
fn find_repo_root<'a>(changed_path: &Path, repo_paths: &'a [PathBuf]) -> Option<&'a Path> {
    for repo_path in repo_paths {
        if changed_path.starts_with(repo_path) {
            return Some(repo_path.as_path());
        }
    }
    None
}
