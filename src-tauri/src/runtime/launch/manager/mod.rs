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
mod tests;
