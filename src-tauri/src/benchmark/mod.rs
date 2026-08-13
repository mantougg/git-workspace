//! Standalone benchmark: generate synthetic repositories and measure scan /
//! status / branch / graph timings plus memory and process metrics.
//!
//! Run via `cargo run --release --example benchmark -- <count> [--json]`.
//! Results are also written to a per-count JSON baseline so successive runs
//! can be compared (regression check).

use std::path::{Path, PathBuf};
use std::time::Instant;

use serde::{Deserialize, Serialize};
use sysinfo::{get_current_pid, Pid, System};

use crate::core::git_status;
use crate::core::graph;
use crate::core::scanner::RepoScanner;

/// Structured benchmark result for a single run. Serializable to JSON so it can
/// be saved as a baseline and diffed against later runs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkResult {
    pub repository_count: usize,
    pub generated_at: String,
    pub generate_ms: u128,
    pub initial_scan_ms: u128,
    pub incremental_scan_ms: u128,
    pub status_refresh_ms: u128,
    pub branch_load_ms: u128,
    pub graph_load_ms: u128,
    /// Peak resident-set size of this process during the run (bytes).
    pub peak_rss_bytes: u64,
    /// Thread count of this process at the end of the run.
    pub thread_count: usize,
    /// Number of `git` CLI processes alive at the end (0 for libgit2-only groups;
    /// meaningful once Batch Fetch/Pull/Push groups run the git CLI).
    pub git_process_count: usize,
}

/// Generate `count` synthetic Git repositories under `root`.
/// Each has 3 commits and a handful of source files.
pub fn generate_repos(root: &Path, count: usize) -> std::io::Result<Vec<PathBuf>> {
    std::fs::create_dir_all(root)?;
    let mut paths = Vec::with_capacity(count);

    for i in 0..count {
        let dir = root.join(format!("repo_{:04}", i));
        let repo = git2::Repository::init(&dir).map_err(io_err)?;

        for c in 0..3 {
            let rel = format!("src/file_{}.txt", c);
            let file = dir.join(&rel);
            std::fs::create_dir_all(file.parent().unwrap())?;
            std::fs::write(&file, format!("content {} commit {}\n", c, c))?;

            let mut index = repo.index().map_err(io_err)?;
            index.add_path(Path::new(&rel)).map_err(io_err)?;
            index.write().map_err(io_err)?;
            let tree_oid = index.write_tree().map_err(io_err)?;
            let tree = repo.find_tree(tree_oid).map_err(io_err)?;
            let sig = git2::Signature::now("bench", "bench@example.com").map_err(io_err)?;

            let head_commit = repo.head().ok().and_then(|h| h.peel_to_commit().ok());
            let parents: Vec<git2::Commit> = head_commit.into_iter().collect();
            let parent_refs: Vec<&git2::Commit> = parents.iter().collect();

            repo.commit(
                Some("HEAD"),
                &sig,
                &sig,
                &format!("commit {}", c),
                &tree,
                &parent_refs,
            )
            .map_err(io_err)?;
        }
        paths.push(dir);
    }
    Ok(paths)
}

fn io_err<E: std::fmt::Display>(e: E) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::Other, e.to_string())
}

/// Current process RSS in bytes.
fn process_rss(system: &mut System, pid: Pid) -> u64 {
    system.refresh_process(pid);
    system.process(pid).map(|p| p.memory()).unwrap_or(0)
}

/// Current process thread count (size of the task set).
fn process_threads(system: &mut System, pid: Pid) -> usize {
    system.refresh_process(pid);
    system
        .process(pid)
        .and_then(|p| p.tasks().map(|tasks| tasks.len()))
        .unwrap_or(0)
}

/// Number of `git` CLI processes currently running on the system.
fn git_process_count(system: &mut System) -> usize {
    system.refresh_processes();
    system
        .processes()
        .values()
        .filter(|p| {
            let name = p.name().to_ascii_lowercase();
            name == "git" || name == "git.exe"
        })
        .count()
}

fn track_peak_rss(system: &mut System, pid: Pid, peak: &mut u64) {
    let rss = process_rss(system, pid);
    if rss > *peak {
        *peak = rss;
    }
}

