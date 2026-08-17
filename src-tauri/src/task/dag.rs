//! T-24 Task DAG: dependency graph scheduler on top of the T-05 worker pool.
//!
//! A DAG is submitted as one unit (`DagSubmitRequest`); its nodes share a
//! `batch_id` (the DAG id), so the T-20 batch aggregate keeps working as the
//! run-level rollup and `task_items` keeps per-repo sub-results. Blocked
//! nodes live in `active_tasks` as `Queued` but are NOT sent to the worker
//! channel until every dependency succeeded (topology-driven, no polling).
//!
//! Failure propagation is configurable per DAG (`FailurePolicy`):
//! - `Continue`: only the failed node's transitive dependents are skipped;
//! - `FailFast`: every unfinished node is cancelled (running ones via their
//!   cooperative cancel flag, pending ones marked cancelled immediately).
//!
//! Cancellation propagates along dependency edges: cancelling an upstream
//! node marks its not-yet-started downstream subtree cancelled; running
//! downstream nodes finish (their own cancellation stays user-driven).
//!
//! The pure state machine (`DagState`) is unit-tested in this module; side
//! effects (channel sends, DB writes, events) live in the `pub(crate)`
//! helpers used by `manager` / `worker`.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use dashmap::DashMap;
use rusqlite::{params, Connection};
use tauri::AppHandle;

use crate::db::dao;
use crate::error::AppResult;
use crate::models::task::{
    BatchState, DagEdge, DagGraph, DagNodeInfo, FailurePolicy, NodeCondition, Task, TaskStatus,
};
use crate::task::queue::TaskMessage;
use crate::task::worker;

/// Cap on retained DAG states (finished ones are evicted first, oldest by
/// creation time), so long sessions don't grow memory unboundedly.
const MAX_RETAINED_DAGS: usize = 64;

/// Per-node captured output cap for reports / DAG view (8 KB tail).
const NODE_OUTPUT_CAP: usize = 8 * 1024;

/// Internal lifecycle of a DAG node (richer than `TaskStatus`: distinguishes
/// blocked-pending from dispatched and skipped from cancelled).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeState {
    /// Blocked on dependencies (or not yet dispatched).
    Pending,
    /// Handed to the worker channel (queued or running there).
    Dispatched,
    Succeeded,
    Failed,
    Cancelled,
    /// Never executed: a dependency failed (skip) or a dispatch condition
    /// evaluated false. Dependents of a condition-skipped node are released.
    Skipped,
}

impl NodeState {
    pub(crate) fn is_final(self) -> bool {
        matches!(
            self,
            NodeState::Succeeded | NodeState::Failed | NodeState::Cancelled | NodeState::Skipped
        )
    }
}

/// One DAG node: static wiring plus live state for the view/report.
#[derive(Debug, Clone)]
pub struct DagNodeState {
    pub task_id: String,
    pub label: String,
    /// Grouping label (T-23 pipeline step id).
    pub group: Option<String>,
    pub repo_path: String,
    pub repo_name: String,
    /// Task ids this node depends on.
    pub depends_on: Vec<String>,
    pub state: NodeState,
    pub skipped: bool,
    /// Scheduler-level attempts so far (incremented on each dispatch).
    pub attempts: u32,
    pub max_attempts: u32,
    pub condition: Option<NodeCondition>,
    pub output: Option<String>,
    pub error: Option<String>,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
}

/// How a node's execution ended (worker-reported).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FinishKind {
    Success,
    Failed,
    Cancelled,
}

/// Side-effect plan produced by `DagState` transitions; the caller executes
/// it against the channel / task maps / DB.
#[derive(Debug, Default)]
pub struct DagOutcome {
    /// Nodes whose dependencies are all satisfied — dispatch them.
    pub ready: Vec<usize>,
    /// Nodes newly skipped because a dependency failed (`Continue` policy).
    pub skipped: Vec<usize>,
    /// Pending nodes cancelled by propagation (fail-fast / upstream cancel).
    pub cancelled: Vec<usize>,
    /// Dispatched nodes to cancel cooperatively via their flag (fail-fast).
    pub to_cancel: Vec<usize>,
    /// The finished node will be retried by the scheduler; the caller must
    /// skip batch accounting and cleanup for this finish.
    pub retried: bool,
}

