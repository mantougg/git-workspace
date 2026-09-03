//! Runtime Workspace benchmark (R-08): staged timings for the Maven
//! discovery → parse → index/resolve → graph → closure → reactor → config
//! pipeline, plus process resource metrics, on a synthetic workspace from
//! [`super::maven_gen`]. §99 targets are checked with explicit PASS/FAIL
//! verdicts; Build / Application Start stay reserved until R-09 / R-10 land.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use sysinfo::{get_current_pid, System};

use crate::maven::{
    compute_runtime_closure, discover_poms, prepare_runtime_reactor, query_dependency_graph,
    sync_workspace_index, DependencyGraphCache, PomCache, RuntimeClosureCache, RuntimeScope,
};
use crate::runtime::{create_config, get_config, CreateRuntimeConfigRequest, RuntimeApplicationConfig};

use super::maven_gen::{artifact_id, generate_maven_workspace, SyntheticMavenWorkspace};
use super::process_rss;

// §99 performance targets (milliseconds). Build / Spring Boot start have no
// fixed SLA (§99) and are tracked as trends once R-09 / R-10 exist.
pub const RUNTIME_DISCOVERY_BUDGET_MS: u128 = 500;
pub const POM_CACHE_HIT_BUDGET_MS: u128 = 50;
pub const GRAPH_CACHE_HIT_BUDGET_MS: u128 = 100;
pub const CONFIG_LOAD_BUDGET_MS: u128 = 50;
pub const FILE_CHANGE_DETECTION_BUDGET_MS: u128 = 300;

/// Structured result of one Runtime benchmark run. Serializable so it can be
/// archived as a baseline and diffed against later runs (趋势追踪).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeBenchmarkResult {
    pub repositories: usize,
    pub modules_per_repository: usize,
    /// Total parsed POMs (one parent + N modules per repository).
    pub project_count: usize,
    pub generated_at: String,
    /// Synthetic workspace generation (not part of any measured stage).
    pub generate_ms: u128,
    // ---- §97 staged timings ----
    /// Cold discovery: workspace scan + full POM parse, no cache.
    pub discovery_cold_ms: u128,
    /// First cached discovery (all POM cache misses = pure parse path).
    pub pom_cache_miss_ms: u128,
    /// Second cached discovery over unchanged POMs (all cache hits).
    pub pom_cache_hit_ms: u128,
    /// Cache hits / misses during that reload. Misses above zero mean the
    /// bounded `PomCache` capacity evicted entries at this scale.
    #[serde(default)]
    pub pom_cache_hit_run_hits: u64,
    #[serde(default)]
    pub pom_cache_hit_run_misses: u64,
    /// Single-POM cache hit (microseconds).
    pub single_pom_cache_hit_us: u128,
    /// R-02 dependency resolve: first SQLite index sync (insert all).
    pub index_sync_ms: u128,
    /// R-02 index sync over an unchanged workspace (incremental no-op).
    pub index_resync_ms: u128,
    /// Dependency graph load from SQLite (cache-miss path).
    pub graph_query_ms: u128,
    /// Dependency graph load served from the in-memory graph cache.
    pub graph_cache_hit_ms: u128,
    /// Runtime closure computation (cache miss).
    pub closure_ms: u128,
    /// Runtime closure served from the closure cache (microseconds).
    pub closure_cache_hit_us: u128,
    /// `prepare_runtime_reactor` — Existing reuse or Synthetic generation.
    pub reactor_ms: u128,
    pub reactor_kind: String,
    /// R-07 runtime config create (JSON file + SQLite metadata).
    pub config_write_ms: u128,
    /// R-07 runtime config load.
    pub config_load_ms: u128,
    /// File write → OS watcher event received (raw `notify` path; the T-06
    /// debounce window is a deliberate batching constant on top of this).
    /// `None` when the platform watcher did not deliver within the timeout.
    pub file_change_detection_ms: Option<u128>,
    // ---- §97 resource metrics ----
    /// RSS right after workspace generation, before the measured stages.
    pub idle_rss_bytes: u64,
    pub peak_rss_bytes: u64,
    pub thread_count: usize,
    /// Mean process CPU usage (%) sampled during the measured stages.
    pub cpu_usage_percent: f32,
    pub disk_read_bytes: u64,
    pub disk_write_bytes: u64,
    /// Peak number of child processes of the benchmark process (expected 0 —
    /// no Maven/Java subprocess is spawned by the measured stages).
    pub child_process_count: usize,
    // ---- Reserved for later tasks ----
    /// R-09 Build Engine wiring point.
    pub build_ms: Option<u128>,
    /// R-10 Launcher wiring point.
    pub app_start_ms: Option<u128>,
}

/// One §99 budget verdict.
#[derive(Debug, Clone, PartialEq)]
pub struct BudgetVerdict {
    pub name: &'static str,
    pub value_ms: f64,
    pub budget_ms: u128,
    pub pass: bool,
}

/// Evaluate the §99 targets that this benchmark measures. The file-change
/// verdict is included only when the watcher delivered a measurement.
pub fn budget_verdicts(r: &RuntimeBenchmarkResult) -> Vec<BudgetVerdict> {
    let mut verdicts = vec![
        BudgetVerdict {
            name: "Maven Project Discovery",
            value_ms: r.discovery_cold_ms as f64,
            budget_ms: RUNTIME_DISCOVERY_BUDGET_MS,
            pass: r.discovery_cold_ms < RUNTIME_DISCOVERY_BUDGET_MS,
        },
        BudgetVerdict {
            name: "POM Cache Hit",
            value_ms: r.pom_cache_hit_ms as f64,
            budget_ms: POM_CACHE_HIT_BUDGET_MS,
            pass: r.pom_cache_hit_ms < POM_CACHE_HIT_BUDGET_MS,
        },
        BudgetVerdict {
            name: "Dependency Graph Cache Hit",
            value_ms: r.graph_cache_hit_ms as f64,
            budget_ms: GRAPH_CACHE_HIT_BUDGET_MS,
            pass: r.graph_cache_hit_ms < GRAPH_CACHE_HIT_BUDGET_MS,
        },
        BudgetVerdict {
            name: "Runtime Configuration Load",
            value_ms: r.config_load_ms as f64,
            budget_ms: CONFIG_LOAD_BUDGET_MS,
            pass: r.config_load_ms < CONFIG_LOAD_BUDGET_MS,
        },
    ];
    if let Some(detection_ms) = r.file_change_detection_ms {
        verdicts.push(BudgetVerdict {
            name: "File Change → Detection",
            value_ms: detection_ms as f64,
            budget_ms: FILE_CHANGE_DETECTION_BUDGET_MS,
            pass: detection_ms < FILE_CHANGE_DETECTION_BUDGET_MS,
        });
    }
    verdicts
}

