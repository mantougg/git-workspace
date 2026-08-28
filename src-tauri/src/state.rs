use std::sync::{Arc, Mutex};

use moka::sync::Cache;
use rusqlite::Connection;

use crate::core::diff::FileDiff;
use crate::core::watcher::FileWatcher;
use crate::maven::PomCache;
use crate::models::repository::RepoStatus;
use crate::runtime::service::RuntimeService;
use crate::runtime::spring_boot::SpringBootDetectionCache;
use crate::task::manager::TaskManager;

/// Upper bound on the in-memory status cache (LRU).
///
/// `RepoStatus` is small (~100 bytes), so 5000 entries ≈ 0.5 MB — far below the
/// 500 MB idle-memory target even for a 1000-repository workspace. The cap is
/// defensive: it prevents unbounded growth when repository paths churn.
const STATUS_CACHE_CAPACITY: u64 = 5000;

/// Upper bound on the revision-diff cache (T-04/T-12, LRU).
///
/// Entries are whole `Vec<FileDiff>` payloads (bounded per file by the
/// 2000-line IPC cap, but potentially ~MB each), so the entry count is kept
/// small: 32 entries worst-case ≈ tens of MB, still far below the 500 MB
/// idle-memory target. Only immutable tree↔tree diffs (commit / branch / tag /
/// A↔B) are cached — workdir diffs mutate and are invalidated by watcher
/// events instead.
const DIFF_CACHE_CAPACITY: u64 = 32;

/// Cache key for immutable revision diffs (T-04: `(path, old_oid, new_oid)`;
/// flags keep Ignore-* renderings apart).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DiffCacheKey {
    pub repo_path: String,
    pub old_oid: String,
    pub new_oid: String,
    pub flags: u8,
}

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
    /// Arc-shared so the R-17 watch engine can attach the same instance as
    /// its task submitter (watch.rs `WatchTaskSubmitter`).
    pub task_manager: Arc<TaskManager>,

    /// File watcher for real-time repository status updates.
    /// Mutex-protected because start/stop require &mut self.
    pub watcher: Mutex<FileWatcher>,

    /// Revision-diff result cache (T-04/T-12), keyed by
    /// `(repo_path, old_oid, new_oid, flags)`. Holds only plain data
    /// (`Vec<FileDiff>`), never libgit2 handles; bounded LRU.
    pub diff_cache: Arc<Cache<DiffCacheKey, Vec<FileDiff>>>,

    /// Runtime Maven POM cache shared by discovery and Spring Boot detection.
    pub pom_cache: Arc<PomCache>,

    /// Runtime Spring Boot source detection cache. Keys include POM and source
    /// fingerprints, so watcher-driven content changes naturally refresh it.
    pub spring_boot_cache: Arc<SpringBootDetectionCache>,

    /// Runtime 控制面（R-12）：§63 命令的读侧 + Runtime 任务的执行体
    /// （同时作为 T-05 TaskManager 的 `RuntimeTaskHandler` 装配）。
    pub runtime: Arc<RuntimeService>,
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

/// Build the bounded LRU revision-diff cache.
pub(crate) fn build_diff_cache() -> Cache<DiffCacheKey, Vec<FileDiff>> {
    Cache::builder()
        .max_capacity(DIFF_CACHE_CAPACITY)
        .build()
}

impl AppState {
    /// Create a new AppState with the given database connection, task manager,
    /// Runtime service (R-12) and shared POM cache.
    pub fn new(
        db: Arc<Mutex<Connection>>,
        task_manager: Arc<TaskManager>,
        runtime: Arc<RuntimeService>,
        pom_cache: Arc<PomCache>,
    ) -> Self {
        Self {
            db,
            status_cache: Arc::new(build_status_cache()),
            task_manager,
            watcher: Mutex::new(FileWatcher::new()),
            diff_cache: Arc::new(build_diff_cache()),
            pom_cache,
            spring_boot_cache: Arc::new(SpringBootDetectionCache::new()),
            runtime,
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
            conflicted: 0,
            has_remote: false,
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

    /// The revision-diff cache must also have an LRU capacity bound
    /// (global constraint: every LRU cache has an upper limit).
    #[test]
    fn diff_cache_has_lru_capacity_bound() {
        let cache = build_diff_cache();
        assert_eq!(cache.policy().max_capacity(), Some(DIFF_CACHE_CAPACITY));
    }
}