/// In-memory state of one submitted DAG.
#[derive(Debug)]
pub struct DagState {
    pub id: String,
    pub name: String,
    pub on_failure: FailurePolicy,
    pub nodes: Vec<DagNodeState>,
    /// task id -> node index.
    index: HashMap<String, usize>,
    /// Unmet dependency count per node.
    remaining: Vec<usize>,
    /// dep index -> dependent indices.
    dependents: Vec<Vec<usize>>,
    pub fail_fast_triggered: bool,
    pub created_at: String,
}

impl DagState {
    /// Build a DAG from resolved nodes and `(dep_idx, node_idx)` edges.
    /// Errors on out-of-range indices or a dependency cycle.
    pub fn build(
        id: String,
        name: String,
        on_failure: FailurePolicy,
        nodes: Vec<DagNodeState>,
        edges: &[(usize, usize)],
    ) -> Result<Self, String> {
        validate_edges(nodes.len(), edges)?;

        let mut remaining = vec![0usize; nodes.len()];
        let mut dependents = vec![Vec::new(); nodes.len()];
        for &(dep, node) in edges {
            remaining[node] += 1;
            dependents[dep].push(node);
        }
        let index = nodes
            .iter()
            .enumerate()
            .map(|(i, n)| (n.task_id.clone(), i))
            .collect();

        Ok(DagState {
            id,
            name,
            on_failure,
            nodes,
            index,
            remaining,
            dependents,
            fail_fast_triggered: false,
            created_at: chrono::Utc::now().to_rfc3339(),
        })
    }

    pub fn index_of(&self, task_id: &str) -> Option<usize> {
        self.index.get(task_id).copied()
    }

    /// Indices of nodes with no unmet dependencies (initial dispatch set).
    pub fn initial_ready(&self) -> Vec<usize> {
        self.remaining
            .iter()
            .enumerate()
            .filter_map(|(i, &r)| (r == 0 && self.nodes[i].state == NodeState::Pending).then_some(i))
            .collect()
    }

    /// Whether every node reached a final state.
    pub fn is_finished(&self) -> bool {
        self.nodes.iter().all(|n| n.state.is_final())
    }

    /// Release the dependents of a succeeded/condition-skipped node:
    /// decrement their unmet counts and return the newly-ready ones.
    fn release_dependents(&mut self, idx: usize) -> Vec<usize> {
        let mut ready = Vec::new();
        for d in self.dependents[idx].clone() {
            self.remaining[d] = self.remaining[d].saturating_sub(1);
            if self.remaining[d] == 0 && self.nodes[d].state == NodeState::Pending {
                ready.push(d);
            }
        }
        ready
    }

    /// Skip the whole not-yet-started subtree of a failed node (`Continue`
    /// policy): every transitive dependent is marked skipped, recursively.
    fn skip_subtree(&mut self, idx: usize) -> Vec<usize> {
        let mut out = Vec::new();
        let mut queue: VecDeque<usize> = self.dependents[idx].iter().copied().collect();
        let mut seen: HashSet<usize> = queue.iter().copied().collect();
        while let Some(d) = queue.pop_front() {
            if self.nodes[d].state == NodeState::Pending {
                self.nodes[d].state = NodeState::Skipped;
                self.nodes[d].skipped = true;
                self.nodes[d].error = Some("依赖失败，已跳过".to_string());
                self.nodes[d].finished_at = Some(chrono::Utc::now().to_rfc3339());
                out.push(d);
            }
            for &next in &self.dependents[d] {
                if seen.insert(next) {
                    queue.push_back(next);
                }
            }
        }
        out
    }

    /// Cancel the whole not-yet-started subtree of a cancelled node
    /// (cancellation propagates along dependency edges).
    fn cancel_subtree(&mut self, idx: usize) -> Vec<usize> {
        let mut out = Vec::new();
        let mut queue: VecDeque<usize> = self.dependents[idx].iter().copied().collect();
        let mut seen: HashSet<usize> = queue.iter().copied().collect();
        while let Some(d) = queue.pop_front() {
            if self.nodes[d].state == NodeState::Pending {
                self.nodes[d].state = NodeState::Cancelled;
                self.nodes[d].error = Some("上游已取消".to_string());
                self.nodes[d].finished_at = Some(chrono::Utc::now().to_rfc3339());
                out.push(d);
            }
            for &next in &self.dependents[d] {
                if seen.insert(next) {
                    queue.push_back(next);
                }
            }
        }
        out
    }

