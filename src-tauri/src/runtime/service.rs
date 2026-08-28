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
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use tauri::AppHandle;

use crate::db::dao;
use crate::error::{AppError, AppResult};
use crate::maven::{
    self, DependencyEdge, DependencyGraphCache, MavenModuleLink, MavenProjectNode, PomCache,
    RuntimeClosure, RuntimeClosureCache, RuntimeScope, SourceMapping,
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
    EnvironmentCompletedPayload, EnvironmentProgressPayload, EnvironmentServiceOutcome,
    ProjectDiscoveredPayload, RestartCompletedPayload, RestartStartedPayload, RuntimeEmission,
    RuntimeEventEmitter, RuntimeStage, ServiceExecState, TauriRuntimeBridge, TauriRuntimeEmitter,
    EVENT_BUILD_COMPLETED, EVENT_BUILD_PROGRESS, EVENT_BUILD_STARTED, EVENT_DEPENDENCY_RESOLVED,
    EVENT_ENVIRONMENT_COMPLETED, EVENT_ENVIRONMENT_PROGRESS, EVENT_PROJECT_DISCOVERED,
    EVENT_RESTART_COMPLETED, EVENT_RESTART_STARTED,
};
use crate::runtime::launch::launcher::{LaunchRunner, SystemLaunchRunner};
use crate::runtime::launch::manager::{
    RuntimeProcessDeps, RuntimeProcessManager, DEFAULT_SAMPLE_INTERVAL,
};
use crate::runtime::launch::{RuntimeProcessInfo, StartOptions};
use crate::runtime::logs::{LogEntry, LogExportOutcome, LogFilter, RuntimeLogEngine};
use crate::runtime::script_approval::{self, ScriptApproval, ScriptApprovalStore};
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

/// `runtime_get_closure`（R-13）的返回：给定 Scope 下的 Runtime Closure
/// 预览（R-03 §14/§15），`cache_hit` 标记是否命中 graph fingerprint 缓存。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClosurePreview {
    pub closure: RuntimeClosure,
    pub cache_hit: bool,
}