/// Process resource sampler: polls RSS / CPU / threads / disk IO / child
/// process count on an independent thread so sampling overhead stays out of
/// the measured stages (which are timed with their own `Instant`s).
struct ResourceSampler {
    stop: Arc<AtomicBool>,
    handle: Option<std::thread::JoinHandle<ResourceSummary>>,
}

#[derive(Debug, Default)]
struct ResourceSummary {
    peak_rss_bytes: u64,
    thread_count: usize,
    cpu_usage_percent: f32,
    disk_read_bytes: u64,
    disk_write_bytes: u64,
    child_process_count: usize,
}

/// Thread count of this benchmark process. `/proc/self/status` is exact on
/// Linux; elsewhere fall back to sysinfo's task set (best effort).
fn own_thread_count() -> usize {
    #[cfg(target_os = "linux")]
    {
        if let Ok(status) = std::fs::read_to_string("/proc/self/status") {
            if let Some(threads) = status
                .lines()
                .find_map(|line| line.strip_prefix("Threads:"))
                .and_then(|value| value.trim().parse::<usize>().ok())
            {
                return threads;
            }
        }
    }
    let pid = get_current_pid().expect("current pid");
    let mut system = System::new();
    super::process_threads(&mut system, pid)
}

impl ResourceSampler {
    fn start() -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let handle = std::thread::spawn({
            let stop = Arc::clone(&stop);
            move || {
                let pid = get_current_pid().expect("current pid");
                let mut system = System::new();
                system.refresh_process(pid);
                let start_disk = system.process(pid).map(|p| p.disk_usage());

                let mut summary = ResourceSummary::default();
                let mut cpu_samples: u64 = 0;
                let mut tick: u64 = 0;
                while !stop.load(Ordering::Relaxed) {
                    let rss = process_rss(&mut system, pid);
                    summary.peak_rss_bytes = summary.peak_rss_bytes.max(rss);
                    summary.thread_count = summary.thread_count.max(own_thread_count());
                    if let Some(process) = system.process(pid) {
                        summary.cpu_usage_percent += process.cpu_usage();
                        cpu_samples += 1;
                    }
                    // Full process scan is the expensive probe — run it at a
                    // fifth of the tick rate. On Linux our own threads appear
                    // as pseudo-processes, so real child processes are
                    // filtered by `thread_kind()`.
                    if tick.is_multiple_of(5) {
                        system.refresh_processes();
                        let children = system
                            .processes()
                            .values()
                            .filter(|p| p.parent() == Some(pid) && p.thread_kind().is_none())
                            .count();
                        summary.child_process_count =
                            summary.child_process_count.max(children);
                    }
                    tick += 1;
                    std::thread::sleep(Duration::from_millis(20));
                }

                system.refresh_process(pid);
                if let (Some(start), Some(end)) = (start_disk, system.process(pid).map(|p| p.disk_usage())) {
                    summary.disk_read_bytes =
                        end.total_read_bytes.saturating_sub(start.total_read_bytes);
                    summary.disk_write_bytes = end
                        .total_written_bytes
                        .saturating_sub(start.total_written_bytes);
                }
                if cpu_samples > 0 {
                    summary.cpu_usage_percent /= cpu_samples as f32;
                }
                summary
            }
        });
        Self {
            stop,
            handle: Some(handle),
        }
    }

    fn stop(mut self) -> ResourceSummary {
        self.stop.store(true, Ordering::Relaxed);
        self.handle
            .take()
            .map(|handle| handle.join().unwrap_or_default())
            .unwrap_or_default()
    }
}

