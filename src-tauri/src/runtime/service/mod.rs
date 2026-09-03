//! RuntimeService（R-12，§63/§64/§65/§66）：Runtime 模块对 Task Engine 与
//! IPC 的统一入口。
//!
//! 文件布局（B-02 拆分，设计文档 §4.1）：
//! - 本文件（`mod.rs`）：[`RuntimeService`] 字段与 `new`/`assemble` 装配、
//!   共享辅助（`find_project` / `emit` / `workspace_root`）、启动对账
//!   `reconcile_on_startup`、公共 re-export；
//! - [`dto`]：SchedulerConfig 与 IPC 请求/返回 DTO；
//! - [`queries`]：§63 读侧查询（projects / graph / processes / logs / health）；
//! - [`operations`]：Build / Start / Stop / Restart / Resolve 单服务操作、
//!   脚本审批管理、TaskRequest 组装；
//! - [`environment`]：R-15 多服务拓扑编排（Start / Stop Environment）；
//! - [`task_handler`]：[`RuntimeTaskHandler`] 实现（TaskType 分发）；
//! - [`cancellation`]：CancelWatch 与 DTO→领域选项映射。
//!
//! 职责契约（不变）：
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

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rusqlite::Connection;
use serde::Serialize;
use tauri::AppHandle;

use crate::db::dao;
use crate::error::AppResult;
use crate::maven::{DependencyGraphCache, MavenProjectNode, PomCache, RuntimeClosureCache};
use crate::runtime::build::runner::{MavenRunner, SpawningMavenRunner};
use crate::runtime::build::scheduler::BuildScheduler;
use crate::runtime::config;
use crate::runtime::events::{RuntimeEmission, RuntimeEventEmitter, TauriRuntimeBridge, TauriRuntimeEmitter};
use crate::runtime::launch::launcher::{LaunchRunner, SystemLaunchRunner};
use crate::runtime::launch::manager::{RuntimeProcessDeps, RuntimeProcessManager, DEFAULT_SAMPLE_INTERVAL};
use crate::runtime::logs::RuntimeLogEngine;
use crate::runtime::script_approval::{self, ScriptApprovalStore};

mod cancellation;
mod dto;
mod environment;
mod operations;
mod queries;
mod task_handler;

use cancellation::CancelWatch;
pub use cancellation::{build_options_of, start_options_of};

pub use dto::{
    scheduler_config_path, ClosurePreview, DependencyGraphView, ProjectInspection, RuntimeLogQuery,
    RuntimeOperationRequest, SchedulerConfig,
};

// ---------------------------------------------------------------------------
// IPC 请求 / 视图类型（§63；golden 快照覆盖）
// ---------------------------------------------------------------------------

/// 依赖边默认返回上限（约 500 模块工作区的全量边数倍余量）。
const DEFAULT_MAX_GRAPH_EDGES: usize = 5000;

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
        let resolve_scheduler = Arc::new(BuildScheduler::new(scheduler_config.max_concurrent_resolves));
        let graph_cache = Arc::new(DependencyGraphCache::new());
        let closure_cache = Arc::new(RuntimeClosureCache::new());
        let bridge = Arc::new(TauriRuntimeBridge::new(Arc::clone(&emitter), Arc::clone(&db)));

        // R-16：健康检查引擎与进程管理器共享同一 emitter / DB。
        let health = crate::runtime::health::HealthEngine::new(Arc::clone(&db), Arc::clone(&emitter));

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

    // ------------------------------------------------------------------
    // R-14 §75 Command Safety：Pre/Post Build Script 确认状态
    // ------------------------------------------------------------------

    // ------------------------------------------------------------------
    // §63 写侧命令的 TaskRequest 组装（提交走 T-05 TaskManager）
    // ------------------------------------------------------------------

    // ------------------------------------------------------------------
    // R-15 §38/§39/§40：环境编排（Start / Stop Environment）
    // ------------------------------------------------------------------

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
            std::thread::spawn(move || match processes.reconcile_on_startup(ws.id) {
                Ok(adopted) if !adopted.is_empty() => {
                    log::info!(
                        "R-12: workspace '{}' adopted {} orphan runtime process(es)",
                        path,
                        adopted.len()
                    );
                }
                Ok(_) => {}
                Err(e) => log::warn!("R-12: startup reconcile failed for workspace '{}': {e}", path),
            });
        }
    }

    // ------------------------------------------------------------------
    // RuntimeTaskHandler（§65：任务执行体）
    // ------------------------------------------------------------------
}

#[cfg(test)]
mod tests;
