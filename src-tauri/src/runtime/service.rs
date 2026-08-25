//! RuntimeService（R-12，§63/§64/§65/§66）：Runtime 模块对 Task Engine 与
//! IPC 的统一入口。
//!
//! 职责：
//! - 实现 [`RuntimeTaskHandler`]，把 `TaskType::Runtime`（Build / Start /
//!   Stop / Restart / ResolveDependencies）分发给 R-09 Build Engine 与
//!   R-10 Process Manager，全程接线取消标志（§65 Task Engine 集成）；
//! - 发射 §64 中 handler 侧的事件（`build_started/completed`（Build-only）、
//!   `restart_started/completed`、`project_discovered`、`dependency_resolved`）；
//!   进程域事件由 [`crate::runtime::events::TauriRuntimeBridge`] 从
//!   R-10/R-11 内部事件桥接；
//! - 持有 Runtime Task Scheduler 的两个 permit 池（§66：Build 默认 2 /
//!   Dependency Resolve 默认 4，经 `runtime-scheduler.json` 可配置、
//!   运行时可调）；
//! - 为 §63 查询类 IPC 提供读接口（projects / graph / processes / logs）。
//!
//! 已知边界（显式记录，遵循全局约束「冲突需说明」）：
//! - `execute_build` 按阶段短持 SQLite 写锁（R-12 收敛），Maven 子进程
//!   运行期间不持锁：并发构建真正可达 §66 上限，UI 查询不被长构建阻塞。
//! - §64 的 `file_changed` 只定类型与快照，发射由 R-17 File Watch 接入。

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use tauri::AppHandle;

use crate::db::dao;
use crate::error::{AppError, AppResult};
use crate::maven::{
    self, DependencyEdge, DependencyGraphCache, MavenModuleLink, MavenProjectNode, PomCache,
    RuntimeClosureCache, SourceMapping,
};
use crate::models::task::{RuntimeOp, RuntimeTaskOptions, TaskRequest, TaskType};
use crate::runtime::build::pipeline::execute_build;
use crate::runtime::build::runner::{MavenRunner, SpawningMavenRunner};
use crate::runtime::build::scheduler::{
    BuildScheduler, DEFAULT_MAX_CONCURRENT_BUILDS, DEFAULT_MAX_CONCURRENT_RESOLVES,
};
use crate::runtime::build::{BuildOptions, BuildRequest, RingTail};
use crate::runtime::config;
use crate::runtime::events::{
    BuildCompletedPayload, BuildProgressPayload, BuildStartedPayload, DependencyResolvedPayload,
    ProjectDiscoveredPayload, RestartCompletedPayload, RestartStartedPayload, RuntimeEmission,
    RuntimeEventEmitter, RuntimeStage, TauriRuntimeBridge, TauriRuntimeEmitter,
    EVENT_BUILD_COMPLETED, EVENT_BUILD_PROGRESS, EVENT_BUILD_STARTED, EVENT_DEPENDENCY_RESOLVED,
    EVENT_PROJECT_DISCOVERED, EVENT_RESTART_COMPLETED, EVENT_RESTART_STARTED,
};
use crate::runtime::launch::launcher::{LaunchRunner, SystemLaunchRunner};
use crate::runtime::launch::manager::{
    RuntimeProcessDeps, RuntimeProcessManager, DEFAULT_SAMPLE_INTERVAL,
};
use crate::runtime::launch::{RuntimeProcessInfo, StartOptions};
use crate::runtime::logs::{LogEntry, LogFilter, RuntimeLogEngine};
use crate::task::runtime::RuntimeTaskHandler;

/// `project_discovered` 单次同步的爆发上限：增量同步按项目逐个发射；
/// 首次全量发现（大量新项目）只发 `dependency_resolved` 汇总，UI 据此
/// 重拉 `runtime_list_projects`，避免事件洪泛（R-12 高频聚合约束）。
const MAX_PROJECT_DISCOVERED_EVENTS: usize = 50;

/// 任务取消 watcher 的轮询间隔。
const CANCEL_WATCH_INTERVAL: Duration = Duration::from_millis(100);

/// 取消触发的优雅停止宽限（比用户主动 Stop 短，取消语义是尽快终止）。
const CANCEL_STOP_GRACE: Duration = Duration::from_secs(5);

// ---------------------------------------------------------------------------
// Scheduler 配置（§66 可配置）
// ---------------------------------------------------------------------------

/// Runtime Task Scheduler 并发上限。落 `<app_data_dir>/runtime-scheduler.json`
/// （机器级配置，先例：`health-weights.json`），serde default 容错缺字段。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SchedulerConfig {
    #[serde(default = "default_max_builds")]
    pub max_concurrent_builds: usize,
    #[serde(default = "default_max_resolves")]
    pub max_concurrent_resolves: usize,
}

fn default_max_builds() -> usize {
    DEFAULT_MAX_CONCURRENT_BUILDS
}

fn default_max_resolves() -> usize {
    DEFAULT_MAX_CONCURRENT_RESOLVES
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            max_concurrent_builds: DEFAULT_MAX_CONCURRENT_BUILDS,
            max_concurrent_resolves: DEFAULT_MAX_CONCURRENT_RESOLVES,
        }
    }
}

impl SchedulerConfig {
    /// 从 JSON 文件加载；文件不存在 / 解析失败回退默认（记日志，不致命）。
    pub fn load(path: &std::path::Path) -> Self {
        match std::fs::read_to_string(path) {
            Ok(text) => match serde_json::from_str::<SchedulerConfig>(&text) {
                Ok(config) => {
                    let config = config.sanitized();
                    log::info!("R-12: scheduler config loaded from {:?}: {:?}", path, config);
                    config
                }
                Err(e) => {
                    log::warn!("R-12: invalid scheduler config {:?}: {}; using defaults", path, e);
                    Self::default()
                }
            },
            Err(_) => Self::default(),
        }
    }

    /// 持久化（原子写，复用 R-07 的写盘助手）。
    pub fn save(&self, path: &std::path::Path) -> AppResult<()> {
        config::write_json_atomic(path, &self.sanitized())
    }

    /// 上限至少为 1（0 会死锁 permit 池）。
    fn sanitized(mut self) -> Self {
        self.max_concurrent_builds = self.max_concurrent_builds.max(1);
        self.max_concurrent_resolves = self.max_concurrent_resolves.max(1);
        self
    }
}

/// 生产配置路径（`<app_data_dir>/runtime-scheduler.json`）。
pub fn scheduler_config_path() -> PathBuf {
    crate::get_app_data_dir().join("runtime-scheduler.json")
}

// ---------------------------------------------------------------------------
// IPC 请求 / 视图类型（§63；golden 快照覆盖）
// ---------------------------------------------------------------------------

/// `runtime_build` / `runtime_start` / `runtime_restart` 的请求。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeOperationRequest {
    pub workspace_id: i64,
    pub runtime_name: String,
    #[serde(default)]
    pub options: RuntimeTaskOptions,
}

/// `runtime_get_logs` 的请求。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeLogQuery {
    pub workspace_id: i64,
    pub runtime_name: String,
    pub process_id: i64,
    #[serde(default)]
    pub filter: LogFilter,
}

/// `runtime_inspect_project` 的返回：DB 索引视角的项目详情（模块关系、
/// 依赖边、源码映射）。POM 级细节（profiles/plugins）不入索引，需要时由
/// UI 走 `preview_maven_command` 等 R-05 命令查看。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectInspection {
    pub project: MavenProjectNode,
    /// 该项目声明的子模块链接。
    pub modules: Vec<MavenModuleLink>,
    /// 该项目作为子模块时的父项目 id。
    pub parent_project_id: Option<i64>,
    /// 该项目的依赖边（出边）。
    pub dependencies: Vec<DependencyEdge>,
    /// 指向该项目的源码映射。
    pub source_mappings: Vec<SourceMapping>,
}

