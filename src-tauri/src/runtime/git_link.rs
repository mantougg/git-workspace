//! R-21 Git 联动引擎（§47 Status 提示 / §48 Branch 联动）。
//!
//! 数据源**只有** T-02 状态缓存（SQLite `repo_status` / `repositories`）与
//! R-02 依赖图索引——禁止为联动触发任何额外 git 操作或网络请求（任务文档
//! 「架构/性能注意点」）；Runtime 侧也不主动修改 Git 状态（全局约束 §11）：
//! 本引擎只监听、提示与编排重建任务。
//!
//! - §47 Status 联动：参与运行中应用闭包的仓库出现 Modified（`repo_status`
//!   快照读取，非实时 git 调用）→ 聚合发出 `runtime_dependency_changed`
//!   （批量仓库合并为一条提示，快照去重防重复打扰）。
//! - §48 Branch 联动：Git 侧 checkout 成功后调用 [`GitLinkEngine::notify_branch_switched`]
//!   → 提交依赖模型重算（R-02 既有 `ResolveDependencies` 路径，失效即重算）
//!   → 周期对账比对 graph fingerprint：**POM 有变化**（fingerprint 变化）才
//!   发提示 / 对 autoRestart 应用自动 Rebuild & Restart；无变化则静默。
//! - §49 操作保护：[`runtime_running_briefs`] 提供轻量 IPC 查询「是否有
//!   运行中应用」（纯 DB 读），由前端在 Checkout 入口弹出确认。

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rusqlite::Connection;

use crate::models::task::{RuntimeOp, RuntimeTaskOptions, TaskRequest, TaskType};
use crate::maven::index::DependencyGraphCache;
use crate::maven::closure::RuntimeClosureCache;
use crate::runtime::events::{
    DependencyChangedPayload, RuntimeEmission, RuntimeEventEmitter, EVENT_DEPENDENCY_CHANGED,
};
use crate::runtime::watch::WatchTaskSubmitter;

/// §47 Status 对账周期（repo_status 由 T-02 watcher 维护，这里只读表）。
pub const LINK_SYNC_INTERVAL: Duration = Duration::from_secs(5);
/// §48 分支切换后等待依赖重算的最长窗口（超时则放弃本轮复核）。
pub const BRANCH_RECHECK_TIMEOUT: Duration = Duration::from_secs(90);
/// 重算未产生 fingerprint 变化时的一次性重试点（覆盖 resolve 先于
/// checkout 完成的竞态）。
const BRANCH_RECHECK_RETRY: Duration = Duration::from_secs(15);

/// §49 运行中应用摘要（保护确认用；轻量 DB 读）。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeRunningBrief {
    pub workspace_id: i64,
    pub runtime_name: String,
    pub status: String,
}

/// §48 待复核的分支切换。
struct PendingBranch {
    pre_fingerprint: String,
    submitted_at: std::time::Instant,
    /// 是否已重试过一次 resolve。
    retried: bool,
}

/// R-21 Git 联动引擎。
pub struct GitLinkEngine {
    db: Arc<Mutex<Connection>>,
    graph_cache: Arc<DependencyGraphCache>,
    closure_cache: Arc<RuntimeClosureCache>,
    emitter: Arc<dyn RuntimeEventEmitter>,
    task_manager: Mutex<Option<Arc<dyn WatchTaskSubmitter>>>,
    /// §47 上次提示快照：(workspace_id, runtime) → dirty repo path 集合；
    /// 快照不变不重发（聚合去重）。
    last_dirty: Mutex<HashMap<(i64, String), BTreeSet<String>>>,
    /// §48 待复核分支切换：workspace_id → 状态。
    pending_branch: Mutex<HashMap<i64, PendingBranch>>,
    stop: Arc<AtomicBool>,
}

