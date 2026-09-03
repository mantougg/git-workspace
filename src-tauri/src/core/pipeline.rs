//! T-23 Workspace Pipeline: step orchestration on top of the T-24 Task DAG.
//!
//! A `Pipeline` is an ordered list of `PipelineStep`s (Sequential by
//! default — each step chains onto the previous executable step; Parallel
//! branches via explicit `depends_on`; Conditional via a per-node dispatch
//! condition evaluated in memory; Retry / Timeout / Cancel map to the DAG /
//! worker mechanisms). Steps apply to every selected repository, so the
//! compiled DAG has one node per (step, repo); dependencies are per-repo
//! chains, which keeps independent repos running in parallel.
//!
//! Built-in steps: Fetch / Check Status / Pull / Build / Test / Report.
//! Build and Test are shell commands (`TaskType::ShellCommand`); Report is a
//! virtual barrier step with no task of its own — the run report aggregates
//! everything once the upstream nodes finish.
//!
//! Templates are stored as JSON in the app data dir (like T-19's
//! `health-weights.json`), no DB table needed.

use serde::{Deserialize, Serialize};

use crate::error::AppResult;
use crate::models::task::{DagNodeRequest, DagSubmitRequest, FailurePolicy, NodeCondition, TaskRequest, TaskType};
use crate::task::dag::{DagState, NodeState};

// ---------------------------------------------------------------------------
// Model
// ---------------------------------------------------------------------------

/// A pipeline definition / template (T-23).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pipeline {
    /// Empty for a new (unsaved) pipeline; assigned on first save.
    #[serde(default)]
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub steps: Vec<PipelineStep>,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
}

/// One orchestration step. Sequential is the default: an empty `depends_on`
/// chains the step onto the previous executable step in the list.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PipelineStep {
    /// Stable key within the pipeline (referenced by `depends_on`).
    pub id: String,
    pub name: String,
    pub kind: StepKind,
    /// Explicit upstream step ids (parallel branches); empty = chain on the
    /// previous executable step.
    #[serde(default)]
    pub depends_on: Vec<String>,
    /// Conditional execution (T-23): the condition is evaluated in memory
    /// when the node becomes ready, based on the repo state left by the
    /// previous step; a false result skips the repo for this step.
    #[serde(default)]
    pub condition: Option<NodeCondition>,
    /// Scheduler-level retries per node (0 = run once).
    #[serde(default)]
    pub retries: u32,
    /// Shell steps (Build/Test): per-command timeout in seconds (the
    /// worker's 300s hard timeout still applies as the outer bound).
    #[serde(default)]
    pub timeout_secs: Option<u64>,
}

/// Built-in step kinds (extensible — new kinds compile to a `TaskType`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum StepKind {
    Fetch,
    CheckStatus,
    Pull,
    Build {
        command: String,
    },
    Test {
        command: String,
    },
    /// Virtual barrier step: no task of its own; the run report treats it as
    /// the aggregation point over all upstream nodes.
    Report,
}

impl StepKind {
    /// Short label used in reports / the DAG view.
    pub fn label(&self) -> &'static str {
        match self {
            StepKind::Fetch => "fetch",
            StepKind::CheckStatus => "checkStatus",
            StepKind::Pull => "pull",
            StepKind::Build { .. } => "build",
            StepKind::Test { .. } => "test",
            StepKind::Report => "report",
        }
    }
}

/// Repository a pipeline run applies to.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoSelection {
    pub repo_path: String,
    pub repo_name: String,
}

// ---------------------------------------------------------------------------
// Validation + compilation (Pipeline -> DAG)
// ---------------------------------------------------------------------------