/// `runtime_get_dependency_graph` 的返回。大 payload 约束（R-12 任务文档）
/// 的落实：`max_edges` 截断依赖边（`truncated` 标记 + `total_dependencies`
/// 给出真实总数），或按 `project_id` 只取单项目出边下钻。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DependencyGraphView {
    pub workspace_id: i64,
    pub fingerprint: String,
    pub projects: Vec<MavenProjectNode>,
    pub modules: Vec<MavenModuleLink>,
    pub dependencies: Vec<DependencyEdge>,
    pub source_mappings: Vec<SourceMapping>,
    pub total_dependencies: usize,
    pub truncated: bool,
}

/// 依赖边默认返回上限（约 500 模块工作区的全量边数倍余量）。
const DEFAULT_MAX_GRAPH_EDGES: usize = 5000;

// ---------------------------------------------------------------------------
// RuntimeService
// ---------------------------------------------------------------------------

/// 生产/测试共用装配覆盖项（测试注入 fake runner 与测试 emitter）。
pub struct RuntimeServiceOverrides {
    pub maven_runner: Arc<dyn MavenRunner>,
    pub launch_runner: Arc<dyn LaunchRunner>,
    pub logs: Arc<RuntimeLogEngine>,
    pub sample_interval: Duration,
}

impl Default for RuntimeServiceOverrides {
    fn default() -> Self {
        Self {
            maven_runner: Arc::new(SpawningMavenRunner),
            launch_runner: Arc::new(SystemLaunchRunner),
            logs: Arc::new(RuntimeLogEngine::new()),
            sample_interval: DEFAULT_SAMPLE_INTERVAL,
        }
    }
}

/// Runtime 模块门面。字段即 §66 的共享设施：进程管理器、日志引擎、
/// 图/闭包缓存、Build/Resolve 双 permit 池——与 Process Manager 内部
/// 使用的是**同一批**实例，限流与缓存全局一致。
pub struct RuntimeService {
    db: Arc<Mutex<Connection>>,
    emitter: Arc<dyn RuntimeEventEmitter>,
    processes: Arc<RuntimeProcessManager>,
    logs: Arc<RuntimeLogEngine>,
    graph_cache: Arc<DependencyGraphCache>,
    closure_cache: Arc<RuntimeClosureCache>,
    build_scheduler: Arc<BuildScheduler>,
    resolve_scheduler: Arc<BuildScheduler>,
    maven_runner: Arc<dyn MavenRunner>,
    pom_cache: Arc<PomCache>,
    scheduler_config_path: PathBuf,
}

impl RuntimeService {
    /// 生产装配：Tauri event 总线 + 真实 runner + 配置文件的并发上限。
    pub fn new(app: AppHandle, db: Arc<Mutex<Connection>>, pom_cache: Arc<PomCache>) -> Arc<Self> {
        let path = scheduler_config_path();
        let config = SchedulerConfig::load(&path);
        Self::assemble(
            db,
            Arc::new(TauriRuntimeEmitter::new(app)),
            pom_cache,
            config,
            path,
            RuntimeServiceOverrides::default(),
        )
    }

    /// 完整装配（测试 seam）：emitter / runner / 日志引擎 / 采样间隔可注入。
    pub(crate) fn assemble(
        db: Arc<Mutex<Connection>>,
        emitter: Arc<dyn RuntimeEventEmitter>,
        pom_cache: Arc<PomCache>,
        scheduler_config: SchedulerConfig,
        scheduler_config_path: PathBuf,
        overrides: RuntimeServiceOverrides,
    ) -> Arc<Self> {
        let build_scheduler = Arc::new(BuildScheduler::new(scheduler_config.max_concurrent_builds));
        let resolve_scheduler =
            Arc::new(BuildScheduler::new(scheduler_config.max_concurrent_resolves));
        let graph_cache = Arc::new(DependencyGraphCache::new());
        let closure_cache = Arc::new(RuntimeClosureCache::new());
        let bridge = Arc::new(TauriRuntimeBridge::new(Arc::clone(&emitter), Arc::clone(&db)));

        let processes = Arc::new(RuntimeProcessManager::with_deps(
            Arc::clone(&db),
            RuntimeProcessDeps {
                graph_cache: Arc::clone(&graph_cache),
                closure_cache: Arc::clone(&closure_cache),
                scheduler: Arc::clone(&build_scheduler),
                maven_runner: Arc::clone(&overrides.maven_runner),
                launch_runner: Arc::clone(&overrides.launch_runner),
                events: bridge,
                logs: Arc::clone(&overrides.logs),
                sample_interval: overrides.sample_interval,
            },
        ));

        Arc::new(Self {
            db,
            emitter,
            processes,
            logs: overrides.logs,
            graph_cache,
            closure_cache,
            build_scheduler,
            resolve_scheduler,
            maven_runner: overrides.maven_runner,
            pom_cache,
            scheduler_config_path,
        })
    }

    fn now() -> String {
        chrono::Utc::now().to_rfc3339()
    }

    fn emit<T: Serialize>(&self, name: &'static str, payload: &T) {
        self.emitter.emit(RuntimeEmission::new(name, payload));
    }

    fn workspace_root(&self, workspace_id: i64) -> AppResult<PathBuf> {
        let conn = self.db.lock().unwrap();
        config::workspace_root(&conn, workspace_id)
    }

    // ------------------------------------------------------------------
    // 查询接口（§63 读侧）
    // ------------------------------------------------------------------

    /// `runtime_list_projects`：workspace 的 Maven 项目索引（DB 视角，
    /// 热路径；未同步过时为空，由 UI 引导触发 `runtime_resolve_dependencies`）。
    pub fn list_projects(&self, workspace_id: i64) -> AppResult<Vec<MavenProjectNode>> {
        let conn = self.db.lock().unwrap();
        Ok(maven::query_dependency_graph(&conn, workspace_id)?.projects)
    }

    /// `runtime_inspect_project`：按 path / artifactId / groupId:artifactId
    /// 三级匹配定位项目（与 R-09 `find_root_project` 同口径）。
    pub fn inspect_project(
        &self,
        workspace_id: i64,
        project: &str,
    ) -> AppResult<ProjectInspection> {
        let conn = self.db.lock().unwrap();
        let graph = maven::query_dependency_graph(&conn, workspace_id)?;
        let node = graph
            .projects
            .iter()
            .find(|p| {
                let path = p.path.to_string_lossy();
                path == project
                    || path.ends_with(project)
                    || p.coordinates.artifact_id == project
                    || format!(
                        "{}:{}",
                        p.coordinates.group_id, p.coordinates.artifact_id
                    ) == project
            })
            .ok_or_else(|| {
                AppError::ProjectNotFound(format!(
                    "项目 '{project}' 不在 workspace #{workspace_id} 的 Maven 索引中；\
                     请先执行依赖解析（runtime.resolve_dependencies）"
                ))
            })?;
        let project_id = node.project_id;
        Ok(ProjectInspection {
            project: node.clone(),
            modules: graph
                .modules
                .iter()
                .filter(|m| m.parent_project_id == project_id)
                .cloned()
                .collect(),
            parent_project_id: graph
                .modules
                .iter()
                .find(|m| m.module_project_id == Some(project_id))
                .map(|m| m.parent_project_id),
            dependencies: graph
                .dependencies
                .iter()
                .filter(|e| e.from_project_id == project_id)
                .cloned()
                .collect(),
            source_mappings: graph
                .source_mappings
                .iter()
                .filter(|m| m.project_id == project_id)
                .cloned()
                .collect(),
        })
    }

    /// `runtime_get_dependency_graph`：全量图（默认截断保护）或单项目下钻。
    pub fn dependency_graph(
        &self,
        workspace_id: i64,
        project_id: Option<i64>,
        max_edges: Option<usize>,
    ) -> AppResult<DependencyGraphView> {
        let conn = self.db.lock().unwrap();
        let graph = maven::query_dependency_graph(&conn, workspace_id)?;
        let (dependencies, total, truncated) = match project_id {
            Some(pid) => {
                let edges = maven::query_project_dependencies(&conn, pid)?;
                let total = edges.len();
                (edges, total, false)
            }
            None => {
                let cap = max_edges.unwrap_or(DEFAULT_MAX_GRAPH_EDGES);
                let total = graph.dependencies.len();
                let truncated = total > cap;
                let edges = graph.dependencies.into_iter().take(cap).collect();
                (edges, total, truncated)
            }
        };
        Ok(DependencyGraphView {
            workspace_id,
            fingerprint: graph.fingerprint,
            projects: graph.projects,
            modules: graph.modules,
            dependencies,
            source_mappings: graph.source_mappings,
            total_dependencies: total,
            truncated,
        })
    }

