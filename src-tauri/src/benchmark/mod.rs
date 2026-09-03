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

/// R-08: synthetic Maven workspace generator (§96 matrix).
pub mod maven_gen;
/// R-08: Runtime pipeline benchmark (staged timings + §99 verdicts).
pub mod runtime;

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

            repo.commit(Some("HEAD"), &sig, &sig, &format!("commit {}", c), &tree, &parent_refs)
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
    let known: std::collections::HashMap<String, Option<i64>> =
        found.iter().map(|r| (r.path.clone(), r.git_dir_mtime)).collect();
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

// ---------------------------------------------------------------------------
// T-04 Diff & Graph acceptance benchmarks
// ---------------------------------------------------------------------------

/// T-04 acceptance budget: second view of the same diff (cache hit) < 50 ms.
pub const DIFF_CACHE_HIT_BUDGET_MS: u128 = 50;
/// T-04 acceptance budget: graph first screen on a 10k+ commit repo < 1 s.
pub const GRAPH_FIRST_SCREEN_BUDGET_MS: u128 = 1000;
/// Commits in the synthetic "big repo" for the graph first-screen test.
pub const GRAPH_BIG_REPO_COMMITS: usize = 10_000;

/// Structured result for the T-04 acceptance benchmarks (diff cache hit +
/// graph first screen on a large repository). Single-repo measurements, kept
/// separate from the per-count `BenchmarkResult` scaling model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffGraphBenchmarkResult {
    pub generated_at: String,
    /// First commit-diff view (cache miss, full libgit2 computation).
    pub diff_cold_ms: u128,
    /// Second view of the same commit diff (LRU cache hit), microseconds.
    pub diff_cache_hit_us: u128,
    /// Commit count of the synthetic big repo.
    pub graph_repo_commits: usize,
    /// Time to generate the big repo (tracked separately, not part of budgets).
    pub graph_generate_ms: u128,
    /// Graph first screen (first page) with an empty SQLite metadata cache.
    pub graph_first_screen_cold_ms: u128,
    /// Graph first screen with the SQLite metadata cache populated.
    pub graph_first_screen_warm_ms: u128,
}

/// Generate a repo with `commits` commits; each commit rewrites a few small
/// files so every tree is distinct (realistic diffs, cheap object writes).
fn generate_commit_history_repo(dir: &Path, commits: usize) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let repo = git2::Repository::init(dir).map_err(io_err)?;

    for c in 0..commits {
        // Rotate through a handful of small files; each commit changes one
        // line in each file so trees stay distinct and diffs non-trivial.
        for f in 0..3 {
            let rel = format!("src/file_{}.txt", f);
            let file = dir.join(&rel);
            std::fs::create_dir_all(file.parent().unwrap())?;
            let mut content = String::new();
            for line in 0..40 {
                if line == c % 40 {
                    content.push_str(&format!("commit {} changed this line\n", c));
                } else {
                    content.push_str(&format!("stable line {}\n", line));
                }
            }
            std::fs::write(&file, content)?;
        }

        let mut index = repo.index().map_err(io_err)?;
        for f in 0..3 {
            index
                .add_path(Path::new(&format!("src/file_{}.txt", f)))
                .map_err(io_err)?;
        }
        index.write().map_err(io_err)?;
        let tree_oid = index.write_tree().map_err(io_err)?;
        let tree = repo.find_tree(tree_oid).map_err(io_err)?;
        let sig = git2::Signature::now("bench", "bench@example.com").map_err(io_err)?;

        let head_commit = repo.head().ok().and_then(|h| h.peel_to_commit().ok());
        let parents: Vec<git2::Commit> = head_commit.into_iter().collect();
        let parent_refs: Vec<&git2::Commit> = parents.iter().collect();

        repo.commit(Some("HEAD"), &sig, &sig, &format!("commit {}", c), &tree, &parent_refs)
            .map_err(io_err)?;
    }
    Ok(())
}

