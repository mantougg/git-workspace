use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use crate::runtime::build::{BuildOptions, LaunchPlan, RunStrategy};
use crate::runtime::launch::LifecycleStatus;

/// spawn 后判定 `Running` 的默认宽限（启动横幅命中可提前翻转）。
pub const DEFAULT_START_GRACE: Duration = Duration::from_secs(5);

/// Stop 的默认优雅宽限：SIGTERM 后等待退出，超时升级杀进程树。
pub const DEFAULT_STOP_GRACE: Duration = Duration::from_secs(10);

/// 指标采样默认间隔（低频节流，全局约束 §5）。
pub const DEFAULT_SAMPLE_INTERVAL: Duration = Duration::from_secs(2);

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
pub(super) struct CachedLaunch {
    pub(super) plan: LaunchPlan,
    pub(super) strategy: RunStrategy,
}

/// 活跃进程句柄：monitor/stop/sampler 共享的同步原语。
#[derive(Clone)]
pub(super) struct ActiveProcess {
    /// 身份（R-12 `signal_build_cancel` 按 runtime 反查句柄，不经过 DB）。
    pub(super) workspace_id: i64,
    pub(super) runtime_name: String,
    /// spawn 后立即填充（streaming pid slot）。
    pub(super) pid_slot: Arc<Mutex<Option<u32>>>,
    /// spawn 时记录的 OS start_time（防 PID 复用，adopted 行来自 DB）。
    pub(super) pid_start_time: Arc<Mutex<Option<u64>>>,
    /// 构建阶段取消（Stop/Kill 打断 execute_build）。
    pub(super) build_cancel: Arc<AtomicBool>,
    /// 强杀进程树信号（Force Kill / Stop grace 超时升级）。
    pub(super) force_kill: Arc<AtomicBool>,
    /// monitor 进度：启动横幅命中 / 退出结果；Condvar 唤醒等待方。
    pub(super) progress: Arc<(Mutex<Progress>, Condvar)>,
    /// 注册时刻（uptime 事件用单调时钟）。
    pub(super) started_instant: Instant,
    /// 是否接管自上次会话的孤儿。
    pub(super) adopted: bool,
}

#[derive(Debug, Default)]
pub(super) struct Progress {
    pub(super) running: bool,
    pub(super) outcome: Option<MonitorOutcome>,
}

#[derive(Debug, Clone)]
pub(super) struct MonitorOutcome {
    pub(super) exit_code: Option<i32>,
    /// true = 走了 kill 树路径。
    pub(super) cancelled: bool,
    /// spawn 失败（io）——进程从未起来。
    pub(super) spawn_error: Option<String>,
}

impl ActiveProcess {
    pub(super) fn new(adopted: bool, workspace_id: i64, runtime_name: &str) -> Self {
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

    pub(super) fn pid(&self) -> Option<u32> {
        *self.pid_slot.lock().unwrap()
    }

    pub(super) fn signal_outcome(&self, outcome: MonitorOutcome) {
        let (lock, cv) = &*self.progress;
        lock.lock().unwrap().outcome = Some(outcome);
        cv.notify_all();
    }
}

/// 退出分类（finalize_exit 的纯函数部分）：
/// - Stopping 期间退出 → Stopped（用户意图优先，退出码仍记录）；
/// - spawn 失败 → Failed（ProcessStartFailed 语义）；
/// - 退出码 0 → Stopped（自然终止）；
/// - adopted 且无码 → Stopped（非父进程拿不到码，宽容处理并记日志）；
/// - 其余（非零 / 被信号杀）→ Failed（ProcessCrashed 语义）。
pub(super) fn classify_exit(from: LifecycleStatus, outcome: &MonitorOutcome, adopted: bool) -> (LifecycleStatus, bool) {
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

pub(super) struct Built {
    pub(super) plan: LaunchPlan,
    pub(super) strategy: RunStrategy,
}

pub(super) enum PidWait {
    Pid(u32),
    Exited,
    Timeout,
}

pub(super) enum RunWait {
    Running,
    Exited,
    GraceElapsed,
}

pub(super) enum Prepared {
    Cached(CachedLaunch),
    NeedBuild(BuildOptions),
}