    /// `runtime_list_processes`。
    pub fn list_processes(&self, workspace_id: i64) -> AppResult<Vec<RuntimeProcessInfo>> {
        self.processes.list_processes(workspace_id)
    }

    /// `runtime_process_status`。
    pub fn process_status(&self, process_id: i64) -> AppResult<Option<RuntimeProcessInfo>> {
        self.processes.get_process(process_id)
    }

    /// `runtime_get_logs`（R-11 引擎 search：跨滚动段、时间序、脱敏在写入侧已完成）。
    pub fn get_logs(&self, query: &RuntimeLogQuery) -> AppResult<Vec<LogEntry>> {
        let root = self.workspace_root(query.workspace_id)?;
        self.logs.search(
            &root,
            &query.runtime_name,
            query.process_id,
            &query.filter,
        )
    }

    /// `runtime_clear_logs`。
    pub fn clear_logs(&self, query: &RuntimeLogQuery) -> AppResult<()> {
        let root = self.workspace_root(query.workspace_id)?;
        self.logs
            .clear(&root, &query.runtime_name, query.process_id)
    }

    /// 当前生效的调度并发上限（§66 可配置的读侧）。
    pub fn scheduler_config(&self) -> SchedulerConfig {
        SchedulerConfig {
            max_concurrent_builds: self.build_scheduler.max(),
            max_concurrent_resolves: self.resolve_scheduler.max(),
        }
    }

    /// 调整并发上限：立即作用于两个 permit 池并持久化。
    pub fn set_scheduler_config(&self, config: &SchedulerConfig) -> AppResult<()> {
        let config = config.sanitized();
        self.build_scheduler.set_max(config.max_concurrent_builds);
        self.resolve_scheduler
            .set_max(config.max_concurrent_resolves);
        config.save(&self.scheduler_config_path)?;
        log::info!("R-12: scheduler config updated: {:?}", config);
        Ok(())
    }

    // ------------------------------------------------------------------
    // §63 写侧命令的 TaskRequest 组装（提交走 T-05 TaskManager）
    // ------------------------------------------------------------------

    /// `runtime_build` / `runtime_start` / `runtime_stop` / `runtime_restart`
    /// 共用的单配置任务组装。
    pub fn operation_task_request(&self, req: &RuntimeOperationRequest, op: RuntimeOp) -> TaskRequest {
        TaskRequest {
            task_type: TaskType::Runtime {
                op,
                workspace_id: req.workspace_id,
                runtime_name: req.runtime_name.clone(),
                options: req.options.clone(),
            },
            repo_path: String::new(),
            repo_name: req.runtime_name.clone(),
        }
    }

    /// `runtime_resolve_dependencies` 的任务组装。
    pub fn resolve_task_request(&self, workspace_id: i64) -> TaskRequest {
        TaskRequest {
            task_type: TaskType::Runtime {
                op: RuntimeOp::ResolveDependencies,
                workspace_id,
                runtime_name: String::new(),
                options: RuntimeTaskOptions::default(),
            },
            repo_path: String::new(),
            repo_name: format!("workspace #{workspace_id} 依赖解析"),
        }
    }

    /// `runtime_start_environment`：为 workspace 下全部 Runtime 配置各组装
    /// 一个 Start 任务（批量提交共享 batch id，T-20 聚合）。
    ///
    /// Phase 1 口径（R-15 之前的 environment = 「该 workspace 的全部配置」）：
    /// 不含服务依赖排序与并行编排（R-15），并发由 Task Scheduler 限流。
    pub fn start_environment_requests(&self, workspace_id: i64) -> AppResult<Vec<TaskRequest>> {
        let conn = self.db.lock().unwrap();
        let configs = config::list_configs(&conn, workspace_id)?;
        drop(conn);
        Ok(configs
            .into_iter()
            .map(|summary| {
                self.operation_task_request(
                    &RuntimeOperationRequest {
                        workspace_id,
                        runtime_name: summary.name,
                        options: RuntimeTaskOptions::default(),
                    },
                    RuntimeOp::Start,
                )
            })
            .collect())
    }

    /// `runtime_stop_environment`：只为当前有活跃进程的配置组装 Stop 任务
    /// （没有活跃进程的配置不需要 Stop 任务，避免空转）。
    pub fn stop_environment_requests(&self, workspace_id: i64) -> AppResult<Vec<TaskRequest>> {
        let running: BTreeSet<String> = self
            .processes
            .list_processes(workspace_id)?
            .into_iter()
            .filter(|p| p.status.is_active())
            .map(|p| p.runtime_name)
            .collect();
        let conn = self.db.lock().unwrap();
        let configs = config::list_configs(&conn, workspace_id)?;
        drop(conn);
        Ok(configs
            .into_iter()
            .filter(|summary| running.contains(&summary.name))
            .map(|summary| {
                self.operation_task_request(
                    &RuntimeOperationRequest {
                        workspace_id,
                        runtime_name: summary.name,
                        options: RuntimeTaskOptions::default(),
                    },
                    RuntimeOp::Stop,
                )
            })
            .collect())
    }

    // ------------------------------------------------------------------
    // 启动对账（R-10 孤儿接管；应用启动时调用一次，best-effort）
    // ------------------------------------------------------------------

    /// 对所有 workspace 做进程对账：活着的孤儿接管、死去的落终态。
    /// 单个 workspace 失败不影响其余。
    pub fn reconcile_on_startup(self: &Arc<Self>) {
        let workspaces = {
            let conn = self.db.lock().unwrap();
            dao::list_workspaces(&conn)
        };
        let workspaces = match workspaces {
            Ok(w) => w,
            Err(e) => {
                log::warn!("R-12: startup reconcile skipped, cannot list workspaces: {e}");
                return;
            }
        };
        for ws in workspaces {
            let service = Arc::clone(self);
            let processes = Arc::clone(&service.processes);
            let path = ws.path.clone();
            std::thread::spawn(move || {
                match processes.reconcile_on_startup(ws.id) {
                    Ok(adopted) if !adopted.is_empty() => {
                        log::info!(
                            "R-12: workspace '{}' adopted {} orphan runtime process(es)",
                            path,
                            adopted.len()
                        );
                    }
                    Ok(_) => {}
                    Err(e) => log::warn!(
                        "R-12: startup reconcile failed for workspace '{}': {e}",
                        path
                    ),
                }
            });
        }
    }

    // ------------------------------------------------------------------
    // RuntimeTaskHandler（§65：任务执行体）
    // ------------------------------------------------------------------

    fn exec_build(
        &self,
        workspace_id: i64,
        runtime_name: &str,
        options: &RuntimeTaskOptions,
        cancel: &Arc<AtomicBool>,
    ) -> AppResult<Option<String>> {
        let at = Self::now();
        self.emit(
            EVENT_BUILD_STARTED,
            &BuildStartedPayload {
                workspace_id,
                runtime_name: runtime_name.to_string(),
                op: RuntimeOp::Build,
                at: at.clone(),
            },
        );
        self.emit(
            EVENT_BUILD_PROGRESS,
            &BuildProgressPayload {
                workspace_id,
                runtime_name: runtime_name.to_string(),
                process_id: None,
                stage: RuntimeStage::Building,
                at,
            },
        );

        let result = self.run_build(workspace_id, runtime_name, options, cancel);
        let at = Self::now();
        match result {
            Ok(outcome) => {
                self.emit(
                    EVENT_BUILD_COMPLETED,
                    &BuildCompletedPayload {
                        workspace_id,
                        runtime_name: runtime_name.to_string(),
                        process_id: None,
                        success: true,
                        duration_ms: Some(outcome.build_duration_ms as u64),
                        error: None,
                        at,
                    },
                );
                Ok(Some(format!(
                    "构建完成：{} 个模块，耗时 {}ms（策略 {}）",
                    outcome.modules_built.len(),
                    outcome.build_duration_ms,
                    outcome.strategy.as_str()
                )))
            }
            Err(error) => {
                self.emit(
                    EVENT_BUILD_COMPLETED,
                    &BuildCompletedPayload {
                        workspace_id,
                        runtime_name: runtime_name.to_string(),
                        process_id: None,
                        success: false,
                        duration_ms: None,
                        error: Some(error.to_string()),
                        at,
                    },
                );
                Err(error)
            }
        }
    }

