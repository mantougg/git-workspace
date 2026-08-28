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

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use chrono::Utc;
use rusqlite::Connection;
use sysinfo::{Pid, ProcessRefreshKind, RefreshKind, System};

use crate::error::{AppError, AppResult};
use crate::maven::closure::RuntimeClosureCache;
use crate::maven::index::DependencyGraphCache;
use crate::process::kill_tree::kill_process_tree;
use crate::process::streaming::OutputStream;
use crate::runtime::build::pipeline::execute_build;
use crate::runtime::build::runner::{MavenRunner, SpawningMavenRunner};
use crate::runtime::build::scheduler::BuildScheduler;
use crate::runtime::build::{BuildOptions, BuildOutputSink, BuildRequest, LaunchPlan, RunStrategy};
use crate::runtime::config;
use crate::runtime::launch::launcher::{self, LaunchRunner, SystemLaunchRunner};
use crate::runtime::launch::store;
use crate::runtime::launch::{
    LifecycleStatus, LoggingEventSink, RuntimeEvent, RuntimeEventSink, RuntimeProcessInfo,
};
use crate::runtime::logs::redact::sensitive_env_values;
use crate::runtime::script_approval::ScriptApprovalStore;
use crate::runtime::logs::{LogPhase, LogSession, RuntimeLogEngine};

/// spawn 后判定 `Running` 的默认宽限（启动横幅命中可提前翻转）。
pub const DEFAULT_START_GRACE: Duration = Duration::from_secs(5);
/// Stop 的默认优雅宽限：SIGTERM 后等待退出，超时升级杀进程树。
pub const DEFAULT_STOP_GRACE: Duration = Duration::from_secs(10);
/// 指标采样默认间隔（低频节流，全局约束 §5）。
pub const DEFAULT_SAMPLE_INTERVAL: Duration = Duration::from_secs(2);
/// 每 N 拍采样落一次 DB（其余只发事件）。
const DB_FLUSH_EVERY_TICKS: u32 = 5;
/// adopted（非子进程）监控的轮询间隔。
const ADOPT_POLL_INTERVAL: Duration = Duration::from_secs(1);

/// 一次 Start 的选项。
#[derive(Debug, Clone)]
pub struct StartOptions {
    /// true = 复用最近构建产物（Restart 路径）；无缓存时回退完整构建并记日志。
    pub skip_build: bool,
    /// 透传 R-09 Build Engine（策略 / offline / 超时等）。
    pub build_options: BuildOptions,
    /// spawn 后判定 Running 的宽限。
    pub start_grace: Duration,
    /// R-15 §82：环境覆盖项（内存生效，不改 Runtime 配置文件）；None = 无覆盖。
    pub overrides: Option<EnvironmentOverrides>,
}

/// 环境编排（R-15）对单个服务的启动覆盖项。只存环境里声明的差异项：
/// JDK / Profile / 追加环境变量 / 端口。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EnvironmentOverrides {
    pub jdk: Option<String>,
    pub profile: Option<String>,
    pub environment: std::collections::BTreeMap<String, String>,
    pub port: Option<u16>,
}

impl Default for StartOptions {
    fn default() -> Self {
        Self {
            skip_build: false,
            build_options: BuildOptions::default(),
            start_grace: DEFAULT_START_GRACE,
            overrides: None,
        }
    }
}

/// 最近构建产物缓存（Restart 复用，验收标准 2）。进程内存驻留：
/// GitWorkspace 重启后首次 Start 总是完整构建。
struct CachedLaunch {
    plan: LaunchPlan,
    strategy: RunStrategy,
}

/// 活跃进程句柄：monitor/stop/sampler 共享的同步原语。
#[derive(Clone)]
struct ActiveProcess {
    /// 身份（R-12 `signal_build_cancel` 按 runtime 反查句柄，不经过 DB）。
    workspace_id: i64,
    runtime_name: String,
    /// spawn 后立即填充（streaming pid slot）。
    pid_slot: Arc<Mutex<Option<u32>>>,
    /// spawn 时记录的 OS start_time（防 PID 复用，adopted 行来自 DB）。
    pid_start_time: Arc<Mutex<Option<u64>>>,
    /// 构建阶段取消（Stop/Kill 打断 execute_build）。
    build_cancel: Arc<AtomicBool>,
    /// 强杀进程树信号（Force Kill / Stop grace 超时升级）。
    force_kill: Arc<AtomicBool>,
    /// monitor 进度：启动横幅命中 / 退出结果；Condvar 唤醒等待方。
    progress: Arc<(Mutex<Progress>, Condvar)>,
    /// 注册时刻（uptime 事件用单调时钟）。
    started_instant: Instant,
    /// 是否接管自上次会话的孤儿。
    adopted: bool,
}

#[derive(Debug, Default)]
struct Progress {
    running: bool,
    outcome: Option<MonitorOutcome>,
}

#[derive(Debug, Clone)]
struct MonitorOutcome {
    exit_code: Option<i32>,
    /// true = 走了 kill 树路径。
    cancelled: bool,
    /// spawn 失败（io）——进程从未起来。
    spawn_error: Option<String>,
}

impl ActiveProcess {
    fn new(adopted: bool, workspace_id: i64, runtime_name: &str) -> Self {
        Self {
            workspace_id,
            runtime_name: runtime_name.to_string(),
            pid_slot: Arc::new(Mutex::new(None)),
            pid_start_time: Arc::new(Mutex::new(None)),
            build_cancel: Arc::new(AtomicBool::new(false)),
            force_kill: Arc::new(AtomicBool::new(false)),
            progress: Arc::new((Mutex::new(Progress::default()), Condvar::new())),
            started_instant: Instant::now(),
            adopted,
        }
    }

    fn pid(&self) -> Option<u32> {
        *self.pid_slot.lock().unwrap()
    }

