//! Runtime Launcher 与 Process Manager（R-10，§27 生命周期、§29 Start、
//! §33 Process Manager、§34 Process 控制）。
//!
//! 职责：按 Runtime 配置（经 R-09 `LaunchPlan`）组装 `java` / `mvn` 命令并
//! 启动 Spring Boot 应用；维护生命周期状态机（[`LifecycleStatus`]）；提供
//! Start / Stop（SIGTERM 优雅优先）/ Restart / Force Kill 进程控制；托管
//! GitWorkspace 重启后的孤儿进程（pid + start_time 核对防 PID 复用）。
//!
//! 边界（任务文档 + 全局约束）：
//! - 进程状态以实际 OS 进程为准，`runtime_processes` 表只是缓存；
//! - 指标采样低频节流，只读 OS 计数器（sysinfo），不为采样 fork 进程；
//! - 端口信息来自启动日志探测，不做端口扫描（端口管理归 R-16）；
//! - 状态迁移与指标经 [`RuntimeEventSink`] 发内部事件——IPC/Event API 与
//!   前端由 R-12 / R-13 接入（与 R-09 同边界）；
//! - 启动命令完整参数经 `LaunchPlan.preview` 可预览、落 `command_preview`
//!   列可追溯（全局约束 §3）；Force Kill 需 `confirmed=true`（§3 二次确认）。
//!
//! 模块内 `LaunchPlan` / 构建环境含未脱敏秘密：命令与环境**不跨 IPC**；
//! [`RuntimeProcessInfo.command_preview`] 不含环境变量，仅命令行参数。

pub mod launcher;
pub mod lifecycle;
pub mod manager;
pub mod port_preflight;
pub mod store;

pub use launcher::{launch_command, LaunchRunner, SystemLaunchRunner};
pub use lifecycle::LifecycleStatus;
pub use manager::{RuntimeProcessManager, StartOptions, DEFAULT_START_GRACE, DEFAULT_STOP_GRACE};
pub use store::RuntimeProcessRow;

use serde::{Deserialize, Serialize};

use crate::runtime::build::RunStrategy;

/// 注入被托管进程的环境变量标记：DB 行 id（孤儿识别与审计用）。
pub const MARKER_PROCESS_ID: &str = "GITWORKSPACE_PROCESS_ID";
/// 注入被托管进程的环境变量标记：Runtime 配置名。
pub const MARKER_RUNTIME_NAME: &str = "GITWORKSPACE_RUNTIME_NAME";

/// 运行中进程的信息快照（§33：PID / Status / CPU / Memory / Ports /
/// Start Time / Uptime）。camelCase 序列化，R-12 起跨 IPC。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeProcessInfo {
    /// `runtime_processes` 行 id。
    pub process_id: i64,
    pub workspace_id: i64,
    pub runtime_name: String,
    pub pid: Option<u32>,
    pub status: LifecycleStatus,
    pub run_strategy: Option<RunStrategy>,
    /// 启动命令预览（§75 可追溯；不含环境变量）。
    pub command_preview: Option<String>,
    pub working_dir: Option<String>,
    /// 启动日志探测到的端口（不做端口扫描）。
    pub ports: Vec<u16>,
    pub exit_code: Option<i32>,
    /// 是否为 GitWorkspace 重启后接管的孤儿进程。
    pub adopted: bool,
    pub started_at: String,
    pub stopped_at: Option<String>,
    pub uptime_seconds: Option<u64>,
    pub cpu_percent: Option<f32>,
    pub memory_bytes: Option<u64>,
}

/// Runtime 进程事件（§27「状态迁移全程发事件」）。
///
/// 内部事件流：本任务内由 [`RuntimeEventSink`] 消费；R-12 把它桥接到
/// Tauri event / Task Engine。camelCase + `kind` tag，IPC-ready。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum RuntimeEvent {
    /// 生命周期状态迁移。
    #[serde(rename_all = "camelCase")]
    Lifecycle {
        process_id: i64,
        runtime_name: String,
        from: LifecycleStatus,
        to: LifecycleStatus,
        /// RFC3339 时间戳。
        at: String,
    },
    /// 指标采样（CPU / 内存 / uptime），节流发出。
    #[serde(rename_all = "camelCase")]
    Metrics {
        process_id: i64,
        cpu_percent: f32,
        memory_bytes: u64,
        uptime_seconds: u64,
    },
    /// 启动日志探测到端口。
    #[serde(rename_all = "camelCase")]
    Ports { process_id: i64, ports: Vec<u16> },
    /// 日志批次（R-11 日志引擎）：聚合节流后发出；`lines` 已全部脱敏。
    #[serde(rename_all = "camelCase")]
    Logs {
        process_id: i64,
        runtime_name: String,
        lines: Vec<crate::runtime::logs::LogLine>,
    },
    /// 进程退出（含自然退出、被停止、崩溃）。
    #[serde(rename_all = "camelCase")]
    Exited {
        process_id: i64,
        runtime_name: String,
        exit_code: Option<i32>,
        /// true = 非预期退出（`ProcessCrashed` 语义）。
        crashed: bool,
    },
}

/// Runtime 事件消费端（R-12 桥接 Tauri event 的挂接点）。
pub trait RuntimeEventSink: Send + Sync {
    fn emit(&self, event: RuntimeEvent);
}

/// 生产默认 sink：写应用日志（R-12 前的事件落点）。
pub struct LoggingEventSink;

impl RuntimeEventSink for LoggingEventSink {
    fn emit(&self, event: RuntimeEvent) {
        match &event {
            RuntimeEvent::Lifecycle {
                process_id,
                runtime_name,
                from,
                to,
                ..
            } => log::info!(
                "R-10: runtime '{runtime_name}' (process {process_id}) {} → {}",
                from.as_str(),
                to.as_str()
            ),
            RuntimeEvent::Exited {
                process_id,
                runtime_name,
                exit_code,
                crashed,
            } => log::info!(
                "R-10: runtime '{runtime_name}' (process {process_id}) exited \
                 (code {exit_code:?}, crashed={crashed})"
            ),
            RuntimeEvent::Metrics { .. } | RuntimeEvent::Ports { .. } => {
                log::debug!("R-10: event {event:?}")
            }
            RuntimeEvent::Logs {
                process_id, lines, ..
            } => log::debug!(
                "R-11: {} log line(s) captured for process {process_id}",
                lines.len()
            ),
        }
    }
}

/// 测试用 sink：收集全部事件供断言。
#[cfg(test)]
#[derive(Default)]
pub struct VecEventSink {
    pub events: std::sync::Mutex<Vec<RuntimeEvent>>,
}

#[cfg(test)]
impl VecEventSink {
    pub fn collected(&self) -> Vec<RuntimeEvent> {
        self.events.lock().unwrap().clone()
    }
}

#[cfg(test)]
impl RuntimeEventSink for VecEventSink {
    fn emit(&self, event: RuntimeEvent) {
        self.events.lock().unwrap().push(event);
    }
}
