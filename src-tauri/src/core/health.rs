//! Workspace Health (T-19, Roadmap §19): anomaly detection derived from the
//! cached `RepoStatus` (T-02), a configurable 0-100 health score, and
//! on-demand heavy checks (large files / LFS / submodule) that stay out of
//! the resident status path.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::core::git_status::is_runtime_path;
use crate::models::repository::RepoStatus;

// Anomaly keys are stable strings: they cross IPC, act as UI filter keys,
// and are the keys in the weights config file. Heavy keys only appear after
// `compute_health_extra` has run for a repo.
pub const ANOMALY_DIRTY: &str = "dirty";
pub const ANOMALY_CONFLICT: &str = "conflict";
pub const ANOMALY_AHEAD: &str = "ahead";
pub const ANOMALY_BEHIND: &str = "behind";
pub const ANOMALY_DETACHED: &str = "detached";
pub const ANOMALY_MISSING_REMOTE: &str = "missing_remote";
pub const ANOMALY_DIVERGED: &str = "diverged";
pub const ANOMALY_UNTRACKED: &str = "untracked";
pub const ANOMALY_LARGE_FILES: &str = "large_files";
pub const ANOMALY_LFS_ERROR: &str = "lfs_error";
pub const ANOMALY_SUBMODULE_ERROR: &str = "submodule_error";

/// Per-anomaly deduction weights for the health score. Loaded from
/// `health-weights.json` in the app data dir; serde defaults fill any field
/// the file omits, so a hand-written partial file is valid (T-19: scoring
/// rules live in a config file, not hardcoded at the call site).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct HealthWeights {
    pub dirty: u32,
    pub conflict: u32,
    pub ahead: u32,
    pub behind: u32,
    pub detached: u32,
    pub missing_remote: u32,
    pub diverged: u32,
    pub untracked: u32,
    pub large_files: u32,
    pub lfs_error: u32,
    pub submodule_error: u32,
}

impl Default for HealthWeights {
    fn default() -> Self {
        Self {
            dirty: 10,
            conflict: 30,
            ahead: 5,
            behind: 8,
            detached: 10,
            missing_remote: 15,
            diverged: 20,
            untracked: 3,
            large_files: 15,
            lfs_error: 25,
            submodule_error: 25,
        }
    }
}

impl HealthWeights {
    /// Deduction weight of one anomaly key (0 for unknown keys).
    pub fn weight_of(&self, anomaly: &str) -> u32 {
        match anomaly {
            ANOMALY_DIRTY => self.dirty,
            ANOMALY_CONFLICT => self.conflict,
            ANOMALY_AHEAD => self.ahead,
            ANOMALY_BEHIND => self.behind,
            ANOMALY_DETACHED => self.detached,
            ANOMALY_MISSING_REMOTE => self.missing_remote,
            ANOMALY_DIVERGED => self.diverged,
            ANOMALY_UNTRACKED => self.untracked,
            ANOMALY_LARGE_FILES => self.large_files,
            ANOMALY_LFS_ERROR => self.lfs_error,
            ANOMALY_SUBMODULE_ERROR => self.submodule_error,
            _ => 0,
        }
    }
}

/// Load weights from `<dir>/health-weights.json`; a missing or invalid file
/// yields the defaults.
pub fn load_health_weights(dir: &Path) -> HealthWeights {
    std::fs::read_to_string(dir.join("health-weights.json"))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// Light anomaly keys derivable from the cached `RepoStatus`, in stable
/// order. No IO, no network — pure derivation (T-19: based on the T-02
/// status cache with zero extra scan cost).
pub fn anomalies_of(status: &RepoStatus) -> Vec<&'static str> {
    let mut out = Vec::new();
    if status.modified + status.added + status.deleted + status.staged > 0 {
        out.push(ANOMALY_DIRTY);
    }
    if status.conflicted > 0 {
        out.push(ANOMALY_CONFLICT);
    }
    if status.ahead > 0 {
        out.push(ANOMALY_AHEAD);
    }
    if status.behind > 0 {
        out.push(ANOMALY_BEHIND);
    }
    if status.is_detached {
        out.push(ANOMALY_DETACHED);
    }
    if !status.has_remote {
        out.push(ANOMALY_MISSING_REMOTE);
    }
    if status.ahead > 0 && status.behind > 0 {
        out.push(ANOMALY_DIVERGED);
    }
    if status.untracked > 0 {
        out.push(ANOMALY_UNTRACKED);
    }
    out
}

/// Health score for one repo: 100 minus the summed weights of the anomalies
/// present, clamped to [0, 100]. (The frontend re-uses the same formula when
/// merging async heavy-check results — keep the two in sync.)
pub fn score_of<'a>(
    anomalies: impl IntoIterator<Item = &'a str>,
    weights: &HealthWeights,
) -> u32 {
    let deduction: u32 = anomalies
        .into_iter()
        .map(|a| weights.weight_of(a))
        .sum();
    100u32.saturating_sub(deduction)
}

