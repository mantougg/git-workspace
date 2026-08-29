use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

use sysinfo::{Pid, ProcessRefreshKind, RefreshKind, System};

use crate::runtime::launch::store;
use crate::runtime::launch::RuntimeEvent;

use super::*;

/// 每 N 拍采样落一次 DB（其余只发事件）。
const DB_FLUSH_EVERY_TICKS: u32 = 5;

impl RuntimeProcessManager {
    pub(super) fn ensure_sampler(self: &Arc<Self>) {
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
}