    /// Build-only 任务直接驱动 R-09 流水线（不经 Process Manager，
    /// 无进程行、无日志会话；输出行进 RingTail 仅供错误上下文）。
    /// 构建期间不持有 DB 锁（execute_build 按阶段自行加锁，R-12）。
    fn run_build(
        &self,
        workspace_id: i64,
        runtime_name: &str,
        options: &RuntimeTaskOptions,
        cancel: &Arc<AtomicBool>,
    ) -> AppResult<crate::runtime::build::BuildOutcome> {
        let workspace_root = self.workspace_root(workspace_id)?;
        let request = BuildRequest {
            workspace_id,
            runtime_name: runtime_name.to_string(),
            options: build_options_of(options),
        };
        let mut sink = RingTail::new();
        execute_build(
            &self.db,
            &workspace_root,
            &self.graph_cache,
            &self.closure_cache,
            &self.build_scheduler,
            &*self.maven_runner,
            &request,
            &mut sink,
            Some(cancel),
        )
    }

    fn exec_start(
        &self,
        workspace_id: i64,
        runtime_name: &str,
        options: &RuntimeTaskOptions,
        cancel: &Arc<AtomicBool>,
    ) -> AppResult<Option<String>> {
        // §65 阶段事件（build_started → build_progress* → build_completed →
        // process_started → health_changed）由桥接从生命周期迁移推导。
        let _watch = CancelWatch::start(&self.processes, workspace_id, runtime_name, cancel);
        let info = self
            .processes
            .start(workspace_id, runtime_name, start_options_of(options))?;
        Ok(Some(format!(
            "'{}' 已启动（pid {:?}，端口 {:?}）",
            runtime_name, info.pid, info.ports
        )))
    }

    fn exec_stop(&self, workspace_id: i64, runtime_name: &str) -> AppResult<Option<String>> {
        match self.processes.stop_runtime(workspace_id, runtime_name, None)? {
            Some(info) => Ok(Some(format!(
                "'{}' 已停止（进程记录 #{}，状态 {}）",
                runtime_name,
                info.process_id,
                info.status.as_str()
            ))),
            None => Ok(Some(format!("'{}' 没有运行中的进程", runtime_name))),
        }
    }

    fn exec_restart(
        &self,
        workspace_id: i64,
        runtime_name: &str,
        options: &RuntimeTaskOptions,
        cancel: &Arc<AtomicBool>,
    ) -> AppResult<Option<String>> {
        self.emit(
            EVENT_RESTART_STARTED,
            &RestartStartedPayload {
                workspace_id,
                runtime_name: runtime_name.to_string(),
                at: Self::now(),
            },
        );
        let _watch = CancelWatch::start(&self.processes, workspace_id, runtime_name, cancel);
        let result = self
            .processes
            .restart(workspace_id, runtime_name, start_options_of(options));
        let (success, error) = match &result {
            Ok(_) => (true, None),
            Err(e) => (false, Some(e.to_string())),
        };
        self.emit(
            EVENT_RESTART_COMPLETED,
            &RestartCompletedPayload {
                workspace_id,
                runtime_name: runtime_name.to_string(),
                success,
                error,
                at: Self::now(),
            },
        );
        let info = result?;
        Ok(Some(format!(
            "'{}' 已重启（pid {:?}）",
            runtime_name, info.pid
        )))
    }

    /// §63 `resolve_dependencies`：发现 + 索引同步，全程本地（全局约束 §10
    /// 网络边界；远程解析发生在 Build，不在此）。
    fn exec_resolve(&self, workspace_id: i64, cancel: &Arc<AtomicBool>) -> AppResult<Option<String>> {
        // §66：Dependency Resolve 并发限流（默认 4）；排队可取消。
        let _permit = self
            .resolve_scheduler
            .acquire_cancelable(cancel)
            .ok_or_else(|| AppError::Task("依赖解析已取消（排队等待解析位时）".into()))?;

        let (root, scan_depth) = {
            let conn = self.db.lock().unwrap();
            let ws = dao::get_workspace(&conn, workspace_id)?;
            (PathBuf::from(ws.path), ws.scan_depth.max(1) as usize)
        };

        // 同步前的已知项目集合（增量发现 diff 用；首次同步为空）。
        let known_paths: BTreeSet<String> = {
            let conn = self.db.lock().unwrap();
            maven::query_dependency_graph(&conn, workspace_id)
                .map(|g| {
                    g.projects
                        .iter()
                        .map(|p| p.path.to_string_lossy().to_string())
                        .collect()
                })
                .unwrap_or_default()
        };

        if cancel.load(Ordering::Relaxed) {
            return Err(AppError::Task("依赖解析已取消".into()));
        }
        let discovery = maven::discover_poms(
            &root,
            scan_depth,
            Some(&self.pom_cache),
            Some(cancel),
        );
        if cancel.load(Ordering::Relaxed) {
            return Err(AppError::Task("依赖解析已取消".into()));
        }

        let local_repository = maven::settings::resolve_local_repository(None);
        let stats = {
            let mut conn = self.db.lock().unwrap();
            maven::sync_workspace_index(&mut conn, workspace_id, &discovery, &local_repository)?
        };
        // 索引已变：图 / 闭包缓存失效（下次读取重建）。
        self.graph_cache.invalidate_all();
        self.closure_cache.invalidate_all();

        let graph = {
            let conn = self.db.lock().unwrap();
            maven::query_dependency_graph(&conn, workspace_id)?
        };

        // project_discovered：增量发现的项目逐个发（有上限，见常量注释）。
        let new_projects: Vec<_> = discovery
            .projects
            .iter()
            .filter(|p| !known_paths.contains(&p.path.to_string_lossy().to_string()))
            .collect();
        if !known_paths.is_empty() && new_projects.len() <= MAX_PROJECT_DISCOVERED_EVENTS {
            for project in new_projects {
                self.emit(
                    EVENT_PROJECT_DISCOVERED,
                    &ProjectDiscoveredPayload {
                        workspace_id,
                        path: display_path(&root, &project.path),
                        coordinates: format!(
                            "{}:{}:{}",
                            project.group_id, project.artifact_id, project.version
                        ),
                        packaging: project.packaging.clone(),
                        at: Self::now(),
                    },
                );
            }
        }

        self.emit(
            EVENT_DEPENDENCY_RESOLVED,
            &DependencyResolvedPayload {
                workspace_id,
                projects: graph.projects.len(),
                dependencies: graph.dependencies.len(),
                source_mappings: graph.source_mappings.len(),
                inserted: stats.inserted,
                updated: stats.updated,
                removed: stats.removed,
                elapsed_ms: discovery.elapsed_ms as u64,
                at: Self::now(),
            },
        );

        Ok(Some(format!(
            "依赖解析完成：{} 个项目 / {} 条依赖边 / {} 条源码映射（新增 {}、更新 {}、移除 {}，{}ms）",
            graph.projects.len(),
            graph.dependencies.len(),
            graph.source_mappings.len(),
            stats.inserted,
            stats.updated,
            stats.removed,
            discovery.elapsed_ms
        )))
    }
}