/// Run the Runtime benchmark over a `repositories × modules` synthetic
/// workspace and return the structured result.
pub fn run_runtime(repositories: usize, modules_per_repository: usize) -> RuntimeBenchmarkResult {
    // Unique per invocation: tests run benchmark instances in parallel threads.
    static RUN_COUNTER: AtomicU64 = AtomicU64::new(0);
    let run_id = RUN_COUNTER.fetch_add(1, Ordering::Relaxed);
    let tmp = std::env::temp_dir().join(format!("gw_bench_runtime_{}_{}", std::process::id(), run_id));
    let _ = std::fs::remove_dir_all(&tmp);

    let gen_start = Instant::now();
    let spec = generate_maven_workspace(&tmp, repositories, modules_per_repository)
        .expect("generate_maven_workspace failed");
    let generate_ms = gen_start.elapsed().as_millis();

    let sampler = ResourceSampler::start();
    let mut system = System::new();
    let pid = get_current_pid().expect("current pid");
    let idle_rss_bytes = process_rss(&mut system, pid);

    // ---- Stage: Maven Project Discovery (cold, no cache) ----
    let stage = Instant::now();
    let discovery = discover_poms(&tmp, 5, None, None);
    let discovery_cold_ms = stage.elapsed().as_millis();
    assert_eq!(
        discovery.projects.len(),
        spec.project_count,
        "discovery must find every synthetic POM"
    );
    assert!(discovery.errors.is_empty(), "synthetic POMs must parse");

    // ---- Stage: POM Cache miss / hit (§99: hit < 50 ms) ----
    let cache = PomCache::new();
    let stage = Instant::now();
    let miss_run = discover_poms(&tmp, 5, Some(&cache), None);
    let pom_cache_miss_ms = stage.elapsed().as_millis();
    assert_eq!(miss_run.stats.misses, spec.project_count as u64);

    // `PomCache::stats()` is cumulative — snapshot before the reload and diff.
    let stats_before = cache.stats();
    let stage = Instant::now();
    let hit_run = discover_poms(&tmp, 5, Some(&cache), None);
    let pom_cache_hit_ms = stage.elapsed().as_millis();
    let hit_run_hits = hit_run.stats.hits - stats_before.hits;
    let hit_run_misses = hit_run.stats.misses - stats_before.misses;
    // Every POM must be served (hit or miss); at large matrices the bounded
    // PomCache capacity evicts entries, and the misses are reported, not hidden.
    assert_eq!(hit_run_hits + hit_run_misses, spec.project_count as u64);

    let any_pom = discovery.projects[0].path.clone();
    let stage = Instant::now();
    let _ = cache.get_or_parse(&any_pom).expect("cached pom");
    let single_pom_cache_hit_us = stage.elapsed().as_micros();

    // ---- Stage: Dependency Resolve / index sync (R-02) ----
    let db_path = tmp.join("bench.db");
    let mut conn = rusqlite::Connection::open(&db_path).expect("open bench db");
    crate::db::init_db(&mut conn).expect("init bench db");
    let workspace_id = insert_workspace(&mut conn, &spec);
    let local_repository = tmp.join("m2");

    let stage = Instant::now();
    let sync = sync_workspace_index(&mut conn, workspace_id, &discovery, &local_repository)
        .expect("index sync failed");
    let index_sync_ms = stage.elapsed().as_millis();
    assert_eq!(sync.inserted, spec.project_count);

    let stage = Instant::now();
    let resync = sync_workspace_index(&mut conn, workspace_id, &discovery, &local_repository)
        .expect("index resync failed");
    let index_resync_ms = stage.elapsed().as_millis();
    assert_eq!(resync.unchanged, spec.project_count);

    // ---- Stage: Dependency graph query / cache hit (§99: hit < 100 ms) ----
    let stage = Instant::now();
    let graph = query_dependency_graph(&conn, workspace_id).expect("graph query failed");
    let graph_query_ms = stage.elapsed().as_millis();

    let graph_cache = DependencyGraphCache::new();
    let first = graph_cache
        .get_or_load(&conn, workspace_id)
        .expect("graph cache miss load failed");
    assert!(!first.cache_hit);
    let stage = Instant::now();
    let second = graph_cache
        .get_or_load(&conn, workspace_id)
        .expect("graph cache hit load failed");
    let graph_cache_hit_ms = stage.elapsed().as_millis();
    assert!(second.cache_hit);

    // ---- Stage: Runtime closure / cache hit (R-03) ----
    let root_artifact = artifact_id(repositories - 1, modules_per_repository - 1);
    let root_project_id = graph
        .projects
        .iter()
        .find(|project| project.coordinates.artifact_id == root_artifact)
        .unwrap_or_else(|| panic!("closure root {root_artifact} must exist"))
        .project_id;

    let stage = Instant::now();
    let closure = compute_runtime_closure(&graph, root_project_id, &RuntimeScope::Auto)
        .expect("closure computation failed");
    let closure_ms = stage.elapsed().as_millis();

    let closure_cache = RuntimeClosureCache::new();
    let _ = closure_cache
        .get_or_compute(&graph, root_project_id, &RuntimeScope::Auto)
        .expect("closure cache miss failed");
    let stage = Instant::now();
    let cached = closure_cache
        .get_or_compute(&graph, root_project_id, &RuntimeScope::Auto)
        .expect("closure cache hit failed");
    let closure_cache_hit_us = stage.elapsed().as_micros();
    assert!(cached.cache_hit);

    // ---- Stage: Synthetic Reactor generation (R-03) ----
    let stage = Instant::now();
    let plan = prepare_runtime_reactor(&graph, &closure, &tmp, "bench-app")
        .expect("reactor plan failed");
    let reactor_ms = stage.elapsed().as_millis();
    let reactor_kind = format!("{:?}", plan.kind);

    // ---- Stage: Runtime config write / load (R-07, §99: load < 50 ms) ----
    let config = RuntimeApplicationConfig {
        name: "bench-app".into(),
        project: graph
            .projects
            .iter()
            .find(|project| project.project_id == root_project_id)
            .expect("root project")
            .path
            .to_string_lossy()
            .to_string(),
        environment: [
            ("APP_ENV".to_string(), "bench".to_string()),
            ("DB_PASSWORD".to_string(), "bench-secret".to_string()),
        ]
        .into_iter()
        .collect(),
        ..Default::default()
    };
    let stage = Instant::now();
    create_config(
        &conn,
        &CreateRuntimeConfigRequest {
            workspace_id,
            config,
        },
    )
    .expect("config create failed");
    let config_write_ms = stage.elapsed().as_millis();

    let stage = Instant::now();
    let loaded = get_config(&conn, workspace_id, "bench-app").expect("config load failed");
    let config_load_ms = stage.elapsed().as_millis();
    assert_eq!(loaded.name, "bench-app");

    // ---- Stage: File Change → Detection (§99: < 300 ms) ----
    let file_change_detection_ms = measure_file_change_detection(&spec.repository_paths[0]);

    let resources = sampler.stop();
    drop(conn);
    let _ = std::fs::remove_dir_all(&tmp);

    RuntimeBenchmarkResult {
        repositories,
        modules_per_repository,
        project_count: spec.project_count,
        generated_at: chrono::Utc::now().to_rfc3339(),
        generate_ms,
        discovery_cold_ms,
        pom_cache_miss_ms,
        pom_cache_hit_ms,
        pom_cache_hit_run_hits: hit_run_hits,
        pom_cache_hit_run_misses: hit_run_misses,
        single_pom_cache_hit_us,
        index_sync_ms,
        index_resync_ms,
        graph_query_ms,
        graph_cache_hit_ms,
        closure_ms,
        closure_cache_hit_us,
        reactor_ms,
        reactor_kind,
        config_write_ms,
        config_load_ms,
        file_change_detection_ms,
        idle_rss_bytes,
        peak_rss_bytes: resources.peak_rss_bytes,
        thread_count: resources.thread_count,
        cpu_usage_percent: resources.cpu_usage_percent,
        disk_read_bytes: resources.disk_read_bytes,
        disk_write_bytes: resources.disk_write_bytes,
        child_process_count: resources.child_process_count,
        build_ms: None,
        app_start_ms: None,
    }
}

fn insert_workspace(conn: &mut rusqlite::Connection, spec: &SyntheticMavenWorkspace) -> i64 {
    conn.execute(
        "INSERT INTO workspaces (name, path, created_at, updated_at) VALUES ('bench', ?1, 't', 't')",
        [spec.root.to_string_lossy().to_string()],
    )
    .expect("insert workspace");
    let workspace_id = conn.last_insert_rowid();
    let scanned: Vec<crate::models::repository::ScannedRepo> = spec
        .repository_paths
        .iter()
        .map(|path| crate::models::repository::ScannedRepo {
            path: path.to_string_lossy().to_string(),
            name: path.file_name().unwrap().to_string_lossy().to_string(),
            relative_path: path.file_name().unwrap().to_string_lossy().to_string(),
            git_dir_mtime: None,
        })
        .collect();
    crate::db::dao::upsert_repositories_batch(conn, workspace_id, &scanned)
        .expect("register repositories");
    workspace_id
}

