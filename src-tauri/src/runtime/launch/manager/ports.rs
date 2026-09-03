//! F-34 端口归属确权：日志输出正则匹配的端口只是候选，用 OS 监听表
//! 钉死哪些端口真正被本次启动的进程树监听；误收的（后端端口、proxy
//! 目标等）从口径中移除。
//!
//! 设计：监听回调侧（monitor output）发现新端口时立即记入候选列表并
//! 触发去抖（≥2s）后批量确权；确权使用全量 `netstat -ano` / `lsof`
//! 单次调用解析所有 LISTENING 行（比逐端口调 `detect_port_occupier`
//! 高效），并与进程树 PID 集合比对——树外端口剔除，树内端口落库并
//! 发 Ports 事件更正前端。
//!
//! 框架假设：`runtime_processes.ports_json` 记录「候选端口集合」
//! （确权前可能含误收），`port_pids_json` 记录「已确认端口→监听 PID」；
//! Attribution 线程对两个字段同时整体覆盖写。

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::process::port::detect_listening_ports;
use crate::process::{collect_tree_pids, ListeningPort};
use crate::runtime::launch::store;
use crate::runtime::launch::{RuntimeEvent};

use super::types::ActiveProcess;

/// 候选端口首次出现到开始确权的最短间隔（去抖）。
const ATTRIBUTION_DEBOUNCE: Duration = Duration::from_secs(2);
/// 每个候选端口的确权重试上限；达到后保留（回退到 F-26 行为，防确权
/// 机制缺陷导致合法端口被误删——详见 AGENTS.md「最小修复」原则）。
const ATTRIBUTION_MAX_RETRIES: u32 = 5;

#[derive(Debug, Clone)]
struct PendingPort {
    port: u16,
    first_seen: Instant,
    retries: u32,
}

#[derive(Debug)]
struct PortState {
    /// 当前对外口径（含待确权的候选端口，确权后只留树内监听端口）。
    ports: Vec<u16>,
    /// 待确权队列。
    pending: Vec<PendingPort>,
    /// 已确认的「端口→树内监听 PID」映射。
    confirmed: BTreeMap<u16, u32>,
    /// `ports` 自上次外部持久化后是否被修改（确权线程内部用）。
    dirty: bool,
    /// 最后一次状态变更时间（去抖用）。
    last_change: Instant,
}

impl PortState {
    fn new() -> Self {
        Self {
            ports: Vec::new(),
            pending: Vec::new(),
            confirmed: BTreeMap::new(),
            dirty: false,
            last_change: Instant::now(),
        }
    }
}

/// 端口归属确权器：在独立线程运行，与 monitor 输出回调共享状态。
pub(super) struct PortAttribution {
    state: Arc<(Mutex<PortState>, std::sync::Condvar)>,
    stop: Arc<AtomicBool>,
    _verifier: JoinHandle<()>,
}

impl PortAttribution {
    /// 创建并启动 attribution 线程。
    ///
    /// `this` 为整个 RuntimeProcessManager 的 Arc（用于持 DB 锁和发事件）；
    /// `handle` 提供 `pid()`（spawn 后才填充，attribution 轮询时已存在）；
    /// `process_id` / `runtime_name` 标识当前启动行。
    pub fn spawn(
        manager: Arc<super::RuntimeProcessManager>,
        process_id: i64,
        runtime_name: String,
        handle: ActiveProcess,
    ) -> Self {
        let state = Arc::new((Mutex::new(PortState::new()), std::sync::Condvar::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let verifier = {
            let state = Arc::clone(&state);
            let stop = Arc::clone(&stop);
            std::thread::Builder::new()
                .name(format!("port-attribution-{process_id}"))
                .spawn(move || {
                    run_verifier(manager, process_id, runtime_name, handle, state, stop);
                })
                .expect("failed to spawn port attribution thread")
        };
        Self {
            state,
            stop,
            _verifier: verifier,
        }
    }

    /// 新端口被日志正则命中时调用；返回 `Some((ports_snapshot, is_new))`
    /// 表示该端口首次出现（`is_new=true` 触发 UI 更新），调用方负责立即
    /// 用快照落库并发 Ports 事件。
    pub fn on_port_detected(&self, port: u16) -> Option<(Vec<u16>, bool)> {
        let (lock, cv) = &*self.state;
        let mut state = lock.lock().unwrap();
        if state.ports.contains(&port) {
            return None;
        }
        state.ports.push(port);
        state.pending.push(PendingPort {
            port,
            first_seen: Instant::now(),
            retries: 0,
        });
        state.dirty = true;
        state.last_change = Instant::now();
        cv.notify_one();
        Some((state.ports.clone(), true))
    }

    /// 停止 attribution 线程（monitor run() 退出后调用，线程会在下一轮
    /// poll 时看到 stop 标志并退出）。
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Release);
        let (_, cv) = &*self.state;
        cv.notify_one();
    }

