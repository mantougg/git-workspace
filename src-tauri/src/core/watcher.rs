use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use moka::sync::Cache;
use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher};
use tauri::{AppHandle, Emitter};
use tokio::sync::mpsc;

use crate::core::git_status;
use crate::error::AppResult;
use crate::models::repository::{RepoStatus, RepoStatusUpdate};

/// Debounce window per repository: same-repo change bursts within this window
/// are merged into a single status refresh.
const DEBOUNCE_MS: u64 = 500;
/// Cross-repo batch window: status updates are buffered and flushed as one IPC
/// event per window, so hundreds of concurrent repo changes stay responsive.
const BATCH_MS: u64 = 100;

/// File watcher that monitors repository directories for changes
/// and triggers incremental status refreshes.
///
/// Uses the `notify` crate for cross-platform filesystem watching. Watches each
/// repository root plus its `.git` directory (NonRecursive), and supports
/// mounting/unmounting repositories incrementally instead of rebuilding.
pub struct FileWatcher {
    watcher: Option<RecommendedWatcher>,
    /// Repository roots currently watched, shared with the event loop so it can
    /// map changed paths → repos without a stale snapshot.
    watched: Arc<Mutex<HashSet<PathBuf>>>,
    started: bool,
}

impl FileWatcher {
    pub fn new() -> Self {
        FileWatcher {
            watcher: None,
            watched: Arc::new(Mutex::new(HashSet::new())),
            started: false,
        }
    }

    /// (Re)configure the set of watched repositories.
    ///
    /// On the first call it boots the OS watcher and event loop; on later calls
    /// it diffs against the current set and mounts only newly-added repositories
    /// and unmounts removed ones — no full rebuild.
    pub fn watch_repositories(
        &mut self,
        repo_paths: Vec<PathBuf>,
        status_cache: Arc<Cache<String, RepoStatus>>,
        app_handle: AppHandle,
    ) -> AppResult<()> {
        let next: HashSet<PathBuf> = repo_paths.into_iter().collect();

        if !self.started {
            self.boot(status_cache, app_handle)?;
            self.started = true;
        }

        // Compute the delta, then update the shared set.
        let (to_add, to_remove) = {
            let mut watched = self.watched.lock().unwrap();
            let (to_add, to_remove) = diff_watch_sets(&watched, &next);
            for p in &to_add {
                watched.insert(p.clone());
            }
            for p in &to_remove {
                watched.remove(p);
            }
            (to_add, to_remove)
        };

        if let Some(watcher) = self.watcher.as_mut() {
            mount(watcher, &to_add);
            unmount(watcher, &to_remove);
        }

        let total = self.watched.lock().unwrap().len();
        log::info!(
            "File watcher watching {} repositories ({} added, {} removed)",
            total,
            to_add.len(),
            to_remove.len()
        );
        Ok(())
    }

    /// Whether the watcher has been started (used by `scan_repositories` to
    /// auto-sync newly discovered / removed repositories).
    pub fn is_running(&self) -> bool {
        self.started
    }

    fn boot(
        &mut self,
        status_cache: Arc<Cache<String, RepoStatus>>,
        app_handle: AppHandle,
    ) -> AppResult<()> {
        let (tx, rx) = mpsc::unbounded_channel::<Vec<PathBuf>>();

        // OS-native watcher (ReadDirectoryChangesW / inotify / FSEvents);
        // `RecommendedWatcher` picks the best backend for the current platform.
        let watcher = RecommendedWatcher::new(
            move |res: Result<notify::Event, notify::Error>| {
                if let Ok(event) = res {
                    let _ = tx.send(event.paths);
                }
            },
            Config::default(),
        )?;

        let watched = Arc::clone(&self.watched);
        tauri::async_runtime::spawn(async move {
            run_event_loop(rx, status_cache, app_handle, watched).await;
        });

        self.watcher = Some(watcher);
        Ok(())
    }

    /// Stop all watching.
    pub fn stop(&mut self) {
        if self.watcher.take().is_some() {
            self.started = false;
            if let Ok(mut watched) = self.watched.lock() {
                watched.clear();
            }
            log::info!("File watcher stopped");
        }
    }
}

impl Default for FileWatcher {
    fn default() -> Self {
        Self::new()
    }
}

/// Compute the set difference between the currently-watched roots and the
/// desired set: `(to_add, to_remove)`.
fn diff_watch_sets(
    current: &HashSet<PathBuf>,
    next: &HashSet<PathBuf>,
) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let to_add: Vec<PathBuf> = next.difference(current).cloned().collect();
    let to_remove: Vec<PathBuf> = current.difference(next).cloned().collect();
    (to_add, to_remove)
}