/// Measure the raw file-change detection latency: arm a `notify` watcher on
/// `repo_root` (the same backend T-06 uses), write a scratch file, and time
/// the arrival of its event. `None` when the backend does not deliver within
/// the timeout (exotic/loaded environments) — reported as N/A, never PASS.
fn measure_file_change_detection(repo_root: &Path) -> Option<u128> {
    use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher};

    let (tx, rx) = std::sync::mpsc::channel();
    let mut watcher =
        RecommendedWatcher::new(move |event| drop(tx.send(event)), Config::default()).ok()?;
    watcher.watch(repo_root, RecursiveMode::Recursive).ok()?;
    // Let the backend finish registering before the probe write.
    std::thread::sleep(Duration::from_millis(200));

    let probe = repo_root.join(".bench-probe");
    let start = Instant::now();
    std::fs::write(&probe, b"probe\n").ok()?;
    const TIMEOUT: Duration = Duration::from_secs(5);
    loop {
        let remaining = TIMEOUT.checked_sub(start.elapsed())?;
        match rx.recv_timeout(remaining) {
            Ok(Ok(event)) if event.paths.iter().any(|path| path == &probe) => {
                let _ = std::fs::remove_file(&probe);
                return Some(start.elapsed().as_millis());
            }
            Ok(_) => continue, // unrelated event (e.g. directory entry)
            Err(_) => return None,
        }
    }
}

/// Render the Markdown report: staged timings, §99 verdicts, resources.
pub fn format_runtime_report(r: &RuntimeBenchmarkResult) -> String {
    let mut s = String::new();
    s.push_str(&format!(
        "## Runtime Benchmark: {} repositories × {} modules ({} POMs)\n\n",
        r.repositories, r.modules_per_repository, r.project_count
    ));
    s.push_str(&format!("- Generated at: {}\n", r.generated_at));
    s.push_str(&format!("- Workspace generate: {} ms\n", r.generate_ms));

    s.push_str("\n### 阶段耗时（§97）\n\n");
    s.push_str("| stage | value |\n|---|---:|\n");
    row_ms(&mut s, "Maven Project Discovery (cold)", r.discovery_cold_ms);
    row_ms(&mut s, "POM Parse (cache miss)", r.pom_cache_miss_ms);
    s.push_str(&format!(
        "| POM Cache Hit (workspace reload, {}/{} hits) | {} ms |\n",
        r.pom_cache_hit_run_hits,
        r.pom_cache_hit_run_hits + r.pom_cache_hit_run_misses,
        r.pom_cache_hit_ms
    ));
    s.push_str(&format!(
        "| Single POM Cache Hit | {:.3} ms |\n",
        r.single_pom_cache_hit_us as f64 / 1000.0
    ));
    row_ms(&mut s, "Dependency Resolve (index sync)", r.index_sync_ms);
    row_ms(&mut s, "Index Resync (unchanged)", r.index_resync_ms);
    row_ms(&mut s, "Dependency Graph Query", r.graph_query_ms);
    row_ms(&mut s, "Dependency Graph Cache Hit", r.graph_cache_hit_ms);
    row_ms(&mut s, "Runtime Closure", r.closure_ms);
    s.push_str(&format!(
        "| Runtime Closure Cache Hit | {:.3} ms |\n",
        r.closure_cache_hit_us as f64 / 1000.0
    ));
    s.push_str(&format!(
        "| Reactor ({}) | {} ms |\n",
        r.reactor_kind, r.reactor_ms
    ));
    row_ms(&mut s, "Runtime Config Write", r.config_write_ms);
    row_ms(&mut s, "Runtime Config Load", r.config_load_ms);
    match r.file_change_detection_ms {
        Some(ms) => row_ms(&mut s, "File Change → Detection", ms),
        None => s.push_str("| File Change → Detection | N/A (watcher 未交付事件) |\n"),
    }
    s.push_str("| Build | N/A（待 R-09 接入） |\n");
    s.push_str("| Application Start | N/A（待 R-10 接入） |\n");

    s.push_str("\n### §99 目标校验\n\n");
    s.push_str("| 指标 | 实测 | 目标 | 判定 |\n|---|---:|---:|:---:|\n");
    for verdict in budget_verdicts(r) {
        s.push_str(&format!(
            "| {} | {:.2} ms | < {} ms | {} |\n",
            verdict.name,
            verdict.value_ms,
            verdict.budget_ms,
            if verdict.pass { "PASS" } else { "FAIL" }
        ));
    }

    s.push_str("\n### 资源（§97）\n\n");
    s.push_str(&format!("- Idle RSS: {:.2} MB\n", mb(r.idle_rss_bytes)));
    s.push_str(&format!("- Peak RSS: {:.2} MB\n", mb(r.peak_rss_bytes)));
    s.push_str(&format!("- CPU (process avg): {:.1} %\n", r.cpu_usage_percent));
    s.push_str(&format!("- Disk read: {:.2} MB\n", mb(r.disk_read_bytes)));
    s.push_str(&format!("- Disk write: {:.2} MB\n", mb(r.disk_write_bytes)));
    s.push_str(&format!("- Thread count (peak): {}\n", r.thread_count));
    s.push_str(&format!("- Child process count (peak): {}\n", r.child_process_count));

    s.push_str(
        "\n### IDEA 对比\n\n口径与场景见 `docs/tasks-runtime/R-08-idea-comparison.md`（§98 半自动对比）。\n",
    );
    s
}

/// Serialize a result to pretty JSON.
pub fn runtime_to_json(r: &RuntimeBenchmarkResult) -> String {
    serde_json::to_string_pretty(r).expect("runtime benchmark result serialization")
}

/// Load a previously saved runtime baseline (JSON) from disk.
pub fn load_runtime_baseline(path: &Path) -> Option<RuntimeBenchmarkResult> {
    let content = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

/// Save a runtime result as a baseline (JSON) to disk.
pub fn save_runtime_baseline(r: &RuntimeBenchmarkResult, path: &Path) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, runtime_to_json(r))
}

/// Baseline file name for a matrix point: `runtime_<repos>x<modules>.json`.
pub fn runtime_baseline_path(dir: &Path, repositories: usize, modules: usize) -> PathBuf {
    dir.join(format!("runtime_{repositories}x{modules}.json"))
}

/// Render a Markdown comparison between a previous baseline and the current run.
pub fn format_runtime_comparison(
    prev: &RuntimeBenchmarkResult,
    curr: &RuntimeBenchmarkResult,
) -> String {
    let mut s = String::new();
    s.push_str("## Runtime benchmark comparison (vs baseline)\n\n");
    s.push_str("| metric | baseline | current | delta |\n|---|---:|---:|---:|\n");
    compare_row(&mut s, "Discovery (cold)", prev.discovery_cold_ms, curr.discovery_cold_ms);
    compare_row(&mut s, "POM Cache Hit", prev.pom_cache_hit_ms, curr.pom_cache_hit_ms);
    compare_row(&mut s, "Index Sync", prev.index_sync_ms, curr.index_sync_ms);
    compare_row(&mut s, "Graph Cache Hit", prev.graph_cache_hit_ms, curr.graph_cache_hit_ms);
    compare_row(&mut s, "Closure", prev.closure_ms, curr.closure_ms);
    compare_row(&mut s, "Reactor", prev.reactor_ms, curr.reactor_ms);
    compare_row(&mut s, "Config Load", prev.config_load_ms, curr.config_load_ms);

    let prev_mb = mb(prev.peak_rss_bytes);
    let curr_mb = mb(curr.peak_rss_bytes);
    let delta = (curr_mb - prev_mb) / prev_mb.max(1.0) * 100.0;
    s.push_str(&format!(
        "| Peak RSS | {prev_mb:.1} MB | {curr_mb:.1} MB | {delta:+.1}% |\n"
    ));
    s
}