impl GitLinkEngine {
    /// 装配引擎并启动对账循环线程。
    pub fn spawn(
        db: Arc<Mutex<Connection>>,
        graph_cache: Arc<DependencyGraphCache>,
        closure_cache: Arc<RuntimeClosureCache>,
        emitter: Arc<dyn RuntimeEventEmitter>,
    ) -> Arc<Self> {
        let engine = Arc::new(Self {
            db,
            graph_cache,
            closure_cache,
            emitter,
            task_manager: Mutex::new(None),
            last_dirty: Mutex::new(HashMap::new()),
            pending_branch: Mutex::new(HashMap::new()),
            stop: Arc::new(AtomicBool::new(false)),
        });
        {
            let engine = Arc::clone(&engine);
            std::thread::Builder::new()
                .name("runtime-git-link".into())
                .spawn(move || loop {
                    if engine.stop.load(Ordering::Relaxed) {
                        break;
                    }
                    engine.sync_tick();
                    std::thread::sleep(LINK_SYNC_INTERVAL);
                })
                .ok();
        }
        engine
    }

    pub fn attach_task_manager(&self, task_manager: Arc<dyn WatchTaskSubmitter>) {
        *self.task_manager.lock().unwrap() = Some(task_manager);
    }

    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    /// 一次对账（公开供测试直接调用）：§47 快照提示 + §48 分支复核。
    pub fn sync_tick(&self) {
        self.sync_status_prompts();
        self.poll_branch_switches();
    }

    // ------------------------------------------------------------------
    // §47 Status 联动
    // ------------------------------------------------------------------

    /// 读取运行中应用闭包内仓库的 dirty 快照（纯 DB，禁 git 调用）。
    /// 返回 (workspace_id, runtime) → (dirty repo paths, affected module GAs)。
    fn current_dirty_snapshot(
        &self,
    ) -> BTreeMap<(i64, String), (BTreeSet<String>, Vec<String>)> {
        let mut result = BTreeMap::new();
        let conn = self.db.lock().unwrap();
        let Ok(workspaces) = crate::db::dao::list_workspaces(&conn) else {
            return result;
        };
        for ws in workspaces {
            let runtime_names: Vec<String> =
                match crate::runtime::launch::store::list_processes(&conn, ws.id) {
                    Ok(rows) => rows
                        .into_iter()
                        .filter(|row| row.status.is_active())
                        .map(|row| row.runtime_name)
                        .collect(),
                    Err(_) => continue,
                };
            for runtime_name in runtime_names {
                let snapshot = self.dirty_for_app(&conn, ws.id, &runtime_name);
                result.insert((ws.id, runtime_name), snapshot);
            }
        }
        result
    }