impl RuntimeTaskHandler for RuntimeService {
    fn execute(&self, task_type: &TaskType, cancel: Arc<AtomicBool>) -> AppResult<Option<String>> {
        let TaskType::Runtime {
            op,
            workspace_id,
            runtime_name,
            options,
        } = task_type
        else {
            return Err(AppError::Task(format!(
                "RuntimeTaskHandler 收到非 Runtime 任务：{task_type:?}"
            )));
        };
        log::info!(
            "R-12: runtime task {:?} '{}' (workspace #{}) started",
            op,
            runtime_name,
            workspace_id
        );
        match op {
            RuntimeOp::Build => self.exec_build(*workspace_id, runtime_name, options, &cancel),
            RuntimeOp::Start => self.exec_start(*workspace_id, runtime_name, options, &cancel),
            RuntimeOp::Stop => self.exec_stop(*workspace_id, runtime_name),
            RuntimeOp::Restart => self.exec_restart(*workspace_id, runtime_name, options, &cancel),
            RuntimeOp::ResolveDependencies => self.exec_resolve(*workspace_id, &cancel),
        }
    }
}

// ---------------------------------------------------------------------------
// 选项映射与取消 watcher
// ---------------------------------------------------------------------------

/// `RuntimeTaskOptions` → R-09 `BuildOptions`（未指定项走 Build 默认，
/// 对齐 IDEA Build 语义）。
pub fn build_options_of(options: &RuntimeTaskOptions) -> BuildOptions {
    let defaults = BuildOptions::default();
    BuildOptions {
        strategy: options.strategy,
        skip_tests: options.skip_tests.unwrap_or(defaults.skip_tests),
        offline: options.offline,
        ..defaults
    }
}

/// `RuntimeTaskOptions` → R-10 `StartOptions`。
pub fn start_options_of(options: &RuntimeTaskOptions) -> StartOptions {
    StartOptions {
        skip_build: options.skip_build,
        build_options: build_options_of(options),
        ..Default::default()
    }
}

/// Start / Restart 的取消 watcher：任务取消标志置位后，先走
/// `signal_build_cancel` 内存快路径杀掉进行中的 Maven 构建（不等 DB 锁），
/// 再走 `stop_runtime` 完成状态迁移与进程收尾；op 结束（Drop）前持续
/// 重试，覆盖「取消早于 start 注册句柄」的竞态。
struct CancelWatch {
    done: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl CancelWatch {
    fn start(
        processes: &Arc<RuntimeProcessManager>,
        workspace_id: i64,
        runtime_name: &str,
        cancel: &Arc<AtomicBool>,
    ) -> Self {
        let done = Arc::new(AtomicBool::new(false));
        let handle = {
            let processes = Arc::clone(processes);
            let cancel = Arc::clone(cancel);
            let done = Arc::clone(&done);
            let runtime_name = runtime_name.to_string();
            std::thread::spawn(move || loop {
                if done.load(Ordering::Relaxed) {
                    break;
                }
                if cancel.load(Ordering::Relaxed) {
                    processes.signal_build_cancel(workspace_id, &runtime_name);
                    if let Err(e) =
                        processes.stop_runtime(workspace_id, &runtime_name, Some(CANCEL_STOP_GRACE))
                    {
                        log::warn!("R-12: cancel-stop of '{runtime_name}' failed: {e}");
                    }
                }
                std::thread::sleep(CANCEL_WATCH_INTERVAL);
            })
        };
        Self {
            done,
            handle: Some(handle),
        }
    }
}

impl Drop for CancelWatch {
    fn drop(&mut self) {
        self.done.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            // watcher 可能正阻塞在 stop_runtime 的 DB 锁上：构建取消快路径
            // 已保证构建尽快退出、锁尽快释放，join 有界。
            let _ = handle.join();
        }
    }
}