/// Per-repo health entry (light checks only).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoHealth {
    pub repo_path: String,
    pub repo_name: String,
    pub branch: String,
    pub anomalies: Vec<String>,
    pub score: u32,
}

/// Workspace health aggregate.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceHealth {
    /// Mean of per-repo scores, rounded to the nearest integer.
    pub score: u32,
    pub total: usize,
    pub anomalous: usize,
    pub repos: Vec<RepoHealth>,
    pub weights: HealthWeights,
}

/// Aggregate per-repo entries into the workspace score.
pub fn aggregate_health(repos: Vec<RepoHealth>, weights: HealthWeights) -> WorkspaceHealth {
    let total = repos.len();
    let anomalous = repos.iter().filter(|r| !r.anomalies.is_empty()).count();
    let score = if total == 0 {
        100
    } else {
        let sum: u64 = repos.iter().map(|r| r.score as u64).sum();
        ((sum + total as u64 / 2) / total as u64) as u32
    };
    WorkspaceHealth {
        score,
        total,
        anomalous,
        repos,
        weights,
    }
}

/// Heavy, on-demand per-repo checks (T-19: computed when the Health page
/// opens, never in the resident status path).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoHealthExtra {
    pub repo_path: String,
    /// Workdir files larger than the threshold (excl. .git/runtime dirs).
    pub large_files: usize,
    /// Size of the largest such file in bytes (0 when none).
    pub largest_file_bytes: u64,
    /// `.gitattributes` declares LFS filters but `git lfs` is unavailable.
    pub lfs_error: bool,
    /// `.gitmodules` declares submodules that are missing or uninitialized.
    pub submodule_error: bool,
}

/// Files above this size in a workdir count as "large" (Roadmap §19 Large
/// Files; matches the pre-commit large-file scan spirit of §47).
const LARGE_FILE_THRESHOLD: u64 = 10 * 1024 * 1024; // 10 MB

/// Run the heavy checks for one repo. `lfs_available` is probed once by the
/// caller and shared across repos.
pub fn compute_health_extra(repo_path: &Path, lfs_available: bool) -> RepoHealthExtra {
    let (large_files, largest_file_bytes) = find_large_files(repo_path);
    RepoHealthExtra {
        repo_path: repo_path.to_string_lossy().to_string(),
        large_files,
        largest_file_bytes,
        lfs_error: !lfs_available && declares_lfs(repo_path),
        submodule_error: has_broken_submodules(repo_path),
    }
}

/// Whether `git lfs` is usable on this machine. Probed once per extras call
/// (spawning a process per repo would blow the concurrency budget).
pub fn lfs_available() -> bool {
    let mut cmd = std::process::Command::new("git");
    cmd.args(["lfs", "version"]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW (0x08000000)：GUI 进程 spawn 控制台子进程必须加
        // 这个 flag，否则健康检查执行时 Windows 会闪 cmd 窗口（F-01c）。
        cmd.creation_flags(0x0800_0000);
    }
    cmd.output().map(|o| o.status.success()).unwrap_or(false)
}

/// Iterative workdir walk (no recursion depth issues on deep trees); skips
/// `.git` and runtime/generated directories at any depth.
fn find_large_files(root: &Path) -> (usize, u64) {
    let mut count = 0;
    let mut largest = 0u64;
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
            if is_dir {
                let name = entry.file_name().to_string_lossy().to_string();
                if !is_runtime_path(&name) {
                    stack.push(path);
                }
                continue;
            }
            let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
            if size > LARGE_FILE_THRESHOLD {
                count += 1;
                largest = largest.max(size);
            }
        }
    }
    (count, largest)
}

fn declares_lfs(repo_path: &Path) -> bool {
    std::fs::read_to_string(repo_path.join(".gitattributes"))
        .map(|s| s.contains("filter=lfs"))
        .unwrap_or(false)
}

/// A submodule counts as broken when `.gitmodules` declares it but its path
/// is missing or has no `.git` entry (uninitialized).
fn has_broken_submodules(repo_path: &Path) -> bool {
    let Ok(modules) = std::fs::read_to_string(repo_path.join(".gitmodules")) else {
        return false;
    };
    parse_gitmodule_paths(&modules).into_iter().any(|rel| {
        let p = repo_path.join(&rel);
        !p.exists() || !p.join(".git").exists()
    })
}

