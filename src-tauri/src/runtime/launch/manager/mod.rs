//! Process Manager（R-10，§29 Start 流程、§33 Process Manager、§34 Process 控制）。
//!
//! [`RuntimeProcessManager`] 编排一次启动的完整生命周期：
//!
//! ```text
//! Created → Preparing（加载配置 / R-06 推断 mainClass / 命中构建缓存？）
//!         → Resolving → Building（R-09 execute_build）
//!         → Starting（spawn + 启动横幅/宽限期判定）→ Running
//!         → Stopping（SIGTERM 优雅优先，grace 超时升级杀进程树）→ Stopped
//! ```
//!
//! 特有设计点（任务文档「架构/性能注意点」）：
//! - **状态以 OS 进程为准**：行内 `pid + pid_start_time` 核对存活、防 PID
//!   复用；GitWorkspace 重启后 [`reconcile_on_startup`][Self::reconcile_on_startup]
//!   对非终态行逐一对账——活的接管（adopted，轮询监控，取不到退出码时宽容
//!   落 Stopped）、死的按崩溃/停止分类收尾。
//! - **Stop 终止整棵树**：SIGTERM 只发 root（Spring Boot shutdown hook 优雅
//!   退出）；grace 超时或 Windows（无 SIGTERM 语义）升级为
//!   [`crate::process::kill_process_tree`]。
//! - **指标节流**：sampler 线程按 `sample_interval`（默认 2s）读 sysinfo
//!   计数器（不 fork 进程），每 5 拍落一次 DB。
//! - **单连接写序列化**：状态/指标写与 R-09 构建共用一条 SQLite 连接
//!   （T-05 Task Manager 同款先例）；构建期间其他进程的收尾写会短暂排队，
//!   崩溃一致性由 reconcile 兜底。也因此 Resolving/Building 两个状态在
//!   `execute_build` 调用前紧邻置位（流水线内部无法插桩），细分进度由
//!   R-12 需要时再暴露。
//!
//! IPC/前端不在本任务（R-12/R-13）；事件经 `RuntimeEventSink` 外流。
//!
//! 文件布局（B-03 拆分，设计文档 §4.2）：
//! - 本文件（`mod.rs`）：[`RuntimeProcessManager`] / [`RuntimeProcessDeps`]、
//!   构造与 Drop、状态迁移核心（`transit` 族 + `current_status`/`row`/`info`）、
//!   查询（get/list/runtime_status）与公共 re-export；
//! - [`types`]：纯类型（StartOptions / ActiveProcess / MonitorOutcome /
//!   Prepared 等）与退出分类 `classify_exit`；
//! - [`metrics`]：指标 sampler 线程（sysinfo 只读，按节流落 DB）；
//! - [`output`]：BuildLogSink、启动横幅/端口探测正则、日志会话开启；
//! - [`monitor`]：spawn 后监控、finalize_exit、启动宽限与等待原语；
//! - [`control`]：stop / kill / restart / reconcile（用户控制面）；
//! - [`start`]：start / start_inner / prepare / run_build（启动执行面）。

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use chrono::Utc;
use rusqlite::Connection;

use crate::error::{AppError, AppResult};
use crate::maven::closure::RuntimeClosureCache;
use crate::maven::index::DependencyGraphCache;
use crate::runtime::build::runner::{MavenRunner, SpawningMavenRunner};
use crate::runtime::build::scheduler::BuildScheduler;
#[cfg(test)]
use crate::runtime::build::{LaunchPlan, RunStrategy};
use crate::runtime::launch::launcher::{LaunchRunner, SystemLaunchRunner};
use crate::runtime::launch::store;
use crate::runtime::launch::{
    LifecycleStatus, LoggingEventSink, RuntimeEvent, RuntimeEventSink, RuntimeProcessInfo,
};
use crate::runtime::logs::RuntimeLogEngine;
use crate::runtime::script_approval::ScriptApprovalStore;

mod control;
mod metrics;
mod monitor;
mod output;
mod start;
mod types;

use output::BuildLogSink;

use types::{
    classify_exit, ActiveProcess, Built, CachedLaunch, MonitorOutcome, PidWait, Prepared, RunWait,
};

pub use types::{
    EnvironmentOverrides, StartOptions, DEFAULT_SAMPLE_INTERVAL, DEFAULT_START_GRACE,
    DEFAULT_STOP_GRACE,
};