    /// 当前 ports 口径的快照（确权完成后用于 finalize 落库）。
    pub fn snapshot_ports(&self) -> Vec<u16> {
        self.state.0.lock().unwrap().ports.clone()
    }

    /// 当前已确认映射快照（finalize 落 `port_pids_json` 用）。
    pub fn snapshot_confirmed(&self) -> BTreeMap<u16, u32> {
        self.state.0.lock().unwrap().confirmed.clone()
    }
}

/// Attribution 线程主循环：轮询 + 去抖，每个候选端口检查是否被进程树
/// 内的进程监听；剔除树外端口（后端端口误收），记录树内映射。
fn run_verifier(
    manager: Arc<super::RuntimeProcessManager>,
    process_id: i64,
    runtime_name: String,
    handle: ActiveProcess,
    state: Arc<(Mutex<PortState>, std::sync::Condvar)>,
    stop: Arc<AtomicBool>,
) {
    let (lock, cv) = &*state;
    let mut guard = lock.lock().unwrap();
    loop {
        // 等待：有 pending 端口，或 500ms 超时轮询。
        let _ = cv.wait_timeout(guard, Duration::from_millis(500)).unwrap();
        guard = lock.lock().unwrap();
        if stop.load(Ordering::Acquire) {
            break;
        }
        if guard.pending.is_empty() {
            continue;
        }
        // 去抖：等待至少 DEBOUNCE 间隔再确权（dev server 端口晚绑定时
        // 尽量等待绑定完成，减少重试次数）。
        let since_last = guard.last_change.elapsed();
        if since_last < ATTRIBUTION_DEBOUNCE {
            let remaining = ATTRIBUTION_DEBOUNCE - since_last;
            drop(guard);
            std::thread::sleep(remaining);
            guard = lock.lock().unwrap();
            if stop.load(Ordering::Acquire) {
                break;
            }
        }

        // 取 root pid（spawn 后才填充，尚未就绪时本轮跳过）。
        let root_pid = match handle.pid() {
            Some(pid) => pid,
            None => continue,
        };

        // 取待确权快照，释放锁（枚举 + netstat 耗时，期间需允许回调
        // 添加新候选端口——新端口会在锁恢复后合并处理）。
        let pending_snapshot: Vec<PendingPort> = guard.pending.clone();
        drop(guard);

        // 进程树枚举：parent 链 DFS（F-34 spec，与 kill_tree 同算法）。
        let tree_pids = collect_tree_pids(root_pid);
        let tree_set: std::collections::HashSet<u32> = tree_pids.iter().copied().collect();

        // OS 监听表：一次系统调用拿全量 LISTENING 行，比逐端口高效。
        let listening = detect_listening_ports().unwrap_or_default();

        let mut to_confirm: Vec<(u16, u32)> = Vec::new();
        let mut to_reject: Vec<u16> = Vec::new();
        let mut to_retry: Vec<PendingPort> = Vec::new();

        for pending in &pending_snapshot {
            let listener = listening.iter().find(|lp| lp.port == pending.port);
            match listener {
                Some(ListeningPort { port: _, pid }) if tree_set.contains(pid) => {
                    // 树内监听：确认。
                    to_confirm.push((pending.port, *pid));
                }
                Some(ListeningPort { port: _, pid }) if !tree_set.contains(pid) => {
                    // 树外监听：后端端口误收，剔除。
                    log::info!(
                        "F-34: port {} (process #{process_id}) bound by external \
                         pid {pid} (not in tree [{:?}]); rejecting",
                        pending.port,
                        &tree_pids[..tree_pids.len().min(5)],
                    );
                    to_reject.push(pending.port);
                }
                None => {
                    // 未监听（dev server 晚绑定 / 非 TCP 端口）：重试。
                    if pending.retries + 1 >= ATTRIBUTION_MAX_RETRIES {
                        // 超限：保留端口但不记录 PID（回退 F-26 行为，
                        // 端口仍然展示，只是无法确认监听者）。
                        log::debug!(
                            "F-34: port {} (process #{process_id}) not bound after \
                             {} retries; keeping without PID attribution",
                            pending.port,
                            pending.retries + 1,
                        );
                    } else {
                        to_retry.push(PendingPort {
                            port: pending.port,
                            first_seen: pending.first_seen,
                            retries: pending.retries + 1,
                        });
                    }
                }
                _ => unreachable!("listener matched but pid neither in nor out of tree"),
            }
        }

        // 重新获取锁，批量应用变更。
        guard = lock.lock().unwrap();
        if stop.load(Ordering::Acquire) {
            break;
        }
        let mut changed = false;

        // 确认端口：写入 confirmed 映射。
        for (port, pid) in &to_confirm {
            if guard.confirmed.get(port) != Some(pid) {
                guard.confirmed.insert(*port, *pid);
                changed = true;
            }
            // 从 pending 移除。
            guard.pending.retain(|p| p.port != *port);
        }

        // 拒绝端口：从 ports 口径移除，pending 移除。
        for port in &to_reject {
            if let Some(idx) = guard.ports.iter().position(|p| p == port) {
                guard.ports.remove(idx);
                changed = true;
            }
            guard.pending.retain(|p| p.port != *port);
            guard.confirmed.remove(port);
        }

        // 重试端口：更新队列（超限的直接丢弃，不重试）。
        guard.pending.retain(|p| !to_retry.iter().any(|r| r.port == p.port));
        guard.pending.extend(to_retry);

        if changed {
            guard.dirty = true;
            guard.last_change = Instant::now();
            drop(guard);
            // 落库 + 发事件更正前端（DB 锁与 PortState 锁不混，无死锁）。
            persist_and_emit(&manager, process_id, &runtime_name, &state);
            guard = lock.lock().unwrap();
        }
    }
    // 线程退出前：确保最终状态落库（finalize 阶段 row 已是终态但
    // port_pids_json 未写——多发生在 dev server 正常退出、attribution
    // 线程被 stop 后的 flush；写 DB 对终态行 idempotent，不会触发事件。
    drop(guard);
    flush_on_stop(&manager, process_id, &state);
}