    fn signal_outcome(&self, outcome: MonitorOutcome) {
        let (lock, cv) = &*self.progress;
        lock.lock().unwrap().outcome = Some(outcome);
        cv.notify_all();
    }
}

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

    /// 启动一个 Runtime：构建（或复用缓存）→ spawn → Running 判定。
    ///
    /// 返回时状态已稳定为 Running / Stopped / Failed 之一；失败路径返回
    /// `Err`（BuildFailed / ProcessStartFailed 等结构化错误）且 DB 行落 Failed。
    pub fn start(
        self: &Arc<Self>,
        workspace_id: i64,
        runtime_name: &str,
        options: StartOptions,
    ) -> AppResult<RuntimeProcessInfo> {
        self.ensure_sampler();
        // 重复启动守卫：同一 (workspace, runtime) 只允许一个活跃进程。
        {
            let conn = self.db.lock().unwrap();
            if let Some(active) = store::find_active(&conn, workspace_id, runtime_name)? {
                return Err(AppError::Conflict(format!(
                    "Runtime '{runtime_name}' 已在运行（进程记录 #{}，状态 {}）。\
                     请先 Stop，或使用 Restart。",
                    active.id,
                    active.status.as_str()
                )));
            }
        }

        let process_id = {
            let conn = self.db.lock().unwrap();
            store::insert_process(&conn, workspace_id, runtime_name)?
        };
        let handle = ActiveProcess::new(false, workspace_id, runtime_name);
        self.active
            .lock()
            .unwrap()
            .insert(process_id, handle.clone());

        let result = self.start_inner(process_id, workspace_id, runtime_name, &options, &handle);
        if result.is_err() {
            // start_inner 的失败路径已负责状态落库与 outcome 信号；这里只摘牌。
            self.active.lock().unwrap().remove(&process_id);
        }
        result
    }

    fn start_inner(
        self: &Arc<Self>,
        process_id: i64,
        workspace_id: i64,
        runtime_name: &str,
        options: &StartOptions,
        handle: &ActiveProcess,
    ) -> AppResult<RuntimeProcessInfo> {
        // ---- Preparing：配置加载 + R-06 mainClass 回退 + 缓存判定 ----
        self.transit(process_id, runtime_name, LifecycleStatus::Preparing, None)?;
        let prepared = match self.prepare(workspace_id, runtime_name, options) {
            Ok(prepared) => prepared,
            Err(error) => {
                self.abort_before_spawn(process_id, runtime_name, handle, None);
                return Err(error);
            }
        };

        // ---- R-11 日志会话：构建 / 运行输出统一接管（脱敏后落盘）----
        // 日志目录不可写等失败直接终止 Start（可行动错误），不带病运行。
        if let Err(error) = self.open_log_session(workspace_id, runtime_name, process_id) {
            self.abort_before_spawn(process_id, runtime_name, handle, None);
            return Err(error);
        }

        // ---- Resolving / Building（skip-build 命中缓存时跳过）----
        let (plan, strategy) = match prepared {
            Prepared::Cached(cached) => {
                log::info!("R-10: reusing cached launch artifacts for '{runtime_name}'");
                (cached.plan, cached.strategy)
            }
            Prepared::NeedBuild(build_options) => {
                self.transit(process_id, runtime_name, LifecycleStatus::Resolving, None)?;
                // execute_build 内部（图/闭包/Reactor → Maven）无法插桩；
                // 构建主体是 Maven 调用，紧邻置位 Building（模块文档说明）。
                self.transit(process_id, runtime_name, LifecycleStatus::Building, None)?;
                match self.run_build(process_id, workspace_id, runtime_name, build_options, handle) {
                    Ok(built) => (built.plan, built.strategy),
                    Err(error) => {
                        // Stop/Kill 在构建期间介入：以停止语义收尾，不再算启动失败。
                        let current = self.current_status(process_id)?;
                        if current == LifecycleStatus::Stopping {
                            self.transit(process_id, runtime_name, LifecycleStatus::Stopped, None)?;
                            handle.signal_outcome(MonitorOutcome {
                                exit_code: None,
                                cancelled: true,
                                spawn_error: None,
                            });
                            self.active.lock().unwrap().remove(&process_id);
                            return self.info(process_id);
                        }
                        self.abort_before_spawn(process_id, runtime_name, handle, None);
                        return Err(error);
                    }
                }
            }
        };

        // ---- Starting：命令组装 + spawn ----
        self.transit(process_id, runtime_name, LifecycleStatus::Starting, None)?;
        let command = match launcher::launch_command(&plan, process_id, runtime_name) {
            Ok(command) => command,
            Err(error) => {
                self.abort_before_spawn(process_id, runtime_name, handle, None);
                return Err(error);
            }
        };
        {
            let conn = self.db.lock().unwrap();
            // preview / working_dir 先行落库；pid 在 spawn 后回填。
            if let Err(error) = store::set_launched_meta(
                &conn,
                process_id,
                strategy,
                &launcher::plan_preview(&plan),
                &launcher::plan_working_dir(&plan),
            ) {
                self.abort_before_spawn(process_id, runtime_name, handle, None);
                return Err(error);
            }
        }
        self.spawn_monitor(process_id, runtime_name.to_string(), command, handle);

        // spawn 失败 / 拿到 pid 之前进程就没了 → outcome 先到。
        let pid = match self.wait_pid_or_outcome(handle, Duration::from_secs(10)) {
            PidWait::Pid(pid) => pid,
            PidWait::Exited => return self.finish_early_exit(process_id, runtime_name, handle),
            PidWait::Timeout => {
                let error = AppError::ProcessStartFailed {
                    runtime: runtime_name.to_string(),
                    reason: "spawn 后 10s 内未能确认进程 pid".into(),
                };
                self.abort_before_spawn(process_id, runtime_name, handle, None);
                return Err(error);
            }
        };
        {
            let start_time = self.deps.launch_runner.start_time(pid);
            *handle.pid_start_time.lock().unwrap() = start_time;
            let conn = self.db.lock().unwrap();
            store::set_pid(&conn, process_id, pid, start_time)?;
        }

        // ---- Running 判定：横幅命中提前翻转；否则宽限到期仍存活即 Running ----
        match self.wait_running_or_outcome(handle, options.start_grace) {
            RunWait::Running => {
                self.transit(process_id, runtime_name, LifecycleStatus::Running, None)?;
                // R-16：进入 Running 后开启健康探针（配置缺失时引擎内部 no-op）。
                if let Some(health) = &self.deps.health {
                    health.start_monitor(process_id, workspace_id, runtime_name);
                }
                self.info(process_id)
            }
            RunWait::Exited => self.finish_early_exit(process_id, runtime_name, handle),
            RunWait::GraceElapsed => {
                let start_time = *handle.pid_start_time.lock().unwrap();
                if self.deps.launch_runner.alive(pid, start_time) {
                    self.transit(process_id, runtime_name, LifecycleStatus::Running, None)?;
                    // R-16：宽限边界翻转 Running 同样开启探针。
                    if let Some(health) = &self.deps.health {
                        health.start_monitor(process_id, workspace_id, runtime_name);
                    }
                    self.info(process_id)
                } else {
                    // 宽限边界上刚好退出：等 monitor 收尾后按退出分类。
                    self.wait_outcome(handle, Duration::from_secs(5));
                    self.finish_early_exit(process_id, runtime_name, handle)
                }
            }
        }
    }

    /// Preparing 阶段的准备工作：加载未脱敏配置（校验存在性）、R-06 推断
    /// mainClass（仅缺省时）、判定走缓存还是完整构建。
    fn prepare(
        &self,
        workspace_id: i64,
        runtime_name: &str,
        options: &StartOptions,
    ) -> AppResult<Prepared> {
        let (mut config, workspace_root) = {
            let conn = self.db.lock().unwrap();
            let config = config::load_config_unredacted(&conn, workspace_id, runtime_name)?;
            let root = config::workspace_root(&conn, workspace_id)?;
            (config, root)
        };

        // R-15 §82：环境覆盖项（内存生效，不改配置文件；应用层在五层合并的
        // Application 层之上追加，与「环境只存覆盖项」一致）。
        if let Some(overrides) = &options.overrides {
            if let Some(jdk) = &overrides.jdk {
                config.jdk = Some(jdk.clone());
            }
            if let Some(profile) = &overrides.profile {
                config.profile = Some(profile.clone());
            }
            config.environment.extend(overrides.environment.clone());
            if let Some(port) = overrides.port {
                config
                    .program_arguments
                    .retain(|arg| !arg.starts_with("--server.port="));
                config
                    .vm_options
                    .retain(|arg| !arg.starts_with("-Dserver.port="));
                config.program_arguments.push(format!("--server.port={port}"));
            }
        }

        // R-14 §79：启动前端口预检——显式端口被占用直接返回 PortOccupied
        // （带占用方 PID / 进程名，§80 可行动提示），避免启动后崩溃。
        super::port_preflight::preflight(&config)?;

        let mut build_options = options.build_options.clone();
        if config.main_class.is_none() {
            // R-06 回退：检测候选并取默认推断；找不到时不硬失败——
            // MavenRun/PackageRun 不需要 mainClass，ClasspathRun 会在
            // LaunchPlan 构造处给出可行动错误。
            match self.infer_main_class(&workspace_root, &config.project) {
                Ok(Some(inferred)) => {
                    log::info!("R-10: mainClass inferred via R-06 for '{runtime_name}': {inferred}");
                    build_options.main_class_override = Some(inferred);
                }
                Ok(None) => log::debug!("R-10: no main class candidate for '{runtime_name}'"),
                Err(error) => log::warn!("R-10: main class detection failed: {error}"),
            }
        }

        if options.skip_build {
            let cached = self
                .launch_cache
                .lock()
                .unwrap()
                .get(&(workspace_id, runtime_name.to_string()))
                .map(|cached| CachedLaunch {
                    plan: cached.plan.clone(),
                    strategy: cached.strategy,
                });
            if let Some(cached) = cached {
                return Ok(Prepared::Cached(cached));
            }
            log::info!(
                "R-10: skip_build requested for '{runtime_name}' but no cached artifacts; \
                 falling back to a full build"
            );
        }
        Ok(Prepared::NeedBuild(build_options))
    }

    /// R-06 自动推断默认 mainClass：按 Runtime 配置的 project 匹配检测结果。
    /// 路径比较对 Windows 分隔符不敏感（配置可能是 `\`、`/` 或混合，R-14 修复）。
    fn infer_main_class(
        &self,
        workspace_root: &std::path::Path,
        project: &str,
    ) -> AppResult<Option<String>> {
        let discovery = crate::maven::discover_poms(workspace_root, 5, None, None);
        let result = crate::runtime::spring_boot::detect_spring_boot_workspace(
            &discovery.projects,
            &discovery.effective,
            None,
        );
        let needle = project.replace('\\', "/");
        let found = result.projects.iter().find(|candidate| {
            let path = candidate.project_path.to_string_lossy().replace('\\', "/");
            path == needle || candidate.module == project
        });
        Ok(found.and_then(|candidate| candidate.default_main_class.clone()))
    }

    /// R-11：开启本次 Start 的日志会话（构建 + 运行输出统一进同一文件）。
    /// 脱敏秘密值取自五层合并环境（与构建/启动环境同源）；仅在内存持有。
    fn open_log_session(
        &self,
        workspace_id: i64,
        runtime_name: &str,
        process_id: i64,
    ) -> AppResult<()> {
        let (workspace_root, secrets) = {
            let conn = self.db.lock().unwrap();
            let root = config::workspace_root(&conn, workspace_id)?;
            let env = config::resolve_environment(&conn, workspace_id, runtime_name)?;
            let env: Vec<(String, String)> = env.into_iter().collect();
            (root, sensitive_env_values(&env))
        };
        self.deps.logs.open_session(
            &workspace_root,
            runtime_name,
            process_id,
            secrets,
            self.deps.events.clone(),
        )?;
        Ok(())
    }

    /// 驱动 R-09 构建流水线。构建输出经 `BuildLogSink` 进入 R-11 日志会话
    /// （流水线 RedactingSink 已脱敏一次，会话侧再脱敏是幂等防御）；
    /// `BuildFailed.log_tail` 由流水线内部的 RingTail 保障。
    fn run_build(
        &self,
        process_id: i64,
        workspace_id: i64,
        runtime_name: &str,
        build_options: BuildOptions,
        handle: &ActiveProcess,
    ) -> AppResult<Built> {
        let workspace_root = {
            let conn = self.db.lock().unwrap();
            config::workspace_root(&conn, workspace_id)?
        };
        let request = BuildRequest {
            workspace_id,
            runtime_name: runtime_name.to_string(),
            options: build_options,
        };
        let outcome = {
            // R-12：不在整个构建期间持 DB 锁——execute_build 按阶段自行加锁，
            // Maven 运行期间锁是空闲的（并发构建 / UI 查询不被阻塞）。
            let mut sink = BuildLogSink {
                session: self.deps.logs.session(process_id),
            };
            execute_build(
                &self.db,
                &workspace_root,
                &self.deps.graph_cache,
                &self.deps.closure_cache,
                &self.deps.scheduler,
                &*self.deps.maven_runner,
                &request,
                &self.deps.script_approvals,
                &mut sink,
                Some(&handle.build_cancel),
            )?
        };
        self.launch_cache.lock().unwrap().insert(
            (workspace_id, runtime_name.to_string()),
            CachedLaunch {
                plan: outcome.launch.clone(),
                strategy: outcome.strategy,
            },
        );
        Ok(Built {
            plan: outcome.launch,
            strategy: outcome.strategy,
        })
    }

    // ------------------------------------------------------------------
    // Stop / Kill / Restart（§34）
    // ------------------------------------------------------------------

    /// 优雅停止：SIGTERM（Unix）→ grace 等待 → 超时升级杀进程树。
    /// Windows 无 SIGTERM 语义：`terminate` 返回 false，直接升级强杀。
    /// 幂等：终态行直接返回当前快照。
    pub fn stop(
        self: &Arc<Self>,
        process_id: i64,
        grace: Option<Duration>,
    ) -> AppResult<RuntimeProcessInfo> {
        let grace = grace.unwrap_or(DEFAULT_STOP_GRACE);
        let row = self.row(process_id)?;
        if row.status.is_terminal() {
            return Ok(store::row_to_info(&row));
        }
        let runtime_name = row.runtime_name.clone();
        let handle = self.active.lock().unwrap().get(&process_id).cloned();
        match handle {
            Some(handle) => {
                if !self.transit_lenient(process_id, &runtime_name, LifecycleStatus::Stopping)? {
                    return self.info(process_id);
                }
                handle.build_cancel.store(true, Ordering::Relaxed);
                if let Some(pid) = handle.pid() {
                    if !self.deps.launch_runner.terminate(pid) {
                        handle.force_kill.store(true, Ordering::Relaxed);
                        if handle.adopted {
                            kill_process_tree(pid);
                        }
                    }
                }
                if !self.wait_outcome(&handle, grace) {
                    log::warn!(
                        "R-10: grace expired stopping '{runtime_name}' (#{process_id}); \
                         escalating to process-tree kill"
                    );
                    handle.force_kill.store(true, Ordering::Relaxed);
                    // F-12：monitor 正常会消费 force_kill 杀树；此处直杀兜底
                    // monitor 失联（如输出 reader 全断的旧路径）造成的进程残留。
                    // kill_process_tree 对已死进程是 no-op，重复调用安全。
                    if let Some(pid) = handle.pid() {
                        kill_process_tree(pid);
                    }
                    self.wait_outcome(&handle, Duration::from_secs(5));
                }
                self.info(process_id)
            }
            None => self.stop_unmanaged(&row, grace),
        }
    }

    /// 停止某个 Runtime 当前活跃的进程；无活跃进程时返回 `None`。
    pub fn stop_runtime(
        self: &Arc<Self>,
        workspace_id: i64,
        runtime_name: &str,
        grace: Option<Duration>,
    ) -> AppResult<Option<RuntimeProcessInfo>> {
        let active = {
            let conn = self.db.lock().unwrap();
            store::find_active(&conn, workspace_id, runtime_name)?
        };
        match active {
            Some(row) => self.stop(row.id, grace).map(Some),
            None => Ok(None),
        }
    }

    /// R-12 任务取消的快路径：仅凭内存句柄置 `build_cancel`（streaming
    /// runner 50ms 轮询后杀 Maven 进程树），不经过 DB——构建期间 DB 写锁
    /// 被 `execute_build` 持有，等锁会把取消延迟到构建自然结束。
    /// 返回是否找到了活跃句柄；后续的 DB 状态迁移由 `stop_runtime` 完成。
    pub fn signal_build_cancel(&self, workspace_id: i64, runtime_name: &str) -> bool {
        let active = self.active.lock().unwrap();
        for handle in active.values() {
            if handle.workspace_id == workspace_id && handle.runtime_name == runtime_name {
                handle.build_cancel.store(true, Ordering::Relaxed);
                return true;
            }
        }
        false
    }

    /// Force Kill（全局约束 §3 二次确认）：`confirmed=false` 直接拒绝。
    /// 立即杀整棵进程树，不发优雅信号。
    pub fn kill(
        self: &Arc<Self>,
        process_id: i64,
        confirmed: bool,
    ) -> AppResult<RuntimeProcessInfo> {
        if !confirmed {
            return Err(AppError::Permission(format!(
                "Force Kill 会直接终止 runtime_processes #{process_id} 的整棵进程树 \
                 （SIGKILL 语义，应用无优雅关闭机会）。请带 confirmed=true 二次确认后重试"
            )));
        }
        let row = self.row(process_id)?;
        if row.status.is_terminal() {
            return Ok(store::row_to_info(&row));
        }
        let runtime_name = row.runtime_name.clone();
        let handle = self.active.lock().unwrap().get(&process_id).cloned();
        if !self.transit_lenient(process_id, &runtime_name, LifecycleStatus::Stopping)? {
            return self.info(process_id);
        }
        match handle {
            Some(handle) => {
                handle.build_cancel.store(true, Ordering::Relaxed);
                handle.force_kill.store(true, Ordering::Relaxed);
                if handle.adopted {
                    // adopted 进程没有 spawn_streaming 盯 kill flag，直接杀树。
                    if let Some(pid) = handle.pid() {
                        kill_process_tree(pid);
                    }
                }
                self.wait_outcome(&handle, Duration::from_secs(5));
            }
            None => {
                if let Some(pid) = row.pid {
                    if self.deps.launch_runner.alive(pid, row.pid_start_time) {
                        kill_process_tree(pid);
                        self.wait_dead(pid, row.pid_start_time, Duration::from_secs(2));
                    }
                }
                self.transit_lenient(process_id, &runtime_name, LifecycleStatus::Stopped)?;
            }
        }
        self.info(process_id)
    }

    /// Restart = Stop + Start（复用最近构建产物，验收标准 2）。
    pub fn restart(
        self: &Arc<Self>,
        workspace_id: i64,
        runtime_name: &str,
        mut options: StartOptions,
    ) -> AppResult<RuntimeProcessInfo> {
        if self.stop_runtime(workspace_id, runtime_name, None)?.is_some() {
            log::info!("R-10: restart stopped previous instance of '{runtime_name}'");
        }
        options.skip_build = true;
        self.start(workspace_id, runtime_name, options)
    }

    // ------------------------------------------------------------------
    // 进程托管：重启后的孤儿对账（§33）
    // ------------------------------------------------------------------

    /// GitWorkspace 启动时调用（R-12 接入）：对账上次会话遗留的非终态行。
    ///
    /// - 活进程（pid + start_time 匹配）→ 接管（adopted），恢复 Stop/Kill 与
    ///   指标采样；上次退出时处于 Stopping 的补发 SIGTERM 完成停止。
    /// - 死进程 → Starting/Running 落 Failed（`ProcessCrashed`，退出码不可得
    ///   记 None）；Stopping 落 Stopped。
    /// - 从未 spawn 的行（Created/Preparing/Resolving/Building）→ Failed
    ///   （启动被 GitWorkspace 退出打断）。
    pub fn reconcile_on_startup(
        self: &Arc<Self>,
        workspace_id: i64,
    ) -> AppResult<Vec<RuntimeProcessInfo>> {
        self.ensure_sampler();
        let rows = {
            let conn = self.db.lock().unwrap();
            store::list_unfinished(&conn, workspace_id)?
        };
        let mut adopted = Vec::new();
        for row in rows {
            let name = row.runtime_name.clone();
            match row.status {
                LifecycleStatus::Starting | LifecycleStatus::Running | LifecycleStatus::Stopping => {
                    let alive = match (row.pid, row.pid_start_time) {
                        (Some(pid), Some(start_time)) => {
                            self.deps.launch_runner.alive(pid, Some(start_time))
                        }
                        _ => false,
                    };
                    if !alive {
                        let to = if row.status == LifecycleStatus::Stopping {
                            LifecycleStatus::Stopped
                        } else {
                            LifecycleStatus::Failed
                        };
                        self.transit_lenient(row.id, &name, to)?;
                        if to == LifecycleStatus::Failed {
                            log::warn!(
                                "R-10: reconcile found '{}' (#{}) gone while GitWorkspace was \
                                 not running; marked Failed (exit code unavailable)",
                                name,
                                row.id
                            );
                        }
                        continue;
                    }
                    // 接管孤儿。
                    let pid = row.pid.expect("alive implies pid");
                    let start_time = row.pid_start_time.expect("alive implies start_time");
                    {
                        let conn = self.db.lock().unwrap();
                        store::set_adopted(&conn, row.id)?;
                    }
                    if row.status == LifecycleStatus::Stopping {
                        // 上次退出时正在停止：补一枪完成它。
                        self.deps.launch_runner.terminate(pid);
                    } else if row.status == LifecycleStatus::Starting {
                        self.transit_lenient(row.id, &name, LifecycleStatus::Running)?;
                    }
                    let handle = ActiveProcess::new(true, workspace_id, &name);
                    *handle.pid_slot.lock().unwrap() = Some(pid);
                    *handle.pid_start_time.lock().unwrap() = Some(start_time);
                    self.active
                        .lock()
                        .unwrap()
                        .insert(row.id, handle.clone());
                    self.spawn_adopted_monitor(row.id, name.clone(), pid, start_time, handle);
                    // R-16：接管的 Running 孤儿同样恢复健康探针。
                    if let Some(health) = &self.deps.health {
                        health.start_monitor(row.id, workspace_id, &name);
                    }
                    adopted.push(store::row_to_info(&self.row(row.id)?));
                    log::info!("R-10: adopted orphan process pid={pid} for runtime '{name}'");
                }
                // 从未 spawn 的半成品行。
                _ => {
                    self.transit_lenient(row.id, &name, LifecycleStatus::Failed)?;
                }
            }
        }
        Ok(adopted)
    }

    // ------------------------------------------------------------------
    // 查询
    // ------------------------------------------------------------------

    pub fn get_process(&self, process_id: i64) -> AppResult<Option<RuntimeProcessInfo>> {
        let conn = self.db.lock().unwrap();
        Ok(store::get_process(&conn, process_id)?.map(|row| store::row_to_info(&row)))
    }

    pub fn list_processes(&self, workspace_id: i64) -> AppResult<Vec<RuntimeProcessInfo>> {
        let conn = self.db.lock().unwrap();
        Ok(store::list_processes(&conn, workspace_id)?
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

    /// spawn 托管 monitor 线程：跑 `LaunchRunner::run` 直到退出，期间把输出
    /// 行送进 R-11 日志会话（内部脱敏落盘）、探测启动横幅与端口，结束后
    /// 先收口日志会话（drain + 完整落盘）再按分类收尾状态。
    fn spawn_monitor(
        self: &Arc<Self>,
        process_id: i64,
        runtime_name: String,
        mut command: std::process::Command,
        handle: &ActiveProcess,
    ) {
        let this = Arc::clone(self);
        let handle = handle.clone();
        std::thread::spawn(move || {
            let log_session = this.deps.logs.session(process_id);
            let mut ports_seen: Vec<u16> = Vec::new();
            let mut running_flagged = false;
            let result = this.deps.launch_runner.run(
                &mut command,
                &handle.force_kill,
                &handle.pid_slot,
                &mut |stream: OutputStream, line: &str| {
                    // R-11：原始行进日志会话（脱敏在会话内部、落盘前完成）；
                    // 横幅/端口探测保持在原始行上进行，行为与 R-10 一致。
                    if let Some(session) = &log_session {
                        session.log(LogPhase::Run, stream, line);
                    }
                    let (started_re, port_re) = startup_detectors();
                    if !running_flagged && started_re.is_match(line) {
                        running_flagged = true;
                        let (lock, cv) = &*handle.progress;
                        lock.lock().unwrap().running = true;
                        cv.notify_all();
                    }
                    if let Some(captures) = port_re.captures(line) {
                        if let Some(Ok(port)) = captures.get(1).map(|m| m.as_str().parse::<u16>())
                        {
                            if !ports_seen.contains(&port) {
                                ports_seen.push(port);
                                let conn = this.db.lock().unwrap();
                                if let Err(error) = store::set_ports(&conn, process_id, &ports_seen)
                                {
                                    log::warn!("R-10: failed to persist ports: {error}");
                                } else {
                                    this.deps.events.emit(RuntimeEvent::Ports {
                                        process_id,
                                        ports: ports_seen.clone(),
                                    });
                                }
                            }
                        }
                    }
                },
            );
            // 先收口日志会话（worker drain + 完整落盘），再发布终态——
            // 终态可观测时日志已可完整回查（R-11 验收标准）。
            this.deps.logs.finish_session(process_id);
            let outcome = match result {
                Ok(exit) => MonitorOutcome {
                    exit_code: exit.exit_code,
                    cancelled: exit.cancelled,
                    spawn_error: None,
                },
                Err(error) => MonitorOutcome {
                    exit_code: None,
                    cancelled: false,
                    spawn_error: Some(error.to_string()),
                },
            };
            this.finalize_exit(process_id, &runtime_name, outcome, &handle);
        });
    }

    /// adopted 进程的轮询 monitor（非子进程，拿不到 wait()/退出码）。
    fn spawn_adopted_monitor(
        self: &Arc<Self>,
        process_id: i64,
        runtime_name: String,
        pid: u32,
        start_time: u64,
        handle: ActiveProcess,
    ) {
        let this = Arc::clone(self);
        std::thread::spawn(move || {
            loop {
                std::thread::sleep(ADOPT_POLL_INTERVAL);
                if handle.force_kill.load(Ordering::Relaxed) {
                    kill_process_tree(pid);
                }
                if !this.deps.launch_runner.alive(pid, Some(start_time)) {
                    this.finalize_exit(
                        process_id,
                        &runtime_name,
                        MonitorOutcome {
                            exit_code: None,
                            cancelled: false,
                            spawn_error: None,
                        },
                        &handle,
                    );
                    break;
                }
            }
        });
    }

    /// 退出收尾：按「停止中 / 退出码 / 是否 adopted」分类终态并落库、发事件。
    /// 分类规则见 [`classify_exit`]。幂等：行已是终态时直接返回。
    fn finalize_exit(
        self: &Arc<Self>,
        process_id: i64,
        runtime_name: &str,
        outcome: MonitorOutcome,
        handle: &ActiveProcess,
    ) {
        if outcome.cancelled {
            // kill 标志置位导致的退出流取消：信号已发，exit_code 多半取不到，日志留痕便于排查。
            log::debug!(
                "R-10: monitor of process #{process_id} ('{runtime_name}') was cancelled by kill flag"
            );
        }
        let (from, to, crashed) = {
            let conn = self.db.lock().unwrap();
            let row = match store::get_process(&conn, process_id) {
                Ok(Some(row)) => row,
                _ => return,
            };
            let from = row.status;
            if from.is_terminal() {
                return; // 已被其他路径收尾（幂等）。
            }
            let (to, crashed) = classify_exit(from, &outcome, handle.adopted);
            if handle.adopted && outcome.exit_code.is_none() && to == LifecycleStatus::Stopped {
                log::info!(
                    "R-10: adopted process #{process_id} ('{runtime_name}') exited; \
                     exit code unavailable to a non-parent"
                );
            }
            if let Err(error) =
                store::transition_status(&conn, process_id, to, Some(outcome.exit_code))
            {
                log::error!("R-10: failed to finalize process #{process_id}: {error}");
                return;
            }
            (from, to, crashed)
        };
        self.emit_transition(process_id, runtime_name, from, to);
        self.deps.events.emit(RuntimeEvent::Exited {
            process_id,
            runtime_name: runtime_name.to_string(),
            exit_code: outcome.exit_code,
            crashed,
        });
        // R-16：进程退出收口健康探针（快照翻 Stopped 并广播；无探针时 no-op）。
        if let Some(health) = &self.deps.health {
            health.stop_monitor(process_id);
        }
        if crashed {
            log::warn!(
                "R-10: {}",
                AppError::ProcessCrashed {
                    runtime: runtime_name.to_string(),
                    pid: handle.pid(),
                    exit_code: outcome.exit_code,
                }
            );
        }
        handle.signal_outcome(outcome);
        self.active.lock().unwrap().remove(&process_id);
    }

    /// spawn 前阶段（Preparing/Resolving/Building/Starting）的失败收尾：
    /// 若 Stop 已介入（Stopping）则尊重停止语义落 Stopped，否则落 Failed。
    /// 同时收口 R-11 日志会话（幂等；会话未开启时 no-op）。
    fn abort_before_spawn(
        &self,
        process_id: i64,
        runtime_name: &str,
        handle: &ActiveProcess,
        exit_code: Option<i32>,
    ) {
        self.deps.logs.finish_session(process_id);
        let current = self.current_status(process_id).unwrap_or(LifecycleStatus::Failed);
        let to = if current == LifecycleStatus::Stopping {
            LifecycleStatus::Stopped
        } else {
            LifecycleStatus::Failed
        };
        if let Err(error) = self.transit(process_id, runtime_name, to, Some(exit_code)) {
            log::error!("R-10: abort transition failed for #{process_id}: {error}");
        }
        handle.signal_outcome(MonitorOutcome {
            exit_code,
            cancelled: false,
            spawn_error: None,
        });
    }

    /// 「Running 之前就退出」的启动结果整理：monitor 已 finalize；把 Failed
    /// 翻译成 `ProcessStartFailed` 错误返回（自然退出码 0 的返回 Stopped 快照）。
    fn finish_early_exit(
        &self,
        process_id: i64,
        runtime_name: &str,
        handle: &ActiveProcess,
    ) -> AppResult<RuntimeProcessInfo> {
        self.wait_outcome(handle, Duration::from_secs(5));
        let row = self.row(process_id)?;
        let info = store::row_to_info(&row);
        if row.status == LifecycleStatus::Failed {
            let outcome = handle.progress.0.lock().unwrap().outcome.clone();
            let reason = match outcome {
                Some(MonitorOutcome {
                    spawn_error: Some(error),
                    ..
                }) => format!("进程 spawn 失败：{error}。请检查 JDK 路径与启动命令"),
                Some(MonitorOutcome { exit_code, .. }) => format!(
                    "进程在启动宽限期内退出（退出码 {exit_code:?}）。\
                     请查看应用日志确认启动失败原因"
                ),
                None => "进程在启动宽限期内退出".to_string(),
            };
            return Err(AppError::ProcessStartFailed {
                runtime: runtime_name.to_string(),
                reason,
            });
        }
        Ok(info)
    }

    /// 非本 manager 托管行的停止（例如 reconcile 之前直接 Stop）：按 OS
    /// 进程实测发信号/杀树，轮询等死，最后落 Stopped。
    fn stop_unmanaged(
        self: &Arc<Self>,
        row: &store::RuntimeProcessRow,
        grace: Duration,
    ) -> AppResult<RuntimeProcessInfo> {
        let name = row.runtime_name.clone();
        if !self.transit_lenient(row.id, &name, LifecycleStatus::Stopping)? {
            return self.info(row.id);
        }
        if let Some(pid) = row.pid {
            if self.deps.launch_runner.alive(pid, row.pid_start_time) {
                if !self.deps.launch_runner.terminate(pid) {
                    kill_process_tree(pid);
                }
                self.wait_dead(pid, row.pid_start_time, grace);
                if self.deps.launch_runner.alive(pid, row.pid_start_time) {
                    kill_process_tree(pid);
                    self.wait_dead(pid, row.pid_start_time, Duration::from_secs(2));
                }
            }
        }
        self.transit_lenient(row.id, &name, LifecycleStatus::Stopped)?;
        self.info(row.id)
    }

    /// 轮询等待进程消失（非子进程场景，无 wait() 可用）。
    fn wait_dead(&self, pid: u32, start_time: Option<u64>, timeout: Duration) {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if !self.deps.launch_runner.alive(pid, start_time) {
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    // ------------------------------------------------------------------
    // 内部：等待原语
    // ------------------------------------------------------------------

    fn wait_pid_or_outcome(&self, handle: &ActiveProcess, timeout: Duration) -> PidWait {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(pid) = handle.pid() {
                return PidWait::Pid(pid);
            }
            {
                let (lock, _) = &*handle.progress;
                if lock.lock().unwrap().outcome.is_some() {
                    return PidWait::Exited;
                }
            }
            if Instant::now() > deadline {
                return PidWait::Timeout;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn wait_running_or_outcome(&self, handle: &ActiveProcess, grace: Duration) -> RunWait {
        let (lock, cv) = &*handle.progress;
        let mut guard = lock.lock().unwrap();
        let deadline = Instant::now() + grace;
        loop {
            if guard.outcome.is_some() {
                return RunWait::Exited;
            }
            if guard.running {
                return RunWait::Running;
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return RunWait::GraceElapsed;
            }
            let (next, _) = cv
                .wait_timeout(guard, remaining.min(Duration::from_millis(50)))
                .unwrap();
            guard = next;
        }
    }

    /// 等待 monitor 收尾；true = 已有 outcome。
    fn wait_outcome(&self, handle: &ActiveProcess, timeout: Duration) -> bool {
        let (lock, cv) = &*handle.progress;
        let mut guard = lock.lock().unwrap();
        let deadline = Instant::now() + timeout;
        loop {
            if guard.outcome.is_some() {
                return true;
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return false;
            }
            let (next, _) = cv
                .wait_timeout(guard, remaining.min(Duration::from_millis(100)))
                .unwrap();
            guard = next;
        }
    }

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
        store::get_process(&conn, process_id)?.ok_or_else(|| {
            AppError::NotFound(format!("runtime_processes 行 {process_id} 不存在"))
        })
    }

    fn info(&self, process_id: i64) -> AppResult<RuntimeProcessInfo> {
        Ok(store::row_to_info(&self.row(process_id)?))
    }

    // ------------------------------------------------------------------
    // 内部：指标采样
    // ------------------------------------------------------------------

    fn ensure_sampler(self: &Arc<Self>) {
        if self.sampler_started.swap(true, Ordering::Relaxed) {
            return;
        }
        let this = Arc::clone(self);
        let interval = this.deps.sample_interval;
        let handle = std::thread::spawn(move || this.sampler_loop(interval));
        *self.sampler_handle.lock().unwrap() = Some(handle);
    }

    fn sampler_loop(&self, interval: Duration) {
        let mut system = System::new_with_specifics(
            RefreshKind::new().with_processes(ProcessRefreshKind::new().with_cpu().with_memory()),
        );
        let mut tick: u32 = 0;
        while !self.sampler_stop.load(Ordering::Relaxed) {
            std::thread::sleep(interval);
            if self.sampler_stop.load(Ordering::Relaxed) {
                break;
            }
            let targets: Vec<(i64, u32, Instant)> = self
                .active
                .lock()
                .unwrap()
                .iter()
                .filter_map(|(id, handle)| {
                    handle.pid().map(|pid| (*id, pid, handle.started_instant))
                })
                .collect();
            if targets.is_empty() {
                continue;
            }
            // sysinfo 读 OS 计数器（/proc），不为采样 fork 进程。
            system.refresh_processes();
            tick = tick.wrapping_add(1);
            for (process_id, pid, started) in targets {
                let Some(process) = system.process(Pid::from_u32(pid)) else {
                    continue; // 已退出；monitor 负责收尾。
                };
                let cpu = process.cpu_usage();
                let memory = process.memory();
                self.deps.events.emit(RuntimeEvent::Metrics {
                    process_id,
                    cpu_percent: cpu,
                    memory_bytes: memory,
                    uptime_seconds: started.elapsed().as_secs(),
                });
                if tick.is_multiple_of(DB_FLUSH_EVERY_TICKS) {
                    let conn = self.db.lock().unwrap();
                    if let Err(error) = store::set_metrics(&conn, process_id, cpu, memory) {
                        log::debug!("R-10: metrics flush skipped for #{process_id}: {error}");
                    }
                }
            }
        }
    }

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

/// 退出分类（finalize_exit 的纯函数部分）：
/// - Stopping 期间退出 → Stopped（用户意图优先，退出码仍记录）；
/// - spawn 失败 → Failed（ProcessStartFailed 语义）；
/// - 退出码 0 → Stopped（自然终止）；
/// - adopted 且无码 → Stopped（非父进程拿不到码，宽容处理并记日志）；
/// - 其余（非零 / 被信号杀）→ Failed（ProcessCrashed 语义）。
fn classify_exit(
    from: LifecycleStatus,
    outcome: &MonitorOutcome,
    adopted: bool,
) -> (LifecycleStatus, bool) {
    if from == LifecycleStatus::Stopping {
        return (LifecycleStatus::Stopped, false);
    }
    if outcome.spawn_error.is_some() {
        return (LifecycleStatus::Failed, false);
    }
    match outcome.exit_code {
        Some(0) => (LifecycleStatus::Stopped, false),
        _ if adopted => (LifecycleStatus::Stopped, false),
        _ => (LifecycleStatus::Failed, true),
    }
}

struct Built {
    plan: LaunchPlan,
    strategy: RunStrategy,
}

enum PidWait {
    Pid(u32),
    Exited,
    Timeout,
}

enum RunWait {
    Running,
    Exited,
    GraceElapsed,
}

enum Prepared {
    Cached(CachedLaunch),
    NeedBuild(BuildOptions),
}

/// R-11 构建输出挂接点：把构建阶段的行转发进本次 Start 的日志会话。
/// 行已被流水线 RedactingSink 脱敏；会话侧的再脱敏是幂等防御。
/// 会话不存在（防御分支）时静默丢弃。
struct BuildLogSink {
    session: Option<Arc<LogSession>>,
}

impl BuildOutputSink for BuildLogSink {
    fn on_line(&mut self, stream: OutputStream, line: &str) {
        if let Some(session) = &self.session {
            session.log(LogPhase::Build, stream, line);
        }
    }
}

/// 启动横幅 / 端口探测正则（只读日志流，不做端口扫描；端口管理归 R-16）。
fn startup_detectors() -> &'static (regex::Regex, regex::Regex) {
    static DETECTORS: std::sync::OnceLock<(regex::Regex, regex::Regex)> = std::sync::OnceLock::new();
    DETECTORS.get_or_init(|| {
        (
            // Spring Boot 启动完成横幅："Started Application in 3.2 seconds ..."。
            regex::Regex::new(r"Started \S+ in [\d.]+ seconds").unwrap(),
            // 内嵌容器端口："Tomcat started on port 8080 (http) ..." /
            // 旧版 "Tomcat started on port(s): 8080" / Netty 同构。
            regex::Regex::new(r"started on port(?:\(s\))?:?\s+(\d+)").unwrap(),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::build::runner::{FakeMavenRunner, FakeRun};
    use crate::runtime::build::BuildOptions;
    use crate::runtime::config::{
        create_config, CreateRuntimeConfigRequest, RuntimeApplicationConfig,
    };
    use crate::runtime::launch::launcher::{FakeBehavior, FakeLaunch, FakeLaunchRunner};
    use crate::runtime::launch::VecEventSink;
    use std::path::{Path, PathBuf};

    // --------------------------------------------------------------
    // fixtures
    // --------------------------------------------------------------

    fn write(path: &Path, content: &str) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, content).unwrap();
    }

    fn unique_root(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "gw_r10_{tag}_{}",
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ))
    }

    /// 最小 fixture：tempdir + workspace 行 + Runtime 配置（skip-build 路径
    /// 不需要 Maven 索引）。
    struct MiniFixture {
        root: PathBuf,
        db: Arc<Mutex<Connection>>,
        workspace_id: i64,
    }

    fn mini_fixture(name: &str) -> MiniFixture {
        let root = unique_root(name);
        std::fs::create_dir_all(&root).unwrap();
        let mut conn = Connection::open_in_memory().unwrap();
        crate::db::init_db(&mut conn).unwrap();
        conn.execute(
            "INSERT INTO workspaces (name, path, created_at, updated_at) VALUES ('w', ?1, 't', 't')",
            [root.to_string_lossy().to_string()],
        )
        .unwrap();
        let workspace_id = conn.last_insert_rowid();
        let db = Arc::new(Mutex::new(conn));
        {
            let conn = db.lock().unwrap();
            create_config(
                &conn,
                &CreateRuntimeConfigRequest {
                    workspace_id,
                    config: RuntimeApplicationConfig {
                        name: "app".into(),
                        project: "app".into(),
                        main_class: Some("com.example.Application".into()),
                        ..Default::default()
                    },
                },
            )
            .unwrap();
        }
        MiniFixture {
            root,
            db,
            workspace_id,
        }
    }

    /// Maven 构建路径 fixture：单仓 parent(pom) + lib(jar) + app(jar→lib)，
    /// 同步依赖图索引（对照 R-09 pipeline 测试 fixture）。
    struct MavenFixture {
        root: PathBuf,
        db: Arc<Mutex<Connection>>,
        workspace_id: i64,
    }

    fn maven_fixture(name: &str, spring_boot: bool) -> MavenFixture {
        let root = unique_root(name);
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
        let app_dependency = if spring_boot {
            "<dependencies><dependency><groupId>com.example</groupId><artifactId>lib</artifactId>\
             <version>1.0.0</version></dependency><dependency><groupId>org.springframework.boot</groupId>\
             <artifactId>spring-boot-starter</artifactId><version>3.2.5</version></dependency></dependencies>"
        } else {
            "<dependencies><dependency><groupId>com.example</groupId><artifactId>lib</artifactId>\
             <version>1.0.0</version></dependency></dependencies>"
        };
        write(
            &root.join("repo/app/pom.xml"),
            &format!(
                "<project><modelVersion>4.0.0</modelVersion><parent><groupId>com.example</groupId>\
                 <artifactId>parent</artifactId><version>1.0.0</version></parent>\
                 <artifactId>app</artifactId>{app_dependency}</project>"
            ),
        );
        if spring_boot {
            write(
                &root.join("repo/app/src/main/java/com/example/app/Application.java"),
                "package com.example.app;\n\
                 import org.springframework.boot.autoconfigure.SpringBootApplication;\n\
                 @SpringBootApplication\npublic class Application {\n    public static void main(String[] args) {}\n}\n",
            );
        }
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
        let discovery = crate::maven::discover_poms(&root, 5, None, None);
        assert!(discovery.errors.is_empty(), "{:?}", discovery.errors);
        crate::maven::sync_workspace_index(&mut conn, workspace_id, &discovery, &root.join("m2"))
            .unwrap();

        let config = RuntimeApplicationConfig {
            name: "app".into(),
            project: root.join("repo/app/pom.xml").to_string_lossy().to_string(),
            main_class: (!spring_boot).then(|| "com.example.app.Application".to_string()),
            ..Default::default()
        };
        create_config(
            &conn,
            &CreateRuntimeConfigRequest {
                workspace_id,
                config,
            },
        )
        .unwrap();
        MavenFixture {
            root,
            db: Arc::new(Mutex::new(conn)),
            workspace_id,
        }
    }

    fn test_manager(
        db: Arc<Mutex<Connection>>,
        launch_runner: Arc<dyn LaunchRunner>,
        maven_runner: Arc<dyn MavenRunner>,
        events: Arc<VecEventSink>,
        sample_interval: Duration,
    ) -> Arc<RuntimeProcessManager> {
        Arc::new(RuntimeProcessManager::with_deps(
            db,
            RuntimeProcessDeps {
                launch_runner,
                maven_runner,
                events,
                sample_interval,
                ..Default::default()
            },
        ))
    }

    fn lifecycle_chain(events: &VecEventSink, process_id: i64) -> Vec<(LifecycleStatus, LifecycleStatus)> {
        events
            .collected()
            .iter()
            .filter_map(|event| match event {
                RuntimeEvent::Lifecycle {
                    process_id: id,
                    from,
                    to,
                    ..
                } if *id == process_id => Some((*from, *to)),
                _ => None,
            })
            .collect()
    }

    fn wait_for_status(
        manager: &RuntimeProcessManager,
        process_id: i64,
        status: LifecycleStatus,
        timeout: Duration,
    ) -> RuntimeProcessInfo {
        let deadline = Instant::now() + timeout;
        loop {
            let info = manager.get_process(process_id).unwrap().unwrap();
            if info.status == status {
                return info;
            }
            assert!(Instant::now() < deadline, "timeout waiting for {status:?}, last {info:?}");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    const BANNER: &str = "Started Application in 1.234 seconds (process running)";
    const TOMCAT: &str = "Tomcat started on port 8080 (http) with context path ''";

    // --------------------------------------------------------------
    // 全闭环（验收标准 1）：Start → Running → Stop → Stopped
    // --------------------------------------------------------------

    #[test]
    fn start_stop_full_cycle_emits_lifecycle_events() {
        let fixture = maven_fixture("cycle", false);
        let events = Arc::new(VecEventSink::default());
        let maven = Arc::new(FakeMavenRunner::new(vec![
            FakeRun {
                lines: vec![(OutputStream::Stdout, "[INFO] BUILD SUCCESS".into())],
                ..Default::default()
            },
            FakeRun {
                output_file_content: Some(String::new()),
                ..Default::default()
            },
        ]));
        let launcher = Arc::new(FakeLaunchRunner::new(vec![FakeLaunch {
            lines: vec![
                (OutputStream::Stdout, TOMCAT.into()),
                (OutputStream::Stdout, BANNER.into()),
            ],
            behavior: FakeBehavior::StayAlive {
                on_terminate: Some(0),
            },
            ..Default::default()
        }]));
        let manager = test_manager(
            fixture.db.clone(),
            launcher,
            maven,
            events.clone(),
            Duration::from_millis(50),
        );

        let info = manager
            .start(fixture.workspace_id, "app", StartOptions::default())
            .unwrap();
        assert_eq!(info.status, LifecycleStatus::Running);
        assert!(info.pid.is_some());
        assert_eq!(info.run_strategy, Some(RunStrategy::ClasspathRun));
        assert!(info.command_preview.as_deref().unwrap().contains("java"));
        let _ = std::fs::remove_dir_all(&fixture.root);

        let stopped = manager.stop(info.process_id, None).unwrap();
        assert_eq!(stopped.status, LifecycleStatus::Stopped);
        assert_eq!(stopped.exit_code, Some(0));
        assert_eq!(stopped.ports, vec![8080], "端口探测来自启动日志");

        use LifecycleStatus::*;
        assert_eq!(
            lifecycle_chain(&events, info.process_id),
            vec![
                (Created, Preparing),
                (Preparing, Resolving),
                (Resolving, Building),
                (Building, Starting),
                (Starting, Running),
                (Running, Stopping),
                (Stopping, Stopped),
            ]
        );
        assert!(events.collected().iter().any(|e| matches!(
            e,
            RuntimeEvent::Ports { process_id, ports } if *process_id == info.process_id && ports.contains(&8080)
        )));
        assert!(events.collected().iter().any(|e| matches!(
            e,
            RuntimeEvent::Exited { process_id, exit_code: Some(0), crashed: false, .. } if *process_id == info.process_id
        )));
    }

    // --------------------------------------------------------------
    // R-11：构建/运行输出统一进日志引擎（脱敏落盘 + 聚合事件 + 回查）
    // --------------------------------------------------------------

    #[test]
    fn build_and_run_output_flow_into_masked_log_session() {
        let fixture = maven_fixture("logpipe", false);
        // 敏感环境变量（工作区层）：五层合并环境 → 日志脱敏秘密值来源。
        {
            let conn = fixture.db.lock().unwrap();
            crate::runtime::set_workspace_environment(
                &conn,
                fixture.workspace_id,
                std::collections::BTreeMap::from([(
                    "DB_PASSWORD".to_string(),
                    "topsecret-value".to_string(),
                )]),
            )
            .unwrap();
        }
        let events = Arc::new(VecEventSink::default());
        let logs = Arc::new(RuntimeLogEngine::new());
        let maven = Arc::new(FakeMavenRunner::new(vec![
            FakeRun {
                lines: vec![(OutputStream::Stdout, "[INFO] BUILD SUCCESS".into())],
                ..Default::default()
            },
            FakeRun {
                output_file_content: Some(String::new()),
                ..Default::default()
            },
        ]));
        let launcher = Arc::new(FakeLaunchRunner::new(vec![FakeLaunch {
            lines: vec![
                (
                    OutputStream::Stdout,
                    "2026-08-23 12:00:00.123  INFO 1 --- [main] c.e.App : connecting with topsecret-value"
                        .into(),
                ),
                (OutputStream::Stdout, TOMCAT.into()),
                (OutputStream::Stdout, BANNER.into()),
            ],
            behavior: FakeBehavior::StayAlive {
                on_terminate: Some(0),
            },
            ..Default::default()
        }]));
        let manager = Arc::new(RuntimeProcessManager::with_deps(
            fixture.db.clone(),
            RuntimeProcessDeps {
                launch_runner: launcher,
                maven_runner: maven,
                events: events.clone(),
                logs: logs.clone(),
                sample_interval: Duration::from_millis(50),
                ..Default::default()
            },
        ));

        let info = manager
            .start(fixture.workspace_id, "app", StartOptions::default())
            .unwrap();
        assert_eq!(info.status, LifecycleStatus::Running);
        let stopped = manager.stop(info.process_id, None).unwrap();
        assert_eq!(stopped.status, LifecycleStatus::Stopped);
        assert_eq!(stopped.ports, vec![8080], "端口探测不受日志接管影响");

        // 落盘：构建 + 运行输出在同一文件，全程脱敏（磁盘无明文 secret）。
        let log_file = fixture
            .root
            .join(".gitworkspace/logs/app")
            .join(format!("{}.log", info.process_id));
        let on_disk = std::fs::read_to_string(&log_file).unwrap();
        assert!(on_disk.contains("[INFO] BUILD SUCCESS"), "构建输出进同一日志");
        assert!(on_disk.contains("Started Application"), "运行输出落盘");
        assert!(!on_disk.contains("topsecret-value"), "磁盘上不得有明文 secret");

        // 聚合事件：Build / Run 两阶段都经 RuntimeEvent::Logs 推送且已脱敏。
        let log_lines: Vec<_> = events
            .collected()
            .into_iter()
            .flat_map(|event| match event {
                RuntimeEvent::Logs {
                    process_id, lines, ..
                } if process_id == info.process_id => lines,
                _ => Vec::new(),
            })
            .collect();
        assert!(log_lines.iter().any(|l| l.phase == LogPhase::Build));
        assert!(log_lines.iter().any(|l| l.phase == LogPhase::Run));
        assert!(log_lines.iter().all(|l| !l.line.contains("topsecret-value")));
        assert!(log_lines
            .iter()
            .any(|l| l.level == Some(crate::runtime::logs::LogLevel::Info)));

        // 进程结束后日志完整保留、可回查（R-11 验收标准）。
        let entries = logs
            .search(
                &fixture.root,
                "app",
                info.process_id,
                &crate::runtime::logs::LogFilter::default(),
            )
            .unwrap();
        assert_eq!(entries.len(), 4, "构建 1 行 + 运行 3 行全部可回查");
        let _ = std::fs::remove_dir_all(&fixture.root);
    }

    // --------------------------------------------------------------
    // 构建失败 → Failed + BuildFailed 结构化错误
    // --------------------------------------------------------------

    #[test]
    fn build_failure_marks_row_failed_and_returns_structured_error() {
        let fixture = maven_fixture("buildfail", false);
        let events = Arc::new(VecEventSink::default());
        let maven = Arc::new(FakeMavenRunner::new(vec![FakeRun {
            lines: vec![(OutputStream::Stderr, "[ERROR] COMPILATION ERROR".into())],
            exit_code: Some(1),
            ..Default::default()
        }]));
        let launcher = Arc::new(FakeLaunchRunner::staying_alive());
        let manager = test_manager(
            fixture.db.clone(),
            launcher,
            maven,
            events.clone(),
            Duration::from_millis(50),
        );

        let error = manager
            .start(fixture.workspace_id, "app", StartOptions::default())
            .unwrap_err();
        assert_eq!(error.code(), "BuildFailed");

        let rows = manager.list_processes(fixture.workspace_id).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].status, LifecycleStatus::Failed);
        use LifecycleStatus::*;
        assert_eq!(
            lifecycle_chain(&events, rows[0].process_id),
            vec![(Created, Preparing), (Preparing, Resolving), (Resolving, Building), (Building, Failed)]
        );
        let _ = std::fs::remove_dir_all(&fixture.root);
    }

    // --------------------------------------------------------------
    // Running 后崩溃 → Failed + 退出码（验收标准 4）
    // --------------------------------------------------------------

    #[test]
    fn crash_after_running_marks_failed_with_exit_code() {
        let fixture = mini_fixture("crash");
        let events = Arc::new(VecEventSink::default());
        let launcher = Arc::new(FakeLaunchRunner::new(vec![FakeLaunch {
            lines: vec![(OutputStream::Stdout, BANNER.into())],
            behavior: FakeBehavior::Exit(Some(1)),
            delay_after_lines: Some(Duration::from_millis(300)),
        }]));
        let manager = test_manager(
            fixture.db.clone(),
            launcher,
            Arc::new(FakeMavenRunner::successful()),
            events.clone(),
            Duration::from_millis(50),
        );
        manager.seed_cached_launch(
            fixture.workspace_id,
            "app",
            crate::runtime::build::LaunchPlan::JavaJar {
                java_exec: PathBuf::from("java"),
                jar_path: PathBuf::from("/ws/app.jar"),
                vm_options: vec![],
                program_arguments: vec![],
                env: vec![],
                working_dir: fixture.root.clone(),
                preview: "java -jar app.jar".into(),
            },
            RunStrategy::PackageRun,
        );

        let info = manager
            .start(
                fixture.workspace_id,
                "app",
                StartOptions {
                    skip_build: true,
                    start_grace: Duration::from_secs(2),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(info.status, LifecycleStatus::Running);

        let failed = wait_for_status(&manager, info.process_id, LifecycleStatus::Failed, Duration::from_secs(3));
        assert_eq!(failed.exit_code, Some(1));
        assert!(events.collected().iter().any(|e| matches!(
            e,
            RuntimeEvent::Exited { process_id, crashed: true, .. } if *process_id == info.process_id
        )));
        let _ = std::fs::remove_dir_all(&fixture.root);
    }

    // --------------------------------------------------------------
    // 启动宽限期内退出 → ProcessStartFailed（可行动错误）
    // --------------------------------------------------------------

    #[test]
    fn early_exit_maps_to_process_start_failed() {
        let fixture = mini_fixture("early");
        let launcher = Arc::new(FakeLaunchRunner::new(vec![FakeLaunch {
            lines: vec![],
            behavior: FakeBehavior::Exit(Some(2)),
            ..Default::default()
        }]));
        let manager = test_manager(
            fixture.db.clone(),
            launcher,
            Arc::new(FakeMavenRunner::successful()),
            Arc::new(VecEventSink::default()),
            Duration::from_millis(50),
        );
        manager.seed_cached_launch(
            fixture.workspace_id,
            "app",
            crate::runtime::build::LaunchPlan::JavaJar {
                java_exec: PathBuf::from("java"),
                jar_path: fixture.root.join("app.jar"),
                vm_options: vec![],
                program_arguments: vec![],
                env: vec![],
                working_dir: fixture.root.clone(),
                preview: "java -jar app.jar".into(),
            },
            RunStrategy::PackageRun,
        );

        let error = manager
            .start(fixture.workspace_id, "app", StartOptions {
                skip_build: true,
                ..Default::default()
            })
            .unwrap_err();
        assert_eq!(error.code(), "ProcessStartFailed");
        assert!(error.to_string().contains("启动宽限期内退出"));
        let rows = manager.list_processes(fixture.workspace_id).unwrap();
        assert_eq!(rows[0].status, LifecycleStatus::Failed);
        assert_eq!(rows[0].exit_code, Some(2));
        let _ = std::fs::remove_dir_all(&fixture.root);
    }

    // --------------------------------------------------------------
    // 重复启动守卫
    // --------------------------------------------------------------

    #[test]
    fn duplicate_start_is_rejected_with_conflict() {
        let fixture = mini_fixture("dup");
        let launcher = Arc::new(FakeLaunchRunner::staying_alive());
        let manager = test_manager(
            fixture.db.clone(),
            launcher,
            Arc::new(FakeMavenRunner::successful()),
            Arc::new(VecEventSink::default()),
            Duration::from_millis(50),
        );
        manager.seed_cached_launch(
            fixture.workspace_id,
            "app",
            crate::runtime::build::LaunchPlan::JavaJar {
                java_exec: PathBuf::from("java"),
                jar_path: fixture.root.join("app.jar"),
                vm_options: vec![],
                program_arguments: vec![],
                env: vec![],
                working_dir: fixture.root.clone(),
                preview: "java -jar app.jar".into(),
            },
            RunStrategy::PackageRun,
        );

        let first = manager
            .start(fixture.workspace_id, "app", StartOptions {
                skip_build: true,
                start_grace: Duration::from_millis(200),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(first.status, LifecycleStatus::Running);

        let error = manager
            .start(fixture.workspace_id, "app", StartOptions::default())
            .unwrap_err();
        assert_eq!(error.code(), "ConflictError");
        assert!(error.to_string().contains("Restart"));

        manager.stop(first.process_id, None).unwrap();
        let _ = std::fs::remove_dir_all(&fixture.root);
    }

    // --------------------------------------------------------------
    // Restart = Stop + Start 且复用最近构建产物（验收标准 2）
    // --------------------------------------------------------------

    #[test]
    fn restart_reuses_cached_artifacts_without_rebuilding() {
        let fixture = maven_fixture("restart", false);
        let events = Arc::new(VecEventSink::default());
        let maven = Arc::new(FakeMavenRunner::successful());
        let launcher = Arc::new(FakeLaunchRunner::staying_alive());
        let manager = test_manager(
            fixture.db.clone(),
            launcher.clone(),
            maven.clone(),
            events.clone(),
            Duration::from_millis(50),
        );

        let options = || StartOptions {
            build_options: BuildOptions {
                strategy: Some(RunStrategy::MavenRun),
                ..Default::default()
            },
            start_grace: Duration::from_millis(200),
            ..Default::default()
        };
        let first = manager
            .start(fixture.workspace_id, "app", options())
            .unwrap();
        assert_eq!(first.status, LifecycleStatus::Running);
        assert_eq!(maven.request_count(), 1, "首次 start 构建一次");

        let second = manager
            .restart(fixture.workspace_id, "app", options())
            .unwrap();
        assert_eq!(second.status, LifecycleStatus::Running);
        assert_ne!(second.process_id, first.process_id, "restart 建新行");
        assert_eq!(
            maven.request_count(),
            1,
            "restart 复用缓存产物，不再调 Maven"
        );
        // skip-build 路径：Preparing 直达 Starting。
        use LifecycleStatus::*;
        let chain = lifecycle_chain(&events, second.process_id);
        assert_eq!(chain[..3], [(Created, Preparing), (Preparing, Starting), (Starting, Running)]);

        let first_row = manager.get_process(first.process_id).unwrap().unwrap();
        assert_eq!(first_row.status, LifecycleStatus::Stopped);
        manager.stop(second.process_id, None).unwrap();
        let _ = std::fs::remove_dir_all(&fixture.root);
    }

    // --------------------------------------------------------------
    // R-06 mainClass 回退：配置缺省时自动推断进 LaunchPlan
    // --------------------------------------------------------------

    #[test]
    fn missing_main_class_is_inferred_via_spring_boot_detection() {
        let fixture = maven_fixture("infer", true);
        // ClasspathRun 两次 Maven 调用：compile + dependency:build-classpath（写出缓存文件）。
        let maven = Arc::new(FakeMavenRunner::new(vec![
            FakeRun {
                lines: vec![(OutputStream::Stdout, "[INFO] BUILD SUCCESS".into())],
                ..Default::default()
            },
            FakeRun {
                output_file_content: Some("/m2/spring-boot-starter.jar".into()),
                ..Default::default()
            },
        ]));
        let launcher = Arc::new(FakeLaunchRunner::new(vec![FakeLaunch {
            lines: vec![(OutputStream::Stdout, BANNER.into())],
            behavior: FakeBehavior::StayAlive {
                on_terminate: Some(0),
            },
            ..Default::default()
        }]));
        let manager = test_manager(
            fixture.db.clone(),
            launcher.clone(),
            maven,
            Arc::new(VecEventSink::default()),
            Duration::from_millis(50),
        );

        let info = manager
            .start(fixture.workspace_id, "app", StartOptions {
                start_grace: Duration::from_millis(200),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(info.status, LifecycleStatus::Running);
        let preview = info.command_preview.unwrap();
        assert!(
            preview.contains("com.example.app.Application"),
            "推断的 mainClass 应进入启动命令预览: {preview}"
        );
        manager.stop(info.process_id, None).unwrap();
        let _ = std::fs::remove_dir_all(&fixture.root);
    }

    // --------------------------------------------------------------
    // R-04 遗留验收：项目绑定 JDK 后，启动实际使用该 JDK 的 java
    // --------------------------------------------------------------

    #[test]
    fn bound_jdk_is_used_for_launch_command() {
        let fixture = maven_fixture("jdkbind", false);
        {
            let conn = fixture.db.lock().unwrap();
            let mut jdk = crate::java::model::JdkInstallation::new(
                "/jdk-21",
                crate::java::model::JdkDiscoverySource::System,
            );
            jdk.major_version = Some(21);
            jdk.is_valid = true;
            crate::java::registry::upsert_jdk(&conn, &jdk).unwrap();
            let mut config =
                crate::runtime::config::load_config_unredacted(&conn, fixture.workspace_id, "app")
                    .unwrap();
            config.jdk = Some("21".into());
            crate::runtime::config::update_config(
                &conn,
                &crate::runtime::config::UpdateRuntimeConfigRequest {
                    workspace_id: fixture.workspace_id,
                    name: "app".into(),
                    config,
                },
            )
            .unwrap();
        }
        let maven = Arc::new(FakeMavenRunner::new(vec![
            FakeRun {
                lines: vec![(OutputStream::Stdout, "[INFO] BUILD SUCCESS".into())],
                ..Default::default()
            },
            FakeRun {
                output_file_content: Some(String::new()),
                ..Default::default()
            },
        ]));
        let launcher = Arc::new(FakeLaunchRunner::new(vec![FakeLaunch {
            lines: vec![(OutputStream::Stdout, BANNER.into())],
            behavior: FakeBehavior::StayAlive {
                on_terminate: Some(0),
            },
            ..Default::default()
        }]));
        let manager = test_manager(
            fixture.db.clone(),
            launcher.clone(),
            maven,
            Arc::new(VecEventSink::default()),
            Duration::from_millis(50),
        );

        let info = manager
            .start(fixture.workspace_id, "app", StartOptions {
                start_grace: Duration::from_millis(200),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(info.status, LifecycleStatus::Running);
        let preview = info.command_preview.unwrap();
        assert!(
            preview.replace('\\', "/").starts_with("/jdk-21/bin/java"),
            "绑定 JDK 的 java 可执行路径应出现在启动命令开头（Windows 分隔符不敏感）: {preview}"
        );

        manager.stop(info.process_id, None).unwrap();
        let _ = std::fs::remove_dir_all(&fixture.root);
    }


    #[test]
    fn classify_exit_table() {
        use LifecycleStatus::*;
        let clean = MonitorOutcome { exit_code: Some(0), cancelled: false, spawn_error: None };
        let crash = MonitorOutcome { exit_code: Some(137), cancelled: false, spawn_error: None };
        let signaled = MonitorOutcome { exit_code: None, cancelled: true, spawn_error: None };
        let spawn_fail = MonitorOutcome { exit_code: None, cancelled: false, spawn_error: Some("io".into()) };

        assert_eq!(classify_exit(Running, &clean, false), (Stopped, false));
        assert_eq!(classify_exit(Running, &crash, false), (Failed, true));
        assert_eq!(classify_exit(Starting, &crash, false), (Failed, true));
        assert_eq!(classify_exit(Stopping, &crash, false), (Stopped, false));
        assert_eq!(classify_exit(Running, &signaled, false), (Failed, true));
        assert_eq!(classify_exit(Running, &signaled, true), (Stopped, false), "adopted 无码宽容");
        assert_eq!(classify_exit(Starting, &spawn_fail, false), (Failed, false));
    }

    // --------------------------------------------------------------
    // 真实进程（unix）：Force Kill / 优雅升级 / 孤儿接管 / 指标
    // --------------------------------------------------------------

    #[cfg(unix)]
    mod real_process {
        use super::*;
        use crate::process::{process_alive, process_start_time};

        /// `sh -c <script>` 的 MavenGoal 计划（经 R-09 executor 组命令）。
        fn sh_plan(script: &str, working_dir: &Path) -> crate::runtime::build::LaunchPlan {
            crate::runtime::build::LaunchPlan::MavenGoal {
                request: crate::maven::exec_model::MavenExecutionRequest {
                    working_dir: working_dir.to_path_buf(),
                    executable: "sh".into(),
                    goals: vec!["-c".into(), script.into()],
                    extra_args: vec![],
                    via_cmd_c: false,
                    local_repository: None,
                },
                env: vec![],
                preview: format!("sh -c {script}"),
            }
        }

        fn real_manager(
            fixture: &MiniFixture,
            events: Arc<VecEventSink>,
            sample_interval: Duration,
        ) -> Arc<RuntimeProcessManager> {
            test_manager(
                fixture.db.clone(),
                Arc::new(crate::runtime::launch::SystemLaunchRunner),
                Arc::new(FakeMavenRunner::successful()),
                events,
                sample_interval,
            )
        }

        fn start_sh(
            manager: &Arc<RuntimeProcessManager>,
            fixture: &MiniFixture,
            script: &str,
        ) -> RuntimeProcessInfo {
            manager.seed_cached_launch(
                fixture.workspace_id,
                "app",
                sh_plan(script, &fixture.root),
                RunStrategy::MavenRun,
            );
            manager
                .start(fixture.workspace_id, "app", StartOptions {
                    skip_build: true,
                    start_grace: Duration::from_millis(300),
                    ..Default::default()
                })
                .unwrap()
        }

        #[test]
        fn force_kill_requires_confirmation_and_leaves_no_orphan() {
            let fixture = mini_fixture("fkill");
            let manager = real_manager(&fixture, Arc::new(VecEventSink::default()), Duration::from_millis(50));
            let info = start_sh(&manager, &fixture, "sleep 300 & wait");
            assert_eq!(info.status, LifecycleStatus::Running);

            // 未确认 → 拒绝（全局约束 §3 二次确认）。
            let error = manager.kill(info.process_id, false).unwrap_err();
            assert_eq!(error.code(), "PermissionError");

            let stopped = manager.kill(info.process_id, true).unwrap();
            assert_eq!(stopped.status, LifecycleStatus::Stopped);

            // 进程树无孤儿残留（验收标准 5）。
            std::thread::sleep(Duration::from_millis(300));
            let mut system = sysinfo::System::new_with_specifics(
                sysinfo::RefreshKind::new().with_processes(sysinfo::ProcessRefreshKind::new()),
            );
            system.refresh_processes();
            let survivors: Vec<_> = system
                .processes()
                .values()
                .filter(|p| p.name() == "sleep" && p.cmd().iter().any(|a| a == "300"))
                .collect();
            assert!(survivors.is_empty(), "sleep 300 must be killed: {survivors:?}");
            let _ = std::fs::remove_dir_all(&fixture.root);
        }

        #[test]
        fn stop_escalates_to_tree_kill_when_sigterm_is_ignored() {
            let fixture = mini_fixture("escalate");
            let manager = real_manager(&fixture, Arc::new(VecEventSink::default()), Duration::from_millis(50));
            // 忽略 SIGTERM 的进程：grace 超时后必须升级杀树。
            let info = start_sh(&manager, &fixture, "trap '' TERM; while true; do sleep 0.05; done");
            let pid = info.pid.unwrap();

            let stopped = manager
                .stop(info.process_id, Some(Duration::from_millis(500)))
                .unwrap();
            assert_eq!(stopped.status, LifecycleStatus::Stopped);
            std::thread::sleep(Duration::from_millis(200));
            assert!(!process_alive(pid, None), "SIGTERM-ignoring process must be tree-killed");
            let _ = std::fs::remove_dir_all(&fixture.root);
        }

        /// F-12 回归（unix 变体）：忽略 SIGTERM 且两路输出均含非法 UTF-8
        /// （reader 全死、channel 断开）的进程——grace 升级时置位的
        /// force_kill 曾因 monitor 阻塞在 child.wait() 而无人消费。
        #[test]
        fn stop_kills_sigterm_ignoring_process_that_closed_streams() {
            let fixture = mini_fixture("f12unix");
            let manager = real_manager(&fixture, Arc::new(VecEventSink::default()), Duration::from_millis(50));
            let info = start_sh(
                &manager,
                &fixture,
                "trap '' TERM; printf '\\377\\376\\n'; printf '\\377\\376\\n' >&2; sleep 300",
            );
            let pid = info.pid.unwrap();

            let stopped = manager
                .stop(info.process_id, Some(Duration::from_millis(500)))
                .unwrap();
            std::thread::sleep(Duration::from_millis(200));
            let alive = process_alive(pid, None);
            if alive {
                crate::process::kill_tree::kill_process_tree(pid);
            }
            assert_eq!(stopped.status, LifecycleStatus::Stopped);
            assert!(!alive, "F-12: reader 断开后升级杀树也必须生效");
            let _ = std::fs::remove_dir_all(&fixture.root);
        }

        #[test]
        fn graceful_stop_uses_sigterm_before_any_kill() {
            let fixture = mini_fixture("graceful");
            let events = Arc::new(VecEventSink::default());
            let manager = real_manager(&fixture, events.clone(), Duration::from_millis(50));
            // trap TERM → 记录并 exit 0：若先收到 SIGTERM 则优雅退出码 0。
            let info = start_sh(&manager, &fixture, "trap 'exit 0' TERM; while true; do sleep 0.1; done");

            let stopped = manager.stop(info.process_id, None).unwrap();
            assert_eq!(stopped.status, LifecycleStatus::Stopped);
            assert_eq!(stopped.exit_code, Some(0), "SIGTERM 触发 trap 优雅退出");
            assert!(events.collected().iter().any(|e| matches!(
                e,
                RuntimeEvent::Exited { process_id, crashed: false, .. } if *process_id == info.process_id
            )));
            let _ = std::fs::remove_dir_all(&fixture.root);
        }

        #[test]
        fn reconcile_adopts_live_orphan_and_fails_gone_rows() {
            let fixture = mini_fixture("orphan");
            // 会话 A：启动真实 sleep 后「崩溃」（drop manager，不 stop）。
            let manager_a = real_manager(&fixture, Arc::new(VecEventSink::default()), Duration::from_secs(3600));
            let info = start_sh(&manager_a, &fixture, "sleep 300");
            let pid = info.pid.unwrap();
            let pid_start = process_start_time(pid).unwrap();
            assert!(process_alive(pid, Some(pid_start)));
            drop(manager_a);

            // 同库补两类遗留行：死进程 Running 行 + 从未 spawn 的 Created 行。
            let (dead_id, created_id) = {
                let conn = fixture.db.lock().unwrap();
                let dead = store::insert_process(&conn, fixture.workspace_id, "dead-app").unwrap();
                for status in [
                    LifecycleStatus::Preparing,
                    LifecycleStatus::Resolving,
                    LifecycleStatus::Building,
                    LifecycleStatus::Starting,
                    LifecycleStatus::Running,
                ] {
                    store::transition_status(&conn, dead, status, None).unwrap();
                }
                store::set_pid(&conn, dead, 4_000_000, Some(1)).unwrap();
                let created = store::insert_process(&conn, fixture.workspace_id, "half-app").unwrap();
                (dead, created)
            };

            // 会话 B：reconcile 接管。
            let manager_b = real_manager(&fixture, Arc::new(VecEventSink::default()), Duration::from_secs(3600));
            let adopted = manager_b.reconcile_on_startup(fixture.workspace_id).unwrap();
            assert_eq!(adopted.len(), 1);
            assert_eq!(adopted[0].process_id, info.process_id);
            assert!(adopted[0].adopted);
            assert_eq!(adopted[0].status, LifecycleStatus::Running);

            let dead = manager_b.get_process(dead_id).unwrap().unwrap();
            assert_eq!(dead.status, LifecycleStatus::Failed);
            let created = manager_b.get_process(created_id).unwrap().unwrap();
            assert_eq!(created.status, LifecycleStatus::Failed);

            // 接管后可正常 Stop（SIGTERM 杀 sleep）。
            let stopped = manager_b
                .stop(info.process_id, Some(Duration::from_secs(5)))
                .unwrap();
            assert_eq!(stopped.status, LifecycleStatus::Stopped);
            std::thread::sleep(Duration::from_millis(200));
            assert!(!process_alive(pid, Some(pid_start)), "adopted orphan must be stopped");
            let _ = std::fs::remove_dir_all(&fixture.root);
        }

        #[test]
        fn sampler_emits_metrics_for_live_process() {
            let fixture = mini_fixture("metrics");
            let events = Arc::new(VecEventSink::default());
            let manager = real_manager(&fixture, events.clone(), Duration::from_millis(30));
            let info = start_sh(&manager, &fixture, "sleep 60");

            std::thread::sleep(Duration::from_millis(250));
            let collected = events.collected();
            let metrics: Vec<_> = collected
                .iter()
                .filter(|e| matches!(e, RuntimeEvent::Metrics { process_id, .. } if *process_id == info.process_id))
                .collect();
            assert!(!metrics.is_empty(), "sampler must emit metrics events");
            if let RuntimeEvent::Metrics { memory_bytes, .. } = metrics[0] {
                assert!(*memory_bytes > 0);
            }

            manager.stop(info.process_id, None).unwrap();
            let _ = std::fs::remove_dir_all(&fixture.root);
        }
    }

    // --------------------------------------------------------------
    // 真实进程（windows）：F-12 回归——reader 断开后 Stop 仍须杀树。
    // Windows 无 SIGTERM（terminate 恒 false），停止全押在 force_kill
    // 链路上，是本缺陷的必现平台。
    // --------------------------------------------------------------

    #[cfg(windows)]
    mod real_process_windows {
        use super::*;
        use crate::process::process_alive;

        /// powershell 向 stdout/stderr 各写一段非法 UTF-8 后长驻：两个 reader
        /// 死亡曾令 monitor 阻塞在 child.wait()，force_kill 无人消费
        /// （F-12 复现路径，等价于 hussar JVM 的 GBK 中文日志输出）。
        fn gbk_output_plan(working_dir: &Path) -> crate::runtime::build::LaunchPlan {
            crate::runtime::build::LaunchPlan::MavenGoal {
                request: crate::maven::exec_model::MavenExecutionRequest {
                    working_dir: working_dir.to_path_buf(),
                    executable: "powershell".into(),
                    goals: vec![
                        "-NoProfile".into(),
                        "-Command".into(),
                        "$b=[byte[]](255,254,10); \
                         [Console]::OpenStandardOutput().Write($b,0,3); \
                         [Console]::OpenStandardError().Write($b,0,3); \
                         Start-Sleep -Seconds 300".into(),
                    ],
                    extra_args: vec![],
                    via_cmd_c: false,
                    local_repository: None,
                },
                env: vec![],
                preview: "powershell invalid-utf8 sleep".into(),
            }
        }

        #[test]
        fn stop_kills_process_whose_output_streams_closed_early() {
            let fixture = mini_fixture("f12win");
            let manager = test_manager(
                fixture.db.clone(),
                Arc::new(crate::runtime::launch::SystemLaunchRunner),
                Arc::new(FakeMavenRunner::successful()),
                Arc::new(VecEventSink::default()),
                Duration::from_millis(50),
            );
            manager.seed_cached_launch(
                fixture.workspace_id,
                "app",
                gbk_output_plan(&fixture.root),
                RunStrategy::MavenRun,
            );
            // 非法字节杀死 reader → 等不到横幅，start_grace 到期按存活判 Running。
            let info = manager
                .start(fixture.workspace_id, "app", StartOptions {
                    skip_build: true,
                    start_grace: Duration::from_secs(3),
                    ..Default::default()
                })
                .unwrap();
            assert_eq!(info.status, LifecycleStatus::Running);
            let pid = info.pid.expect("spawn 后应有 pid");

            let stopped = manager
                .stop(info.process_id, Some(Duration::from_secs(2)))
                .unwrap();
            std::thread::sleep(Duration::from_millis(300));
            let alive = process_alive(pid, None);
            if alive {
                crate::process::kill_tree::kill_process_tree(pid);
            }
            assert_eq!(stopped.status, LifecycleStatus::Stopped);
            assert!(!alive, "F-12: stop 后进程必须真实消失，不得残留孤儿");
            let _ = std::fs::remove_dir_all(&fixture.root);
        }
    }

    // --------------------------------------------------------------
    // 真实 Maven + 真实 JVM 集成测试（验收标准 1/4 的端到端口径）。
    // 需要 PATH 上的 `mvn`（自带 JDK）；缺失时跳过并标注。首次运行
    // 会联网拉依赖（R-09 测试同款），属预期。
    // --------------------------------------------------------------

    mod real_maven {
        use super::*;

        const SPRING_BOOT_VERSION: &str = "3.2.5";
        const INTEGRATION_TIMEOUT: Duration = Duration::from_secs(600);

        fn maven_available() -> bool {
            let maven = if cfg!(windows) { "mvn.cmd" } else { "mvn" };
            std::process::Command::new(maven)
                .arg("-version")
                .output()
                .is_ok()
        }

        fn parent_pom() -> String {
            format!(
                r#"<?xml version="1.0" encoding="UTF-8"?>
<project xmlns="http://maven.apache.org/POM/4.0.0">
  <modelVersion>4.0.0</modelVersion>
  <groupId>com.r10</groupId>
  <artifactId>r10-parent</artifactId>
  <version>1.0.0</version>
  <packaging>pom</packaging>
  <modules><module>app</module></modules>
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
        <version>{SPRING_BOOT_VERSION}</version>
        <type>pom</type>
        <scope>import</scope>
      </dependency>
    </dependencies>
  </dependencyManagement>
</project>
"#
            )
        }

        fn app_pom() -> String {
            r#"<?xml version="1.0" encoding="UTF-8"?>
<project xmlns="http://maven.apache.org/POM/4.0.0">
  <modelVersion>4.0.0</modelVersion>
  <parent>
    <groupId>com.r10</groupId>
    <artifactId>r10-parent</artifactId>
    <version>1.0.0</version>
  </parent>
  <artifactId>app</artifactId>
  <dependencies>
    <dependency>
      <groupId>org.springframework.boot</groupId>
      <artifactId>spring-boot-starter-web</artifactId>
    </dependency>
  </dependencies>
</project>
"#
            .to_string()
        }

        /// 单仓 parent + app（spring-boot-starter-web 驻留应用）fixture。
        fn boot_fixture(
            name: &str,
            program_arguments: &[&str],
            vm_options: &[&str],
        ) -> (PathBuf, Arc<Mutex<Connection>>, i64) {
            let root = unique_root(name);
            std::fs::create_dir_all(&root).unwrap();
            write(&root.join("repo/pom.xml"), &parent_pom());
            write(&root.join("repo/app/pom.xml"), &app_pom());
            write(
                &root.join("repo/app/src/main/java/com/r10/app/Application.java"),
                "package com.r10.app;\n\n\
                 import org.springframework.boot.SpringApplication;\n\
                 import org.springframework.boot.autoconfigure.SpringBootApplication;\n\n\
                 @SpringBootApplication\n\
                 public class Application {\n    public static void main(String[] args) {\n        SpringApplication.run(Application.class, args);\n    }\n}\n",
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
            let discovery = crate::maven::discover_poms(&root, 5, None, None);
            assert!(discovery.errors.is_empty(), "{:?}", discovery.errors);
            crate::maven::sync_workspace_index(&mut conn, workspace_id, &discovery, &root.join("m2"))
                .unwrap();
            // 真实集成测试前提：JDK 17+（Spring Boot 3.2.5 class 61）。把
            // 当前 JAVA_HOME 注册为 JDK 并绑定到配置——启动 JVM 与构建
            // 同源，避免「构建 17、运行 8」的版本错配（R-14 修复）。
            let bound_jdk = std::env::var("JAVA_HOME")
                .ok()
                .filter(|home| !home.is_empty())
                .map(|home| {
                    let mut jdk = crate::java::model::JdkInstallation::new(
                        home.clone(),
                        crate::java::model::JdkDiscoverySource::System,
                    );
                    jdk.is_valid = true;
                    crate::java::registry::upsert_jdk(&conn, &jdk).unwrap();
                    home
                });
            create_config(
                &conn,
                &CreateRuntimeConfigRequest {
                    workspace_id,
                    config: RuntimeApplicationConfig {
                        name: "app".into(),
                        project: root.join("repo/app/pom.xml").to_string_lossy().to_string(),
                        main_class: Some("com.r10.app.Application".into()),
                        jdk: bound_jdk.clone(),
                        program_arguments: program_arguments
                            .iter()
                            .map(|arg| arg.to_string())
                            .collect(),
                        vm_options: vm_options.iter().map(|opt| opt.to_string()).collect(),
                        ..Default::default()
                    },
                },
            )
            .unwrap();
            (root, Arc::new(Mutex::new(conn)), workspace_id)
        }

        fn real_manager(
            db: Arc<Mutex<Connection>>,
            events: Arc<VecEventSink>,
        ) -> Arc<RuntimeProcessManager> {
            Arc::new(RuntimeProcessManager::with_deps(db, RuntimeProcessDeps {
                events,
                ..Default::default()
            }))
        }

        fn classpath_options() -> StartOptions {
            StartOptions {
                build_options: BuildOptions {
                    strategy: Some(RunStrategy::ClasspathRun),
                    timeout: Some(INTEGRATION_TIMEOUT),
                    ..Default::default()
                },
                // 真实 JVM + Spring 上下文的启动远慢于 fake；横幅命中会提前返回。
                start_grace: Duration::from_secs(120),
                ..Default::default()
            }
        }

        /// 验收标准 1 端到端：真实 Spring Boot 应用 Start → Running（横幅）
        /// → 端口探测 → Stop → Stopped，JVM 不残留。
        #[test]
        fn classpath_run_full_cycle_with_real_spring_boot_app() {
            if !maven_available() {
                eprintln!("R-10: no `mvn` on PATH; skipping real spring boot start test");
                return;
            }
            let (root, db, workspace_id) = boot_fixture("bootcycle", &["--server.port=0"], &[]);
            let events = Arc::new(VecEventSink::default());
            let manager = real_manager(db.clone(), events.clone());

            let info = manager
                .start(workspace_id, "app", classpath_options())
                .unwrap_or_else(|error| panic!("real start failed: {error}"));
            assert_eq!(info.status, LifecycleStatus::Running);
            let pid = info.pid.expect("real process must have a pid");

            // 端口来自启动日志正则（--server.port=0 → 随机端口）；Tomcat 端口
            // 行先于横幅，但 monitor 写库是异步的，这里轮询兜底。
            let deadline = Instant::now() + Duration::from_secs(10);
            let ports = loop {
                let ports = manager
                    .get_process(info.process_id)
                    .unwrap()
                    .map(|row| row.ports)
                    .unwrap_or_default();
                if !ports.is_empty() || Instant::now() > deadline {
                    break ports;
                }
                std::thread::sleep(Duration::from_millis(200));
            };
            assert!(!ports.is_empty(), "启动日志应探测到随机端口");

            let stopped = manager
                .stop(info.process_id, Some(Duration::from_secs(30)))
                .unwrap();
            assert_eq!(stopped.status, LifecycleStatus::Stopped);
            std::thread::sleep(Duration::from_millis(300));
            assert!(
                !crate::process::process_alive(pid, None),
                "stop 后 JVM 不应残留"
            );

            use LifecycleStatus::*;
            assert_eq!(
                lifecycle_chain(&events, info.process_id),
                vec![
                    (Created, Preparing),
                    (Preparing, Resolving),
                    (Resolving, Building),
                    (Building, Starting),
                    (Starting, Running),
                    (Running, Stopping),
                    (Stopping, Stopped),
                ]
            );
            let _ = std::fs::remove_dir_all(&root);
        }

        /// F-04 端到端：「IDEA 启动」预设的 VM options 不影响真实启动——
        /// 用预设参数把 fixture Spring Boot 应用起到 Running 再停掉。
        /// 预设清单与前端 `src/config/launchPresets.ts` 保持一致（刻意排除
        /// idea_rt.jar javaagent 与 @arg_file 等 IDEA 私有项）。
        #[test]
        fn idea_preset_vm_options_boot_real_spring_boot_app() {
            if !maven_available() {
                eprintln!("F-04: no `mvn` on PATH; skipping IDEA preset boot test");
                return;
            }
            const IDEA_PRESET_VM_OPTIONS: &[&str] = &[
                "-XX:TieredStopAtLevel=1",
                "-Dspring.output.ansi.enabled=always",
                "-Dcom.sun.management.jmxremote",
                "-Dspring.jmx.enabled=true",
                "-Dspring.liveBeansView.mbeanDomain",
                "-Dspring.application.admin.enabled=true",
                "-Dmanagement.endpoints.jmx.exposure.include=*",
                "-Dfile.encoding=UTF-8",
            ];
            let (root, db, workspace_id) =
                boot_fixture("bootpreset", &["--server.port=0"], IDEA_PRESET_VM_OPTIONS);
            let manager = real_manager(db.clone(), Arc::new(VecEventSink::default()));

            let info = manager
                .start(workspace_id, "app", classpath_options())
                .unwrap_or_else(|error| panic!("F-04 preset start failed: {error}"));
            assert_eq!(info.status, LifecycleStatus::Running);

            let stopped = manager
                .stop(info.process_id, Some(Duration::from_secs(30)))
                .unwrap();
            assert_eq!(stopped.status, LifecycleStatus::Stopped);
            let _ = std::fs::remove_dir_all(&root);
        }

        /// 验收标准 4 端到端：非法端口 → 启动期退出 → ProcessStartFailed
        /// + 行落 Failed 且带非零退出码。
        #[test]
        fn invalid_port_crashes_during_startup_and_marks_failed() {
            if !maven_available() {
                eprintln!("R-10: no `mvn` on PATH; skipping crash integration test");
                return;
            }
            let (root, db, workspace_id) = boot_fixture("bootcrash", &["--server.port=99999"], &[]);
            let manager = real_manager(db.clone(), Arc::new(VecEventSink::default()));

            let error = manager
                .start(workspace_id, "app", classpath_options())
                .unwrap_err();
            assert_eq!(error.code(), "ProcessStartFailed");

            let row = store::list_processes(&db.lock().unwrap(), workspace_id)
                .unwrap()
                .into_iter()
                .next()
                .expect("one process row");
            assert_eq!(row.status, LifecycleStatus::Failed);
            assert!(row.exit_code.is_some_and(|code| code != 0));
            let _ = std::fs::remove_dir_all(&root);
        }

        /// F-12 真实场景复测（manual，需显式 opt-in）：
        /// `cargo test manual_hussar_stop -- --ignored --nocapture`
        /// 依赖本机 release.2 工作区（不存在则跳过）。链路：完整 mvn 构建
        /// → ClasspathRun（F-11 pathing jar）→ Running → stop(15s) → JVM
        /// 必须真实消失。修复前此场景 stop 返回成功但 JVM 残留（GBK 日志
        /// 杀死输出 reader，monitor 阻塞 wait 无法消费 force_kill）。
        #[test]
        #[ignore = "manual: 依赖本机 release.2 工作区（F-12 真实场景复测）"]
        fn manual_hussar_base_web_stop_kills_jvm() {
            let env_root = Path::new(r"D:\AWork\Code\9.6.0-release.2\env");
            let app_dir = env_root.join("hussar-base-web");
            if !app_dir.join("pom.xml").exists() || !maven_available() {
                eprintln!("F-12 manual: release.2 工作区或 mvn 不存在，跳过");
                return;
            }

            let mut conn = Connection::open_in_memory().unwrap();
            crate::db::init_db(&mut conn).unwrap();
            conn.execute(
                "INSERT INTO workspaces (name, path, created_at, updated_at) VALUES ('w', ?1, 't', 't')",
                [env_root.to_string_lossy().to_string()],
            )
            .unwrap();
            let workspace_id = conn.last_insert_rowid();
            crate::db::dao::upsert_repositories_batch(
                &mut conn,
                workspace_id,
                &[crate::models::repository::ScannedRepo {
                    path: app_dir.to_string_lossy().to_string(),
                    name: "hussar-base-web".into(),
                    relative_path: "hussar-base-web".into(),
                    git_dir_mtime: None,
                }],
            )
            .unwrap();
            let discovery = crate::maven::discover_poms(env_root, 5, None, None);
            // 生产同源：Maven 原生 ~/.m2 本地仓库（§73）。
            let m2 = PathBuf::from(std::env::var("USERPROFILE").expect("USERPROFILE")).join(".m2");
            crate::maven::sync_workspace_index(&mut conn, workspace_id, &discovery, &m2).unwrap();

            // 构建与运行同源 JDK（R-14）：hussar 场景绑 JAVA_HOME（temurin-8）。
            let jdk_home =
                std::env::var("JAVA_HOME").expect("manual 测试需要 JAVA_HOME（temurin-8）");
            let mut jdk = crate::java::model::JdkInstallation::new(
                jdk_home.clone(),
                crate::java::model::JdkDiscoverySource::System,
            );
            jdk.is_valid = true;
            crate::java::registry::upsert_jdk(&conn, &jdk).unwrap();

            create_config(
                &conn,
                &CreateRuntimeConfigRequest {
                    workspace_id,
                    config: RuntimeApplicationConfig {
                        name: "app".into(),
                        project: app_dir.join("pom.xml").to_string_lossy().to_string(),
                        // 缺省 → R-06 自动推断 mainClass（F-05 链路，复现原场景）。
                        main_class: None,
                        jdk: Some(jdk_home),
                        ..Default::default()
                    },
                },
            )
            .unwrap();

            let manager = real_manager(Arc::new(Mutex::new(conn)), Arc::new(VecEventSink::default()));
            let info = manager
                .start(workspace_id, "app", classpath_options())
                .unwrap_or_else(|error| panic!("F-12 manual: start failed: {error}"));
            assert_eq!(info.status, LifecycleStatus::Running);
            let pid = info.pid.expect("Running 应有 pid");

            let stopped = manager
                .stop(info.process_id, Some(Duration::from_secs(15)))
                .unwrap();
            assert_eq!(stopped.status, LifecycleStatus::Stopped);
            std::thread::sleep(Duration::from_millis(500));
            assert!(
                !crate::process::process_alive(pid, None),
                "F-12: stop 后 JVM 必须真实消失（pid={pid}）"
            );
        }
    }
}