/// 相对 workspace 根展示 POM 所在目录（事件 payload 用）。
fn display_path(root: &std::path::Path, pom_path: &std::path::Path) -> String {
    let relative = pom_path
        .strip_prefix(root)
        .unwrap_or(pom_path)
        .to_string_lossy()
        .to_string();
    relative
        .strip_suffix("/pom.xml")
        .unwrap_or(&relative)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::task::RuntimeOp;
    use crate::process::streaming::{OutputStream, StreamingExit};
    use crate::runtime::build::runner::{FakeMavenRunner, FakeRun};
    use crate::runtime::build::{BuildOutputSink, RunStrategy};
    use crate::runtime::config::{CreateRuntimeConfigRequest, RuntimeApplicationConfig};
    use crate::runtime::events::{
        VecEmitter, EVENT_HEALTH_CHANGED, EVENT_PROCESS_STARTED, EVENT_PROCESS_STOPPED,
    };
    use crate::runtime::launch::launcher::{FakeBehavior, FakeLaunch, FakeLaunchRunner};
    use crate::runtime::launch::LifecycleStatus;
    use std::path::Path;
    use std::sync::atomic::AtomicUsize;
    use std::time::Instant;
    // --------------------------------------------------------------
    // fixtures（对齐 R-10 manager 测试的 MavenFixture 模式）
    // --------------------------------------------------------------

    fn write(path: &Path, content: &str) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, content).unwrap();
    }

    struct Fixture {
        root: PathBuf,
        db: Arc<Mutex<Connection>>,
        workspace_id: i64,
    }

    /// 单仓 parent(pom) + lib(jar) + app(jar→lib)，同步依赖图索引 + 配置。
    fn maven_fixture(tag: &str) -> Fixture {
        let root = std::env::temp_dir().join(format!(
            "gw_r12_{tag}_{}",
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
        crate::db::dao::upsert_repositories_batch(
            &mut conn,
            workspace_id,
            &[crate::models::repository::ScannedRepo {
                path: root.join("repo").to_string_lossy().to_string(),
                name: "repo".into(),
                relative_path: "repo".into(),
                git_dir_mtime: None,
            }],
        )
        .unwrap();
        let discovery = maven::discover_poms(&root, 5, None, None);
        assert!(discovery.errors.is_empty(), "{:?}", discovery.errors);
        maven::sync_workspace_index(&mut conn, workspace_id, &discovery, &root.join("m2"))
            .unwrap();

        config::create_config(
            &conn,
            &CreateRuntimeConfigRequest {
                workspace_id,
                config: RuntimeApplicationConfig {
                    name: "app".into(),
                    project: root.join("repo/app/pom.xml").to_string_lossy().to_string(),
                    main_class: Some("com.example.app.Application".into()),
                    ..Default::default()
                },
            },
        )
        .unwrap();
        Fixture {
            root,
            db: Arc::new(Mutex::new(conn)),
            workspace_id,
        }
    }

    fn test_service(
        fixture: &Fixture,
        emitter: Arc<VecEmitter>,
        maven_runner: Arc<dyn MavenRunner>,
        launch_runner: Arc<dyn LaunchRunner>,
    ) -> Arc<RuntimeService> {
        RuntimeService::assemble(
            Arc::clone(&fixture.db),
            emitter,
            Arc::new(PomCache::new()),
            SchedulerConfig::default(),
            fixture.root.join("scheduler.json"),
            RuntimeServiceOverrides {
                maven_runner,
                launch_runner,
                sample_interval: Duration::from_millis(50),
                ..Default::default()
            },
        )
    }

    fn runtime_task(op: RuntimeOp, workspace_id: i64, name: &str, options: RuntimeTaskOptions) -> TaskType {
        TaskType::Runtime {
            op,
            workspace_id,
            runtime_name: name.into(),
            options,
        }
    }

    // --------------------------------------------------------------
    // tests
    // --------------------------------------------------------------

    /// §63/§65：Build 任务经 handler 执行成功，事件序列
    /// build_started → build_progress(building) → build_completed(success)。
    #[test]
    fn build_op_succeeds_and_emits_event_sequence() {
        let fixture = maven_fixture("build");
        let emitter = Arc::new(VecEmitter::default());
        let maven = Arc::new(FakeMavenRunner::successful());
        let service = test_service(
            &fixture,
            Arc::clone(&emitter),
            maven,
            Arc::new(FakeLaunchRunner::staying_alive()),
        );

        let task = runtime_task(
            RuntimeOp::Build,
            fixture.workspace_id,
            "app",
            RuntimeTaskOptions {
                strategy: Some(RunStrategy::MavenRun),
                ..Default::default()
            },
        );
        let cancel = Arc::new(AtomicBool::new(false));
        let output = service.execute(&task, cancel).unwrap();
        assert!(output.unwrap().contains("构建完成"));

        assert_eq!(
            emitter.names(),
            vec![EVENT_BUILD_STARTED, EVENT_BUILD_PROGRESS, EVENT_BUILD_COMPLETED]
        );
        let completed = &emitter.collected()[2];
        assert_eq!(completed.payload["success"], serde_json::json!(true));
        assert!(completed.payload["durationMs"].is_number());
    }

    /// §65 验收主路径：Start 任务完整生命周期事件序列
    /// build_* → process_* → health_changed，且阶段一一对应
    /// Preparing/Resolving/Building/Starting。
    #[test]
    fn start_op_emits_full_lifecycle_sequence() {
        let fixture = maven_fixture("start");
        let emitter = Arc::new(VecEmitter::default());
        let service = test_service(
            &fixture,
            Arc::clone(&emitter),
            Arc::new(FakeMavenRunner::successful()),
            Arc::new(FakeLaunchRunner::staying_alive()),
        );

        let task = runtime_task(
            RuntimeOp::Start,
            fixture.workspace_id,
            "app",
            RuntimeTaskOptions {
                strategy: Some(RunStrategy::MavenRun),
                ..Default::default()
            },
        );
        let cancel = Arc::new(AtomicBool::new(false));
        service.execute(&task, cancel).unwrap();

        let names = emitter.names();
        assert_eq!(
            names,
            vec![
                EVENT_BUILD_STARTED,     // Preparing
                EVENT_BUILD_PROGRESS,    // preparing
                EVENT_BUILD_PROGRESS,    // resolving
                EVENT_BUILD_PROGRESS,    // building
                EVENT_BUILD_COMPLETED,   // 构建阶段结束
                EVENT_BUILD_PROGRESS,    // starting
                EVENT_PROCESS_STARTED,   // Running
                EVENT_HEALTH_CHANGED,    // up
            ]
        );
        let stages: Vec<String> = emitter
            .collected()
            .iter()
            .filter(|e| e.name == EVENT_BUILD_PROGRESS)
            .map(|e| e.payload["stage"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(stages, vec!["preparing", "resolving", "building", "starting"]);

        // 进程行进入 Running。
        let info = service
            .process_status(service.list_processes(fixture.workspace_id).unwrap()[0].process_id)
            .unwrap()
            .unwrap();
        assert_eq!(info.status, LifecycleStatus::Running);
    }

    /// §64/§66：Stop 任务停掉运行中的应用，事件
    /// process_stopped + health_changed(down) 各一次。
    #[test]
    fn stop_op_stops_running_app() {
        let fixture = maven_fixture("stop");
        let emitter = Arc::new(VecEmitter::default());
        let service = test_service(
            &fixture,
            Arc::clone(&emitter),
            Arc::new(FakeMavenRunner::successful()),
            Arc::new(FakeLaunchRunner::staying_alive()),
        );
        let cancel = Arc::new(AtomicBool::new(false));
        service
            .execute(
                &runtime_task(
                    RuntimeOp::Start,
                    fixture.workspace_id,
                    "app",
                    RuntimeTaskOptions {
                        strategy: Some(RunStrategy::MavenRun),
                        ..Default::default()
                    },
                ),
                cancel.clone(),
            )
            .unwrap();
        let before = emitter.names().len();

        service
            .execute(
                &runtime_task(RuntimeOp::Stop, fixture.workspace_id, "app", Default::default()),
                cancel,
            )
            .unwrap();

        let after: Vec<_> = emitter.names()[before..].to_vec();
        assert_eq!(
            after,
            vec![EVENT_PROCESS_STOPPED, EVENT_HEALTH_CHANGED]
        );
    }

    /// Restart 任务包裹 restart_started / restart_completed，内部 Start
    /// 的生命周期事件照常发出（skip-build 路径）。
    #[test]
    fn restart_op_wraps_start_with_restart_events() {
        let fixture = maven_fixture("restart");
        let emitter = Arc::new(VecEmitter::default());
        let service = test_service(
            &fixture,
            Arc::clone(&emitter),
            Arc::new(FakeMavenRunner::successful()),
            Arc::new(FakeLaunchRunner::staying_alive()),
        );
        let cancel = Arc::new(AtomicBool::new(false));
        let start_task = runtime_task(
            RuntimeOp::Start,
            fixture.workspace_id,
            "app",
            RuntimeTaskOptions {
                strategy: Some(RunStrategy::MavenRun),
                ..Default::default()
            },
        );
        service.execute(&start_task, cancel.clone()).unwrap();
        let before = emitter.names().len();

        service
            .execute(
                &runtime_task(RuntimeOp::Restart, fixture.workspace_id, "app", Default::default()),
                cancel,
            )
            .unwrap();

        let after: Vec<_> = emitter.names()[before..].to_vec();
        assert_eq!(after.first(), Some(&EVENT_RESTART_STARTED));
        assert_eq!(after.last(), Some(&EVENT_RESTART_COMPLETED));
        assert!(after.contains(&EVENT_PROCESS_STOPPED));
        assert!(after.contains(&EVENT_PROCESS_STARTED));
        assert_eq!(
            emitter.collected().last().unwrap().payload["success"],
            serde_json::json!(true)
        );
    }

    /// §66 验收：取消进行中的 Start —— 构建中的 Maven 被取消快路径终止，
    /// 进程行落到终态，任务以错误返回（worker 会标记 Cancelled）。
    #[test]
    fn cancel_during_start_aborts_build_and_finalizes() {
        let fixture = maven_fixture("cancel");
        let emitter = Arc::new(VecEmitter::default());
        // 构建挂起直到取消标志置位（FakeRun.duration 以 10ms 粒度检查取消）。
        let maven = Arc::new(FakeMavenRunner::new(vec![FakeRun {
            duration: Some(Duration::from_secs(30)),
            ..Default::default()
        }]));
        let service = test_service(
            &fixture,
            Arc::clone(&emitter),
            maven,
            Arc::new(FakeLaunchRunner::staying_alive()),
        );
        let task = runtime_task(
            RuntimeOp::Start,
            fixture.workspace_id,
            "app",
            RuntimeTaskOptions {
                strategy: Some(RunStrategy::MavenRun),
                ..Default::default()
            },
        );
        let cancel = Arc::new(AtomicBool::new(false));

        let started = Instant::now();
        let cancel2 = Arc::clone(&cancel);
        let canceller = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(200));
            cancel2.store(true, Ordering::Relaxed);
        });
        let result = service.execute(&task, cancel);
        canceller.join().unwrap();

        // 取消语义（R-10 设计）：watcher 先 signal_build_cancel 杀 Maven 构建，
        // 再 stop_runtime 置 Stopping —— start 以停止语义收尾返回 Ok
        // （若 Stopping 尚未可见则走 abort 路径返回 Err）。任务层的最终状态
        // 由 worker 的 cancel flag 兜底标记为 Cancelled（worker.rs 收尾检查）。
        let _ = &result; // Ok/Err 皆可，终态以进程行为准
        assert!(
            started.elapsed() < Duration::from_secs(15),
            "cancel must abort promptly, took {:?}",
            started.elapsed()
        );
        let rows = service.list_processes(fixture.workspace_id).unwrap();
        assert!(
            rows[0].status.is_terminal(),
            "row must be terminal after cancel, got {:?}",
            rows[0].status
        );
        assert!(
            matches!(
                rows[0].status,
                LifecycleStatus::Stopped | LifecycleStatus::Failed
            ),
            "cancelled start must end Stopped (stop semantics) or Failed (abort)"
        );
        // 构建确实被终止：build_completed(failure) 或 process_stopped 必居其一。
        let names = emitter.names();
        assert!(
            names.contains(&EVENT_BUILD_COMPLETED) || names.contains(&EVENT_PROCESS_STOPPED),
            "expected terminal events, got {names:?}"
        );
    }

    /// §63/§64：ResolveDependencies 同步索引并发 dependency_resolved 汇总；
    /// 首次全量发现不发 project_discovered 洪泛。
    #[test]
    fn resolve_op_syncs_index_and_emits_summary() {
        let fixture = maven_fixture("resolve");
        let emitter = Arc::new(VecEmitter::default());
        let service = test_service(
            &fixture,
            Arc::clone(&emitter),
            Arc::new(FakeMavenRunner::successful()),
            Arc::new(FakeLaunchRunner::staying_alive()),
        );
        // 清空索引，模拟「首次解析」由本任务完成。
        {
            let conn = fixture.db.lock().unwrap();
            conn.execute("DELETE FROM maven_dependencies", []).unwrap();
            conn.execute("DELETE FROM maven_modules", []).unwrap();
            conn.execute("DELETE FROM maven_source_mappings", []).unwrap();
            conn.execute("DELETE FROM maven_projects", []).unwrap();
        }

        let task = runtime_task(
            RuntimeOp::ResolveDependencies,
            fixture.workspace_id,
            "",
            Default::default(),
        );
        let output = service
            .execute(&task, Arc::new(AtomicBool::new(false)))
            .unwrap();
        assert!(output.unwrap().contains("依赖解析完成"));

        let names = emitter.names();
        assert_eq!(names, vec![EVENT_DEPENDENCY_RESOLVED]);
        let payload = &emitter.collected()[0].payload;
        assert_eq!(payload["projects"], serde_json::json!(3));

        // 再同步一次：索引无变化，known 非空且无新项目 → 仍只有汇总事件。
        service
            .execute(&task, Arc::new(AtomicBool::new(false)))
            .unwrap();
        assert_eq!(
            emitter.names().iter().filter(|n| **n == EVENT_PROJECT_DISCOVERED).count(),
            0
        );

        // 查询侧：3 个项目、app → lib 依赖边。
        let projects = service.list_projects(fixture.workspace_id).unwrap();
        assert_eq!(projects.len(), 3);
        let inspection = service.inspect_project(fixture.workspace_id, "app").unwrap();
        assert_eq!(inspection.dependencies.len(), 1);
        let graph = service.dependency_graph(fixture.workspace_id, None, None).unwrap();
        assert!(!graph.truncated);
        assert_eq!(graph.total_dependencies, graph.dependencies.len());
    }

    /// 选项映射：skip_tests 缺省跟随 BuildOptions 默认（true），显式 false 生效。
    #[test]
    fn options_map_to_build_and_start_options() {
        let defaults = build_options_of(&RuntimeTaskOptions::default());
        assert!(defaults.skip_tests);
        let explicit = build_options_of(&RuntimeTaskOptions {
            skip_tests: Some(false),
            offline: true,
            ..Default::default()
        });
        assert!(!explicit.skip_tests);
        assert!(explicit.offline);

        let start = start_options_of(&RuntimeTaskOptions {
            skip_build: true,
            ..Default::default()
        });
        assert!(start.skip_build);
    }

    /// §66 可配置：set_scheduler_config 立即生效并持久化，重载后一致。
    #[test]
    fn scheduler_config_roundtrips_and_applies() {
        let fixture = maven_fixture("cfg");
        let emitter = Arc::new(VecEmitter::default());
        let service = test_service(
            &fixture,
            emitter,
            Arc::new(FakeMavenRunner::successful()),
            Arc::new(FakeLaunchRunner::staying_alive()),
        );
        assert_eq!(service.scheduler_config().max_concurrent_builds, 2);

        service
            .set_scheduler_config(&SchedulerConfig {
                max_concurrent_builds: 1,
                max_concurrent_resolves: 8,
            })
            .unwrap();
        assert_eq!(service.scheduler_config().max_concurrent_builds, 1);
        assert_eq!(service.scheduler_config().max_concurrent_resolves, 8);
        assert_eq!(service.build_scheduler.max(), 1);

        let loaded = SchedulerConfig::load(&fixture.root.join("scheduler.json"));
        assert_eq!(loaded.max_concurrent_builds, 1);
        assert_eq!(loaded.max_concurrent_resolves, 8);
    }

    /// environment 任务组装：start 覆盖全部配置；stop 只覆盖有活跃进程的。
    #[test]
    fn environment_requests_cover_configs() {
        let fixture = maven_fixture("env");
        let emitter = Arc::new(VecEmitter::default());
        let service = test_service(
            &fixture,
            emitter,
            Arc::new(FakeMavenRunner::successful()),
            Arc::new(FakeLaunchRunner::staying_alive()),
        );
        let start = service.start_environment_requests(fixture.workspace_id).unwrap();
        assert_eq!(start.len(), 1);
        assert!(matches!(
            start[0].task_type,
            TaskType::Runtime { op: RuntimeOp::Start, .. }
        ));

        // 未启动时 stop environment 为空；启动后覆盖。
        assert!(service
            .stop_environment_requests(fixture.workspace_id)
            .unwrap()
            .is_empty());
        let cancel = Arc::new(AtomicBool::new(false));
        service
            .execute(
                &runtime_task(
                    RuntimeOp::Start,
                    fixture.workspace_id,
                    "app",
                    RuntimeTaskOptions {
                        strategy: Some(RunStrategy::MavenRun),
                        ..Default::default()
                    },
                ),
                cancel,
            )
            .unwrap();
        assert_eq!(
            service
                .stop_environment_requests(fixture.workspace_id)
                .unwrap()
                .len(),
            1
        );
    }

    // --------------------------------------------------------------
    // §66 并发验收 + 真实 Maven 集成
    // --------------------------------------------------------------

    /// 记录并发峰值的 Maven runner：每次 run 睡 150ms 制造并发窗口。
    struct CountingRunner {
        running: AtomicUsize,
        max_seen: AtomicUsize,
    }

    impl MavenRunner for CountingRunner {
        fn resolve_maven(
            &self,
            _project_dir: &Path,
            local_repository: &Path,
        ) -> AppResult<crate::maven::ResolvedMaven> {
            Ok(crate::maven::ResolvedMaven {
                executable: crate::maven::MavenExecutable::new(
                    "fake-mvn",
                    crate::maven::MavenSource::System,
                    None,
                ),
                local_repository: local_repository.to_path_buf(),
                uses_wrapper: false,
            })
        }

        fn run(
            &self,
            _request: &crate::maven::MavenExecutionRequest,
            _env: &[(String, String)],
            sink: &mut dyn BuildOutputSink,
            _cancel: Option<&AtomicBool>,
            _timeout: Option<Duration>,
        ) -> AppResult<StreamingExit> {
            let current = self.running.fetch_add(1, Ordering::SeqCst) + 1;
            self.max_seen.fetch_max(current, Ordering::SeqCst);
            std::thread::sleep(Duration::from_millis(150));
            self.running.fetch_sub(1, Ordering::SeqCst);
            sink.on_line(OutputStream::Stdout, "BUILD SUCCESS");
            Ok(StreamingExit {
                exit_code: Some(0),
                timed_out: false,
                cancelled: false,
            })
        }
    }

    /// §66 验收：3 个并发 Build 任务经过共享 permit 池，并发 Maven 构建
    /// 峰值不超过 2，其余排队执行且全部成功。
    #[test]
    fn concurrent_builds_are_capped_by_scheduler() {
        let fixture = maven_fixture("conc");
        let emitter = Arc::new(VecEmitter::default());
        let counting = Arc::new(CountingRunner {
            running: AtomicUsize::new(0),
            max_seen: AtomicUsize::new(0),
        });
        let service = test_service(
            &fixture,
            emitter,
            counting.clone(),
            Arc::new(FakeLaunchRunner::staying_alive()),
        );

        // 三个配置分别指向 parent / lib / app。
        for (name, pom) in [
            ("cfg-parent", "repo/pom.xml"),
            ("cfg-lib", "repo/lib/pom.xml"),
            ("cfg-app", "repo/app/pom.xml"),
        ] {
            let conn = fixture.db.lock().unwrap();
            config::create_config(
                &conn,
                &CreateRuntimeConfigRequest {
                    workspace_id: fixture.workspace_id,
                    config: RuntimeApplicationConfig {
                        name: name.into(),
                        project: fixture.root.join(pom).to_string_lossy().to_string(),
                        ..Default::default()
                    },
                },
            )
            .unwrap();
        }

        let mut handles = Vec::new();
        for name in ["cfg-parent", "cfg-lib", "cfg-app"] {
            let service = Arc::clone(&service);
            handles.push(std::thread::spawn(move || {
                service.execute(
                    &runtime_task(
                        RuntimeOp::Build,
                        fixture.workspace_id,
                        name,
                        RuntimeTaskOptions {
                            strategy: Some(RunStrategy::MavenRun),
                            ..Default::default()
                        },
                    ),
                    Arc::new(AtomicBool::new(false)),
                )
            }));
        }
        for handle in handles {
            let result = handle.join().unwrap();
            assert!(result.is_ok(), "queued build must succeed: {result:?}");
        }
        assert_eq!(
            counting.max_seen.load(Ordering::SeqCst),
            2,
            "build concurrency must stay at the §66 cap"
        );
    }

    // ---- 真实 Maven 集成（Synthetic Reactor 走真实 mvn；无 mvn 时跳过并标注）----

    fn maven_available() -> bool {
        let maven = if cfg!(windows) { "mvn.cmd" } else { "mvn" };
        std::process::Command::new(maven)
            .arg("-version")
            .output()
            .is_ok()
    }

    /// 单仓 Spring Boot fixture（对齐 R-09 `setup_single_repo_boot`，
    /// 坐标换成 com.r12）：repo/(parent + lib + app)，app 依赖 lib +
    /// spring-boot-starter（外部依赖，靠 ~/.m2 缓存命中）。
    fn spring_boot_fixture(tag: &str) -> Fixture {
        let root = std::env::temp_dir().join(format!(
            "gw_r12_it_{tag}_{}",
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        std::fs::create_dir_all(&root).unwrap();
        write(
            &root.join("repo/pom.xml"),
            r#"<?xml version="1.0" encoding="UTF-8"?>
<project xmlns="http://maven.apache.org/POM/4.0.0">
  <modelVersion>4.0.0</modelVersion>
  <groupId>com.r12</groupId>
  <artifactId>r12-parent</artifactId>
  <version>1.0.0</version>
  <packaging>pom</packaging>
  <modules><module>lib</module><module>app</module></modules>
  <properties>
    <maven.compiler.source>17</maven.compiler.source>
    <maven.compiler.target>17</maven.compiler.target>
    <project.build.sourceEncoding>UTF-8</project.build.sourceEncoding>
  </properties>
  <dependencyManagement>
    <dependencies>
      <dependency>
        <groupId>org.springframework.boot</groupId>
        <artifactId>spring-boot-dependencies</artifactId>
        <version>3.2.5</version>
        <type>pom</type>
        <scope>import</scope>
      </dependency>
    </dependencies>
  </dependencyManagement>
</project>
"#,
        );
        write(
            &root.join("repo/lib/pom.xml"),
            r#"<?xml version="1.0" encoding="UTF-8"?>
<project xmlns="http://maven.apache.org/POM/4.0.0">
  <modelVersion>4.0.0</modelVersion>
  <parent><groupId>com.r12</groupId><artifactId>r12-parent</artifactId><version>1.0.0</version></parent>
  <artifactId>lib</artifactId>
</project>
"#,
        );
        write(
            &root.join("repo/lib/src/main/java/com/r12/lib/Lib.java"),
            "package com.r12.lib;\n\npublic final class Lib {\n    private Lib() {}\n    public static String greet() { return \"hi\"; }\n}\n",
        );
        write(
            &root.join("repo/app/pom.xml"),
            r#"<?xml version="1.0" encoding="UTF-8"?>
<project xmlns="http://maven.apache.org/POM/4.0.0">
  <modelVersion>4.0.0</modelVersion>
  <parent><groupId>com.r12</groupId><artifactId>r12-parent</artifactId><version>1.0.0</version></parent>
  <artifactId>app</artifactId>
  <dependencies>
    <dependency><groupId>com.r12</groupId><artifactId>lib</artifactId><version>1.0.0</version></dependency>
    <dependency><groupId>org.springframework.boot</groupId><artifactId>spring-boot-starter</artifactId></dependency>
  </dependencies>
</project>
"#,
        );
        write(
            &root.join("repo/app/src/main/java/com/r12/app/Application.java"),
            "package com.r12.app;\n\nimport org.springframework.boot.autoconfigure.SpringBootApplication;\n\n@SpringBootApplication\npublic class Application {\n    public static void main(String[] args) {\n        System.out.println(com.r12.lib.Lib.greet());\n    }\n}\n",
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
        crate::db::dao::upsert_repositories_batch(
            &mut conn,
            workspace_id,
            &[crate::models::repository::ScannedRepo {
                path: root.join("repo").to_string_lossy().to_string(),
                name: "repo".into(),
                relative_path: "repo".into(),
                git_dir_mtime: None,
            }],
        )
        .unwrap();
        let discovery = maven::discover_poms(&root, 6, None, None);
        assert!(discovery.errors.is_empty(), "{:?}", discovery.errors);
        maven::sync_workspace_index(&mut conn, workspace_id, &discovery, &root.join("m2"))
            .unwrap();

        config::create_config(
            &conn,
            &CreateRuntimeConfigRequest {
                workspace_id,
                config: RuntimeApplicationConfig {
                    name: "app".into(),
                    project: root.join("repo/app/pom.xml").to_string_lossy().to_string(),
                    main_class: Some("com.r12.app.Application".into()),
                    ..Default::default()
                },
            },
        )
        .unwrap();
        Fixture {
            root,
            db: Arc::new(Mutex::new(conn)),
            workspace_id,
        }
    }

    /// R-12 端到端：Build 任务驱动真实 mvn 走完 Synthetic Reactor 构建
    /// （ClasspathRun = compile + dependency:build-classpath），事件序列完整。
    #[test]
    fn build_op_with_real_maven_builds_synthetic_reactor() {
        if !maven_available() {
            eprintln!("R-12: no `mvn` on PATH; skipping real-maven integration test");
            return;
        }
        let fixture = spring_boot_fixture("realmvn");
        let emitter = Arc::new(VecEmitter::default());
        // 生产 runner：SpawningMavenRunner 驱动真实 mvn。
        let service = RuntimeService::assemble(
            Arc::clone(&fixture.db),
            emitter.clone(),
            Arc::new(PomCache::new()),
            SchedulerConfig::default(),
            fixture.root.join("scheduler.json"),
            RuntimeServiceOverrides::default(),
        );

        let task = runtime_task(
            RuntimeOp::Build,
            fixture.workspace_id,
            "app",
            RuntimeTaskOptions {
                strategy: Some(RunStrategy::ClasspathRun),
                ..Default::default()
            },
        );
        let output = service
            .execute(&task, Arc::new(AtomicBool::new(false)))
            .unwrap_or_else(|e| panic!("real maven build failed: {e}"));
        assert!(output.unwrap().contains("构建完成"));

        // Synthetic Reactor 只落在 .gitworkspace/（用户项目只读，全局约束 §2）。
        assert!(fixture.root.join(".gitworkspace/runtime/app").exists());
        assert!(!fixture.root.join("repo/.gitworkspace").exists());

        let names = emitter.names();
        assert_eq!(
            names,
            vec![EVENT_BUILD_STARTED, EVENT_BUILD_PROGRESS, EVENT_BUILD_COMPLETED]
        );
        assert_eq!(
            emitter.collected()[2].payload["success"],
            serde_json::json!(true)
        );

        let _ = std::fs::remove_dir_all(&fixture.root);
    }
}