/// Manager 的可替换依赖（生产默认值 + 测试注入 seam）。
pub struct RuntimeProcessDeps {
    pub graph_cache: Arc<DependencyGraphCache>,
    pub closure_cache: Arc<RuntimeClosureCache>,
    pub scheduler: Arc<BuildScheduler>,
    pub maven_runner: Arc<dyn MavenRunner>,
    pub launch_runner: Arc<dyn LaunchRunner>,
    pub events: Arc<dyn RuntimeEventSink>,
    /// R-11 日志引擎：构建/运行输出统一接管（会话在 Start 时开启）。
    pub logs: Arc<RuntimeLogEngine>,
    pub sample_interval: Duration,
    /// R-14 §75：Pre/Post Build Script 确认状态（传给 R-09 流水线）。
    pub script_approvals: ScriptApprovalStore,
    /// R-16 §41 健康检查引擎；`None` = 不探针（R-12 生命周期推导语义）。
    pub health: Option<Arc<crate::runtime::health::HealthEngine>>,
}

impl Default for RuntimeProcessDeps {
    fn default() -> Self {
        Self {
            graph_cache: Arc::new(DependencyGraphCache::new()),
            closure_cache: Arc::new(RuntimeClosureCache::new()),
            scheduler: Arc::new(BuildScheduler::default()),
            maven_runner: Arc::new(SpawningMavenRunner),
            launch_runner: Arc::new(SystemLaunchRunner),
            events: Arc::new(LoggingEventSink),
            logs: Arc::new(RuntimeLogEngine::new()),
            sample_interval: DEFAULT_SAMPLE_INTERVAL,
            script_approvals: ScriptApprovalStore::new(
                crate::runtime::script_approval::script_approvals_path(),
            ),
            health: None,
        }
    }
}

/// Runtime 进程管理器。注意：`start` / `stop` / `restart` /
/// `reconcile_on_startup` 都是同步阻塞调用（构建可能耗时数分钟）——R-12
/// 会把它们放进任务队列线程。
pub struct RuntimeProcessManager {
    db: Arc<Mutex<Connection>>,
    deps: RuntimeProcessDeps,
    active: Arc<Mutex<HashMap<i64, ActiveProcess>>>,
    launch_cache: Arc<Mutex<HashMap<(i64, String), CachedLaunch>>>,
    sampler_stop: Arc<AtomicBool>,
    sampler_started: Arc<AtomicBool>,
    sampler_handle: Mutex<Option<JoinHandle<()>>>,
}

impl RuntimeProcessManager {
    pub fn new(db: Arc<Mutex<Connection>>) -> Self {
        Self::with_deps(db, RuntimeProcessDeps::default())
    }

    pub fn with_deps(db: Arc<Mutex<Connection>>, deps: RuntimeProcessDeps) -> Self {
        Self {
            db,
            deps,
            active: Arc::new(Mutex::new(HashMap::new())),
            launch_cache: Arc::new(Mutex::new(HashMap::new())),
            sampler_stop: Arc::new(AtomicBool::new(false)),
            sampler_started: Arc::new(AtomicBool::new(false)),
            sampler_handle: Mutex::new(None),
        }
    }

    // ------------------------------------------------------------------
    // Start（§29）
    // ------------------------------------------------------------------

    // ------------------------------------------------------------------
    // Stop / Kill / Restart（§34）
    // ------------------------------------------------------------------

    // ------------------------------------------------------------------
    // 进程托管：重启后的孤儿对账（§33）
    // ------------------------------------------------------------------

    // ------------------------------------------------------------------
    // 查询
    // ------------------------------------------------------------------

    pub fn get_process(&self, process_id: i64) -> AppResult<Option<RuntimeProcessInfo>> {
        let conn = self.db.lock().unwrap();
        self.get_process_with_connection(&conn, process_id)
    }

    pub fn list_processes(&self, workspace_id: i64) -> AppResult<Vec<RuntimeProcessInfo>> {
        let conn = self.db.lock().unwrap();
        self.list_processes_with_connection(&conn, workspace_id)
    }