    /// Record a node's execution result and compute the follow-up plan.
    /// No-op (default outcome) when the node is already in a final state —
    /// this keeps user-cancel and worker-finish races idempotent.
    pub fn on_finish(&mut self, idx: usize, kind: FinishKind) -> DagOutcome {
        if self.nodes[idx].state.is_final() {
            return DagOutcome::default();
        }
        let now = chrono::Utc::now().to_rfc3339();

        match kind {
            FinishKind::Success => {
                self.nodes[idx].state = NodeState::Succeeded;
                self.nodes[idx].finished_at = Some(now);
                DagOutcome {
                    ready: self.release_dependents(idx),
                    ..Default::default()
                }
            }
            FinishKind::Failed => {
                // Scheduler-level retry (per-attempt the worker's own network
                // retry still applies). Not counted as failed yet.
                if self.nodes[idx].attempts < self.nodes[idx].max_attempts {
                    return DagOutcome {
                        retried: true,
                        ..Default::default()
                    };
                }
                self.nodes[idx].state = NodeState::Failed;
                self.nodes[idx].finished_at = Some(now);
                match self.on_failure {
                    FailurePolicy::Continue => DagOutcome {
                        skipped: self.skip_subtree(idx),
                        ..Default::default()
                    },
                    FailurePolicy::FailFast => {
                        self.fail_fast_triggered = true;
                        let mut cancelled = Vec::new();
                        let mut to_cancel = Vec::new();
                        for (i, n) in self.nodes.iter_mut().enumerate() {
                            match n.state {
                                NodeState::Pending => {
                                    n.state = NodeState::Cancelled;
                                    n.error = Some("fail-fast 触发，已取消".to_string());
                                    n.finished_at = Some(chrono::Utc::now().to_rfc3339());
                                    cancelled.push(i);
                                }
                                NodeState::Dispatched if i != idx => to_cancel.push(i),
                                _ => {}
                            }
                        }
                        DagOutcome {
                            cancelled,
                            to_cancel,
                            ..Default::default()
                        }
                    }
                }
            }
            FinishKind::Cancelled => {
                self.nodes[idx].state = NodeState::Cancelled;
                self.nodes[idx].finished_at = Some(now);
                DagOutcome {
                    cancelled: self.cancel_subtree(idx),
                    ..Default::default()
                }
            }
        }
    }

    /// Skip a ready node because its dispatch condition evaluated false
    /// (T-23 Conditional): the node is marked skipped but its dependents are
    /// released (a condition skip is not a failure).
    pub fn skip_for_condition(&mut self, idx: usize) -> Vec<usize> {
        if self.nodes[idx].state != NodeState::Pending {
            return Vec::new();
        }
        self.nodes[idx].state = NodeState::Skipped;
        self.nodes[idx].skipped = true;
        self.nodes[idx].error = Some("条件不满足，已跳过".to_string());
        self.nodes[idx].finished_at = Some(chrono::Utc::now().to_rfc3339());
        self.release_dependents(idx)
    }
}