fn row_ms(out: &mut String, name: &str, ms: u128) {
    out.push_str(&format!("| {name} | {ms} ms |\n"));
}

fn mb(bytes: u64) -> f64 {
    bytes as f64 / 1024.0 / 1024.0
}

fn compare_row(out: &mut String, name: &str, prev_ms: u128, curr_ms: u128) {
    let delta = (curr_ms as f64 - prev_ms as f64) / prev_ms.max(1) as f64 * 100.0;
    out.push_str(&format!(
        "| {name} | {prev_ms} ms | {curr_ms} ms | {delta:+.1}% |\n"
    ));
}

// ---------------------------------------------------------------------------
// R-09 Build stage（手动跑：由 example 的 `build` 子命令触发，不进 cargo test）
// ---------------------------------------------------------------------------

/// R-09 Build benchmark 结果。Build 不设固定 SLA（§99），只记录趋势；
/// 关注 `second_build_ms <= first_build_ms`（Maven 原生 ~/.m2 缓存 +
/// R-09 classpath 缓存生效）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildBenchmarkResult {
    pub repositories: usize,
    pub modules_per_repository: usize,
    pub generated_at: String,
    pub strategy: String,
    pub reactor_kind: String,
    pub modules_built: usize,
    pub first_build_ms: u128,
    pub second_build_ms: u128,
}

/// 生成可真实构建的工作区。
///
/// 与 [`maven_gen`] 同形（repositories × modules、in-repo 依赖链、跨仓边），
/// 但**不含虚构远程依赖**：maven_gen 的 `org.bench.external:commons-external`
/// 是故意不可解析的 Remote 依赖（服务解析基准），真实 `mvn compile` 会因它
/// 失败，所以 Build 基准用自包含工作区。
fn generate_buildable_workspace(
    root: &Path,
    repositories: usize,
    modules_per_repository: usize,
) -> std::io::Result<Vec<PathBuf>> {
    assert!(repositories >= 1 && modules_per_repository >= 1);
    std::fs::create_dir_all(root)?;
    let mut repository_paths = Vec::with_capacity(repositories);
    for repo in 0..repositories {
        let repo_dir = root.join(format!("repo_{repo:04}"));
        std::fs::create_dir_all(&repo_dir)?;
        let module_entries: String = (0..modules_per_repository)
            .map(|module| format!("    <module>module_{module:02}</module>\n"))
            .collect();
        std::fs::write(
            repo_dir.join("pom.xml"),
            format!(
                r#"<?xml version="1.0" encoding="UTF-8"?>
<project xmlns="http://maven.apache.org/POM/4.0.0">
  <modelVersion>4.0.0</modelVersion>
  <groupId>com.benchbuild.r{repo}</groupId>
  <artifactId>repo-{repo:03}-parent</artifactId>
  <version>1.0.0</version>
  <packaging>pom</packaging>
  <properties>
    <maven.compiler.source>17</maven.compiler.source>
    <maven.compiler.target>17</maven.compiler.target>
  </properties>
  <modules>
{module_entries}  </modules>
</project>
"#
            ),
        )?;
        for module in 0..modules_per_repository {
            let module_dir = repo_dir.join(format!("module_{module:02}"));
            std::fs::create_dir_all(&module_dir)?;
            let mut dependencies = String::new();
            if module > 0 {
                dependencies.push_str(&format!(
                    "    <dependency>\n      <groupId>com.benchbuild.r{repo}</groupId>\n      <artifactId>module-{repo:03}-{:02}</artifactId>\n      <version>${{project.version}}</version>\n    </dependency>\n",
                    module - 1
                ));
            }
            if module == modules_per_repository - 1 && repo > 0 {
                dependencies.push_str(&format!(
                    "    <dependency>\n      <groupId>com.benchbuild.r{}</groupId>\n      <artifactId>module-{:03}-00</artifactId>\n      <version>1.0.0</version>\n    </dependency>\n",
                    repo - 1,
                    repo - 1
                ));
            }
            std::fs::write(
                module_dir.join("pom.xml"),
                format!(
                    r#"<?xml version="1.0" encoding="UTF-8"?>
<project xmlns="http://maven.apache.org/POM/4.0.0">
  <modelVersion>4.0.0</modelVersion>
  <parent>
    <groupId>com.benchbuild.r{repo}</groupId>
    <artifactId>repo-{repo:03}-parent</artifactId>
    <version>1.0.0</version>
  </parent>
  <artifactId>module-{repo:03}-{module:02}</artifactId>
  <dependencies>
{dependencies}  </dependencies>
</project>
"#
                ),
            )?;
            let package_dir = module_dir
                .join("src/main/java/com/benchbuild")
                .join(format!("r{repo}"))
                .join(format!("m{module}"));
            std::fs::create_dir_all(&package_dir)?;
            std::fs::write(
                package_dir.join("App.java"),
                format!(
                    "package com.benchbuild.r{repo}.m{module};\n\npublic final class App {{\n    private App() {{}}\n    public static int id() {{\n        return {repo} * 1000 + {module};\n    }}\n}}\n"
                ),
            )?;
        }
        git2::Repository::init(&repo_dir).map_err(super::io_err)?;
        repository_paths.push(repo_dir);
    }
    Ok(repository_paths)
}