    /// Read-only query variant for callers that already hold the shared DB
    /// connection (for example AI Context Builder). Keeping the lock at the
    /// outer boundary prevents same-thread SQLite mutex re-entry.
    pub(crate) fn get_process_with_connection(
        &self,
        conn: &rusqlite::Connection,
        process_id: i64,
    ) -> AppResult<Option<RuntimeProcessInfo>> {
        Ok(store::get_process(conn, process_id)?.map(|row| store::row_to_info(&row)))
    }

    pub(crate) fn list_processes_with_connection(
        &self,
        conn: &rusqlite::Connection,
        workspace_id: i64,
    ) -> AppResult<Vec<RuntimeProcessInfo>> {
        Ok(store::list_processes(conn, workspace_id)?
            .iter()
            .map(store::row_to_info)
            .collect())
    }

    /// 某 Runtime 最新一条进程记录（Dashboard 状态槽位用）。
    pub fn runtime_status(
        &self,
        workspace_id: i64,
        runtime_name: &str,
    ) -> AppResult<Option<RuntimeProcessInfo>> {
        let conn = self.db.lock().unwrap();
        let rows = store::list_processes(&conn, workspace_id)?;
        Ok(rows
            .into_iter()
            .find(|row| row.runtime_name == runtime_name)
            .map(|row| store::row_to_info(&row)))
    }

    // ------------------------------------------------------------------
    // 内部：monitor / sampler / 状态迁移
    // ------------------------------------------------------------------

    // ------------------------------------------------------------------
    // 内部：等待原语
    // ------------------------------------------------------------------

    // ------------------------------------------------------------------
    // 内部：状态迁移与读
    // ------------------------------------------------------------------

    /// 迁移并发生命周期事件；非法迁移返回错误。
    fn transit(
        &self,
        process_id: i64,
        runtime_name: &str,
        to: LifecycleStatus,
        exit_code: Option<Option<i32>>,
    ) -> AppResult<LifecycleStatus> {
        let (from, to) = {
            let conn = self.db.lock().unwrap();
            store::transition_status(&conn, process_id, to, exit_code)?
        };
        self.emit_transition(process_id, runtime_name, from, to);
        Ok(to)
    }

    /// 宽容迁移：行已是终态（竞态收尾完成）时返回 `false` 而非报错；
    /// 其他非法迁移仍报错。
    fn transit_lenient(
        &self,
        process_id: i64,
        runtime_name: &str,
        to: LifecycleStatus,
    ) -> AppResult<bool> {
        let current = self.current_status(process_id)?;
        if current.is_terminal() {
            return Ok(false);
        }
        self.transit(process_id, runtime_name, to, None)?;
        Ok(true)
    }

    fn emit_transition(
        &self,
        process_id: i64,
        runtime_name: &str,
        from: LifecycleStatus,
        to: LifecycleStatus,
    ) {
        self.deps.events.emit(RuntimeEvent::Lifecycle {
            process_id,
            runtime_name: runtime_name.to_string(),
            from,
            to,
            at: Utc::now().to_rfc3339(),
        });
    }

    fn current_status(&self, process_id: i64) -> AppResult<LifecycleStatus> {
        Ok(self.row(process_id)?.status)
    }

    fn row(&self, process_id: i64) -> AppResult<store::RuntimeProcessRow> {
        let conn = self.db.lock().unwrap();
        store::get_process(&conn, process_id)?
            .ok_or_else(|| AppError::NotFound(format!("runtime_processes 行 {process_id} 不存在")))
    }

    fn info(&self, process_id: i64) -> AppResult<RuntimeProcessInfo> {
        Ok(store::row_to_info(&self.row(process_id)?))
    }

    // ------------------------------------------------------------------
    // 内部：指标采样
    // ------------------------------------------------------------------

    #[cfg(test)]
    pub(crate) fn seed_cached_launch(
        &self,
        workspace_id: i64,
        runtime_name: &str,
        plan: LaunchPlan,
        strategy: RunStrategy,
    ) {
        self.launch_cache.lock().unwrap().insert(
            (workspace_id, runtime_name.to_string()),
            CachedLaunch { plan, strategy },
        );
    }
}

impl Drop for RuntimeProcessManager {
    fn drop(&mut self) {
        // 停 sampler；**不杀被托管进程**——它们按设计成为孤儿，下次启动由
        // reconcile 接管（任务文档「进程托管」）。
        self.sampler_stop.store(true, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests;