/// Validate edge indices and acyclicity (Kahn's algorithm) before any task
/// row is persisted, so a malformed DAG is rejected up front.
pub fn validate_edges(node_count: usize, edges: &[(usize, usize)]) -> Result<(), String> {
    for &(dep, node) in edges {
        if dep >= node_count || node >= node_count {
            return Err(format!(
                "依赖序号越界：{} -> {}（共 {} 个节点）",
                dep, node, node_count
            ));
        }
        if dep == node {
            return Err(format!("节点 {} 不能依赖自身", node));
        }
    }

    let mut indeg = vec![0usize; node_count];
    let mut dependents = vec![Vec::new(); node_count];
    for &(dep, node) in edges {
        indeg[node] += 1;
        dependents[dep].push(node);
    }
    let mut queue: VecDeque<usize> = (0..node_count).filter(|&i| indeg[i] == 0).collect();
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
    if visited != node_count {
        return Err("任务依赖存在环，无法拓扑排序".to_string());
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Side-effect helpers (channel dispatch, cancellation application, events).
// Called by the manager (submit/cancel) and the worker finish hook.
// ---------------------------------------------------------------------------

/// Shared context for DAG side effects.
pub struct DagContext<'a> {
    pub dags: &'a Arc<DashMap<String, DagState>>,
    pub sender: &'a tokio::sync::mpsc::Sender<TaskMessage>,
    pub active_tasks: &'a Arc<DashMap<String, Task>>,
    pub cancel_flags: &'a Arc<DashMap<String, Arc<AtomicBool>>>,
    pub db: &'a Arc<Mutex<Connection>>,
    pub app: &'a AppHandle,
    pub batches: &'a Arc<DashMap<String, BatchState>>,
}

/// Evaluate a dispatch condition in memory (T-23 Conditional steps; no
/// intermediate state is persisted).
fn evaluate_condition(condition: &NodeCondition, repo_path: &str) -> bool {
    match condition {
        NodeCondition::RepoClean => crate::core::git_status::get_repo_status(
            std::path::Path::new(repo_path),
        )
        .map(|s| s.is_clean)
        .unwrap_or(false),
    }
}

/// Dispatch newly-ready nodes, cascading through condition skips. Marks the
/// node's task `Queued` -> sends it to the worker channel; a node whose
/// condition is false is marked skipped (accounted into the batch) and its
/// own dependents are released in turn.
pub fn dispatch_ready(dag: &mut DagState, ready: Vec<usize>, ctx: &DagContext<'_>) {
    let mut worklist: VecDeque<usize> = ready.into();
    while let Some(idx) = worklist.pop_front() {
        if dag.nodes[idx].state != NodeState::Pending {
            continue; // cancelled while waiting, or already handled
        }
        let node = dag.nodes[idx].clone();

        // Condition gate (T-23): skip without failing, release dependents.
        if let Some(cond) = node.condition {
            if !evaluate_condition(&cond, &node.repo_path) {
                let released = dag.skip_for_condition(idx);
                finalize_node_as(dag, idx, TaskStatus::Cancelled, true, ctx);
                worklist.extend(released);
                continue;
            }
        }

        // The user may have cancelled the task directly while it was blocked.
        let cancelled_task = ctx
            .active_tasks
            .get(&node.task_id)
            .map(|t| matches!(t.status, TaskStatus::Cancelled))
            .unwrap_or(true);
        if cancelled_task {
            let outcome = dag.on_finish(idx, FinishKind::Cancelled);
            finalize_node_as(dag, idx, TaskStatus::Cancelled, false, ctx);
            for c in outcome.cancelled {
                finalize_node_as(dag, c, TaskStatus::Cancelled, false, ctx);
            }
            continue;
        }

        // Dispatch: bump attempts, mark the task queued and hand it over.
        dag.nodes[idx].attempts += 1;
        dag.nodes[idx].state = NodeState::Dispatched;
        if dag.nodes[idx].started_at.is_none() {
            dag.nodes[idx].started_at = Some(chrono::Utc::now().to_rfc3339());
        }
        let Some(mut entry) = ctx.active_tasks.get_mut(&node.task_id) else {
            continue;
        };
        entry.status = TaskStatus::Queued;
        let task = entry.clone();
        drop(entry);
        worker::emit_progress(ctx.app, &task);
        if ctx.sender.try_send(TaskMessage { task }).is_err() {
            // Channel closed (shutdown): fail the node definitively so the
            // DAG cannot hang.
            dag.nodes[idx].attempts = dag.nodes[idx].max_attempts;
            let outcome = dag.on_finish(idx, FinishKind::Failed);
            finalize_node_as(
                dag,
                idx,
                TaskStatus::Failed {
                    error: "任务队列已关闭".to_string(),
                },
                false,
                ctx,
            );
            apply_outcome(dag, outcome, ctx);
        }
    }
}

/// Apply the propagation plan of a finished node (skip/cancel/fail-fast)
/// and dispatch anything released. Shared by the worker finish hook and the
/// manager's cancel path.
pub fn apply_outcome(dag: &mut DagState, outcome: DagOutcome, ctx: &DagContext<'_>) {
    for idx in outcome.skipped {
        finalize_node_as(dag, idx, TaskStatus::Cancelled, true, ctx);
    }
    for idx in outcome.cancelled {
        finalize_node_as(dag, idx, TaskStatus::Cancelled, false, ctx);
    }
    for idx in outcome.to_cancel {
        let task_id = dag.nodes[idx].task_id.clone();
        if let Some(flag) = ctx.cancel_flags.get(&task_id) {
            flag.store(true, Ordering::Relaxed);
        }
    }
    dispatch_ready(dag, outcome.ready, ctx);
}

/// Mark a node finished without worker involvement (skipped / cancelled
/// while pending): update the task row, persist, emit, and account it into
/// the batch aggregate exactly once.
fn finalize_node_as(
    dag: &mut DagState,
    idx: usize,
    status: TaskStatus,
    skipped: bool,
    ctx: &DagContext<'_>,
) {
    let node = &mut dag.nodes[idx];
    if skipped {
        node.skipped = true;
    }
    if matches!(status, TaskStatus::Failed { .. }) {
        node.state = NodeState::Failed;
    }
    let task_id = node.task_id.clone();

    let Some(mut entry) = ctx.active_tasks.get_mut(&task_id) else {
        return;
    };
    entry.status = status.clone();
    let task = entry.clone();
    drop(entry);

    persist_task_status(ctx.db, &task_id, status.key());
    worker::emit_progress(ctx.app, &task);
    worker::update_batch(ctx.batches, ctx.db, ctx.app, &task);
}

/// Persist a status transition for a DAG node (history + crash recovery).
fn persist_task_status(db: &Arc<Mutex<Connection>>, task_id: &str, status: &str) {
    let Ok(conn) = db.lock() else {
        return;
    };
    let now = chrono::Utc::now().to_rfc3339();
    if let Err(e) = dao::update_task_status(&conn, task_id, status, Some(&now)) {
        log::warn!("Failed to persist DAG node {} status: {}", task_id, e);
    }
}

/// Cancel a blocked (never dispatched) node at the user's request: mark it
/// cancelled, propagate along dependency edges, and account everything into
/// the batch aggregate. Returns false when the node is not pending (already
/// dispatched nodes are cancelled cooperatively through the worker path).
pub fn cancel_pending_node(dag: &mut DagState, idx: usize, ctx: &DagContext<'_>) -> bool {
    if dag.nodes[idx].state != NodeState::Pending {
        return false;
    }
    let outcome = dag.on_finish(idx, FinishKind::Cancelled);
    finalize_node_as(dag, idx, TaskStatus::Cancelled, false, ctx);
    apply_outcome(dag, outcome, ctx);
    true
}

/// Worker finish hook: evolve the DAG, propagate failure/cancellation,
/// release dependents. Returns true when the node is being retried (the
/// caller then skips batch accounting and the 30s cleanup for this finish).
pub fn on_task_finished(ctx: &DagContext<'_>, task: &Task, output: Option<String>) -> bool {
    let Some(dag_id) = task.batch_id.clone() else {
        return false;
    };
    let Some(mut entry) = ctx.dags.get_mut(&dag_id) else {
        return false;
    };
    let dag = entry.value_mut();
    let Some(idx) = dag.index_of(&task.id) else {
        return false;
    };

    let kind = match &task.status {
        TaskStatus::Success => FinishKind::Success,
        TaskStatus::Failed { .. } => FinishKind::Failed,
        TaskStatus::Cancelled => FinishKind::Cancelled,
        _ => return false, // non-final; nothing to do
    };

    // Capture output / error tails for the DAG view and run report (bounded).
    match &task.status {
        TaskStatus::Failed { error } => {
            dag.nodes[idx].error = Some(tail(error, NODE_OUTPUT_CAP));
        }
        TaskStatus::Success => {
            if let Some(out) = output {
                if !out.trim().is_empty() {
                    dag.nodes[idx].output = Some(tail(&out, NODE_OUTPUT_CAP));
                }
            }
        }
        _ => {}
    }

    let outcome = dag.on_finish(idx, kind);
    if outcome.retried {
        // Scheduler-level retry: re-queue the node without accounting it.
        log::warn!(
            "DAG node {} failed (attempt {}/{}), re-queueing",
            task.id,
            dag.nodes[idx].attempts,
            dag.nodes[idx].max_attempts
        );
        dag.nodes[idx].state = NodeState::Pending;
        dispatch_ready(dag, vec![idx], ctx);
        return true;
    }

    apply_outcome(dag, outcome, ctx);
    false
}

/// Keep at most `cap` bytes from the end of a string (UTF-8 safe-ish).
fn tail(s: &str, cap: usize) -> String {
    if s.len() <= cap {
        return s.to_string();
    }
    let mut start = s.len() - cap;
    while !s.is_char_boundary(start) {
        start += 1;
    }
    s[start..].to_string()
}

/// Evict finished DAGs (oldest first) beyond the retention cap.
pub fn evict_finished(dags: &DashMap<String, DagState>) {
    if dags.len() <= MAX_RETAINED_DAGS {
        return;
    }
    let mut finished: Vec<(String, String)> = dags
        .iter()
        .filter(|e| e.value().is_finished())
        .map(|e| (e.key().clone(), e.value().created_at.clone()))
        .collect();
    finished.sort_by(|a, b| a.1.cmp(&b.1));
    let to_remove = dags.len() - MAX_RETAINED_DAGS;
    for (id, _) in finished.into_iter().take(to_remove) {
        dags.remove(&id);
    }
}

/// Build the visualization payload (nodes + edges + live states, T-24).
pub fn build_graph(dag: &DagState) -> DagGraph {
    let mut edges = Vec::new();
    let nodes = dag
        .nodes
        .iter()
        .map(|n| {
            for dep in &n.depends_on {
                edges.push(DagEdge {
                    from: dep.clone(),
                    to: n.task_id.clone(),
                });
            }
            let status = match n.state {
                NodeState::Pending => TaskStatus::Queued,
                NodeState::Dispatched => TaskStatus::Running { progress: 0.0 },
                NodeState::Succeeded => TaskStatus::Success,
                NodeState::Failed => TaskStatus::Failed {
                    error: n.error.clone().unwrap_or_else(|| "失败".to_string()),
                },
                NodeState::Cancelled | NodeState::Skipped => TaskStatus::Cancelled,
            };
            DagNodeInfo {
                task_id: n.task_id.clone(),
                label: n.label.clone(),
                group: n.group.clone(),
                repo_path: n.repo_path.clone(),
                repo_name: n.repo_name.clone(),
                depends_on: n.depends_on.clone(),
                status,
                skipped: n.skipped,
                attempts: n.attempts,
                output: n.output.clone(),
                started_at: n.started_at.clone(),
                finished_at: n.finished_at.clone(),
            }
        })
        .collect();
    DagGraph {
        dag_id: dag.id.clone(),
        name: dag.name.clone(),
        on_failure: dag.on_failure,
        nodes,
        edges,
    }
}

// ---------------------------------------------------------------------------
// task_dependencies persistence (T-24 §41 schema). Kept in this module per
// the parallel-work rules; writes go through the single-writer connection in
// one transaction.
// ---------------------------------------------------------------------------

/// Insert DAG edges (tasks row ids) into `task_dependencies` in one
/// transaction. Best-effort: callers log and continue on failure.
pub(crate) fn insert_task_dependencies(conn: &Connection, edges: &[(i64, i64)]) -> AppResult<()> {
    let tx = conn.unchecked_transaction()?;
    {
        let mut stmt = tx.prepare(
            "INSERT OR IGNORE INTO task_dependencies (task_id, depends_on_id) VALUES (?1, ?2)",
        )?;
        for (task_row, dep_row) in edges {
            stmt.execute(params![task_row, dep_row])?;
        }
    }
    tx.commit()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: &str, deps: &[&str], max_attempts: u32) -> DagNodeState {
        DagNodeState {
            task_id: id.to_string(),
            label: id.to_string(),
            group: None,
            repo_path: format!("/ws/{id}"),
            repo_name: id.to_string(),
            depends_on: deps.iter().map(|d| d.to_string()).collect(),
            state: NodeState::Pending,
            skipped: false,
            attempts: 0,
            max_attempts,
            condition: None,
            output: None,
            error: None,
            started_at: None,
            finished_at: None,
        }
    }

    fn dag(policy: FailurePolicy, nodes: Vec<DagNodeState>, edges: &[(usize, usize)]) -> DagState {
        DagState::build("dag-1".into(), "test".into(), policy, nodes, edges).unwrap()
    }

    /// T-24 acceptance: dependent nodes are released in topological order.
    #[test]
    fn chain_releases_in_topological_order() {
        let mut d = dag(
            FailurePolicy::Continue,
            vec![node("a", &[], 1), node("b", &["a"], 1), node("c", &["b"], 1)],
            &[(0, 1), (1, 2)],
        );
        assert_eq!(d.initial_ready(), vec![0]);

        let o = d.on_finish(0, FinishKind::Success);
        assert_eq!(o.ready, vec![1]);
        assert!(d.on_finish(1, FinishKind::Success).ready == vec![2]);
        assert!(d.is_finished() == false);
        d.on_finish(2, FinishKind::Success);
        assert!(d.is_finished());
    }

    /// Diamond: the join node waits for BOTH parallel branches.
    #[test]
    fn diamond_join_waits_for_all_branches() {
        let mut d = dag(
            FailurePolicy::Continue,
            vec![
                node("a", &[], 1),
                node("b", &["a"], 1),
                node("c", &["a"], 1),
                node("d", &["b", "c"], 1),
            ],
            &[(0, 1), (0, 2), (1, 3), (2, 3)],
        );
        let o = d.on_finish(0, FinishKind::Success);
        assert_eq!(o.ready.len(), 2, "both branches become ready in parallel");
        assert!(d.on_finish(1, FinishKind::Success).ready.is_empty());
        assert_eq!(d.on_finish(2, FinishKind::Success).ready, vec![3]);
    }

    /// Continue policy: a failure skips only its own subtree; independent
    /// branches are unaffected.
    #[test]
    fn continue_policy_skips_only_dependents() {
        // a -> b -> c ; x independent
        let mut d = dag(
            FailurePolicy::Continue,
            vec![
                node("a", &[], 1),
                node("b", &["a"], 1),
                node("c", &["b"], 1),
                node("x", &[], 1),
            ],
            &[(0, 1), (1, 2)],
        );
        assert_eq!(d.initial_ready().len(), 2);
        d.nodes[0].attempts = 1; // dispatched once (dispatch_ready bumps this)
        let o = d.on_finish(0, FinishKind::Failed);
        assert!(!o.retried);
        assert_eq!(o.skipped, vec![1, 2], "transitive dependents skipped");
        assert!(o.to_cancel.is_empty() && o.cancelled.is_empty());
        assert_eq!(d.nodes[3].state, NodeState::Pending, "independent branch unaffected");
        let o = d.on_finish(3, FinishKind::Success);
        assert!(o.ready.is_empty());
        assert!(d.is_finished());
    }

    /// A node with multiple parents is skipped when ANY parent fails, even
    /// if the other parent later succeeds.
    #[test]
    fn multi_parent_skipped_when_one_parent_fails() {
        let mut d = dag(
            FailurePolicy::Continue,
            vec![node("a", &[], 1), node("b", &[], 1), node("c", &["a", "b"], 1)],
            &[(0, 2), (1, 2)],
        );
        d.nodes[0].attempts = 1; // dispatched once
        let o = d.on_finish(0, FinishKind::Failed);
        assert_eq!(o.skipped, vec![2]);
        // The surviving parent succeeding must not resurrect the skipped join.
        let o = d.on_finish(1, FinishKind::Success);
        assert!(o.ready.is_empty());
        assert_eq!(d.nodes[2].state, NodeState::Skipped);
    }

    /// Fail-fast: pending nodes are cancelled and running nodes are listed
    /// for cooperative cancellation.
    #[test]
    fn fail_fast_cancels_everything_unfinished() {
        // a -> b ; c running (dispatched), d pending under b
        let mut d = dag(
            FailurePolicy::FailFast,
            vec![
                node("a", &[], 1),
                node("b", &["a"], 1),
                node("c", &[], 1),
                node("d", &["b"], 1),
            ],
            &[(0, 1), (1, 3)],
        );
        d.nodes[2].state = NodeState::Dispatched; // c is running
        d.nodes[0].attempts = 1; // dispatched once
        let o = d.on_finish(0, FinishKind::Failed);
        assert!(d.fail_fast_triggered);
        assert_eq!(o.cancelled, vec![1, 3], "pending nodes cancelled");
        assert_eq!(o.to_cancel, vec![2], "running node gets the cancel flag");
        assert_eq!(d.nodes[0].state, NodeState::Failed);
        assert_eq!(d.nodes[2].state, NodeState::Dispatched);
    }

    /// Cancellation propagates down the dependency edges only.
    #[test]
    fn cancel_propagates_to_pending_subtree() {
        // a -> b -> c ; x independent
        let mut d = dag(
            FailurePolicy::FailFast,
            vec![
                node("a", &[], 1),
                node("b", &["a"], 1),
                node("c", &["b"], 1),
                node("x", &[], 1),
            ],
            &[(0, 1), (1, 2)],
        );
        let o = d.on_finish(0, FinishKind::Cancelled);
        assert_eq!(o.cancelled, vec![1, 2]);
        assert_eq!(d.nodes[3].state, NodeState::Pending, "independent node unaffected");
    }

    /// Scheduler-level retry: a node with attempts left is retried, not
    /// failed; the final failure propagates once attempts are exhausted.
    #[test]
    fn retry_delays_failure_until_attempts_exhausted() {
        let mut d = dag(
            FailurePolicy::Continue,
            vec![node("a", &[], 2), node("b", &["a"], 1)],
            &[(0, 1)],
        );
        d.nodes[0].attempts = 1;
        d.nodes[0].state = NodeState::Dispatched;
        let o = d.on_finish(0, FinishKind::Failed);
        assert!(o.retried, "first failure retries");
        assert_eq!(d.nodes[0].state, NodeState::Dispatched);

        d.nodes[0].attempts = 2;
        let o = d.on_finish(0, FinishKind::Failed);
        assert!(!o.retried, "attempts exhausted -> definitive failure");
        assert_eq!(o.skipped, vec![1]);
        assert_eq!(d.nodes[0].state, NodeState::Failed);
    }

    /// A condition skip releases dependents (it is not a failure).
    #[test]
    fn condition_skip_releases_dependents() {
        let mut d = dag(
            FailurePolicy::Continue,
            vec![node("a", &[], 1), node("b", &["a"], 1), node("c", &["b"], 1)],
            &[(0, 1), (1, 2)],
        );
        let ready = d.skip_for_condition(0);
        assert_eq!(ready, vec![1]);
        assert_eq!(d.nodes[0].state, NodeState::Skipped);
        assert!(d.nodes[0].skipped);
    }

    /// Finishing an already-final node is a no-op (cancel/worker race).
    #[test]
    fn finish_on_final_node_is_noop() {
        let mut d = dag(FailurePolicy::FailFast, vec![node("a", &[], 1)], &[]);
        d.on_finish(0, FinishKind::Success);
        let o = d.on_finish(0, FinishKind::Cancelled);
        assert!(o.cancelled.is_empty() && o.ready.is_empty() && !o.retried);
    }

    /// Cycles and out-of-range indices are rejected before anything persists.
    #[test]
    fn invalid_dags_are_rejected() {
        assert!(validate_edges(2, &[(0, 1), (1, 0)]).is_err(), "cycle");
        assert!(validate_edges(1, &[(0, 0)]).is_err(), "self loop");
        assert!(validate_edges(1, &[(0, 3)]).is_err(), "out of range");
        assert!(validate_edges(2, &[(0, 1)]).is_ok());
    }

    /// build() must reject a cyclic graph too.
    #[test]
    fn build_rejects_cycle() {
        let r = DagState::build(
            "d".into(),
            "n".into(),
            FailurePolicy::Continue,
            vec![node("a", &["b"], 1), node("b", &["a"], 1)],
            &[(0, 1), (1, 0)],
        );
        assert!(r.is_err());
    }

    /// tail() keeps the end of an over-cap string on a char boundary.
    #[test]
    fn tail_caps_at_boundary() {
        let s = "a".repeat(100);
        assert_eq!(tail(&s, 10).len(), 10);
        let multi = "中".repeat(100);
        let t = tail(&multi, 10);
        assert!(t.len() <= 12 && t.ends_with('中'));
    }
}