/// R-09 Build 基准：对合成工作区的最后一个模块跑两次 Classpath Run
/// （真实 mvn，可用性不足时返回 `None` 由调用方记录 skip）。
pub fn run_build_benchmark(
    repositories: usize,
    modules_per_repository: usize,
) -> Option<BuildBenchmarkResult> {
    use crate::runtime::build::pipeline::execute_build;
    use crate::runtime::build::runner::SpawningMavenRunner;
    use crate::runtime::build::scheduler::BuildScheduler;
    use crate::runtime::build::{BuildOptions, BuildRequest, RingTail, RunStrategy};
    use crate::runtime::{create_config, CreateRuntimeConfigRequest, RuntimeApplicationConfig};

    let maven = if cfg!(windows) { "mvn.cmd" } else { "mvn" };
    if std::process::Command::new(maven)
        .arg("-version")
        .output()
        .is_err()
    {
        eprintln!("build benchmark: `{maven}` unavailable, skipping");
        return None;
    }

    static BUILD_RUN_COUNTER: AtomicU64 = AtomicU64::new(0);
    let run_id = BUILD_RUN_COUNTER.fetch_add(1, Ordering::Relaxed);
    let tmp = std::env::temp_dir().join(format!(
        "gw_bench_build_{}_{}",
        std::process::id(),
        run_id
    ));
    let _ = std::fs::remove_dir_all(&tmp);

    let repository_paths = generate_buildable_workspace(&tmp, repositories, modules_per_repository)
        .expect("generate buildable workspace");

    let db_path = tmp.join("bench.db");
    let mut conn = rusqlite::Connection::open(&db_path).expect("open bench db");
    crate::db::init_db(&mut conn).expect("init bench db");
    conn.execute(
        "INSERT INTO workspaces (name, path, created_at, updated_at) VALUES ('bench', ?1, 't', 't')",
        [tmp.to_string_lossy().to_string()],
    )
    .expect("insert workspace");
    let workspace_id = conn.last_insert_rowid();
    let scanned: Vec<crate::models::repository::ScannedRepo> = repository_paths
        .iter()
        .map(|path| crate::models::repository::ScannedRepo {
            path: path.to_string_lossy().to_string(),
            name: path.file_name().unwrap().to_string_lossy().to_string(),
            relative_path: path.file_name().unwrap().to_string_lossy().to_string(),
            git_dir_mtime: None,
        })
        .collect();
    crate::db::dao::upsert_repositories_batch(&mut conn, workspace_id, &scanned)
        .expect("register repositories");

    let discovery = discover_poms(&tmp, 5, None, None);
    assert!(discovery.errors.is_empty(), "{:?}", discovery.errors);
    sync_workspace_index(&mut conn, workspace_id, &discovery, &tmp.join("m2"))
        .expect("index sync failed");

    let graph = query_dependency_graph(&conn, workspace_id).expect("graph query failed");
    let root_artifact = format!(
        "module-{:03}-{:02}",
        repositories - 1,
        modules_per_repository - 1
    );
    let root_project = graph
        .projects
        .iter()
        .find(|project| project.coordinates.artifact_id == root_artifact)
        .unwrap_or_else(|| panic!("build root {root_artifact} must exist"));
    let root_pom_path = root_project.path.to_string_lossy().to_string();
    let modules_built_root = graph
        .projects
        .iter()
        .filter(|project| project.coordinates.artifact_id == root_artifact)
        .count();
    assert_eq!(modules_built_root, 1);

    create_config(
        &conn,
        &CreateRuntimeConfigRequest {
            workspace_id,
            config: RuntimeApplicationConfig {
                name: "bench-build".into(),
                project: root_pom_path,
                main_class: Some("com.benchbuild.App".into()),
                ..Default::default()
            },
        },
    )
    .expect("config create failed");

    let scheduler = BuildScheduler::new(1);
    let runner = SpawningMavenRunner;
    // execute_build（R-12 起）接共享连接，按阶段自行加锁。
    let conn = std::sync::Arc::new(std::sync::Mutex::new(conn));
    let mut timings = Vec::with_capacity(2);
    let mut reactor_kind = String::new();
    let mut modules_built = 0usize;
    for round in 0..2 {
        // 每轮用全新的内存缓存（classpath 缓存在磁盘上，第二轮命中）。
        let graph_cache = DependencyGraphCache::new();
        let closure_cache = RuntimeClosureCache::new();
        let request = BuildRequest {
            workspace_id,
            runtime_name: "bench-build".into(),
            options: BuildOptions {
                strategy: Some(RunStrategy::ClasspathRun),
                timeout: Some(Duration::from_secs(15 * 60)),
                ..Default::default()
            },
        };
        let mut sink = RingTail::new();
        let stage = Instant::now();
        let outcome = execute_build(
            &conn,
            &tmp,
            &graph_cache,
            &closure_cache,
            &scheduler,
            &runner,
            &request,
            &crate::runtime::script_approval::ScriptApprovalStore::new(
                tmp.join("approvals.json"),
            ),
            &mut sink,
            None,
        )
        .unwrap_or_else(|error| {
            panic!("build benchmark round {round} failed: {error}\n{}", sink.tail())
        });
        timings.push(stage.elapsed().as_millis());
        reactor_kind = format!("{:?}", outcome.reactor_kind);
        modules_built = outcome.modules_built.len();
    }

    drop(conn);
    let _ = std::fs::remove_dir_all(&tmp);

    Some(BuildBenchmarkResult {
        repositories,
        modules_per_repository,
        generated_at: chrono::Utc::now().to_rfc3339(),
        strategy: RunStrategy::ClasspathRun.as_str().to_string(),
        reactor_kind,
        modules_built,
        first_build_ms: timings[0],
        second_build_ms: timings[1],
    })
}

/// R-18 mvnd 对比基准结果：同一合成工作区、同一 Runtime 配置，分别以
/// mvn / mvnd 驱动多次构建（Build / Restart 场景的量化收益）。
#[derive(Debug, Clone, serde::Serialize)]
pub struct MvndBenchmarkResult {
    pub repositories: usize,
    pub modules_per_repository: usize,
    pub generated_at: String,
    pub mvnd_available: bool,
    /// mvn 驱动的 N 次构建耗时（ms）。
    pub mvn_builds_ms: Vec<u128>,
    /// mvnd 驱动的 N 次构建耗时（ms）；mvnd 不可用时为空。
    pub mvnd_builds_ms: Vec<u128>,
}

