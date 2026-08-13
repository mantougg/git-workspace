use std::sync::{Arc, Mutex};

use moka::sync::Cache;
use rusqlite::Connection;

use crate::core::watcher::FileWatcher;
use crate::models::repository::RepoStatus;
use crate::task::manager::TaskManager;

/// Upper bound on the in-memory status cache (LRU).
///
/// `RepoStatus` is small (~100 bytes), so 5000 entries ≈ 0.5 MB — far below the
/// 500 MB idle-memory target even for a 1000-repository workspace. The cap is
/// defensive: it prevents unbounded growth when repository paths churn.
const STATUS_CACHE_CAPACITY: u64 = 5000;

/// Global application state managed by Tauri.
///
/// Holds:
/// - SQLite database connection (mutex-protected)
/// - In-memory cache of repository statuses (bounded LRU cache, shared via Arc)
/// - Background task manager (git fetch/pull/push/commit queue)
/// - File watcher for real-time status updates
pub struct AppState {
    /// SQLite database connection (single-connection, mutex-protected, shared
    /// with the task manager for task persistence).
    pub db: Arc<Mutex<Connection>>,

    /// In-memory cache of repository statuses, keyed by repo path, with an LRU
    /// capacity bound. Holds only plain data (`RepoStatus`), never libgit2
    /// handles. Wrapped in Arc so it can be shared with the watcher's task.
    pub status_cache: Arc<Cache<String, RepoStatus>>,

    /// Background task manager for Git operations (fetch/pull/push/commit).
    /// Thread-safe internally (uses DashMap + tokio channel).
    pub task_manager: TaskManager,

    /// File watcher for real-time repository status updates.
    /// Mutex-protected because start/stop require &mut self.
    pub watcher: Mutex<FileWatcher>,
}

/// Build the bounded LRU status cache.
///
/// Separated from `AppState::new` so the capacity bound can be asserted in
/// tests (the "LRU 上限生效" acceptance criterion) without constructing a full
/// Tauri app state.
pub(crate) fn build_status_cache() -> Cache<String, RepoStatus> {
    Cache::builder()
        .max_capacity(STATUS_CACHE_CAPACITY)
        .build()
}

impl AppState {
    /// Create a new AppState with the given database connection and task manager.
    pub fn new(db: Arc<Mutex<Connection>>, task_manager: TaskManager) -> Self {
        Self {
            db,
            status_cache: Arc::new(build_status_cache()),
            task_manager,
            watcher: Mutex::new(FileWatcher::new()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(branch: &str) -> RepoStatus {
        RepoStatus {
            branch: branch.to_string(),
            is_detached: false,
            ahead: 0,
            behind: 0,
            modified: 0,
            added: 0,
            deleted: 0,
            untracked: 0,
            staged: 0,
            is_clean: true,
        }
    }

    /// The app status cache must be constructed with an LRU capacity bound
    /// (acceptance: 1000-repo idle memory < 500 MB — the cache cannot grow
    /// unbounded).
    #[test]
    fn app_status_cache_has_lru_capacity_bound() {
        let cache = build_status_cache();
        assert_eq!(cache.policy().max_capacity(), Some(STATUS_CACHE_CAPACITY));
    }

    /// The status cache must never exceed its capacity. moka evicts lazily and
    /// uses an approximate LRU (TinyLFU), so we assert the hard invariant — the
    /// entry count stays within the bound after pending maintenance runs — not a
    /// specific eviction order.
    #[test]
    fn status_cache_respects_capacity_bound() {
        let cache: Cache<String, RepoStatus> =
            Cache::builder().max_capacity(2).build();

        for i in 0..100 {
            cache.insert(format!("repo_{}", i), status("x"));
        }
        cache.run_pending_tasks();

        assert!(
            cache.entry_count() <= 2,
            "cache must stay within its capacity bound, got {}",
            cache.entry_count()
        );
    }
}