/// 按 path / artifactId / groupId:artifactId 匹配项目（与 R-09
/// `find_root_project` 同口径；R-13 供 closure_preview 复用）。
///
/// 路径匹配对 Windows 分隔符不敏感（R-14 修复：R-02 索引路径统一为正斜杠，
/// 配置/查询参数可能是反斜杠）。
fn find_project<'a>(projects: &'a [MavenProjectNode], project: &str) -> Option<&'a MavenProjectNode> {
    let needle = project.replace('\\', "/");
    projects.iter().find(|p| {
        let path = p.path.to_string_lossy().replace('\\', "/");
        path == needle
            || path.ends_with(&needle)
            || p.coordinates.artifact_id == project
            || format!("{}:{}", p.coordinates.group_id, p.coordinates.artifact_id) == project
    })
}

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
    script_approvals: ScriptApprovalStore,
    /// R-16 健康检查引擎（与进程管理器共享实例）。
    health: Arc<crate::runtime::health::HealthEngine>,
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
            script_approval::script_approvals_path(),
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
        script_approvals_path: PathBuf,
        overrides: RuntimeServiceOverrides,
    ) -> Arc<Self> {
        let build_scheduler = Arc::new(BuildScheduler::new(scheduler_config.max_concurrent_builds));
        let resolve_scheduler =
            Arc::new(BuildScheduler::new(scheduler_config.max_concurrent_resolves));
        let graph_cache = Arc::new(DependencyGraphCache::new());
        let closure_cache = Arc::new(RuntimeClosureCache::new());
        let bridge = Arc::new(TauriRuntimeBridge::new(Arc::clone(&emitter), Arc::clone(&db)));

        // R-16：健康检查引擎与进程管理器共享同一 emitter / DB。
        let health = crate::runtime::health::HealthEngine::new(
            Arc::clone(&db),
            Arc::clone(&emitter),
        );

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
                script_approvals: ScriptApprovalStore::new(script_approvals_path.clone()),
                health: Some(Arc::clone(&health)),
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
            script_approvals: ScriptApprovalStore::new(script_approvals_path),
            health,
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
        let node = find_project(&graph.projects, project).ok_or_else(|| {
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

    /// `runtime_get_closure`（R-13）：按给定 Scope 计算闭包预览，供
    /// Runtime Scope 视图使用（R-03 fingerprint 缓存热路径）。
    pub fn closure_preview(
        &self,
        workspace_id: i64,
        project: &str,
        scope: &RuntimeScope,
    ) -> AppResult<ClosurePreview> {
        let conn = self.db.lock().unwrap();
        let graph = self.graph_cache.get_or_load(&conn, workspace_id)?.graph;
        let node = find_project(&graph.projects, project).ok_or_else(|| {
            AppError::ProjectNotFound(format!(
                "项目 '{project}' 不在 workspace #{workspace_id} 的 Maven 索引中；\
                 请先执行依赖解析（runtime.resolve_dependencies）"
            ))
        })?;
        let lookup = self
            .closure_cache
            .get_or_compute(&graph, node.project_id, scope)?;
        Ok(ClosurePreview {
            closure: lookup.closure,
            cache_hit: lookup.cache_hit,
        })
    }

    /// `runtime_list_processes`。
    pub fn list_processes(&self, workspace_id: i64) -> AppResult<Vec<RuntimeProcessInfo>> {
        self.processes.list_processes(workspace_id)
    }

    /// R-17：watch 引擎装配所需的共享设施（进程管理器 / 图缓存 / 闭包缓存）。
    /// 返回的是与 RuntimeService 内部**同一批**实例（缓存与限流全局一致），
    /// 仅供 lib.rs 装配 `RuntimeWatchEngine` 使用。
    pub fn watch_shared_parts(
        &self,
    ) -> (
        Arc<RuntimeProcessManager>,
        Arc<DependencyGraphCache>,
        Arc<RuntimeClosureCache>,
    ) {
        (
            Arc::clone(&self.processes),
            Arc::clone(&self.graph_cache),
            Arc::clone(&self.closure_cache),
        )
    }

    /// `runtime_process_status`。
    pub fn process_status(&self, process_id: i64) -> AppResult<Option<RuntimeProcessInfo>> {
        self.processes.get_process(process_id)
    }

    /// R-16 `runtime_get_health`：单进程健康快照（无探针为 None）。
    pub fn get_health(
        &self,
        process_id: i64,
    ) -> Option<crate::runtime::health::HealthSnapshot> {
        self.health.snapshot(process_id)
    }

    /// R-16 `runtime_list_health`：workspace 下全部探针快照。
    pub fn list_health(
        &self,
        workspace_id: i64,
    ) -> Vec<crate::runtime::health::HealthSnapshot> {
        self.health.snapshots(workspace_id)
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

    /// R-13 `runtime_export_logs`：导出到用户选择的目标文件（R-11 §36，
    /// 与 `search` 同一过滤管道，导出内容与显示一致）。
    pub fn export_logs(
        &self,
        query: &RuntimeLogQuery,
        dest: &str,
    ) -> AppResult<LogExportOutcome> {
        let root = self.workspace_root(query.workspace_id)?;
        self.logs.export(
            &root,
            &query.runtime_name,
            query.process_id,
            &query.filter,
            Path::new(dest),
        )
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
    // R-14 §75 Command Safety：Pre/Post Build Script 确认状态
    // ------------------------------------------------------------------

    /// `runtime_get_script_approvals`：全部脚本确认记录（UI 管理列表）。
    pub fn script_approval_list(&self) -> Vec<ScriptApproval> {
        self.script_approvals.list()
    }

    /// `runtime_approve_script`：确认一条脚本。后端从配置读脚本内容、
    /// 计算内容哈希并生成预览——哈希必然与流水线校验的一致。
    /// 返回确认记录（`is_new` 语义由调用方按需忽略）。
    pub fn approve_script(
        &self,
        workspace_id: i64,
        runtime_name: &str,
        script_type: &str,
    ) -> AppResult<ScriptApproval> {
        if script_type != "pre" && script_type != "post" {
            return Err(AppError::RuntimeConfig(format!(
                "script_type 必须是 pre / post，收到 '{script_type}'"
            )));
        }
        let config = {
            let conn = self.db.lock().unwrap();
            config::get_config(&conn, workspace_id, runtime_name)?
        };
        let script = match script_type {
            "pre" => config.pre_build_script.as_deref(),
            _ => config.post_build_script.as_deref(),
        }
        .ok_or_else(|| {
            AppError::RuntimeConfig(format!(
                "Runtime '{runtime_name}' 没有配置 {script_type}_build_script"
            ))
        })?;
        let hash = script_approval::script_hash(script);
        let preview = script_approval::script_preview(script);
        self.script_approvals
            .approve(workspace_id, runtime_name, script_type, &hash, &preview)?;
        Ok(ScriptApproval {
            workspace_id,
            runtime_name: runtime_name.to_string(),
            script_type: script_type.to_string(),
            script_hash: hash,
            preview,
            approved_at: chrono::Utc::now().to_rfc3339(),
            last_executed_at: None,
        })
    }

    /// `runtime_reset_script_approvals`：按范围撤销确认（「不再询问」可重置）。
    /// 返回删除条数。
    pub fn reset_script_approvals(
        &self,
        workspace_id: Option<i64>,
        runtime_name: Option<&str>,
    ) -> AppResult<usize> {
        self.script_approvals.reset(workspace_id, runtime_name)
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
    // R-15 §38/§39/§40：环境编排（Start / Stop Environment）
    // ------------------------------------------------------------------

    /// `runtime_start_named_environment` 的任务组装（环境名放 `runtime_name`
    /// 字段；任务面板显示为「环境 <name>」）。
    pub fn named_environment_task_request(
        &self,
        workspace_id: i64,
        environment: &str,
        op: RuntimeOp,
    ) -> TaskRequest {
        TaskRequest {
            task_type: TaskType::Runtime {
                op,
                workspace_id,
                runtime_name: environment.to_string(),
                options: RuntimeTaskOptions::default(),
            },
            repo_path: String::new(),
            repo_name: format!("环境 {environment}"),
        }
    }

    fn emit_environment_progress(
        &self,
        workspace_id: i64,
        environment: &str,
        service: &str,
        state: ServiceExecState,
        detail: Option<String>,
    ) {
        self.emit(
            EVENT_ENVIRONMENT_PROGRESS,
            &EnvironmentProgressPayload {
                workspace_id,
                environment: environment.to_string(),
                service: service.to_string(),
                state,
                detail,
                at: Self::now(),
            },
        );
    }

    /// R-16 就绪门限：等待服务 Healthy（或就绪超时放行）。
    ///
    /// - 有探针：轮询健康快照，`Healthy` 即就绪；超时（默认 60s，可按服务
    ///   覆盖）按警告放行（应用仍在运行，只是未达 Healthy）。
    /// - 无探针：`processes.start` 返回时进程已确认 Running，视为就绪
    ///   （快照缺位时的首个轮询窗口即返回超时放行语义，秒级）。
    fn wait_service_ready(
        &self,
        _workspace_id: i64,
        _runtime_name: &str,
        process_id: i64,
        timeout: Duration,
    ) -> Result<String, String> {
        let deadline = Instant::now() + timeout;
        let started = Instant::now();
        // 无探针服务：processes.start 返回时已确认 Running，立即就绪
        // （R-15 第一版「固定顺序占位」的退化路径；探针门限归 R-16 引擎）。
        if !self.health.has_monitor(process_id) {
            // 给探针注册留一个小窗口（Running 迁移与 monitor spawn 同线程序，
            // 此处只是防御性二次确认）。
            std::thread::sleep(Duration::from_millis(150));
            if !self.health.has_monitor(process_id) {
                return Ok("进程 Running（未配置探针，跳过就绪等待）".into());
            }
        }
        loop {
            // 进程先死 → 就绪等待失败（编排按失败处理，依赖分支跳过）。
            if let Ok(Some(info)) = self.processes.get_process(process_id) {
                if info.status.is_terminal() {
                    return Err(format!(
                        "服务在就绪等待期间退出（状态 {}）",
                        info.status.as_str()
                    ));
                }
            }
            if let Some(snapshot) = self.health.snapshot(process_id) {
                match snapshot.phase {
                    crate::runtime::events::HealthStatus::Healthy => {
                        return Ok(format!("Healthy（{}ms）", started.elapsed().as_millis()));
                    }
                    crate::runtime::events::HealthStatus::Stopped => {
                        return Err("探针判定服务已停止".into());
                    }
                    _ => {}
                }
            }
            if Instant::now() >= deadline {
                return Ok(format!(
                    "就绪等待超时（{:?}），进程仍在运行，放行依赖分支",
                    timeout
                ));
            }
            std::thread::sleep(Duration::from_millis(250));
        }
    }

    /// §38 Start Environment：拓扑分波，波内并行启动（构建并发受 §66
    /// Build permit 池约束），波间严格串行；依赖失败的服务及其下游标记
    /// Skipped（部分失败语义：不影响无依赖分支）。
    fn exec_start_environment(
        &self,
        workspace_id: i64,
        environment_name: &str,
        cancel: &Arc<AtomicBool>,
    ) -> AppResult<Option<String>> {
        let environment = {
            let conn = self.db.lock().unwrap();
            let root = config::workspace_root(&conn, workspace_id)?;
            let environment =
                crate::runtime::environment::get_environment(&root, environment_name)?;
            crate::runtime::environment::validate_environment_configs(
                &conn,
                workspace_id,
                &environment,
            )?;
            environment
        };
        let env_name: &str = &environment.name;
        let waves = crate::runtime::environment::topo_sort_services(&environment)?;
        log::info!(
            "R-15: starting environment '{}' ({} services, {} waves)",
            environment.name,
            environment.services.len(),
            waves.len()
        );

        // 服务终态收集（state + detail）。
        let mut outcomes: std::collections::BTreeMap<String, (ServiceExecState, Option<String>)> =
            environment
                .services
                .iter()
                .map(|s| (s.runtime_name.clone(), (ServiceExecState::Starting, None)))
                .collect();

        for wave in &waves {
            if cancel.load(Ordering::Relaxed) {
                // 取消：剩余服务标记 Skipped，汇总后返回。
                for name in waves.iter().flatten() {
                    if let Some(entry) = outcomes.get_mut(name) {
                        if entry.0 == ServiceExecState::Starting && entry.1.is_none() {
                            *entry = (ServiceExecState::Skipped, Some("环境启动已取消".into()));
                        }
                    }
                }
                break;
            }
            // 波内并行：scoped threads（波结束即 join，可安全借用 &self）。
            // 构建阶段受 §66 Build permit 池约束（排队调度而非无脑并发）。
            // 依赖状态在进入本波前已定稿（依赖都在更早的波次），提前检查。
            let plans: Vec<(crate::runtime::environment::EnvironmentService, Vec<String>)> = wave
                .iter()
                .filter_map(|service_name| {
                    let service = environment
                        .services
                        .iter()
                        .find(|s| &s.runtime_name == service_name)?;
                    let failed_deps: Vec<String> = service
                        .depends_on
                        .iter()
                        .filter(|dep| {
                            outcomes
                                .get(*dep)
                                .map(|(state, _)| *state != ServiceExecState::Ready)
                                .unwrap_or(true)
                        })
                        .cloned()
                        .collect();
                    Some((service.clone(), failed_deps))
                })
                .collect();
            let results: Vec<(String, ServiceExecState, Option<String>)> =
                std::thread::scope(|scope| {
                    let mut handles = Vec::new();
                    for (service, failed_deps) in plans {
                        let cancel = Arc::clone(cancel);
                        handles.push(scope.spawn(move || {
                            start_environment_service(
                                self,
                                workspace_id,
                                env_name,
                                &service,
                                &failed_deps,
                                &cancel,
                            )
                        }));
                    }
                    handles
                        .into_iter()
                        .map(|handle| handle.join().expect("environment service thread"))
                        .collect()
                });
            for (name, state, detail) in results {
                outcomes.insert(name, (state, detail));
            }
        }

        // 汇总事件 + 任务结果。
        let service_outcomes: Vec<EnvironmentServiceOutcome> = outcomes
            .iter()
            .map(|(name, (state, detail))| EnvironmentServiceOutcome {
                service: name.clone(),
                state: *state,
                detail: detail.clone(),
            })
            .collect();
        let ready = service_outcomes
            .iter()
            .filter(|o| o.state == ServiceExecState::Ready)
            .count();
        let skipped = service_outcomes
            .iter()
            .filter(|o| o.state == ServiceExecState::Skipped)
            .count();
        let failed: Vec<String> = service_outcomes
            .iter()
            .filter(|o| o.state == ServiceExecState::Failed)
            .map(|o| o.service.clone())
            .collect();
        let success = failed.is_empty() && ready > 0;
        let summary = format!(
            "环境 '{}' 编排完成：{} Ready / {} Failed / {} Skipped / 共 {} 服务",
            environment.name,
            ready,
            failed.len(),
            skipped,
            service_outcomes.len()
        );
        self.emit(
            EVENT_ENVIRONMENT_COMPLETED,
            &EnvironmentCompletedPayload {
                workspace_id,
                environment: environment.name.clone(),
                success,
                services: service_outcomes,
                at: Self::now(),
            },
        );
        if success {
            Ok(Some(summary))
        } else if ready == 0 {
            Err(AppError::Task(format!(
                "{summary}；失败服务：{}",
                failed.join(", ")
            )))
        } else {
            // 部分成功：任务成功收尾，失败明细在汇总与事件中可见。
            Ok(Some(format!("{summary}；失败服务：{}", failed.join(", "))))
        }
    }

    /// §38 Stop Environment：逆拓扑序分波并行停止（先停下游，再停上游）。
    fn exec_stop_environment(
        &self,
        workspace_id: i64,
        environment_name: &str,
    ) -> AppResult<Option<String>> {
        let environment = {
            let conn = self.db.lock().unwrap();
            let root = config::workspace_root(&conn, workspace_id)?;
            crate::runtime::environment::get_environment(&root, environment_name)?
        };
        let mut waves = crate::runtime::environment::topo_sort_services(&environment)?;
        waves.reverse();
        let env_name: &str = &environment.name;

        let mut outcomes: std::collections::BTreeMap<String, (ServiceExecState, Option<String>)> =
            environment
                .services
                .iter()
                .map(|s| (s.runtime_name.clone(), (ServiceExecState::Stopped, None)))
                .collect();

        for wave in &waves {
            let results: Vec<(String, ServiceExecState, Option<String>)> =
                std::thread::scope(|scope| {
                    let mut handles = Vec::new();
                    for service_name in wave {
                        let service_name = service_name.clone();
                        handles.push(scope.spawn(move || {
                            let result =
                                self.processes
                                    .stop_runtime(workspace_id, &service_name, None);
                            let (state, detail) = match result {
                                Ok(Some(info)) => (
                                    ServiceExecState::Stopped,
                                    Some(format!("已停止（pid {:?}）", info.pid)),
                                ),
                                Ok(None) => {
                                    (ServiceExecState::Stopped, Some("未在运行".to_string()))
                                }
                                Err(error) => (ServiceExecState::Failed, Some(error.to_string())),
                            };
                            self.emit_environment_progress(
                                workspace_id,
                                env_name,
                                &service_name,
                                state,
                                detail.clone(),
                            );
                            (service_name, state, detail)
                        }));
                    }
                    handles
                        .into_iter()
                        .map(|handle| handle.join().expect("environment stop thread"))
                        .collect()
                });
            for (name, state, detail) in results {
                outcomes.insert(name, (state, detail));
            }
        }

        let service_outcomes: Vec<EnvironmentServiceOutcome> = outcomes
            .iter()
            .map(|(name, (state, detail))| EnvironmentServiceOutcome {
                service: name.clone(),
                state: *state,
                detail: detail.clone(),
            })
            .collect();
        let stopped = service_outcomes
            .iter()
            .filter(|o| o.state == ServiceExecState::Stopped)
            .count();
        let failed: Vec<String> = service_outcomes
            .iter()
            .filter(|o| o.state == ServiceExecState::Failed)
            .map(|o| o.service.clone())
            .collect();
        let summary = format!(
            "环境 '{}' 停止完成：{}/{} 已停止；失败：[{}]",
            environment.name,
            stopped,
            service_outcomes.len(),
            failed.join(", ")
        );
        self.emit(
            EVENT_ENVIRONMENT_COMPLETED,
            &EnvironmentCompletedPayload {
                workspace_id,
                environment: environment.name.clone(),
                success: failed.is_empty(),
                services: service_outcomes,
                at: Self::now(),
            },
        );
        if failed.is_empty() {
            Ok(Some(summary))
        } else {
            Err(AppError::Task(summary))
        }
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
            &self.script_approvals,
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

    /// R-17/R-21 的 Rebuild & Restart 入口：Stop → 完整构建 → Start
    /// （与 `restart` 的 skip_build 复用相对；源码变更后必须重建）。
    fn exec_rebuild_restart(
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
        if self
            .processes
            .stop_runtime(workspace_id, runtime_name, None)?
            .is_some()
        {
            log::info!("R-17: rebuild-restart stopped previous instance of '{runtime_name}'");
        }
        let mut start_options = start_options_of(options);
        start_options.skip_build = false;
        let result = self.processes.start(workspace_id, runtime_name, start_options);
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
            "'{}' 已重建并重启（pid {:?}）",
            runtime_name, info.pid
        )))
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
        // 路径比较对 Windows 分隔符不敏感（DB 索引为正斜杠，discovery 为
        // 原生路径，R-14 修复）。
        let new_projects: Vec<_> = discovery
            .projects
            .iter()
            .filter(|p| {
                !known_paths.contains(&p.path.to_string_lossy().replace('\\', "/"))
            })
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

// ---------------------------------------------------------------------------
// R-15 §40：波内单服务启动（scoped thread 体内）
// ---------------------------------------------------------------------------

/// 启动环境内的一个服务：依赖未就绪 → Skipped；否则 Start（带环境覆盖项）
/// → R-16 就绪门限（Healthy / 超时放行 / 进程死亡即失败）。
#[allow(clippy::too_many_arguments)]
fn start_environment_service(
    service_runtime: &RuntimeService,
    workspace_id: i64,
    environment_name: &str,
    service: &crate::runtime::environment::EnvironmentService,
    failed_deps: &[String],
    cancel: &Arc<AtomicBool>,
) -> (String, ServiceExecState, Option<String>) {
    let runtime_name = &service.runtime_name;
    if !failed_deps.is_empty() {
        let detail = format!(
            "依赖未就绪：{}（部分失败语义：跳过本服务）",
            failed_deps.join(", ")
        );
        service_runtime.emit_environment_progress(
            workspace_id,
            environment_name,
            runtime_name,
            ServiceExecState::Skipped,
            Some(detail.clone()),
        );
        return (runtime_name.clone(), ServiceExecState::Skipped, Some(detail));
    }

    service_runtime.emit_environment_progress(
        workspace_id,
        environment_name,
        runtime_name,
        ServiceExecState::Starting,
        None,
    );
    // 每服务一个取消 watcher（构建取消快路径 + 停止收尾）。
    let _watch = CancelWatch::start(
        &service_runtime.processes,
        workspace_id,
        runtime_name,
        cancel,
    );
    let options = StartOptions {
        overrides: Some(crate::runtime::launch::EnvironmentOverrides {
            jdk: service.jdk.clone(),
            profile: service.profile.clone(),
            environment: service.environment.clone(),
            port: service.port,
        }),
        ..Default::default()
    };
    match service_runtime
        .processes
        .start(workspace_id, runtime_name, options)
    {
        Ok(info) => {
            let timeout = Duration::from_secs(
                service
                    .ready_timeout_seconds
                    .unwrap_or(crate::runtime::environment::DEFAULT_READY_TIMEOUT_SECS),
            );
            match service_runtime.wait_service_ready(
                workspace_id,
                runtime_name,
                info.process_id,
                timeout,
            ) {
                Ok(detail) => {
                    service_runtime.emit_environment_progress(
                        workspace_id,
                        environment_name,
                        runtime_name,
                        ServiceExecState::Ready,
                        Some(detail.clone()),
                    );
                    (runtime_name.clone(), ServiceExecState::Ready, Some(detail))
                }
                Err(detail) => {
                    service_runtime.emit_environment_progress(
                        workspace_id,
                        environment_name,
                        runtime_name,
                        ServiceExecState::Failed,
                        Some(detail.clone()),
                    );
                    (runtime_name.clone(), ServiceExecState::Failed, Some(detail))
                }
            }
        }
        Err(error) => {
            let detail = error.to_string();
            service_runtime.emit_environment_progress(
                workspace_id,
                environment_name,
                runtime_name,
                ServiceExecState::Failed,
                Some(detail.clone()),
            );
            (runtime_name.clone(), ServiceExecState::Failed, Some(detail))
        }
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
            RuntimeOp::StartEnvironment => {
                self.exec_start_environment(*workspace_id, runtime_name, &cancel)
            }
            RuntimeOp::StopEnvironment => {
                self.exec_stop_environment(*workspace_id, runtime_name)
            }
            RuntimeOp::RebuildRestart => {
                self.exec_rebuild_restart(*workspace_id, runtime_name, options, &cancel)
            }
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
        // R-17：watch 影响分析的必建子集透传给流水线（与指纹子集合并）。
        affected_modules: options.affected_modules.clone(),
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
            fixture.root.join("approvals.json"),
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

    /// R-13 `closure_preview`：给定 Scope 返回闭包预览，Manual 剔除模块后
    /// 收缩；缓存命中标记正确。
    #[test]
    fn closure_preview_computes_scope_and_reports_cache_hit() {
        let fixture = maven_fixture("closure");
        let emitter = Arc::new(VecEmitter::default());
        let service = test_service(
            &fixture,
            emitter,
            Arc::new(FakeMavenRunner::successful()),
            Arc::new(FakeLaunchRunner::staying_alive()),
        );
        let project = fixture
            .root
            .join("repo/app/pom.xml")
            .to_string_lossy()
            .to_string();
        let lib_project = fixture.root.join("repo/lib/pom.xml").to_string_lossy().to_string();

        // Auto：闭包 = app + lib（lib 是 app 的源码依赖，parent 不进闭包）。
        let auto = service
            .closure_preview(fixture.workspace_id, &project, &RuntimeScope::Auto)
            .unwrap();
        let auto_ids: Vec<i64> = auto.closure.projects.iter().map(|p| p.project_id).collect();
        assert!(
            auto_ids.contains(&auto.closure.root_project_id),
            "root must be inside the auto closure"
        );
        assert!(
            auto.closure.projects.len() >= 2,
            "app + lib expected in closure, got {:?}",
            auto_ids
        );

        // 二次计算（同 fingerprint + 同 scope）应命中缓存。
        let cached = service
            .closure_preview(fixture.workspace_id, &project, &RuntimeScope::Auto)
            .unwrap();
        assert!(cached.cache_hit, "second auto preview must hit the closure cache");

        // Manual 空集：闭包收缩为仅 root（root 不可被排除，R-03 语义）。
        let empty = service
            .closure_preview(
                fixture.workspace_id,
                &project,
                &RuntimeScope::Manual { project_ids: vec![] },
            )
            .unwrap();
        assert_eq!(empty.closure.projects.len(), 1);
        assert_eq!(empty.closure.projects[0].project_id, auto.closure.root_project_id);

        // Hybrid：include=[root]，排除 lib → 闭包仅 app。
        let lib_id = service
            .closure_preview(fixture.workspace_id, &lib_project, &RuntimeScope::Auto)
            .unwrap()
            .closure
            .root_project_id;
        let hybrid = service
            .closure_preview(
                fixture.workspace_id,
                &project,
                &RuntimeScope::Hybrid {
                    include_project_ids: vec![auto.closure.root_project_id],
                    exclude_project_ids: vec![lib_id],
                },
            )
            .unwrap();
        assert_eq!(hybrid.closure.projects.len(), 1);
        assert_eq!(hybrid.closure.projects[0].project_id, auto.closure.root_project_id);

        // 未知项目 → ProjectNotFound 可行动错误。
        let err = service
            .closure_preview(fixture.workspace_id, "no/such/project", &RuntimeScope::Auto)
            .unwrap_err();
        assert_eq!(err.code(), "ProjectNotFound");
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
    // R-16 §41：健康探针与进程生命周期集成
    // --------------------------------------------------------------

    /// 配置了 health_check 的应用：Start 后探针 Starting → Healthy；
    /// Stop 后收口为 Stopped（进程退出 → finalize_exit → stop_monitor）。
    #[test]
    fn health_probe_transitions_with_lifecycle() {
        let fixture = maven_fixture("health");
        // 真实本地端口：探针 Port 方式连它（FakeLaunchRunner 不开端口，
        // 因此显式配置 port，不经启动日志探测）。
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        {
            let conn = fixture.db.lock().unwrap();
            config::update_config(
                &conn,
                &config::UpdateRuntimeConfigRequest {
                    workspace_id: fixture.workspace_id,
                    name: "app".into(),
                    config: RuntimeApplicationConfig {
                        name: "app".into(),
                        project: fixture
                            .root
                            .join("repo/app/pom.xml")
                            .to_string_lossy()
                            .to_string(),
                        main_class: Some("com.example.app.Application".into()),
                        health_check: Some(crate::runtime::health::HealthCheckConfig {
                            kind: crate::runtime::health::HealthCheckKind::Port,
                            port: Some(port),
                            interval_ms: Some(500),
                            timeout_ms: Some(500),
                            ..Default::default()
                        }),
                        ..Default::default()
                    },
                },
            )
            .unwrap();
        }
        let emitter = Arc::new(VecEmitter::default());
        let service = test_service(
            &fixture,
            Arc::clone(&emitter),
            Arc::new(FakeMavenRunner::successful()),
            Arc::new(FakeLaunchRunner::staying_alive()),
        );

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
                Arc::new(AtomicBool::new(false)),
            )
            .unwrap();

        // 等待探针翻到 Healthy（首个探测在一个间隔内发生）。
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut healthy_seen = false;
        while Instant::now() < deadline {
            if let Some(snapshot) = service.get_health(
                service.list_processes(fixture.workspace_id).unwrap()[0].process_id,
            ) {
                if snapshot.phase == crate::runtime::events::HealthStatus::Healthy {
                    healthy_seen = true;
                    break;
                }
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(healthy_seen, "probe must reach Healthy while running");

        // Stop：进程退出收口探针 → Stopped。
        service
            .execute(
                &runtime_task(RuntimeOp::Stop, fixture.workspace_id, "app", Default::default()),
                Arc::new(AtomicBool::new(false)),
            )
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut stopped_seen = false;
        while Instant::now() < deadline {
            if let Some(snapshot) = service.get_health(
                service.list_processes(fixture.workspace_id).unwrap()[0].process_id,
            ) {
                if snapshot.phase == crate::runtime::events::HealthStatus::Stopped {
                    stopped_seen = true;
                    break;
                }
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(stopped_seen, "probe must be finalized to Stopped after exit");
        drop(listener);
    }

    /// 未配置 health_check 的应用：无探针快照（R-12 up/down 语义保持）。
    #[test]
    fn no_health_config_means_no_probe() {
        let fixture = maven_fixture("nohealth");
        let emitter = Arc::new(VecEmitter::default());
        let service = test_service(
            &fixture,
            emitter,
            Arc::new(FakeMavenRunner::successful()),
            Arc::new(FakeLaunchRunner::staying_alive()),
        );
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
                Arc::new(AtomicBool::new(false)),
            )
            .unwrap();
        std::thread::sleep(Duration::from_millis(300));
        let process_id = service.list_processes(fixture.workspace_id).unwrap()[0].process_id;
        assert!(
            service.get_health(process_id).is_none(),
            "without health_check config there must be no probe snapshot"
        );
    }

    // --------------------------------------------------------------
    // R-15 §38/§39/§40：环境编排
    // --------------------------------------------------------------

    /// 记录 Maven 调用顺序的 runner（`-f` reactor pom 区分服务；顺序断言
    /// 拓扑波次）。
    struct OrderingRunner {
        workdirs: Mutex<Vec<String>>,
    }

    impl MavenRunner for OrderingRunner {
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
            request: &crate::maven::MavenExecutionRequest,
            _env: &[(String, String)],
            sink: &mut dyn BuildOutputSink,
            _cancel: Option<&AtomicBool>,
            _timeout: Option<Duration>,
        ) -> AppResult<StreamingExit> {
            let reactor = request
                .extra_args
                .iter()
                .position(|arg| arg == "-f")
                .and_then(|i| request.extra_args.get(i + 1))
                .cloned()
                .unwrap_or_default();
            self.workdirs.lock().unwrap().push(reactor);
            sink.on_line(OutputStream::Stdout, "BUILD SUCCESS");
            Ok(StreamingExit {
                exit_code: Some(0),
                timed_out: false,
                cancelled: false,
            })
        }
    }

    fn env_service(name: &str, deps: &[&str]) -> crate::runtime::environment::EnvironmentService {
        crate::runtime::environment::EnvironmentService {
            runtime_name: name.into(),
            depends_on: deps.iter().map(|s| s.to_string()).collect(),
            jdk: None,
            profile: None,
            environment: Default::default(),
            port: None,
            external_notes: None,
            ready_timeout_seconds: None,
        }
    }

    /// Start Environment：无依赖服务并行（第一波），依赖服务按拓扑序串行；
    /// 全部就绪后 completed(success) 汇总。
    #[test]
    fn environment_start_follows_topology_and_readies_all() {
        let fixture = maven_fixture("envstart");
        let emitter = Arc::new(VecEmitter::default());
        let lib_dir = fixture.root.join("repo/lib").to_string_lossy().to_string();
        let app_dir = fixture.root.join("repo/app").to_string_lossy().to_string();
        let ordering = Arc::new(OrderingRunner {
            workdirs: Mutex::new(Vec::new()),
        });
        let service = test_service(
            &fixture,
            Arc::clone(&emitter),
            ordering.clone(),
            Arc::new(FakeLaunchRunner::staying_alive()),
        );

        // 四个配置：common/lib、file/app（第一波无依赖）；auth/lib、gateway/app。
        for (name, pom) in [
            ("common", "repo/lib/pom.xml"),
            ("file", "repo/app/pom.xml"),
            ("auth", "repo/lib/pom.xml"),
            ("gateway", "repo/app/pom.xml"),
        ] {
            let conn = fixture.db.lock().unwrap();
            config::create_config(
                &conn,
                &CreateRuntimeConfigRequest {
                    workspace_id: fixture.workspace_id,
                    config: RuntimeApplicationConfig {
                        name: name.into(),
                        project: fixture.root.join(pom).to_string_lossy().to_string(),
                        main_class: Some("com.example.app.Application".into()),
                        // PackageRun：单次 Maven 调用 + jar 产物校验（见下），
                        // 避免假 runner 下的 ClasspathRun classpath 文件生成。
                        profile: Some("prod".into()),
                        ..Default::default()
                    },
                },
            )
            .unwrap();
        }
        // PackageRun 需要 target jar 产物存在。
        for (dir, artifact) in [("repo/lib", "lib"), ("repo/app", "app")] {
            let target = fixture.root.join(dir).join("target");
            std::fs::create_dir_all(&target).unwrap();
            std::fs::write(target.join(format!("{artifact}-1.0.0.jar")), b"jar").unwrap();
        }
        let environment = crate::runtime::environment::RuntimeEnvironment {
            schema_version: 1,
            name: "Development".into(),
            description: None,
            services: vec![
                env_service("gateway", &["auth"]),
                env_service("auth", &["common"]),
                env_service("common", &[]),
                env_service("file", &[]),
            ],
        };
        crate::runtime::environment::save_environment(&fixture.root, &environment).unwrap();

        let output = service
            .execute(
                &runtime_task(
                    RuntimeOp::StartEnvironment,
                    fixture.workspace_id,
                    "Development",
                    Default::default(),
                ),
                Arc::new(AtomicBool::new(false)),
            )
            .unwrap();
        assert!(output.unwrap().contains("4 Ready"));

        // Maven 调用顺序：波 0（common/file，并行）→ 波 1（auth）→ 波 2
        // （gateway）。auth 的构建必须晚于两个无依赖服务的构建完成。
        let workdirs = ordering.workdirs.lock().unwrap();
        assert_eq!(workdirs.len(), 4, "each service builds once");
        // lib 配置的 reactor 是单项目 pom；app 配置（依赖 lib）的 reactor 是
        // 父 pom（带 -pl app）。
        let kind_of = |reactor: &str| {
            if reactor.ends_with("/repo/lib/pom.xml") {
                "lib"
            } else {
                "app"
            }
        };
        assert_eq!(kind_of(&workdirs[2]), "lib", "wave 1 = auth (lib reactor)");
        assert_eq!(kind_of(&workdirs[3]), "app", "wave 2 = gateway (app reactor)");
        drop(workdirs);
        let _ = (&lib_dir, &app_dir);

        // 事件：completed(success=true)，4 服务全部 ready。
        let names = emitter.names();
        assert!(names.contains(&EVENT_ENVIRONMENT_PROGRESS));
        assert_eq!(names.last(), Some(&EVENT_ENVIRONMENT_COMPLETED));
        let collected = emitter.collected();
        let completed = collected.last().unwrap();
        assert_eq!(completed.payload["success"], serde_json::json!(true));
        assert_eq!(completed.payload["services"].as_array().unwrap().len(), 4);
        for outcome in completed.payload["services"].as_array().unwrap() {
            assert_eq!(outcome["state"], serde_json::json!("ready"), "{outcome}");
        }
    }

    /// 部分失败语义：单服务启动失败 → 其依赖方 Skipped，无依赖分支照常
    /// Ready；completed(success=false) 正确汇总。
    #[test]
    fn environment_start_partial_failure_skips_dependents() {
        let fixture = maven_fixture("envfail");
        let emitter = Arc::new(VecEmitter::default());
        let service = test_service(
            &fixture,
            Arc::clone(&emitter),
            Arc::new(FakeMavenRunner::successful()),
            Arc::new(FakeLaunchRunner::staying_alive()),
        );
        for (name, pom) in [
            ("ok", "repo/lib/pom.xml"),
            ("broken", "repo/missing/pom.xml"), // prepare 阶段即失败
            ("dependent", "repo/app/pom.xml"),
        ] {
            let conn = fixture.db.lock().unwrap();
            config::create_config(
                &conn,
                &CreateRuntimeConfigRequest {
                    workspace_id: fixture.workspace_id,
                    config: RuntimeApplicationConfig {
                        name: name.into(),
                        project: fixture.root.join(pom).to_string_lossy().to_string(),
                        main_class: Some("com.example.app.Application".into()),
                        profile: Some("prod".into()),
                        ..Default::default()
                    },
                },
            )
            .unwrap();
        }
        for (dir, artifact) in [("repo/lib", "lib"), ("repo/app", "app")] {
            let target = fixture.root.join(dir).join("target");
            std::fs::create_dir_all(&target).unwrap();
            std::fs::write(target.join(format!("{artifact}-1.0.0.jar")), b"jar").unwrap();
        }
        let environment = crate::runtime::environment::RuntimeEnvironment {
            schema_version: 1,
            name: "Demo".into(),
            description: None,
            services: vec![
                env_service("dependent", &["broken"]),
                env_service("broken", &[]),
                env_service("ok", &[]),
            ],
        };
        crate::runtime::environment::save_environment(&fixture.root, &environment).unwrap();

        let output = service
            .execute(
                &runtime_task(
                    RuntimeOp::StartEnvironment,
                    fixture.workspace_id,
                    "Demo",
                    Default::default(),
                ),
                Arc::new(AtomicBool::new(false)),
            )
            .unwrap();
        // 部分成功：任务以 Ok 收尾（ok Ready），失败明细在汇总里。
        assert!(output.unwrap().contains("Failed"));
        let collected = emitter.collected();
        let completed = collected.last().unwrap();
        assert_eq!(completed.payload["success"], serde_json::json!(false));
        let states: std::collections::BTreeMap<String, String> = completed.payload["services"]
            .as_array()
            .unwrap()
            .iter()
            .map(|o| {
                (
                    o["service"].as_str().unwrap().to_string(),
                    o["state"].as_str().unwrap().to_string(),
                )
            })
            .collect();
        assert_eq!(states["ok"], "ready");
        assert_eq!(states["broken"], "failed");
        assert_eq!(states["dependent"], "skipped");
    }

    /// Stop Environment：运行中的服务全部停止。
    #[test]
    fn environment_stop_stops_running_services() {
        let fixture = maven_fixture("envstop");
        let emitter = Arc::new(VecEmitter::default());
        let service = test_service(
            &fixture,
            Arc::clone(&emitter),
            Arc::new(FakeMavenRunner::successful()),
            Arc::new(FakeLaunchRunner::staying_alive()),
        );
        for (name, pom) in [("a", "repo/lib/pom.xml"), ("b", "repo/app/pom.xml")] {
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
        let environment = crate::runtime::environment::RuntimeEnvironment {
            schema_version: 1,
            name: "Local".into(),
            description: None,
            services: vec![env_service("a", &[]), env_service("b", &["a"])],
        };
        crate::runtime::environment::save_environment(&fixture.root, &environment).unwrap();

        // 先启动 a（b 不启动）。
        service
            .execute(
                &runtime_task(
                    RuntimeOp::Start,
                    fixture.workspace_id,
                    "a",
                    RuntimeTaskOptions {
                        strategy: Some(RunStrategy::MavenRun),
                        ..Default::default()
                    },
                ),
                Arc::new(AtomicBool::new(false)),
            )
            .unwrap();
        let before = emitter.names().len();

        let output = service
            .execute(
                &runtime_task(
                    RuntimeOp::StopEnvironment,
                    fixture.workspace_id,
                    "Local",
                    Default::default(),
                ),
                Arc::new(AtomicBool::new(false)),
            )
            .unwrap();
        assert!(output.unwrap().contains("已停止"));
        let collected = emitter.collected();
        let completed = collected.last().unwrap();
        assert_eq!(completed.payload["environment"], serde_json::json!("Local"));
        let states: Vec<(String, String)> = completed.payload["services"]
            .as_array()
            .unwrap()
            .iter()
            .map(|o| {
                (
                    o["service"].as_str().unwrap().to_string(),
                    o["state"].as_str().unwrap().to_string(),
                )
            })
            .collect();
        assert!(states.contains(&("a".into(), "stopped".into())));
        assert!(states.contains(&("b".into(), "stopped".into())));
        // a 真正被停止：进程事件发出。
        let after: Vec<_> = emitter.names()[before..].to_vec();
        assert!(after.contains(&EVENT_PROCESS_STOPPED));
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
            fixture.root.join("approvals.json"),
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