/// R-18 mvnd 收益测量（§20/§73）：`runs` 次连续构建（无源码变化，即
/// 「频繁 Build / Restart」场景）。mvnd 不可用时 `mvnd_builds_ms` 为空
/// 并返回 `mvnd_available=false`（环境相关基准，调用方记录 skip）。
pub fn run_mvnd_build_benchmark(
    repositories: usize,
    modules_per_repository: usize,
    runs: usize,
) -> Option<MvndBenchmarkResult> {
    use crate::runtime::build::runner::{BuildEngineHint, MavenRunner, SpawningMavenRunner};
    use crate::runtime::build::scheduler::BuildScheduler;
    use crate::runtime::build::{BuildOptions, BuildRequest, RingTail, RunStrategy};
    use crate::runtime::{create_config, CreateRuntimeConfigRequest, RuntimeApplicationConfig};

    assert!(runs >= 1);
    let maven = if cfg!(windows) { "mvn.cmd" } else { "mvn" };
    if std::process::Command::new(maven).arg("-version").output().is_err() {
        eprintln!("mvnd benchmark: `{maven}` unavailable, skipping");
        return None;
    }
    let mvnd = crate::maven::mvnd::detect_mvnd();
    if !mvnd.available {
        eprintln!("mvnd benchmark: mvnd not installed; measuring mvn baseline only");
    }

    static MVND_RUN_COUNTER: AtomicU64 = AtomicU64::new(0);
    let run_id = MVND_RUN_COUNTER.fetch_add(1, Ordering::Relaxed);
    let tmp = std::env::temp_dir().join(format!("gw_bench_mvnd_{}_{}", std::process::id(), run_id));
    let _ = std::fs::remove_dir_all(&tmp);

    let repository_paths =
        generate_buildable_workspace(&tmp, repositories, modules_per_repository).expect("workspace");
    let db_path = tmp.join("bench.db");
    let mut conn = rusqlite::Connection::open(&db_path).expect("open bench db");
    crate::db::init_db(&mut conn).expect("init bench db");
    conn.execute(
        "INSERT INTO workspaces (name, path, created_at, updated_at) VALUES ('bench', ?1, 't', 't')",
        [tmp.to_string_lossy().to_string()],
    )
    .expect("insert workspace");
    let workspace_id = conn.last_insert_rowid();
    let scanned: Vec<crate::models::repository::ScannedRepo> = repository_paths
        .iter()
        .map(|path| crate::models::repository::ScannedRepo {
            path: path.to_string_lossy().to_string(),
            name: path.file_name().unwrap().to_string_lossy().to_string(),
            relative_path: path.file_name().unwrap().to_string_lossy().to_string(),
            git_dir_mtime: None,
        })
        .collect();
    crate::db::dao::upsert_repositories_batch(&mut conn, workspace_id, &scanned).expect("repos");
    let discovery = discover_poms(&tmp, 5, None, None);
    assert!(discovery.errors.is_empty(), "{:?}", discovery.errors);
    sync_workspace_index(&mut conn, workspace_id, &discovery, &tmp.join("m2")).expect("index sync");
    let graph = query_dependency_graph(&conn, workspace_id).expect("graph");
    let root_artifact = format!("module-{:03}-{:02}", repositories - 1, modules_per_repository - 1);
    let root_project = graph
        .projects
        .iter()
        .find(|p| p.coordinates.artifact_id == root_artifact)
        .expect("root module")
        .clone();
    let root_pom_path = root_project.path.to_string_lossy().to_string();
    // reactor pom = 仓库父 pom；构建目标 = -pl <root ga> -am（compile 全上游）。
    let reactor_pom = root_project
        .path
        .parent()
        .and_then(|dir| dir.parent())
        .map(|repo_dir| repo_dir.join("pom.xml"))
        .expect("repo parent pom");
    let root_ga = format!(
        "{}:{}",
        root_project.coordinates.group_id, root_project.coordinates.artifact_id
    );
    create_config(
        &conn,
        &CreateRuntimeConfigRequest {
            workspace_id,
            config: RuntimeApplicationConfig {
                name: "bench-mvnd".into(),
                project: root_pom_path,
                main_class: Some("com.benchbuild.App".into()),
                build_engine: Some("maven".into()),
                ..Default::default()
            },
        },
    )
    .expect("config create failed");

    let _scheduler = BuildScheduler::new(1);
    let conn = std::sync::Arc::new(std::sync::Mutex::new(conn));

    let build_once = |engine_hint: BuildEngineHint| -> u128 {
        let _graph_cache = DependencyGraphCache::new();
        let _closure_cache = RuntimeClosureCache::new();
        let _request = BuildRequest {
            workspace_id,
            runtime_name: "bench-mvnd".into(),
            options: BuildOptions {
                strategy: Some(RunStrategy::ClasspathRun),
                timeout: Some(Duration::from_secs(15 * 60)),
                dependency_cache: false,
                ..Default::default()
            },
        };
        let mut sink = RingTail::new();
        let stage = Instant::now();
        let resolved = SpawningMavenRunner
            .resolve_maven_for_engine(
                &tmp,
                &crate::maven::settings::resolve_local_repository_effective(None),
                engine_hint,
            )
            .expect("resolve engine")
            .unwrap_or_else(|| {
                panic!("engine {engine_hint:?} unavailable");
            });
        // 直接驱动 Maven 调用（compile）以隔离引擎差异：ClasspathRun 的
        // compile 步骤 = goals + reactor 参数（与 execute_build 相同形态）。
        let request2 = crate::maven::executor::build_request(
            &resolved.executable,
            &tmp,
            vec!["compile".into()],
            vec![
                "-f".into(),
                reactor_pom.to_string_lossy().into_owned(),
                "-pl".into(),
                root_ga.clone(),
                "-am".into(),
            ],
            Some(resolved.local_repository.clone()),
        );
        let exit = SpawningMavenRunner
            .run(&request2, &[], &mut sink, None, Some(Duration::from_secs(15 * 60)))
            .expect("maven run");
        assert_eq!(exit.exit_code, Some(0), "benchmark build failed: {}", sink.tail());
        stage.elapsed().as_millis()
    };

    // 每引擎各跑 runs 次；第一轮预热本地仓库（不计入对比也不计入 baseline ——
    // 两引擎各自预热，保证公平）。
    let _ = build_once(BuildEngineHint::Maven);
    let mut mvn_builds = Vec::with_capacity(runs);
    for _ in 0..runs {
        mvn_builds.push(build_once(BuildEngineHint::Maven));
    }
    let mut mvnd_builds = Vec::new();
    if mvnd.available {
        let _ = build_once(BuildEngineHint::Mvnd);
        for _ in 0..runs {
            mvnd_builds.push(build_once(BuildEngineHint::Mvnd));
        }
    }

    drop(conn);
    let _ = std::fs::remove_dir_all(&tmp);

    Some(MvndBenchmarkResult {
        repositories,
        modules_per_repository,
        generated_at: chrono::Utc::now().to_rfc3339(),
        mvnd_available: mvnd.available,
        mvn_builds_ms: mvn_builds,
        mvnd_builds_ms: mvnd_builds,
    })
}