/// Validate a pipeline definition (ids, references, commands, acyclicity).
pub fn validate_pipeline(pipeline: &Pipeline) -> Result<(), String> {
    if pipeline.name.trim().is_empty() {
        return Err("Pipeline 名称不能为空".to_string());
    }
    if pipeline.steps.is_empty() {
        return Err("Pipeline 至少需要一个步骤".to_string());
    }
    if !pipeline.steps.iter().any(|s| !matches!(s.kind, StepKind::Report)) {
        return Err("Pipeline 至少需要一个可执行步骤（Report 是虚拟汇聚步骤）".to_string());
    }

    let mut ids = std::collections::HashSet::new();
    for s in &pipeline.steps {
        if s.id.trim().is_empty() {
            return Err("步骤 id 不能为空".to_string());
        }
        if !ids.insert(s.id.as_str()) {
            return Err(format!("步骤 id 重复：{}", s.id));
        }
        match &s.kind {
            StepKind::Build { command } | StepKind::Test { command } => {
                if command.trim().is_empty() {
                    return Err(format!("步骤「{}」的命令不能为空", s.name));
                }
            }
            _ => {}
        }
        for dep in &s.depends_on {
            if dep == &s.id {
                return Err(format!("步骤「{}」不能依赖自身", s.name));
            }
            if !pipeline.steps.iter().any(|x| &x.id == dep) {
                return Err(format!("步骤「{}」依赖了不存在的步骤 id：{}", s.name, dep));
            }
            if pipeline
                .steps
                .iter()
                .any(|x| &x.id == dep && matches!(x.kind, StepKind::Report))
            {
                return Err(format!("步骤「{}」不能依赖 Report 步骤（虚拟汇聚步骤无任务）", s.name));
            }
        }
    }

    // Acyclicity over the resolved upstream lists (explicit depends_on or
    // the implicit chain edge) — Kahn's algorithm.
    let exec: Vec<&PipelineStep> = pipeline
        .steps
        .iter()
        .filter(|s| !matches!(s.kind, StepKind::Report))
        .collect();
    let pos = |id: &str| exec.iter().position(|s| s.id == id);
    let mut indeg = vec![0usize; exec.len()];
    let mut dependents = vec![Vec::new(); exec.len()];
    for (i, s) in exec.iter().enumerate() {
        for up in upstream_step_ids(pipeline, s) {
            let Some(u) = pos(&up) else {
                return Err(format!("步骤「{}」的上游解析失败：{}", s.name, up));
            };
            indeg[i] += 1;
            dependents[u].push(i);
        }
    }
    let mut queue: std::collections::VecDeque<usize> = (0..exec.len()).filter(|&i| indeg[i] == 0).collect();
    let mut visited = 0usize;
    while let Some(i) = queue.pop_front() {
        visited += 1;
        for &d in &dependents[i] {
            indeg[d] -= 1;
            if indeg[d] == 0 {
                queue.push_back(d);
            }
        }
    }
    if visited != exec.len() {
        return Err("步骤依赖存在环，无法编排".to_string());
    }
    Ok(())
}

/// Upstream step ids of a step: its explicit `depends_on`, or the previous
/// executable step in list order (implicit sequential chain). Report steps
/// are skipped over when chaining.
fn upstream_step_ids<'p>(pipeline: &'p Pipeline, step: &PipelineStep) -> Vec<String> {
    if !step.depends_on.is_empty() {
        return step.depends_on.clone();
    }
    let Some(pos) = pipeline.steps.iter().position(|s| s.id == step.id) else {
        return Vec::new();
    };
    pipeline.steps[..pos]
        .iter()
        .rev()
        .find(|s| !matches!(s.kind, StepKind::Report))
        .map(|s| s.id.clone())
        .into_iter()
        .collect()
}

/// The task a step compiles to for one repository.
fn task_for_step(step: &PipelineStep, repo: &RepoSelection) -> TaskRequest {
    let task_type = match &step.kind {
        StepKind::Fetch => TaskType::Fetch,
        StepKind::Pull => TaskType::Pull,
        // Check Status: branch + porcelain summary for the run report; the
        // structured clean/dirty verdict for Conditional gates is computed
        // via libgit2 at dispatch time (in memory, no intermediate state).
        StepKind::CheckStatus => TaskType::ShellCommand {
            command: "git status --porcelain=v1 --branch".to_string(),
            timeout_secs: Some(60),
        },
        StepKind::Build { command } | StepKind::Test { command } => TaskType::ShellCommand {
            command: command.clone(),
            timeout_secs: step.timeout_secs,
        },
        StepKind::Report => unreachable!("Report is a virtual step without a task"),
    };
    TaskRequest {
        task_type,
        repo_path: repo.repo_path.clone(),
        repo_name: repo.repo_name.clone(),
    }
}