/// Extract `path = ...` values from `.gitmodules` (the only key we need).
fn parse_gitmodule_paths(content: &str) -> Vec<String> {
    content
        .lines()
        .filter_map(|l| l.trim().strip_prefix("path"))
        .filter_map(|rest| rest.trim_start().strip_prefix('='))
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status() -> RepoStatus {
        RepoStatus {
            branch: "main".into(),
            is_detached: false,
            ahead: 0,
            behind: 0,
            modified: 0,
            added: 0,
            deleted: 0,
            untracked: 0,
            staged: 0,
            conflicted: 0,
            has_remote: true,
            is_clean: true,
        }
    }

    #[test]
    fn anomalies_clean_repo_has_none() {
        assert!(anomalies_of(&status()).is_empty());
    }

    #[test]
    fn anomalies_cover_all_light_kinds() {
        let mut s = status();
        s.modified = 1;
        s.conflicted = 2;
        s.ahead = 1;
        s.behind = 1;
        s.is_detached = true;
        s.has_remote = false;
        s.untracked = 5;
        let a = anomalies_of(&s);
        assert_eq!(
            a,
            vec![
                ANOMALY_DIRTY,
                ANOMALY_CONFLICT,
                ANOMALY_AHEAD,
                ANOMALY_BEHIND,
                ANOMALY_DETACHED,
                ANOMALY_MISSING_REMOTE,
                ANOMALY_DIVERGED,
                ANOMALY_UNTRACKED,
            ]
        );
    }

    #[test]
    fn diverged_requires_both_ahead_and_behind() {
        let mut s = status();
        s.ahead = 3;
        assert!(!anomalies_of(&s).contains(&ANOMALY_DIVERGED));
        s.behind = 1;
        assert!(anomalies_of(&s).contains(&ANOMALY_DIVERGED));
    }

    #[test]
    fn score_deducts_weights_and_clamps_at_zero() {
        let w = HealthWeights::default();
        assert_eq!(score_of([ANOMALY_CONFLICT], &w), 70);
        assert_eq!(
            score_of([ANOMALY_CONFLICT, ANOMALY_DIRTY, ANOMALY_UNTRACKED], &w),
            57
        );
        // Everything wrong at once must clamp at 0, not underflow.
        let all = anomalies_of(&{
            let mut s = status();
            s.modified = 1;
            s.conflicted = 1;
            s.ahead = 1;
            s.behind = 1;
            s.is_detached = true;
            s.has_remote = false;
            s.untracked = 1;
            s
        });
        assert_eq!(score_of(all, &w), 0);
    }

    #[test]
    fn weights_load_defaults_when_file_missing() {
        let dir = std::env::temp_dir().join(format!(
            "gw_health_noweights_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let w = load_health_weights(&dir);
        assert_eq!(w.conflict, 30);
    }

    #[test]
    fn weights_partial_file_overrides_defaults() {
        let dir = std::env::temp_dir().join(format!(
            "gw_health_weights_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("health-weights.json"),
            r#"{ "conflict": 50, "dirty": 1 }"#,
        )
        .unwrap();
        let w = load_health_weights(&dir);
        assert_eq!(w.conflict, 50);
        assert_eq!(w.dirty, 1);
        assert_eq!(w.diverged, 20, "unset fields keep defaults");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn aggregate_health_means_and_counts() {
        let mk = |score: u32, anomalies: Vec<&str>| RepoHealth {
            repo_path: "p".into(),
            repo_name: "n".into(),
            branch: "b".into(),
            anomalies: anomalies.into_iter().map(String::from).collect(),
            score,
        };
        let h = aggregate_health(
            vec![mk(100, vec![]), mk(50, vec!["dirty"]), mk(75, vec![])],
            HealthWeights::default(),
        );
        assert_eq!(h.score, 75);
        assert_eq!(h.total, 3);
        assert_eq!(h.anomalous, 1);
        assert_eq!(aggregate_health(vec![], HealthWeights::default()).score, 100);
    }

    #[test]
    fn gitmodule_paths_parsed() {
        let content = r#"
[submodule "libs/a"]
	path = libs/a
	url = https://x/a.git
[submodule "b"]
    path = vendor/b
"#;
        assert_eq!(
            parse_gitmodule_paths(content),
            vec!["libs/a".to_string(), "vendor/b".to_string()]
        );
    }

    #[test]
    fn large_files_walk_skips_runtime_dirs() {
        let dir = std::env::temp_dir().join(format!(
            "gw_health_large_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join("node_modules/pkg")).unwrap();
        std::fs::create_dir_all(dir.join("src")).unwrap();
        let big = vec![0u8; (LARGE_FILE_THRESHOLD + 1) as usize];
        std::fs::write(dir.join("src/big.bin"), &big).unwrap();
        std::fs::write(dir.join("node_modules/pkg/big2.bin"), &big).unwrap();
        std::fs::write(dir.join("src/small.txt"), "tiny").unwrap();

        let (count, largest) = find_large_files(&dir);
        assert_eq!(count, 1, "node_modules must be skipped");
        assert_eq!(largest, LARGE_FILE_THRESHOLD + 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn broken_submodule_detected_only_when_path_not_initialized() {
        let dir = std::env::temp_dir().join(format!(
            "gw_health_sub_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join(".gitmodules"),
            "[submodule \"a\"]\n\tpath = libs/a\n\turl = https://x/a.git\n",
        )
        .unwrap();
        assert!(has_broken_submodules(&dir), "missing path = broken");

        // Initialized submodule (path with a .git entry) clears the error.
        std::fs::create_dir_all(dir.join("libs/a/.git")).unwrap();
        assert!(!has_broken_submodules(&dir));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