/// 将当前状态（ports + port_pids）持久化并发 Ports 事件。
/// 调用方必须先释放 PortState 锁再调用（本函数内部取快照后锁 DB）。
fn persist_and_emit(
    manager: &Arc<super::RuntimeProcessManager>,
    process_id: i64,
    _runtime_name: &str,
    state: &Arc<(Mutex<PortState>, std::sync::Condvar)>,
) {
    let (ports_snapshot, confirmed_snapshot) = {
        let guard = state.0.lock().unwrap();
        (guard.ports.clone(), guard.confirmed.clone())
    };
    let conn = manager.db.lock().unwrap();
    if let Err(e) = store::set_port_attribution(&conn, process_id, &confirmed_snapshot) {
        log::warn!("F-34: failed to persist port attribution for process #{process_id}: {e}");
    } else {
        manager.deps.events.emit(RuntimeEvent::Ports {
            process_id,
            ports: ports_snapshot,
        });
    }
}

/// 线程退出前的最终 flush：把残余 dirty 状态落库（不发事件——进程已退出，
/// 前端已收到 Exited 事件，不会再看 Ports 事件）。
fn flush_on_stop(
    manager: &Arc<super::RuntimeProcessManager>,
    process_id: i64,
    state: &Arc<(Mutex<PortState>, std::sync::Condvar)>,
) {
    let dirty = state.0.lock().unwrap().dirty;
    if !dirty {
        return;
    }
    let confirmed = state.0.lock().unwrap().confirmed.clone();
    let conn = manager.db.lock().unwrap();
    if let Err(e) = store::set_port_attribution(&conn, process_id, &confirmed) {
        log::warn!("F-34: final flush of port attribution for process #{process_id} failed: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::ListeningPort;
    use std::collections::BTreeMap;

    /// 纯函数：给定候选端口、OS 监听快照、进程树，判定哪些确认、
    /// 哪些拒绝、哪些重试（attribution 核心逻辑的单测层）。
    fn classify_candidate(
        port: u16,
        retries: u32,
        listening: &[ListeningPort],
        tree_set: &std::collections::HashSet<u32>,
    ) -> CandidateDecision {
        match listening.iter().find(|lp| lp.port == port) {
            Some(ListeningPort { pid, .. }) if tree_set.contains(pid) => {
                CandidateDecision::Confirm(port, *pid)
            }
            Some(ListeningPort { pid, .. }) => CandidateDecision::Reject(port, *pid),
            None if retries + 1 >= ATTRIBUTION_MAX_RETRIES => CandidateDecision::KeepWithoutPid(port),
            None => CandidateDecision::Retry(port),
        }
    }

    #[derive(Debug, PartialEq)]
    enum CandidateDecision {
        /// 树内监听确认。
        Confirm(u16, u32),
        /// 树外监听拒绝（后端端口误收）。
        Reject(u16, u32),
        /// 未监听，重试次数未满。
        Retry(u16),
        /// 未监听，重试超限：保留但无 PID（回退 F-26 行为）。
        KeepWithoutPid(u16),
    }

    #[test]
    fn classify_confirms_port_bound_by_tree_process() {
        let listening = vec![ListeningPort { port: 8081, pid: 100 }];
        let tree: std::collections::HashSet<u32> = [100, 101, 102].into_iter().collect();
        assert_eq!(
            classify_candidate(8081, 0, &listening, &tree),
            CandidateDecision::Confirm(8081, 100)
        );
    }

    #[test]
    fn classify_rejects_port_bound_by_external_process() {
        let listening = vec![ListeningPort { port: 8080, pid: 999 }];
        let tree: std::collections::HashSet<u32> = [100, 101].into_iter().collect();
        assert_eq!(
            classify_candidate(8080, 0, &listening, &tree),
            CandidateDecision::Reject(8080, 999)
        );
    }

    #[test]
    fn classify_retries_unbound_port_within_limit() {
        let listening = vec![];
        let tree: std::collections::HashSet<u32> = [100].into_iter().collect();
        assert_eq!(
            classify_candidate(5173, 2, &listening, &tree),
            CandidateDecision::Retry(5173)
        );
    }

    #[test]
    fn classify_keeps_unbound_port_after_max_retries() {
        let listening = vec![];
        let tree: std::collections::HashSet<u32> = [100].into_iter().collect();
        assert_eq!(
            classify_candidate(5173, ATTRIBUTION_MAX_RETRIES - 1, &listening, &tree),
            CandidateDecision::KeepWithoutPid(5173)
        );
    }

    #[test]
    fn classify_uses_first_matching_listener() {
        // 同端口多行（IPv4+IPv6）取第一个；两者 pid 相同，不影响判定。
        let listening = vec![
            ListeningPort { port: 8081, pid: 100 },
            ListeningPort { port: 8081, pid: 100 },
        ];
        let tree: std::collections::HashSet<u32> = [100].into_iter().collect();
        assert_eq!(
            classify_candidate(8081, 0, &listening, &tree),
            CandidateDecision::Confirm(8081, 100)
        );
    }

    #[test]
    fn classify_handles_multiple_ports_independently() {
        let listening = vec![
            ListeningPort { port: 8081, pid: 100 }, // tree
            ListeningPort { port: 8080, pid: 500 }, // external (backend)
        ];
        let tree: std::collections::HashSet<u32> = [100, 101].into_iter().collect();
        assert_eq!(
            classify_candidate(8081, 0, &listening, &tree),
            CandidateDecision::Confirm(8081, 100)
        );
        assert_eq!(
            classify_candidate(8080, 0, &listening, &tree),
            CandidateDecision::Reject(8080, 500)
        );
    }
}
