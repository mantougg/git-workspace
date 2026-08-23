//! Build 并发限流闸（R-09，全局约束 §6：最大并发 Build = 2，可配置）。
//!
//! 只是 std `Mutex + Condvar` 的 permit 池——限流语义，不是并行执行框架；
//! 任务队列集成（T-05）由 R-12 完成，届时把 `acquire` 放在任务执行体内即可。

use std::sync::{Arc, Condvar, Mutex};

/// 全局约束 §6 的起步值：最大并发 Build = 2。
pub const DEFAULT_MAX_CONCURRENT_BUILDS: usize = 2;

/// Build permit 池。`acquire` 阻塞到有空位；permit Drop 时归还。
pub struct BuildScheduler {
    shared: Arc<Shared>,
}

struct Shared {
    available: Mutex<usize>,
    condvar: Condvar,
}

impl BuildScheduler {
    pub fn new(max_concurrent: usize) -> Self {
        assert!(max_concurrent >= 1, "max_concurrent must be at least 1");
        Self {
            shared: Arc::new(Shared {
                available: Mutex::new(max_concurrent),
                condvar: Condvar::new(),
            }),
        }
    }

    /// 获取一个构建 permit；无空位时阻塞等待。
    pub fn acquire(&self) -> BuildPermit {
        let mut available = self.shared.available.lock().unwrap();
        while *available == 0 {
            available = self.shared.condvar.wait(available).unwrap();
        }
        *available -= 1;
        BuildPermit {
            shared: Arc::clone(&self.shared),
        }
    }

    /// 当前剩余空位（测试与诊断用）。
    pub fn available(&self) -> usize {
        *self.shared.available.lock().unwrap()
    }
}

impl Default for BuildScheduler {
    fn default() -> Self {
        Self::new(DEFAULT_MAX_CONCURRENT_BUILDS)
    }
}

/// 持有一个构建空位；Drop 时归还并唤醒等待者。
pub struct BuildPermit {
    shared: Arc<Shared>,
}

impl Drop for BuildPermit {
    fn drop(&mut self) {
        let mut available = self.shared.available.lock().unwrap();
        *available += 1;
        self.shared.condvar.notify_one();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{Duration, Instant};

    #[test]
    fn never_exceeds_two_concurrent_builds() {
        let scheduler = Arc::new(BuildScheduler::new(2));
        let running = Arc::new(AtomicUsize::new(0));
        let max_seen = Arc::new(AtomicUsize::new(0));

        let mut handles = Vec::new();
        for _ in 0..8 {
            let scheduler = Arc::clone(&scheduler);
            let running = Arc::clone(&running);
            let max_seen = Arc::clone(&max_seen);
            handles.push(std::thread::spawn(move || {
                let _permit = scheduler.acquire();
                let current = running.fetch_add(1, Ordering::SeqCst) + 1;
                max_seen.fetch_max(current, Ordering::SeqCst);
                std::thread::sleep(Duration::from_millis(50));
                running.fetch_sub(1, Ordering::SeqCst);
            }));
        }
        for handle in handles {
            handle.join().unwrap();
        }
        assert_eq!(max_seen.load(Ordering::SeqCst), 2);
        assert_eq!(scheduler.available(), 2);
    }

    #[test]
    fn dropped_permit_unblocks_waiter() {
        let scheduler = BuildScheduler::new(1);
        let permit = scheduler.acquire();
        assert_eq!(scheduler.available(), 0);
        drop(permit);
        let start = Instant::now();
        let _permit = scheduler.acquire();
        assert!(start.elapsed() < Duration::from_secs(1));
    }
}