/// Start watching each repository root and its `.git` directory (NonRecursive).
fn mount(watcher: &mut RecommendedWatcher, paths: &[PathBuf]) {
    for path in paths {
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
}

/// Stop watching each repository root and its `.git` directory.
fn unmount(watcher: &mut RecommendedWatcher, paths: &[PathBuf]) {
    for path in paths {
        let _ = watcher.unwatch(&path.join(".git"));
        let _ = watcher.unwatch(path);
    }
}

/// Event loop: consumes filesystem change events, maps them to affected
/// repositories (via T-02's `find_affected_repos`), refreshes statuses with a
/// per-repo debounce, and flushes batched `repo_status_changed_batch` events.
async fn run_event_loop(
    mut rx: mpsc::UnboundedReceiver<Vec<PathBuf>>,
    cache: Arc<Cache<String, RepoStatus>>,
    handle: AppHandle,
    watched: Arc<Mutex<HashSet<PathBuf>>>,
) {
    let debounce = Duration::from_millis(DEBOUNCE_MS);
    let mut last_refresh: HashMap<PathBuf, Instant> = HashMap::new();
    let mut pending: Vec<RepoStatusUpdate> = Vec::new();
    let mut flush = tokio::time::interval(Duration::from_millis(BATCH_MS));

    loop {
        tokio::select! {
            maybe = rx.recv() => {
                let Some(changed_paths) = maybe else { break };

                // Map changed paths → affected repo roots (shared set).
                let (roots, changed) = {
                    let watched = watched.lock().unwrap();
                    let roots: Vec<String> = watched
                        .iter()
                        .map(|p| p.to_string_lossy().to_string())
                        .collect();
                    let changed: Vec<String> = changed_paths
                        .iter()
                        .map(|p| p.to_string_lossy().to_string())
                        .collect();
                    (roots, changed)
                };

                let now = Instant::now();
                for root in git_status::find_affected_repos(&changed, &roots) {
                    let repo_path = PathBuf::from(root);

                    // Debounce: skip if this repo was refreshed recently.
                    if let Some(&last) = last_refresh.get(&repo_path) {
                        if now.duration_since(last) < debounce {
                            continue;
                        }
                    }
                    last_refresh.insert(repo_path.clone(), now);

                    // Incremental refresh: recompute this repo's status off the
                    // async worker thread (libgit2 + file IO are blocking).
                    let path_str = repo_path.to_string_lossy().to_string();
                    let repo_for_blocking = repo_path.clone();
                    match tokio::task::spawn_blocking(move || {
                        git_status::get_repo_status(&repo_for_blocking)
                    })
                    .await
                    {
                        Ok(Ok(new_status)) => {
                            cache.insert(path_str.clone(), new_status.clone());
                            pending.push(RepoStatusUpdate {
                                repo_path: path_str,
                                status: new_status,
                            });
                        }
                        Ok(Err(e)) => log::warn!(
                            "Failed to refresh status for {:?}: {}",
                            repo_path,
                            e
                        ),
                        Err(e) => log::warn!(
                            "Status refresh task failed for {:?}: {}",
                            repo_path,
                            e
                        ),
                    }
                }
            }
            _ = flush.tick() => {
                if !pending.is_empty() {
                    if let Err(e) = handle.emit("repo_status_changed_batch", &pending) {
                        log::warn!("Failed to emit repo_status_changed_batch: {}", e);
                    }
                    pending.clear();
                }
            }
        }
    }

    // Drain any remaining updates before the loop exits.
    if !pending.is_empty() {
        let _ = handle.emit("repo_status_changed_batch", &pending);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diff_watch_sets_splits_added_and_removed() {
        let current: HashSet<PathBuf> = ["D:/ws/a", "D:/ws/b"]
            .iter()
            .map(PathBuf::from)
            .collect();
        let next: HashSet<PathBuf> = ["D:/ws/b", "D:/ws/c"]
            .iter()
            .map(PathBuf::from)
            .collect();

        let (to_add, to_remove) = diff_watch_sets(&current, &next);

        assert_eq!(to_add, vec![PathBuf::from("D:/ws/c")]);
        assert_eq!(to_remove, vec![PathBuf::from("D:/ws/a")]);
    }

    #[test]
    fn diff_watch_sets_identical_yields_no_change() {
        let set: HashSet<PathBuf> = ["D:/ws/a"].iter().map(PathBuf::from).collect();
        let (to_add, to_remove) = diff_watch_sets(&set, &set);
        assert!(to_add.is_empty());
        assert!(to_remove.is_empty());
    }
}
