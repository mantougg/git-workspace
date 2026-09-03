use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use crate::error::{AppError, AppResult};
use crate::process::kill_tree::kill_process_tree;
use crate::runtime::launch::store;
use crate::runtime::launch::{LifecycleStatus, RuntimeProcessInfo};

use super::*;

impl RuntimeProcessManager {
    /// 优雅停止：SIGTERM（Unix）→ grace 等待 → 超时升级杀进程树。
    /// Windows 无 SIGTERM 语义：`terminate` 返回 false，直接升级强杀。
    /// 幂等：终态行直接返回当前快照。
    pub fn stop(self: &Arc<Self>, process_id: i64, grace: Option<Duration>) -> AppResult<RuntimeProcessInfo> {
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
    pub fn kill(self: &Arc<Self>, process_id: i64, confirmed: bool) -> AppResult<RuntimeProcessInfo> {
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

    /// GitWorkspace 启动时调用（R-12 接入）：对账上次会话遗留的非终态行。
    ///
    /// - 活进程（pid + start_time 匹配）→ 接管（adopted），恢复 Stop/Kill 与
    ///   指标采样；上次退出时处于 Stopping 的补发 SIGTERM 完成停止。
    /// - 死进程 → Starting/Running 落 Failed（`ProcessCrashed`，退出码不可得
    ///   记 None）；Stopping 落 Stopped。
    /// - 从未 spawn 的行（Created/Preparing/Resolving/Building）→ Failed
    ///   （启动被 GitWorkspace 退出打断）。
    pub fn reconcile_on_startup(self: &Arc<Self>, workspace_id: i64) -> AppResult<Vec<RuntimeProcessInfo>> {
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
                        (Some(pid), Some(start_time)) => self.deps.launch_runner.alive(pid, Some(start_time)),
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
                    self.active.lock().unwrap().insert(row.id, handle.clone());
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
}