/// R-18 mvnd 对比报告（有 mvnd 时量化收益；无 mvnd 时仅基线）。
pub fn format_mvnd_report(r: &MvndBenchmarkResult) -> String {
    let avg = |xs: &[u128]| -> u128 {
        if xs.is_empty() { 0 } else { xs.iter().sum::<u128>() / xs.len() as u128 }
    };
    let mvn_avg = avg(&r.mvn_builds_ms);
    let mvnd_avg = avg(&r.mvnd_builds_ms);
    let mut s = String::new();
    s.push_str(&format!(
        "## mvnd Benchmark (R-18): {} repositories × {} modules × {} runs\n\n",
        r.repositories,
        r.modules_per_repository,
        r.mvn_builds_ms.len()
    ));
    s.push_str(&format!("- mvn avg: {mvn_avg} ms\n"));
    if r.mvnd_available {
        s.push_str(&format!("- mvnd avg: {mvnd_avg} ms\n"));
        let gain = (mvn_avg as f64 - mvnd_avg as f64) / mvn_avg.max(1) as f64 * 100.0;
        s.push_str(&format!("- mvnd 收益: {gain:+.1}%\n"));
    } else {
        s.push_str("- mvnd: 未安装（收益测量跳过）\n");
    }
    s
}

/// Render the R-09 Build benchmark report（趋势由人工读，不设 PASS/FAIL）。
pub fn format_build_report(r: &BuildBenchmarkResult) -> String {
    let mut s = String::new();
    s.push_str(&format!(
        "## Build Benchmark (R-09): {} repositories × {} modules\n\n",
        r.repositories, r.modules_per_repository
    ));
    s.push_str(&format!("- Generated at: {}\n", r.generated_at));
    s.push_str(&format!("- Strategy: {} / Reactor: {}\n", r.strategy, r.reactor_kind));
    s.push_str(&format!("- Modules in Runtime Closure: {}\n", r.modules_built));
    s.push_str(&format!("- First build: {} ms\n", r.first_build_ms));
    s.push_str(&format!("- Second build: {} ms\n", r.second_build_ms));
    let delta = r.second_build_ms as f64 - r.first_build_ms as f64;
    s.push_str(&format!(
        "- 二次构建趋势: {:+.1}%（Maven 原生缓存 + classpath 缓存生效时 <= 0）\n",
        delta / r.first_build_ms.max(1) as f64 * 100.0
    ));
    s
}

/// Serialize a build benchmark result to pretty JSON.
pub fn build_to_json(r: &BuildBenchmarkResult) -> String {
    serde_json::to_string_pretty(r).expect("build benchmark result serialization")
}

#[cfg(test)]
mod mvnd_tests {
    use super::*;

    /// R-18 验收：mvnd 模式构建功能正确且可量化（mvnd 未安装时打印 skip
    /// 原因，仅测 mvn 基线——环境相关基准按全局约束 §4 处理）。
    #[test]
    fn mvnd_benchmark_measures_engine_comparison() {
        let result = run_mvnd_build_benchmark(1, 2, 2).expect("benchmark must run (mvn available)");
        assert_eq!(result.mvn_builds_ms.len(), 2, "mvn baseline runs recorded");
        if !result.mvnd_available {
            eprintln!("R-18: mvnd not installed; skipping mvnd comparison (mvn baseline only)");
            eprintln!("{}", format_mvnd_report(&result));
            return;
        }
        assert_eq!(result.mvnd_builds_ms.len(), 2, "mvnd runs recorded");
        eprintln!("{}", format_mvnd_report(&result));
        // 只断言形态有效（收益大小取决于环境，不设硬阈值——报告供人工读）。
        assert!(result.mvn_builds_ms.iter().all(|&ms| ms > 0));
        assert!(result.mvnd_builds_ms.iter().all(|&ms| ms > 0));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// End-to-end smoke over a tiny matrix: every stage must run, the §99
    /// verdicts must pass, and the cross-repo closure must force a Synthetic
    /// reactor.
    #[test]
    fn runtime_benchmark_smoke() {
        let r = run_runtime(2, 3);
        assert_eq!(r.project_count, 2 * 4);
        assert_eq!(r.reactor_kind, "Synthetic");
        for verdict in budget_verdicts(&r) {
            assert!(
                verdict.pass,
                "{} too slow: {:.2} ms (budget < {} ms)",
                verdict.name, verdict.value_ms, verdict.budget_ms
            );
        }

        let report = format_runtime_report(&r);
        assert!(report.contains("Runtime Benchmark"));
        assert!(report.contains("PASS"));
        assert!(report.contains("Synthetic"));

        let json = runtime_to_json(&r);
        let parsed: RuntimeBenchmarkResult = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.project_count, r.project_count);
        assert_eq!(parsed.discovery_cold_ms, r.discovery_cold_ms);
    }

    #[test]
    fn comparison_renders_metric_delta() {
        // Single-repo workspace: the closure stays inside one reactor, so the
        // plan must reuse the Existing reactor (no Synthetic generation).
        let prev = run_runtime(1, 2);
        assert_eq!(prev.reactor_kind, "Existing");

        let mut curr = prev.clone();
        curr.discovery_cold_ms = prev.discovery_cold_ms + 50;
        let report = format_runtime_comparison(&prev, &curr);
        assert!(report.contains("Discovery (cold)"));
        assert!(report.contains("ms"));
    }

    /// The benchmark's synthetic workspace must yield a Synthetic Reactor that
    /// real Maven accepts (`mvn validate`), offline. Skipped when no `mvn` is
    /// on PATH.
    #[test]
    fn synthetic_reactor_passes_real_maven_validate_when_available() {
        let maven = if cfg!(windows) { "mvn.cmd" } else { "mvn" };
        if std::process::Command::new(maven)
            .arg("-version")
            .output()
            .is_err()
        {
            eprintln!("skipping real Maven validation because `{maven}` is unavailable");
            return;
        }

        let root = std::env::temp_dir().join(format!("gw_bench_mvn_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let spec = generate_maven_workspace(&root, 2, 2).unwrap();
        let discovery = discover_poms(&root, 5, None, None);
        let db_path = root.join("bench.db");
        let mut conn = rusqlite::Connection::open(&db_path).unwrap();
        crate::db::init_db(&mut conn).unwrap();
        let workspace_id = insert_workspace(&mut conn, &spec);
        sync_workspace_index(&mut conn, workspace_id, &discovery, &root.join("m2")).unwrap();
        let graph = query_dependency_graph(&conn, workspace_id).unwrap();
        let root_id = graph
            .projects
            .iter()
            .find(|project| project.coordinates.artifact_id == artifact_id(1, 1))
            .unwrap()
            .project_id;
        let closure = compute_runtime_closure(&graph, root_id, &RuntimeScope::Auto).unwrap();
        let plan = prepare_runtime_reactor(&graph, &closure, &root, "bench-app").unwrap();
        assert_eq!(format!("{:?}", plan.kind), "Synthetic");

        let output = std::process::Command::new(maven)
            .args(["-q", "-o", "-f"])
            .arg(&plan.pom_path)
            .arg("validate")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "mvn validate failed:\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
