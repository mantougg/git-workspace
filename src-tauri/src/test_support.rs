//! Shared test helpers (B-01 测试 fixture 集中).
//!
//! Test-only module: compiled exclusively under `cfg(test)` so it never
//! becomes part of the production API surface.

use std::path::{Path, PathBuf};

/// Create `path`'s parent directories, then write `content` as UTF-8 text.
pub(crate) fn write(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, content).unwrap();
}

/// Unique temp dir path `{prefix}_{tag}_{nanos}` under `std::env::temp_dir()`
/// (含 temp 目录的测试统一走 `std::env::temp_dir()`，见根 AGENTS.md 平台规范)。
pub(crate) fn temp_root(prefix: &str, tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "{prefix}_{tag}_{}",
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
    ))
}