/// Generate a linear history of `commits` commits fast: every commit shares
/// one small tree (graph benchmarks exercise commit metadata + DAG shape, not
/// file contents). Commit times increase strictly so TIME order matches the
/// chain order deterministically.
fn generate_linear_history_repo(dir: &Path, commits: usize) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let repo = git2::Repository::init(dir).map_err(io_err)?;

    let rel = "src/file.txt";
    let file = dir.join(rel);
    std::fs::create_dir_all(file.parent().unwrap())?;
    std::fs::write(&file, "content\n")?;
    let mut index = repo.index().map_err(io_err)?;
    index.add_path(Path::new(rel)).map_err(io_err)?;
    index.write().map_err(io_err)?;
    let tree_oid = index.write_tree().map_err(io_err)?;
    let tree = repo.find_tree(tree_oid).map_err(io_err)?;

    const BASE_TIME: i64 = 1_700_000_000;
    for c in 0..commits {
        let time = git2::Time::new(BASE_TIME + c as i64, 0);
        let sig = git2::Signature::new("bench", "bench@example.com", &time).map_err(io_err)?;
        let head_commit = repo.head().ok().and_then(|h| h.peel_to_commit().ok());
        let parents: Vec<git2::Commit> = head_commit.into_iter().collect();
        let parent_refs: Vec<&git2::Commit> = parents.iter().collect();
        repo.commit(Some("HEAD"), &sig, &sig, &format!("commit {}", c), &tree, &parent_refs)
            .map_err(io_err)?;
    }
    Ok(())
}