/// Compile a pipeline over the selected repositories into a DAG submission
/// (T-23 on T-24): one node per (executable step, repo); dependencies are
/// per-repo chains so independent repos run in parallel, bounded by the
/// worker pool (§45).
pub fn compile_pipeline(
    pipeline: &Pipeline,
    repos: &[RepoSelection],
    on_failure: FailurePolicy,
) -> Result<DagSubmitRequest, String> {
    validate_pipeline(pipeline)?;
    if repos.is_empty() {
        return Err("请至少选择一个仓库".to_string());
    }

    let exec: Vec<&PipelineStep> = pipeline
        .steps
        .iter()
        .filter(|s| !matches!(s.kind, StepKind::Report))
        .collect();

    let mut nodes: Vec<DagNodeRequest> = Vec::with_capacity(exec.len() * repos.len());
    for s in &exec {
        let upstreams = upstream_step_ids(pipeline, s);
        for r in repos {
            // Per-repo dependency: this node's upstreams are the same repo's
            // nodes in each upstream step. Node index grid: step-major.
            let depends_on = upstreams
                .iter()
                .map(|u| {
                    let u_pos = exec
                        .iter()
                        .position(|x| &x.id == u)
                        .ok_or_else(|| format!("步骤「{}」的上游解析失败：{}", s.name, u))?;
                    Ok(u_pos * repos.len() + repos.iter().position(|x| x.repo_path == r.repo_path).unwrap())
                })
                .collect::<Result<Vec<usize>, String>>()?;
            nodes.push(DagNodeRequest {
                task: task_for_step(s, r),
                depends_on,
                max_attempts: 1 + s.retries,
                condition: s.condition,
                group: Some(s.id.clone()),
                label: Some(format!("{} · {}", s.name, r.repo_name)),
            });
        }
    }

    Ok(DagSubmitRequest {
        name: pipeline.name.clone(),
        nodes,
        on_failure,
    })
}

// ---------------------------------------------------------------------------
// Built-in sample template (T-23 示例流)
// ---------------------------------------------------------------------------

/// The built-in sample flow: Fetch All → Check Status → Pull Clean → Build
/// → Test → Report. Returned unsaved (empty id); saving persists it as a
/// template.
pub fn sample_pipeline() -> Pipeline {
    let step = |id: &str, name: &str, kind: StepKind| PipelineStep {
        id: id.to_string(),
        name: name.to_string(),
        kind,
        depends_on: Vec::new(),
        condition: None,
        retries: 0,
        timeout_secs: None,
    };
    let mut pull = step("pull-clean", "Pull Clean", StepKind::Pull);
    pull.condition = Some(NodeCondition::RepoClean);

    Pipeline {
        id: String::new(),
        name: "示例：Fetch → Check → Pull Clean → Build → Test → Report".to_string(),
        description: "内置示例流（T-23）：抓取全部仓库 → 检查状态 → 仅干净仓库 Pull → 构建 → 测试 → 汇总报告"
            .to_string(),
        steps: vec![
            step("fetch-all", "Fetch All", StepKind::Fetch),
            step("check-status", "Check Status", StepKind::CheckStatus),
            pull,
            step(
                "build",
                "Build",
                StepKind::Build {
                    command: "cargo build".to_string(),
                },
            ),
            step(
                "test",
                "Test",
                StepKind::Test {
                    command: "cargo test".to_string(),
                },
            ),
            step("report", "Report", StepKind::Report),
        ],
        created_at: String::new(),
        updated_at: String::new(),
    }
}