    /// 单应用的 dirty 快照：闭包内模块所属仓库出现 Modified → 集合 + GA。
    fn dirty_for_app(
        &self,
        conn: &Connection,
        workspace_id: i64,
        runtime_name: &str,
    ) -> (BTreeSet<String>, Vec<String>) {
        // repo_status 只对「有缓存行」的仓库存在；无行 = 尚未扫描，不提示。
        let mut dirty_repos: BTreeMap<i64, String> = BTreeMap::new();
        {
            let mut stmt = conn
                .prepare(
                    "SELECT r.id, r.path, rs.modified_count
                     FROM repositories r
                     JOIN repo_status rs ON rs.repo_id = r.id
                     WHERE r.workspace_id = ?1 AND rs.modified_count > 0",
                )
                .expect("repo_status query");
            let rows = stmt
                .query_map([workspace_id], |row| {
                    Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, i64>(2)?))
                })
                .expect("repo_status query_map");
            for row in rows {
                let (id, path, _modified) = row.expect("repo_status row");
                dirty_repos.insert(id, path.replace('\\', "/"));
            }
        }
        if dirty_repos.is_empty() {
            return (BTreeSet::new(), Vec::new());
        }

        let Ok(lookup) = self.graph_cache.get_or_load(conn, workspace_id) else {
            return (BTreeSet::new(), Vec::new());
        };
        let graph = lookup.graph;
        // 运行中的应用：任何 active 进程行已在上层过滤；这里取其闭包。
        let cfg = match crate::runtime::config::load_config_unredacted(
            conn,
            workspace_id,
            runtime_name,
        ) {
            Ok(c) => c,
            Err(_) => return (BTreeSet::new(), Vec::new()),
        };
        let needle = cfg.project.replace('\\', "/");
        let Some(root) = graph.projects.iter().find(|p| {
            let path = p.path.to_string_lossy().replace('\\', "/");
            path == needle
                || path.ends_with(&needle)
                || p.coordinates.artifact_id == cfg.project
        }) else {
            return (BTreeSet::new(), Vec::new());
        };
        let Ok(closure_lookup) = self
            .closure_cache
            .get_or_compute(&graph, root.project_id, &cfg.scope)
        else {
            return (BTreeSet::new(), Vec::new());
        };

        let mut dirty_paths = BTreeSet::new();
        let mut affected = Vec::new();
        for project in &closure_lookup.closure.projects {
            let Some(repo_id) = project.repository_id else {
                continue;
            };
            if let Some(path) = dirty_repos.get(&repo_id) {
                dirty_paths.insert(path.clone());
                affected.push(format!(
                    "{}:{}",
                    project.coordinates.group_id, project.coordinates.artifact_id
                ));
            }
        }
        (dirty_paths, affected)
    }

    /// §47 对账：快照与上次不同 → 聚合发一条提示（含清空语义）。
    fn sync_status_prompts(&self) {
        let snapshot = self.current_dirty_snapshot();
        let mut last = self.last_dirty.lock().unwrap();
        // 清理不再运行的应用快照。
        last.retain(|key, _| snapshot.contains_key(key));
        for (key, (dirty, affected)) in snapshot {
            let unchanged = last.get(&key).is_some_and(|prev| *prev == dirty);
            if unchanged {
                continue;
            }
            // 应用启动且无任何 dirty 仓库：不发声（首次空快照无需清除）。
            if dirty.is_empty() && !last.contains_key(&key) {
                last.insert(key.clone(), dirty);
                continue;
            }
            last.insert(key.clone(), dirty.clone());
            // 空集合 = 恢复干净 → 发空提示让 UI 清除横幅。
            self.emitter.emit(RuntimeEmission::new(
                EVENT_DEPENDENCY_CHANGED,
                &DependencyChangedPayload {
                    workspace_id: key.0,
                    runtime_name: key.1.clone(),
                    reason: "filesModified".into(),
                    repos: dirty.into_iter().collect(),
                    affected_modules: affected,
                    at: chrono::Utc::now().to_rfc3339(),
                },
            ));
        }
    }

    // ------------------------------------------------------------------
    // §48 Branch 联动
    // ------------------------------------------------------------------

    /// Git 侧 checkout 成功后调用（commands/branch.rs / batch.rs）：
    /// 记录 pre-fingerprint 并提交依赖模型重算。非阻塞、幂等（同 workspace
    /// 连续多次切换只保留最后一次）。
    pub fn notify_branch_switched(&self, repo_path: &str) {
        let workspace_id = {
            let conn = self.db.lock().unwrap();
            match conn
                .query_row(
                    "SELECT workspace_id FROM repositories WHERE path = ?1",
                    [repo_path],
                    |row| row.get::<_, i64>(0),
                )
                .ok()
            {
                Some(ws) => ws,
                None => return, // 不在索引内的仓库（如 worktree）静默忽略。
            }
        };

        let pre_fingerprint = {
            let conn = self.db.lock().unwrap();
            match self.graph_cache.get_or_load(&conn, workspace_id) {
                Ok(lookup) => lookup.graph.fingerprint,
                Err(_) => {
                    log::warn!("R-21: branch switch in workspace #{workspace_id}: graph load failed");
                    return;
                }
            }
        };

        // §48：Invalidate → Recalculate（走 R-02 既有 ResolveDependencies）。
        self.submit_resolve(workspace_id);
        log::info!(
            "R-21: branch switched on repo {repo_path:?} (workspace #{workspace_id}); \
             dependency recalc submitted"
        );
        self.pending_branch.lock().unwrap().insert(
            workspace_id,
            PendingBranch {
                pre_fingerprint,
                submitted_at: std::time::Instant::now(),
                retried: false,
            },
        );
    }

    /// §48 对账：fingerprint 变化 = POM 有变化 → 提示 + autoRestart 应用
    /// 自动 Rebuild & Restart；超时/无变化 → 放弃。
    fn poll_branch_switches(&self) {
        let actions: Vec<(i64, bool)> = {
            let conn = self.db.lock().unwrap();
            let mut pending = self.pending_branch.lock().unwrap();
            let mut actions = Vec::new();
            pending.retain(|workspace_id, state| {
                let elapsed = state.submitted_at.elapsed();
                let current = self
                    .graph_cache
                    .get_or_load(&conn, *workspace_id)
                    .map(|l| l.graph.fingerprint)
                    .unwrap_or_default();
                if current != state.pre_fingerprint {
                    log::info!(
                        "R-21: workspace #{workspace_id} POM changed after branch switch \
                         (fingerprint {pre} → {current})",
                        pre = &state.pre_fingerprint[..state.pre_fingerprint.len().min(8)],
                        current = &current[..current.len().min(8)],
                    );
                    actions.push((*workspace_id, true));
                    return false; // 复核完成，移除。
                }
                if elapsed >= BRANCH_RECHECK_TIMEOUT {
                    log::info!(
                        "R-21: workspace #{workspace_id} branch recheck timed out without POM change"
                    );
                    actions.push((*workspace_id, false));
                    return false;
                }
                if !state.retried && elapsed >= BRANCH_RECHECK_RETRY {
                    // resolve 可能先于 checkout 完成：重算一次再等。
                    state.retried = true;
                    self.submit_resolve(*workspace_id);
                }
                true
            });
            actions
        };

        for (workspace_id, pom_changed) in actions {
            if !pom_changed {
                continue;
            }
            self.emit_pom_changed(workspace_id);
        }
    }

    /// POM 变化：对每个运行中应用发提示；autoRestart 开启的应用自动重建。
    fn emit_pom_changed(&self, workspace_id: i64) {
        let (active, auto_restart_names) = {
            let conn = self.db.lock().unwrap();
            let Ok(rows) = crate::runtime::launch::store::list_processes(&conn, workspace_id)
            else {
                return;
            };
            let mut active = Vec::new();
            let mut auto_restart_names = Vec::new();
            for row in rows {
                if !row.status.is_active() {
                    continue;
                }
                let auto_restart = crate::runtime::config::load_config_unredacted(
                    &conn,
                    workspace_id,
                    &row.runtime_name,
                )
                .ok()
                .and_then(|cfg| cfg.auto_restart);
                if auto_restart == Some(true) {
                    auto_restart_names.push(row.runtime_name.clone());
                }
                active.push(row.runtime_name);
            }
            (active, auto_restart_names)
        };

        for runtime_name in active {
            let (dirty, affected) = {
                let conn = self.db.lock().unwrap();
                self.dirty_for_app(&conn, workspace_id, &runtime_name)
            };
            self.emitter.emit(RuntimeEmission::new(
                EVENT_DEPENDENCY_CHANGED,
                &DependencyChangedPayload {
                    workspace_id,
                    runtime_name,
                    reason: "branchSwitched".into(),
                    repos: dirty.into_iter().collect(),
                    affected_modules: affected,
                    at: chrono::Utc::now().to_rfc3339(),
                },
            ));
        }
        // autoRestart 应用：§48 链路最后一环 Rebuild if required。
        for runtime_name in auto_restart_names {
            self.submit_rebuild_restart(workspace_id, &runtime_name);
        }
    }

    // ------------------------------------------------------------------
    // 提交
    // ------------------------------------------------------------------

    fn submit_task(&self, request: TaskRequest) -> bool {
        let Some(submitter) = self.task_manager.lock().unwrap().clone() else {
            log::debug!("R-21: task manager not attached; dropping task");
            return false;
        };
        match submitter.submit(request) {
            Ok(id) => {
                log::debug!("R-21: task {id} submitted");
                true
            }
            Err(e) => {
                log::warn!("R-21: task submit failed: {e}");
                false
            }
        }
    }

    fn submit_resolve(&self, workspace_id: i64) {
        let request = TaskRequest {
            task_type: TaskType::Runtime {
                op: RuntimeOp::ResolveDependencies,
                workspace_id,
                runtime_name: String::new(),
                options: RuntimeTaskOptions::default(),
            },
            repo_path: String::new(),
            repo_name: format!("R-21: 分支切换后依赖重算（workspace #{workspace_id}）"),
        };
        self.submit_task(request);
    }

    fn submit_rebuild_restart(&self, workspace_id: i64, runtime_name: &str) {
        let request = TaskRequest {
            task_type: TaskType::Runtime {
                op: RuntimeOp::RebuildRestart,
                workspace_id,
                runtime_name: runtime_name.to_string(),
                options: RuntimeTaskOptions::default(),
            },
            repo_path: String::new(),
            repo_name: format!("R-21: 分支切换 POM 变化 → 重建重启 · {runtime_name}"),
        };
        self.submit_task(request);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::repository::ScannedRepo;
    use crate::runtime::config::CreateRuntimeConfigRequest;
    use crate::runtime::events::VecEmitter;

    fn write(path: &std::path::Path, content: &str) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, content).unwrap();
    }

    /// 测试用假任务提交端。
    struct Recorder(Mutex<Vec<(RuntimeOp, String)>>);

    impl Recorder {
        fn new() -> Arc<Self> {
            Arc::new(Self(Mutex::new(Vec::new())))
        }
        fn ops(&self) -> Vec<(RuntimeOp, String)> {
            self.0.lock().unwrap().clone()
        }
    }

    impl WatchTaskSubmitter for Recorder {
        fn submit(&self, request: TaskRequest) -> crate::error::AppResult<String> {
            if let TaskType::Runtime {
                op, runtime_name, ..
            } = &request.task_type
            {
                self.0.lock().unwrap().push((*op, runtime_name.clone()));
            }
            Ok(format!("t-{}", self.0.lock().unwrap().len()))
        }
    }

    /// 单仓 parent + lib + app（app 依赖 lib）+ 依赖图索引 + 运行中进程。
    struct Fixture {
        root: std::path::PathBuf,
        db: Arc<Mutex<Connection>>,
        workspace_id: i64,
        repo_path: String,
        auto_restart: bool,
    }

    fn link_fixture(tag: &str, auto_restart: bool) -> Fixture {
        let root = std::env::temp_dir().join(format!(
            "gw_r21_{tag}_{}",
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        std::fs::create_dir_all(&root).unwrap();
        write(
            &root.join("repo/pom.xml"),
            "<project><modelVersion>4.0.0</modelVersion><groupId>com.example</groupId>\
             <artifactId>parent</artifactId><version>1.0.0</version><packaging>pom</packaging>\
             <modules><module>lib</module><module>app</module></modules></project>",
        );
        write(
            &root.join("repo/lib/pom.xml"),
            "<project><modelVersion>4.0.0</modelVersion><parent><groupId>com.example</groupId>\
             <artifactId>parent</artifactId><version>1.0.0</version></parent>\
             <artifactId>lib</artifactId></project>",
        );
        write(
            &root.join("repo/app/pom.xml"),
            "<project><modelVersion>4.0.0</modelVersion><parent><groupId>com.example</groupId>\
             <artifactId>parent</artifactId><version>1.0.0</version></parent>\
             <artifactId>app</artifactId><dependencies><dependency><groupId>com.example</groupId>\
             <artifactId>lib</artifactId><version>1.0.0</version></dependency></dependencies></project>",
        );
        git2::Repository::init(root.join("repo")).unwrap();

        let mut conn = Connection::open_in_memory().unwrap();
        crate::db::init_db(&mut conn).unwrap();
        conn.execute(
            "INSERT INTO workspaces (name, path, created_at, updated_at) VALUES ('w', ?1, 't', 't')",
            [root.to_string_lossy().to_string()],
        )
        .unwrap();
        let workspace_id = conn.last_insert_rowid();
        let repo_path = root.join("repo").to_string_lossy().to_string();
        crate::db::dao::upsert_repositories_batch(
            &mut conn,
            workspace_id,
            &[ScannedRepo {
                path: repo_path.clone(),
                name: "repo".into(),
                relative_path: "repo".into(),
                git_dir_mtime: None,
            }],
        )
        .unwrap();
        let discovery = crate::maven::discover_poms(&root, 5, None, None);
        assert!(discovery.errors.is_empty(), "{:?}", discovery.errors);
        crate::maven::sync_workspace_index(&mut conn, workspace_id, &discovery, &root.join("m2"))
            .unwrap();

        crate::runtime::config::create_config(
            &conn,
            &CreateRuntimeConfigRequest {
                workspace_id,
                config: crate::runtime::config::RuntimeApplicationConfig {
                    name: "app".into(),
                    project: root.join("repo/app/pom.xml").to_string_lossy().to_string(),
                    main_class: Some("com.example.app.Application".into()),
                    auto_restart: Some(auto_restart),
                    ..Default::default()
                },
            },
        )
        .unwrap();
        // 运行中的 app 进程。
        conn.execute(
            "INSERT INTO runtime_processes (workspace_id, runtime_name, status, started_at, updated_at)
             VALUES (?1, 'app', 'running', 't', 't')",
            [workspace_id],
        )
        .unwrap();

        Fixture {
            root,
            db: Arc::new(Mutex::new(conn)),
            workspace_id,
            repo_path,
            auto_restart,
        }
    }

    fn mark_repo_dirty(fixture: &Fixture, modified_count: i64) {
        let conn = fixture.db.lock().unwrap();
        let repo_id: i64 = conn
            .query_row(
                "SELECT id FROM repositories WHERE path = ?1",
                [&fixture.repo_path],
                |r| r.get(0),
            )
            .unwrap();
        conn.execute(
            "INSERT INTO repo_status (repo_id, branch, is_dirty, modified_count, updated_at)
             VALUES (?1, 'main', ?2, ?2, 't')
             ON CONFLICT(repo_id) DO UPDATE SET is_dirty = ?2, modified_count = ?2",
            rusqlite::params![repo_id, modified_count > 0],
        )
        .unwrap();
    }

    fn test_engine(fixture: &Fixture) -> (GitLinkEngine, Arc<Recorder>, Arc<VecEmitter>) {
        let recorder = Recorder::new();
        let emitter = Arc::new(VecEmitter::default());
        let engine = GitLinkEngine {
            db: Arc::clone(&fixture.db),
            graph_cache: Arc::new(DependencyGraphCache::new()),
            closure_cache: Arc::new(RuntimeClosureCache::new()),
            emitter: Arc::clone(&emitter) as Arc<dyn RuntimeEventEmitter>,
            task_manager: Mutex::new(Some(
                Arc::clone(&recorder) as Arc<dyn WatchTaskSubmitter>
            )),
            last_dirty: Mutex::new(HashMap::new()),
            pending_branch: Mutex::new(HashMap::new()),
            stop: Arc::new(AtomicBool::new(false)),
        };
        (engine, recorder, emitter)
    }

    /// §47：闭包内仓库 Modified → 聚合一条提示（仓库 + 受影响模块）；
    /// 快照不变不重发；恢复干净 → 发空提示清除。
    #[test]
    fn status_link_emits_aggregated_prompt_once() {
        let fixture = link_fixture("status", false);
        let (engine, recorder, emitter) = test_engine(&fixture);
        assert!(recorder.ops().is_empty());

        mark_repo_dirty(&fixture, 3);
        engine.sync_tick();
        let emissions = emitter.collected();
        assert_eq!(emissions.len(), 1, "one aggregated prompt: {:?}", emissions.len());
        assert_eq!(emissions[0].name, EVENT_DEPENDENCY_CHANGED);
        let payload = &emissions[0].payload;
        assert_eq!(payload["reason"], "filesModified");
        assert_eq!(payload["runtimeName"], "app");
        let repos = payload["repos"].as_array().unwrap();
        assert_eq!(repos.len(), 1);
        assert!(repos[0].as_str().unwrap().ends_with("/repo"));
        let modules = payload["affectedModules"].as_array().unwrap();
        assert_eq!(modules.len(), 2, "lib + app both affected");
        // 任务侧不动：§47 只提示。
        assert!(recorder.ops().is_empty());

        // 快照不变 → 不重发。
        engine.sync_tick();
        assert_eq!(emitter.collected().len(), 1);

        // 恢复干净 → 空提示（清除语义）。
        mark_repo_dirty(&fixture, 0);
        engine.sync_tick();
        let emissions = emitter.collected();
        assert_eq!(emissions.len(), 2);
        assert_eq!(emissions[1].payload["repos"].as_array().unwrap().len(), 0);
    }

    /// §48：分支切换 → 依赖重算提交；POM 有变化（fingerprint 变化）→
    /// 提示 + autoRestart 应用自动 Rebuild & Restart。
    #[test]
    fn branch_switch_recalculates_and_rebuilds_on_pom_change() {
        let fixture = link_fixture("branch", true);
        let (engine, recorder, emitter) = test_engine(&fixture);

        // checkout 成功后的通知：提交依赖重算，记录 pre-fingerprint。
        engine.notify_branch_switched(&fixture.repo_path);
        let ops = recorder.ops();
        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0].0, RuntimeOp::ResolveDependencies);

        // fingerprint 未变（POM 无变化）→ 不提示、不重建。
        engine.sync_tick();
        assert_eq!(emitter.collected().len(), 0, "no prompt before POM change");

        // 分支切换导致 POM 变化 → 重同步索引（生产上由 resolve 任务完成）。
        write(
            &fixture.root.join("repo/lib/pom.xml"),
            "<project><modelVersion>4.0.0</modelVersion><parent><groupId>com.example</groupId>\
             <artifactId>parent</artifactId><version>1.0.0</version></parent>\
             <artifactId>lib</artifactId><properties><branch>switched</branch></properties></project>",
        );
        {
            let mut conn = fixture.db.lock().unwrap();
            let discovery = crate::maven::discover_poms(&fixture.root, 5, None, None);
            crate::maven::sync_workspace_index(
                &mut conn,
                fixture.workspace_id,
                &discovery,
                &fixture.root.join("m2"),
            )
            .unwrap();
        }
        engine.sync_tick();

        // 提示（branchSwitched）+ autoRestart → 自动 RebuildRestart。
        let emissions = emitter.collected();
        assert_eq!(emissions.len(), 1);
        assert_eq!(emissions[0].payload["reason"], "branchSwitched");
        let ops = recorder.ops();
        assert_eq!(ops.len(), 2, "resolve + rebuild: {ops:?}");
        assert_eq!(ops[1].0, RuntimeOp::RebuildRestart);
        assert_eq!(ops[1].1, "app");

        // 复核完成后 pending 清空：再 tick 无新动作。
        engine.sync_tick();
        assert_eq!(emitter.collected().len(), 1);
        assert_eq!(recorder.ops().len(), 2);
    }

    /// POM 无变化的分支切换：复核不触发 Rebuild（重算也提交过一次）。
    #[test]
    fn branch_switch_without_pom_change_does_not_rebuild() {
        let fixture = link_fixture("branch-clean", false);
        let (engine, recorder, emitter) = test_engine(&fixture);

        engine.notify_branch_switched(&fixture.repo_path);
        assert_eq!(recorder.ops().len(), 1);
        // fingerprint 未变 → 无提示、无重建（复核保持 pending 至超时）。
        engine.sync_tick();
        assert!(emitter.collected().is_empty());
        assert_eq!(recorder.ops().len(), 1, "no rebuild without POM change");
        assert!(
            engine.pending_branch.lock().unwrap().contains_key(&fixture.workspace_id),
            "pending kept for recheck window"
        );
    }
}
