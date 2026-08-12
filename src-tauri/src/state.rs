use std::sync::{Arc, Mutex};

use dashmap::DashMap;
use rusqlite::Connection;

use crate::core::watcher::FileWatcher;
use crate::models::repository::RepoStatus;
use crate::task::manager::TaskManager;

/// Global application state managed by Tauri.
///
/// Holds:
/// - SQLite database connection (mutex-protected)
/// - In-memory cache of repository statuses (concurrent DashMap, shared via Arc)
/// - Background task manager (git fetch/pull/push/commit queue)
/// - File watcher for real-time status updates
pub struct AppState {
    /// SQLite database connection (single-connection, mutex-protected).
    pub db: Mutex<Connection>,

    /// In-memory cache of repository statuses, keyed by repo path.
    /// Wrapped in Arc so it can be shared with the file watcher's async task.
    pub status_cache: Arc<DashMap<String, RepoStatus>>,

    /// Background task manager for Git operations (fetch/pull/push/commit).
    /// Thread-safe internally (uses DashMap + tokio channel).
    pub task_manager: TaskManager,

    /// File watcher for real-time repository status updates.
    /// Mutex-protected because start/stop require &mut self.
    pub watcher: Mutex<FileWatcher>,
}

impl AppState {
    /// Create a new AppState with the given database connection and task manager.
    pub fn new(conn: Connection, task_manager: TaskManager) -> Self {
        Self {
            db: Mutex::new(conn),
            status_cache: Arc::new(DashMap::new()),
            task_manager,
            watcher: Mutex::new(FileWatcher::new()),
        }
    }
}