// ---------------------------------------------------------------------------
// Template storage (JSON file in the app data dir, T-19 pattern)
// ---------------------------------------------------------------------------

const TEMPLATES_FILE: &str = "pipeline-templates.json";

/// Load all saved templates; a missing/invalid file yields an empty list.
pub fn load_templates() -> Vec<Pipeline> {
    std::fs::read_to_string(crate::get_app_data_dir().join(TEMPLATES_FILE))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// Persist the full template list (atomic-ish: write to temp then rename).
pub fn save_templates(templates: &[Pipeline]) -> AppResult<()> {
    let dir = crate::get_app_data_dir();
    std::fs::create_dir_all(&dir)?;
    let tmp = dir.join(format!("{}.tmp", TEMPLATES_FILE));
    std::fs::write(&tmp, serde_json::to_string_pretty(templates)?)?;
    std::fs::rename(&tmp, dir.join(TEMPLATES_FILE))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Run report (T-23 执行报告)
// ---------------------------------------------------------------------------

/// Per-repository outcome of one step.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StepItemReport {
    pub task_id: String,
    pub repo_path: String,
    pub repo_name: String,
    /// queued / running / success / failed / cancelled / skipped
    pub status: String,
    /// Error message or skip note.
    pub message: Option<String>,
    /// Captured output tail (bounded, T-24).
    pub output: Option<String>,
    pub attempts: u32,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub duration_ms: Option<i64>,
}

/// Aggregate + per-repo detail of one step.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StepReport {
    pub step_id: String,
    pub name: String,
    pub kind: String,
    /// pending / running / success / partialSuccess / failed / cancelled / skipped
    pub status: String,
    pub total: usize,
    pub succeeded: usize,
    pub failed: usize,
    pub skipped: usize,
    pub cancelled: usize,
    pub items: Vec<StepItemReport>,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub duration_ms: Option<i64>,
}

/// The full execution report of one pipeline run (T-23).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PipelineRunReport {
    pub run_id: String,
    pub pipeline_name: String,
    /// running / success / partialSuccess / failed / cancelled
    pub status: String,
    pub total: usize,
    pub succeeded: usize,
    pub failed: usize,
    pub skipped: usize,
    pub cancelled: usize,
    pub steps: Vec<StepReport>,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub duration_ms: Option<i64>,
}

fn node_status_str(state: NodeState) -> &'static str {
    match state {
        NodeState::Pending => "queued",
        NodeState::Dispatched => "running",
        NodeState::Succeeded => "success",
        NodeState::Failed => "failed",
        NodeState::Cancelled => "cancelled",
        NodeState::Skipped => "skipped",
    }
}

fn parse_ts(s: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    chrono::DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|d| d.with_timezone(&chrono::Utc))
}

fn duration_ms(started: Option<&str>, finished: Option<&str>) -> Option<i64> {
    let start = parse_ts(started?)?;
    let end = finished.and_then(parse_ts).unwrap_or_else(chrono::Utc::now);
    Some((end - start).num_milliseconds().max(0))
}

/// Aggregate status from per-item outcome counts.
fn aggregate_status(
    total: usize,
    succeeded: usize,
    failed: usize,
    skipped: usize,
    cancelled: usize,
    finished_all: bool,
    any_started: bool,
) -> String {
    if !finished_all {
        return if any_started { "running" } else { "pending" }.to_string();
    }
    if failed == 0 && cancelled == 0 && skipped == 0 {
        "success".to_string()
    } else if succeeded == 0 && failed > 0 && skipped == 0 && cancelled == 0 {
        "failed".to_string()
    } else if cancelled == total {
        "cancelled".to_string()
    } else if skipped == total {
        "skipped".to_string()
    } else {
        "partialSuccess".to_string()
    }
}

