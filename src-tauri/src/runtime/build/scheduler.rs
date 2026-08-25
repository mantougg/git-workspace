//! Build 并发限流闸（R-09，全局约束 §6：最大并发 Build = 2，可配置）。
//!
//! 只是 std `Mutex + Condvar` 的 permit 池——限流语义，不是并行执行框架；
//! 任务队列集成（T-05）由 R-12 完成，届时把 `acquire` 放在任务执行体内即可。
//!
//! R-12 扩展：上限运行时可调（[`BuildScheduler::set_max`]，§66「可配置」），
//! 并提供 [`BuildScheduler::acquire_cancelable`] 让排队中的任务响应取消
//! （R-12「排队、取消、超时」）。Dependency Resolve 的限流复用同一实现
//! （起步 4 并发，[`DEFAULT_MAX_CONCURRENT_RESOLVES`]）。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

/// 全局约束 §6 的起步值：最大并发 Build = 2。
pub const DEFAULT_MAX_CONCURRENT_BUILDS: usize = 2;
/// 全局约束 §6 的起步值：最大并发 Dependency Resolve = 4。
pub const DEFAULT_MAX_CONCURRENT_RESOLVES: usize = 4;

/// 等 permit 阻塞在 condvar 上时，定期看一眼取消标志的轮询间隔。
const CANCEL_POLL_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Debug)]
struct PoolState {
    /// 已发出且未归还的 permit 数（唯一账本；`available = max - outstanding`）。
    outstanding: usize,
    max: usize,
}

struct Shared {
    state: Mutex<PoolState>,
    condvar: Condvar,
}

/// Build permit 池。`acquire` 阻塞到有空位；permit Drop 时归还。
/// `set_max` 运行时调整上限：调大立即放行等待者；调小不回收已发出的
/// permit，随归还自然收敛（账本只记 outstanding，改 max 即生效）。
pub struct BuildScheduler {
    shared: Arc<Shared>,
}

impl BuildScheduler {
    pub fn new(max_concurrent: usize) -> Self {
        assert!(max_concurrent >= 1, "max_concurrent must be at least 1");
        Self {
            shared: Arc::new(Shared {
                state: Mutex::new(PoolState {
                    outstanding: 0,
                    max: max_concurrent,
                }),
                condvar: Condvar::new(),
            }),
        }
    }

    /// 获取一个构建 permit；无空位时阻塞等待。
    pub fn acquire(&self) -> BuildPermit {
        let mut state = self.shared.state.lock().unwrap();
        while state.outstanding >= state.max {
            state = self.shared.condvar.wait(state).unwrap();
        }
        state.outstanding += 1;
        BuildPermit {
            shared: Arc::clone(&self.shared),
        }
    }

    /// 获取 permit，等待期间响应取消：取消标志置位时返回 `None`（排队取消）。
    pub fn acquire_cancelable(&self, cancel: &AtomicBool) -> Option<BuildPermit> {
        let mut state = self.shared.state.lock().unwrap();
        loop {
            if state.outstanding < state.max {
                state.outstanding += 1;
                return Some(BuildPermit {
                    shared: Arc::clone(&self.shared),
                });
            }
            if cancel.load(Ordering::Relaxed) {
                return None;
            }
            let (guard, _timeout) = self
                .shared
                .condvar
                .wait_timeout(state, CANCEL_POLL_INTERVAL)
                .unwrap();
            state = guard;
        }
    }

    /// 运行时调整并发上限（§66 可配置）。调大立即唤醒等待者补位；调小
    /// 不回收已发出的 permit，归还时自然收敛到新上限。
    pub fn set_max(&self, new_max: usize) {
        assert!(new_max >= 1, "max_concurrent must be at least 1");
        let mut state = self.shared.state.lock().unwrap();
        state.max = new_max;
        drop(state);
        self.shared.condvar.notify_all();
    }

    /// 当前剩余空位（测试与诊断用）。
    pub fn available(&self) -> usize {
        let state = self.shared.state.lock().unwrap();
        state.max.saturating_sub(state.outstanding)
    }

    /// 当前上限。
    pub fn max(&self) -> usize {
        self.shared.state.lock().unwrap().max
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
        let mut state = self.shared.state.lock().unwrap();
        state.outstanding = state.outstanding.saturating_sub(1);
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

    /// R-12 §66：排队中的任务在取消标志置位后及时返回 None，不再占 permit。
    #[test]
    fn acquire_cancelable_returns_none_on_cancel() {
        let scheduler = Arc::new(BuildScheduler::new(1));
        let _held = scheduler.acquire();
        let cancel = Arc::new(AtomicBool::new(false));

        let scheduler2 = Arc::clone(&scheduler);
        let cancel2 = Arc::clone(&cancel);
        let handle = std::thread::spawn(move || scheduler2.acquire_cancelable(&cancel2));

        std::thread::sleep(Duration::from_millis(50));
        cancel.store(true, Ordering::Relaxed);
        let result = handle.join().unwrap();
        assert!(result.is_none());
        // 取消的等待者没有占用 permit：空位仍被 `_held` 持有。
        assert_eq!(scheduler.available(), 0);
    }

    #[test]
    fn acquire_cancelable_acquires_immediately_when_free() {
        let scheduler = BuildScheduler::new(1);
        let cancel = AtomicBool::new(false);
        let permit = scheduler.acquire_cancelable(&cancel);
        assert!(permit.is_some());
        assert_eq!(scheduler.available(), 0);
    }

    /// R-12 §66 可配置：调大立即放行等待者。
    #[test]
    fn set_max_increase_unblocks_waiters() {
        let scheduler = Arc::new(BuildScheduler::new(1));
        let _held = scheduler.acquire();

        let scheduler2 = Arc::clone(&scheduler);
        let handle = std::thread::spawn(move || {
            let _permit = scheduler2.acquire();
            std::thread::sleep(Duration::from_millis(20));
        });

        std::thread::sleep(Duration::from_millis(50));
        scheduler.set_max(2);
        handle.join().unwrap();
        assert_eq!(scheduler.max(), 2);
    }

    /// 调小不回收已发出的 permit，归还时自然收敛到新上限。
    #[test]
    fn set_max_decrease_converges_on_return() {
        let scheduler = BuildScheduler::new(2);
        let p1 = scheduler.acquire();
        let p2 = scheduler.acquire();
        scheduler.set_max(1);
        drop(p1); // 超出新上限，归还被丢弃
        assert_eq!(scheduler.available(), 0);
        drop(p2); // available(0) < max(1)，归还生效
        assert_eq!(scheduler.available(), 1);
        // 之后最多再发出 1 个 permit。
        let p3 = scheduler.acquire();
        assert_eq!(scheduler.available(), 0);
        drop(p3);
    }
}