/// Run the T-04 acceptance benchmarks:
///
/// 1. Diff cache hit — measure the real command path (`cached_tree_diff`) for
///    a first (cold) and second (cache-hit) view of the same commit diff.
/// 2. Graph first screen — generate a `graph_commits`-commit repo and measure
///    the real command path (`load_commit_history_cached`, first page of 100)
///    against an empty and a populated SQLite metadata cache.
pub fn run_diff_graph(graph_commits: usize) -> DiffGraphBenchmarkResult {
    use crate::commands::diff::cached_tree_diff;
    use crate::commands::graph::load_commit_history_cached;
    use crate::core::diff::DiffConfig;
    use crate::models::repository::ScannedRepo;
    use crate::state::build_diff_cache;

    let tmp = std::env::temp_dir().join(format!("gw_bench_t04_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);

    // --- 1. Diff cache hit (T-04: second view < 50 ms) ---
    let diff_repo_dir = tmp.join("diff_repo");
    generate_commit_history_repo(&diff_repo_dir, 30).expect("generate diff repo failed");

    let repo = git2::Repository::open(&diff_repo_dir).expect("open diff repo");
    let old_tree = repo
        .revparse_single("HEAD~1")
        .and_then(|o| o.peel_to_tree())
        .expect("resolve HEAD~1 tree");
    let new_tree = repo
        .revparse_single("HEAD")
        .and_then(|o| o.peel_to_tree())
        .expect("resolve HEAD tree");
    let config = DiffConfig::default();
    let cache = build_diff_cache();
    let repo_path = diff_repo_dir.to_string_lossy().to_string();

    let cold_start = Instant::now();
    let cold_files =
        cached_tree_diff(&cache, &repo, &repo_path, &old_tree, &new_tree, &config).expect("cold diff failed");
    let diff_cold_ms = cold_start.elapsed().as_millis();
    assert!(!cold_files.is_empty(), "synthetic diff must be non-empty");

    let hit_start = Instant::now();
    let hit_files =
        cached_tree_diff(&cache, &repo, &repo_path, &old_tree, &new_tree, &config).expect("cache-hit diff failed");
    let diff_cache_hit_us = hit_start.elapsed().as_micros();
    assert_eq!(cold_files.len(), hit_files.len(), "cache hit must match cold");

    // --- 2. Graph first screen on a big repo (T-04: 10k+ commits < 1 s) ---
    let big_repo_dir = tmp.join("big_repo");
    let gen_start = Instant::now();
    generate_linear_history_repo(&big_repo_dir, graph_commits).expect("generate big repo failed");
    let graph_generate_ms = gen_start.elapsed().as_millis();

    // File-backed DB so the WAL/pragma path matches production.
    let db_path = tmp.join("bench.db");
    let mut conn = rusqlite::Connection::open(&db_path).expect("open bench db");
    crate::db::init_db(&mut conn).expect("init bench db");
    conn.execute(
        "INSERT INTO workspaces (name, path, created_at, updated_at) VALUES ('w', ?1, 't', 't')",
        [tmp.to_string_lossy().to_string()],
    )
    .expect("insert workspace");
    let ws_id = conn.last_insert_rowid();
    let big_path = big_repo_dir.to_string_lossy().to_string();
    crate::db::dao::upsert_repositories_batch(
        &mut conn,
        ws_id,
        &[ScannedRepo {
            path: big_path.clone(),
            name: "big_repo".into(),
            relative_path: "big_repo".into(),
            git_dir_mtime: None,
        }],
    )
    .expect("insert repository");

    // Cold: empty commits cache — parses + persists the first page.
    let cold_start = Instant::now();
    let cold = load_commit_history_cached(&mut conn, &big_repo_dir, 100).expect("cold graph first screen failed");
    let graph_first_screen_cold_ms = cold_start.elapsed().as_millis();
    assert_eq!(cold.len(), 100, "first screen must return a full page");
    // The first page must be the newest 100 commits in chain order — guards
    // the T-04 TIME-sort pagination fix against ordering regressions.
    assert_eq!(cold[0].message, format!("commit {}", graph_commits - 1));
    assert_eq!(cold[99].message, format!("commit {}", graph_commits - 100));

    // Warm: metadata cache populated — no commit parsing.
    let warm_start = Instant::now();
    let warm = load_commit_history_cached(&mut conn, &big_repo_dir, 100).expect("warm graph first screen failed");
    let graph_first_screen_warm_ms = warm_start.elapsed().as_millis();
    assert_eq!(warm.len(), 100);

    drop(conn);
    let _ = std::fs::remove_dir_all(&tmp);

    DiffGraphBenchmarkResult {
        generated_at: chrono::Utc::now().to_rfc3339(),
        diff_cold_ms,
        diff_cache_hit_us,
        graph_repo_commits: graph_commits,
        graph_generate_ms,
        graph_first_screen_cold_ms,
        graph_first_screen_warm_ms,
    }
}

/// Render the T-04 benchmark report with PASS/FAIL against the budgets.
pub fn format_diff_graph_report(r: &DiffGraphBenchmarkResult) -> String {
    let hit_ms = r.diff_cache_hit_us as f64 / 1000.0;
    let diff_ok = r.diff_cache_hit_us < DIFF_CACHE_HIT_BUDGET_MS * 1000;
    let graph_cold_ok = r.graph_first_screen_cold_ms < GRAPH_FIRST_SCREEN_BUDGET_MS;
    let graph_warm_ok = r.graph_first_screen_warm_ms < GRAPH_FIRST_SCREEN_BUDGET_MS;

    let mut s = String::new();
    s.push_str("## Benchmark: T-04 Diff & Graph acceptance\n\n");
    s.push_str(&format!("- Generated at: {}\n", r.generated_at));
    s.push_str(&format!("- Diff first view (cache miss): {} ms\n", r.diff_cold_ms));
    s.push_str(&format!(
        "- Diff second view (cache hit): {:.3} ms — budget < {} ms [{}]\n",
        hit_ms,
        DIFF_CACHE_HIT_BUDGET_MS,
        if diff_ok { "PASS" } else { "FAIL" }
    ));
    s.push_str(&format!(
        "- Big repo: {} commits (generated in {} ms)\n",
        r.graph_repo_commits, r.graph_generate_ms
    ));
    s.push_str(&format!(
        "- Graph first screen, cold cache: {} ms — budget < {} ms [{}]\n",
        r.graph_first_screen_cold_ms,
        GRAPH_FIRST_SCREEN_BUDGET_MS,
        if graph_cold_ok { "PASS" } else { "FAIL" }
    ));
    s.push_str(&format!(
        "- Graph first screen, warm cache: {} ms — budget < {} ms [{}]\n",
        r.graph_first_screen_warm_ms,
        GRAPH_FIRST_SCREEN_BUDGET_MS,
        if graph_warm_ok { "PASS" } else { "FAIL" }
    ));
    s
}

/// Serialize a T-04 result to pretty JSON.
pub fn diff_graph_to_json(r: &DiffGraphBenchmarkResult) -> String {
    serde_json::to_string_pretty(r).expect("diff-graph result serialization")
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

    /// The T-04 harness must run end-to-end on a small repo: diff cache hit
    /// serves the second view, graph first screen fills a full page both cold
    /// and warm, and the report carries the budget verdicts.
    #[test]
    fn diff_graph_benchmark_smoke() {
        // 150 commits > 100 page size, so the first screen is a full page.
        let r = run_diff_graph(150);
        assert_eq!(r.graph_repo_commits, 150);
        // A cache hit is in-memory; even in debug builds it is far below budget.
        assert!(r.diff_cache_hit_us < DIFF_CACHE_HIT_BUDGET_MS * 1000);

        let report = format_diff_graph_report(&r);
        assert!(report.contains("T-04 Diff & Graph"));
        assert!(report.contains("PASS") || report.contains("FAIL"));

        let json = diff_graph_to_json(&r);
        let parsed: DiffGraphBenchmarkResult = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.graph_repo_commits, 150);
        assert_eq!(parsed.diff_cache_hit_us, r.diff_cache_hit_us);
    }

    /// Stage-by-stage diagnostic probe for the graph first screen on a 10k
    /// repo (kept ignored; run explicitly when investigating the T-04 budget):
    /// `cargo test --release --lib walk_stage_probe -- --ignored --nocapture`
    /// The generated repo is reused across runs (marker file tracks count).
    #[test]
    #[ignore]
    fn walk_stage_probe() {
        use std::collections::{BinaryHeap, HashSet};

        let dir = std::env::temp_dir().join("gw_probe_repo_10k");
        let marker = dir.join(".probe_commits");
        let n: usize = 10_000;
        let have: usize = std::fs::read_to_string(&marker)
            .ok()
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(0);
        if have < n {
            let _ = std::fs::remove_dir_all(&dir);
            let t = Instant::now();
            generate_linear_history_repo(&dir, n).expect("gen");
            eprintln!("generate {} commits: {:?}", n, t.elapsed());
            std::fs::write(&marker, n.to_string()).unwrap();
        }

        // 1. Current call pattern: push_head, then set_sorting(TIME).
        let t = Instant::now();
        let oids = crate::core::graph::revwalk_oids(&dir, 100).unwrap();
        eprintln!("revwalk TIME take(100): {:?} ({} oids)", t.elapsed(), oids.len());

        // 2. Sorting set BEFORE pushing (libgit2 docs hint order matters).
        let t = Instant::now();
        {
            let repo = git2::Repository::open(&dir).unwrap();
            let mut w = repo.revwalk().unwrap();
            w.set_sorting(git2::Sort::TIME).unwrap();
            w.push_head().unwrap();
            let v: Vec<_> = w.take(100).flatten().collect();
            eprintln!(
                "revwalk sort-then-push TIME take(100): {:?} ({} oids)",
                t.elapsed(),
                v.len()
            );
            assert_eq!(v[0].to_string(), oids[0]);
        }

        // 3. No sorting (insertion order — lazy by construction).
        let t = Instant::now();
        {
            let repo = git2::Repository::open(&dir).unwrap();
            let mut w = repo.revwalk().unwrap();
            w.set_sorting(git2::Sort::NONE).unwrap();
            w.push_head().unwrap();
            let v: Vec<_> = w.take(100).flatten().collect();
            eprintln!("revwalk NONE take(100): {:?} ({} oids)", t.elapsed(), v.len());
            assert_eq!(v[0].to_string(), oids[0]);
        }

        // 4. Scaling check: does TIME cost grow with take()?
        let t = Instant::now();
        let oids1000 = crate::core::graph::revwalk_oids(&dir, 1000).unwrap();
        eprintln!("revwalk TIME take(1000): {:?} ({} oids)", t.elapsed(), oids1000.len());

        // 5. Candidate fix: hand-rolled lazy newest-first heap walk.
        let t = Instant::now();
        {
            let repo = git2::Repository::open(&dir).unwrap();
            let head = repo.head().unwrap().peel_to_commit().unwrap();
            let mut seen: HashSet<git2::Oid> = HashSet::new();
            let mut heap: BinaryHeap<(i64, u64, git2::Oid)> = BinaryHeap::new();
            let mut seq: u64 = 0;
            seen.insert(head.id());
            heap.push((head.time().seconds(), seq, head.id()));
            let mut out: Vec<git2::Oid> = Vec::new();
            while out.len() < 100 {
                let Some((_, _, oid)) = heap.pop() else { break };
                out.push(oid);
                let commit = repo.find_commit(oid).unwrap();
                for pid in commit.parent_ids() {
                    if seen.insert(pid) {
                        seq += 1;
                        let p = repo.find_commit(pid).unwrap();
                        heap.push((p.time().seconds(), seq, pid));
                    }
                }
            }
            eprintln!("manual heap walk(100): {:?} ({} oids)", t.elapsed(), out.len());
            assert_eq!(out.len(), 100);
            assert_eq!(out[0].to_string(), oids[0]);
            // Manual walk must match the revwalk order exactly on this repo.
            assert_eq!(out.iter().map(|o| o.to_string()).collect::<Vec<_>>(), oids);
        }

        // 6. Reference: object-read cost floor for 100 commits.
        let t = Instant::now();
        let repo = git2::Repository::open(&dir).unwrap();
        for s in &oids {
            let oid = git2::Oid::from_str(s).unwrap();
            std::hint::black_box(crate::core::graph::commit_record_from_oid(&repo, &oid));
        }
        eprintln!("parse 100 commits: {:?}", t.elapsed());
    }
}