/// Run the benchmark for `count` repositories and return the structured result.
pub fn run(count: usize) -> BenchmarkResult {
    let tmp = std::env::temp_dir().join(format!("gw_bench_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);

    let pid = get_current_pid().expect("current pid");
    let mut system = System::new();
    let mut peak_rss: u64 = 0;

    let gen_start = Instant::now();
    let repos = generate_repos(&tmp, count).expect("generate_repos failed");
    let generate_ms = gen_start.elapsed().as_millis();
    track_peak_rss(&mut system, pid, &mut peak_rss);

    let scanner = RepoScanner::new(5);

    // Initial scan.
    let scan_start = Instant::now();
    let found = scanner.scan(&tmp);
    let initial_scan_ms = scan_start.elapsed().as_millis();
    track_peak_rss(&mut system, pid, &mut peak_rss);

    // Incremental rescan (all known paths hit the cache).
    let known: std::collections::HashMap<String, Option<i64>> = found
        .iter()
        .map(|r| (r.path.clone(), r.git_dir_mtime))
        .collect();
    let incr_start = Instant::now();
    let _found_incr = scanner.scan_incremental(&tmp, None, &known);
    let incremental_scan_ms = incr_start.elapsed().as_millis();
    track_peak_rss(&mut system, pid, &mut peak_rss);

    // Status refresh (sequential baseline).
    let status_start = Instant::now();
    for r in &repos {
        let _ = git_status::get_repo_status(r);
    }
    let status_refresh_ms = status_start.elapsed().as_millis();
    track_peak_rss(&mut system, pid, &mut peak_rss);

    // Branch load.
    let branch_start = Instant::now();
    for r in &repos {
        let _ = graph::get_branches(r);
    }
    let branch_load_ms = branch_start.elapsed().as_millis();
    track_peak_rss(&mut system, pid, &mut peak_rss);

    // Graph load (commit history, capped at 100 commits per repo).
    let graph_start = Instant::now();
    for r in &repos {
        let _ = graph::get_commit_history(r, 100);
    }
    let graph_load_ms = graph_start.elapsed().as_millis();
    track_peak_rss(&mut system, pid, &mut peak_rss);

    let thread_count = process_threads(&mut system, pid);
    let git_procs = git_process_count(&mut system);

    let _ = std::fs::remove_dir_all(&tmp);

    BenchmarkResult {
        repository_count: count,
        generated_at: chrono::Utc::now().to_rfc3339(),
        generate_ms,
        initial_scan_ms,
        incremental_scan_ms,
        status_refresh_ms,
        branch_load_ms,
        graph_load_ms,
        peak_rss_bytes: peak_rss,
        thread_count,
        git_process_count: git_procs,
    }
}

/// Render a human-readable Markdown report.
pub fn format_report(r: &BenchmarkResult) -> String {
    let mut s = String::new();
    s.push_str(&format!("## Benchmark: {} repositories\n\n", r.repository_count));
    s.push_str(&format!("- Generated at: {}\n", r.generated_at));
    s.push_str(&format!("- Generate: {} ms\n", r.generate_ms));
    s.push_str(&format!(
        "- Initial scan: {} ms ({} repos)\n",
        r.initial_scan_ms, r.repository_count
    ));
    s.push_str(&format!("- Incremental rescan: {} ms\n", r.incremental_scan_ms));
    s.push_str(&format!("- Status refresh: {} ms\n", r.status_refresh_ms));
    s.push_str(&format!("- Branch load: {} ms\n", r.branch_load_ms));
    s.push_str(&format!("- Graph load: {} ms\n", r.graph_load_ms));
    s.push_str(&format!(
        "- Per-repo status avg: {:.2} ms\n",
        r.status_refresh_ms as f64 / r.repository_count.max(1) as f64
    ));
    s.push_str(&format!(
        "- Peak RSS: {:.2} MB\n",
        r.peak_rss_bytes as f64 / 1024.0 / 1024.0
    ));
    s.push_str(&format!("- Thread count: {}\n", r.thread_count));
    s.push_str(&format!("- Git process count: {}\n", r.git_process_count));
    s
}

/// Serialize a result to pretty JSON.
pub fn to_json(r: &BenchmarkResult) -> String {
    serde_json::to_string_pretty(r).expect("benchmark result serialization")
}

/// Load a previously saved baseline (JSON) from disk.
pub fn load_baseline(path: &Path) -> Option<BenchmarkResult> {
    let content = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

/// Save a result as a baseline (JSON) to disk.
pub fn save_baseline(r: &BenchmarkResult, path: &Path) -> std::io::Result<()> {
    std::fs::write(path, to_json(r))
}

/// Render a Markdown comparison between a previous baseline and the current run.
pub fn format_comparison(prev: &BenchmarkResult, curr: &BenchmarkResult) -> String {
    let mut s = String::new();
    s.push_str("## Benchmark comparison (vs baseline)\n\n");
    s.push_str("| metric | baseline | current | delta |\n|---|---|---:|---:|\n");
    compare_row(&mut s, "Initial scan", prev.initial_scan_ms, curr.initial_scan_ms);
    compare_row(&mut s, "Status refresh", prev.status_refresh_ms, curr.status_refresh_ms);
    compare_row(&mut s, "Branch load", prev.branch_load_ms, curr.branch_load_ms);
    compare_row(&mut s, "Graph load", prev.graph_load_ms, curr.graph_load_ms);

    let prev_mb = prev.peak_rss_bytes as f64 / 1048576.0;
    let curr_mb = curr.peak_rss_bytes as f64 / 1048576.0;
    let delta = (curr_mb - prev_mb) / prev_mb.max(1.0) * 100.0;
    s.push_str(&format!(
        "| Peak RSS | {:.1} MB | {:.1} MB | {:+.1}% |\n",
        prev_mb, curr_mb, delta
    ));
    s
}

fn compare_row(out: &mut String, name: &str, prev_ms: u128, curr_ms: u128) {
    let delta = (curr_ms as f64 - prev_ms as f64) / prev_ms.max(1) as f64 * 100.0;
    out.push_str(&format!(
        "| {} | {} ms | {} ms | {:+.1}% |\n",
        name, prev_ms, curr_ms, delta
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> BenchmarkResult {
        BenchmarkResult {
            repository_count: 10,
            generated_at: "2026-08-13T00:00:00Z".to_string(),
            generate_ms: 1000,
            initial_scan_ms: 200,
            incremental_scan_ms: 50,
            status_refresh_ms: 800,
            branch_load_ms: 600,
            graph_load_ms: 500,
            peak_rss_bytes: 12 * 1024 * 1024,
            thread_count: 8,
            git_process_count: 0,
        }
    }

    #[test]
    fn json_roundtrip_preserves_fields() {
        let r = sample();
        let json = to_json(&r);
        let parsed: BenchmarkResult = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.repository_count, r.repository_count);
        assert_eq!(parsed.initial_scan_ms, r.initial_scan_ms);
        assert_eq!(parsed.peak_rss_bytes, r.peak_rss_bytes);
    }

    #[test]
    fn comparison_renders_metric_delta() {
        let prev = sample();
        let mut curr = sample();
        curr.initial_scan_ms = 240; // +20%
        let s = format_comparison(&prev, &curr);
        assert!(s.contains("Initial scan"));
        assert!(s.contains("+20.0%"));
    }
}
