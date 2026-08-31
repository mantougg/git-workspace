use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::error::{AppError, AppResult};
use crate::process::kill_tree::kill_process_tree;
use crate::process::streaming::OutputStream;
use crate::runtime::launch::store;
use crate::runtime::launch::{LifecycleStatus, RuntimeEvent, RuntimeProcessInfo};
use crate::runtime::logs::LogPhase;

use super::output::{startup_banner, startup_port};
use super::*;

/// adopted（非子进程）监控的轮询间隔。
const ADOPT_POLL_INTERVAL: Duration = Duration::from_secs(1);

impl RuntimeProcessManager {
    /// spawn 托管 monitor 线程：跑 `LaunchRunner::run` 直到退出，期间把输出
    /// 行送进 R-11 日志会话（内部脱敏落盘）、探测启动横幅与端口，结束后
    /// 先收口日志会话（drain + 完整落盘）再按分类收尾状态。
    pub(super) fn spawn_monitor(
        self: &Arc<Self>,
        process_id: i64,
        runtime_name: String,
        mut command: std::process::Command,
        handle: &ActiveProcess,
        kind: crate::runtime::config::RuntimeKind,
        start_grace: Duration,
    ) {
        let this = Arc::clone(self);
        let handle = handle.clone();
        std::thread::spawn(move || {
            let log_session = this.deps.logs.session(process_id);
            let mut ports_seen: Vec<u16> = Vec::new();
            let mut running_flagged = false;
            let monitor_started = Instant::now();
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
                    if !running_flagged && startup_banner(kind, line) {
                        running_flagged = true;
                        let (lock, cv) = &*handle.progress;
                        lock.lock().unwrap().running = true;
                        cv.notify_all();
                    }
                    let within_grace = monitor_started.elapsed() <= start_grace;
                    if kind != crate::runtime::config::RuntimeKind::Node || within_grace {
                        if let Some(port) = startup_port(kind, line) {
                            if !ports_seen.contains(&port)
                                && (kind != crate::runtime::config::RuntimeKind::Node
                                    || ports_seen.is_empty())
                            {
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
    pub(super) fn spawn_adopted_monitor(
        self: &Arc<Self>,
        process_id: i64,
        runtime_name: String,
        pid: u32,
        start_time: u64,
        handle: ActiveProcess,
    ) {
        let this = Arc::clone(self);
        std::thread::spawn(move || loop {
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
        });
    }

    /// 退出收尾：按「停止中 / 退出码 / 是否 adopted」分类终态并落库、发事件。
    /// 分类规则见 [`classify_exit`]。幂等：行已是终态时直接返回。
    pub(super) fn finalize_exit(
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

    /// 「Running 之前就退出」的启动结果整理：monitor 已 finalize；把 Failed
    /// 翻译成 `ProcessStartFailed` 错误返回（自然退出码 0 的返回 Stopped 快照）。
    pub(super) fn finish_early_exit(
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

    /// 轮询等待进程消失（非子进程场景，无 wait() 可用）。
    pub(super) fn wait_dead(&self, pid: u32, start_time: Option<u64>, timeout: Duration) {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if !self.deps.launch_runner.alive(pid, start_time) {
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    pub(super) fn wait_pid_or_outcome(&self, handle: &ActiveProcess, timeout: Duration) -> PidWait {
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

    pub(super) fn wait_running_or_outcome(
        &self,
        handle: &ActiveProcess,
        grace: Duration,
    ) -> RunWait {
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
    pub(super) fn wait_outcome(&self, handle: &ActiveProcess, timeout: Duration) -> bool {
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
}