/// Build the execution report of a run from the live DAG state (T-23; the
/// step structure comes from the pipeline definition, the live states from
/// the DAG nodes grouped by step id).
pub fn build_run_report(run_id: &str, pipeline: &Pipeline, dag: &DagState) -> PipelineRunReport {
    let mut steps = Vec::with_capacity(pipeline.steps.len());
    let mut run_started: Option<String> = None;
    let mut run_finished: Option<String> = None;

    let exec_nodes: Vec<_> = dag.nodes.iter().collect();

    for step in &pipeline.steps {
        let nodes: Vec<_> = exec_nodes
            .iter()
            .filter(|n| n.group.as_deref() == Some(step.id.as_str()))
            .collect();

        if matches!(step.kind, StepKind::Report) {
            // Virtual barrier step: mirrors the run-level aggregate.
            steps.push(StepReport {
                step_id: step.id.clone(),
                name: step.name.clone(),
                kind: step.kind.label().to_string(),
                status: String::new(), // filled after the run aggregate
                total: 0,
                succeeded: 0,
                failed: 0,
                skipped: 0,
                cancelled: 0,
                items: Vec::new(),
                started_at: None,
                finished_at: None,
                duration_ms: None,
            });
            continue;
        }

        let mut items = Vec::with_capacity(nodes.len());
        let (mut succ, mut fail, mut skip, mut cancel) = (0usize, 0usize, 0usize, 0usize);
        let mut started: Option<String> = None;
        let mut finished: Option<String> = None;
        let mut any_started = false;

        for n in &nodes {
            match n.state {
                NodeState::Succeeded => succ += 1,
                NodeState::Failed => fail += 1,
                NodeState::Skipped => skip += 1,
                NodeState::Cancelled => cancel += 1,
                _ => {}
            }
            if n.started_at.is_some() {
                any_started = true;
                started = min_ts(started, n.started_at.clone());
            }
            if n.state.is_final() {
                finished = max_ts(finished, n.finished_at.clone());
            }
            items.push(StepItemReport {
                task_id: n.task_id.clone(),
                repo_path: n.repo_path.clone(),
                repo_name: n.repo_name.clone(),
                status: node_status_str(n.state).to_string(),
                message: n.error.clone(),
                output: n.output.clone(),
                attempts: n.attempts,
                started_at: n.started_at.clone(),
                finished_at: n.finished_at.clone(),
                duration_ms: duration_ms(n.started_at.as_deref(), n.finished_at.as_deref()),
            });
        }

        let total = nodes.len();
        let finished_count = succ + fail + skip + cancel;
        steps.push(StepReport {
            step_id: step.id.clone(),
            name: step.name.clone(),
            kind: step.kind.label().to_string(),
            status: aggregate_status(total, succ, fail, skip, cancel, finished_count == total, any_started),
            total,
            succeeded: succ,
            failed: fail,
            skipped: skip,
            cancelled: cancel,
            items,
            started_at: started.clone(),
            finished_at: finished.clone(),
            duration_ms: duration_ms(started.as_deref(), finished.as_deref()),
        });

        run_started = min_ts(run_started, started);
        run_finished = max_ts(run_finished, finished);
    }

    // Run-level aggregate over all executable nodes.
    let (mut succ, mut fail, mut skip, mut cancel) = (0usize, 0usize, 0usize, 0usize);
    for n in &exec_nodes {
        match n.state {
            NodeState::Succeeded => succ += 1,
            NodeState::Failed => fail += 1,
            NodeState::Skipped => skip += 1,
            NodeState::Cancelled => cancel += 1,
            _ => {}
        }
    }
    let total = exec_nodes.len();
    let finished_all = succ + fail + skip + cancel == total;
    let run_status = aggregate_status(total, succ, fail, skip, cancel, finished_all, run_started.is_some());

    // Fill the virtual Report step with the run aggregate.
    for s in steps.iter_mut() {
        if matches!(
            pipeline.steps.iter().find(|p| p.id == s.step_id).map(|p| &p.kind),
            Some(StepKind::Report)
        ) {
            s.status = run_status.clone();
            s.total = total;
            s.succeeded = succ;
            s.failed = fail;
            s.skipped = skip;
            s.cancelled = cancel;
            s.started_at = run_started.clone();
            s.finished_at = if finished_all { run_finished.clone() } else { None };
            s.duration_ms = duration_ms(run_started.as_deref(), s.finished_at.as_deref());
        }
    }

    PipelineRunReport {
        run_id: run_id.to_string(),
        pipeline_name: pipeline.name.clone(),
        status: run_status,
        total,
        succeeded: succ,
        failed: fail,
        skipped: skip,
        cancelled: cancel,
        steps,
        started_at: run_started.clone(),
        finished_at: if finished_all { run_finished.clone() } else { None },
        duration_ms: duration_ms(
            run_started.as_deref(),
            if finished_all { run_finished.as_deref() } else { None },
        ),
    }
}

fn min_ts(a: Option<String>, b: Option<String>) -> Option<String> {
    match (a, b) {
        (Some(x), Some(y)) => Some(if x <= y { x } else { y }),
        (Some(x), None) => Some(x),
        (None, y) => y,
    }
}

fn max_ts(a: Option<String>, b: Option<String>) -> Option<String> {
    match (a, b) {
        (Some(x), Some(y)) => Some(if x >= y { x } else { y }),
        (Some(x), None) => Some(x),
        (None, y) => y,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repos() -> Vec<RepoSelection> {
        vec![
            RepoSelection {
                repo_path: "/ws/a".into(),
                repo_name: "a".into(),
            },
            RepoSelection {
                repo_path: "/ws/b".into(),
                repo_name: "b".into(),
            },
        ]
    }

    /// T-23 acceptance: the sample pipeline compiles into a per-repo DAG
    /// (one node per step×repo, per-repo chains).
    #[test]
    fn sample_pipeline_compiles_to_per_repo_dag() {
        let p = sample_pipeline();
        validate_pipeline(&p).unwrap();
        let dag = compile_pipeline(&p, &repos(), FailurePolicy::Continue).unwrap();

        // 5 executable steps × 2 repos (Report is virtual).
        assert_eq!(dag.nodes.len(), 10);
        // fetch(0..2) -> check(2..4) -> pull(4..6) -> build(6..8) -> test(8..10)
        let fetch_a = &dag.nodes[0];
        assert!(fetch_a.depends_on.is_empty());
        let check_a = &dag.nodes[2];
        assert_eq!(check_a.depends_on, vec![0]);
        let pull_a = &dag.nodes[4];
        assert_eq!(pull_a.depends_on, vec![2]);
        assert_eq!(pull_a.condition, Some(NodeCondition::RepoClean));
        let build_b = &dag.nodes[7];
        assert_eq!(build_b.depends_on, vec![5], "repo b chains through its own pull node");
        let test_b = &dag.nodes[9];
        assert_eq!(test_b.depends_on, vec![7]);
    }

    /// Validation: duplicate ids, missing refs, empty commands, cycles.
    #[test]
    fn validation_rejects_bad_pipelines() {
        let mut p = sample_pipeline();

        let mut dup = p.clone();
        dup.steps[1].id = "fetch-all".into();
        assert!(validate_pipeline(&dup).is_err(), "duplicate step id");

        let mut missing = p.clone();
        missing.steps[1].depends_on = vec!["nope".into()];
        assert!(validate_pipeline(&missing).is_err(), "missing dep ref");

        let mut empty_cmd = p.clone();
        empty_cmd.steps[3].kind = StepKind::Build { command: "  ".into() };
        assert!(validate_pipeline(&empty_cmd).is_err(), "empty build command");

        let mut cycle = p.clone();
        cycle.steps[0].depends_on = vec!["build".into()];
        assert!(validate_pipeline(&cycle).is_err(), "dependency cycle");

        let mut dep_on_report = p.clone();
        dep_on_report.steps[0].depends_on = vec!["report".into()];
        assert!(validate_pipeline(&dep_on_report).is_err(), "dep on Report step");

        p.steps.truncate(0);
        assert!(validate_pipeline(&p).is_err(), "no steps");
    }

    /// Parallel branches: two steps depending on the same upstream run as
    /// sibling branches (fan-out), and a join step waits on both (fan-in).
    #[test]
    fn explicit_depends_on_builds_parallel_branches() {
        let mut p = sample_pipeline();
        // build depends only on check-status (parallel with pull-clean),
        // test depends on both pull-clean and build (join).
        p.steps[3].depends_on = vec!["check-status".into()];
        p.steps[4].depends_on = vec!["pull-clean".into(), "build".into()];
        validate_pipeline(&p).unwrap();
        let dag = compile_pipeline(&p, &repos(), FailurePolicy::Continue).unwrap();

        let build_a = &dag.nodes[6];
        assert_eq!(build_a.depends_on, vec![2], "build fans out from check-status");
        let test_a = &dag.nodes[8];
        assert_eq!(test_a.depends_on, vec![4, 6], "test joins pull + build");
    }

    /// Retry / timeout config maps onto the compiled nodes.
    #[test]
    fn retry_and_timeout_map_to_nodes() {
        let mut p = sample_pipeline();
        p.steps[3].retries = 2;
        p.steps[3].timeout_secs = Some(120);
        let dag = compile_pipeline(&p, &repos(), FailurePolicy::FailFast).unwrap();
        let build = &dag.nodes[6];
        assert_eq!(build.max_attempts, 3);
        match &build.task.task_type {
            TaskType::ShellCommand { command, timeout_secs } => {
                assert_eq!(command, "cargo build");
                assert_eq!(*timeout_secs, Some(120));
            }
            other => panic!("expected ShellCommand, got {:?}", other),
        }
        assert!(matches!(dag.on_failure, FailurePolicy::FailFast));
    }

    /// Template storage round-trip (temp dir via app-data override is not
    /// available; exercise serde round-trip instead).
    #[test]
    fn template_serde_round_trip() {
        let p = sample_pipeline();
        let json = serde_json::to_string(&vec![p.clone()]).unwrap();
        let back: Vec<Pipeline> = serde_json::from_str(&json).unwrap();
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].steps.len(), p.steps.len());
        assert_eq!(back[0].steps[2].condition, Some(NodeCondition::RepoClean));
        validate_pipeline(&back[0]).unwrap();
    }

    /// The run report aggregates per-step and run-level outcomes (T-23).
    #[test]
    fn run_report_aggregates_steps() {
        use crate::models::task::FailurePolicy as FP;
        use crate::task::dag::{DagNodeState, DagState};

        let node = |id: &str, group: &str, state: NodeState| DagNodeState {
            task_id: id.into(),
            label: id.into(),
            group: Some(group.into()),
            repo_path: "/ws/a".into(),
            repo_name: "a".into(),
            depends_on: vec![],
            state,
            skipped: state == NodeState::Skipped,
            attempts: 1,
            max_attempts: 1,
            condition: None,
            output: None,
            error: None,
            started_at: Some("2026-08-17T00:00:00Z".into()),
            finished_at: Some("2026-08-17T00:00:02Z".into()),
        };
        let dag = DagState::build(
            "run-1".into(),
            "p".into(),
            FP::Continue,
            vec![
                node("n1", "fetch-all", NodeState::Succeeded),
                node("n2", "fetch-all", NodeState::Failed),
                node("n3", "check-status", NodeState::Skipped),
            ],
            &[],
        )
        .unwrap();

        let mut p = sample_pipeline();
        p.steps.truncate(2); // fetch-all + check-status
        let report = build_run_report("run-1", &p, &dag);

        assert_eq!(report.steps.len(), 2);
        let fetch = &report.steps[0];
        assert_eq!(fetch.total, 2);
        assert_eq!(fetch.succeeded, 1);
        assert_eq!(fetch.failed, 1);
        assert_eq!(fetch.status, "partialSuccess");
        assert_eq!(fetch.duration_ms, Some(2000));
        let check = &report.steps[1];
        assert_eq!(check.skipped, 1);
        assert_eq!(check.status, "skipped");
        assert_eq!(report.status, "partialSuccess");
        assert_eq!(report.total, 3);
    }
}
