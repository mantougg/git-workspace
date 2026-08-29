use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use crate::models::task::RuntimeTaskOptions;
use crate::runtime::build::BuildOptions;
use crate::runtime::launch::manager::RuntimeProcessManager;
use crate::runtime::launch::StartOptions;

/// 任务取消 watcher 的轮询间隔。
const CANCEL_WATCH_INTERVAL: Duration = Duration::from_millis(100);

/// 取消触发的优雅停止宽限（比用户主动 Stop 短，取消语义是尽快终止）。
const CANCEL_STOP_GRACE: Duration = Duration::from_secs(5);

/// `RuntimeTaskOptions` → R-09 `BuildOptions`（未指定项走 Build 默认，
/// 对齐 IDEA Build 语义）。
pub fn build_options_of(options: &RuntimeTaskOptions) -> BuildOptions {
    let defaults = BuildOptions::default();
    BuildOptions {
        strategy: options.strategy,
        skip_tests: options.skip_tests.unwrap_or(defaults.skip_tests),
        offline: options.offline,
        // R-17：watch 影响分析的必建子集透传给流水线（与指纹子集合并）。
        affected_modules: options.affected_modules.clone(),
        ..defaults
    }
}

/// `RuntimeTaskOptions` → R-10 `StartOptions`。
pub fn start_options_of(options: &RuntimeTaskOptions) -> StartOptions {
    StartOptions {
        skip_build: options.skip_build,
        build_options: build_options_of(options),
        ..Default::default()
    }
}

/// Start / Restart 的取消 watcher：任务取消标志置位后，先走
/// `signal_build_cancel` 内存快路径杀掉进行中的 Maven 构建（不等 DB 锁），
/// 再走 `stop_runtime` 完成状态迁移与进程收尾；op 结束（Drop）前持续
/// 重试，覆盖「取消早于 start 注册句柄」的竞态。
pub(super) struct CancelWatch {
    done: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl CancelWatch {
    pub(super) fn start(
        processes: &Arc<RuntimeProcessManager>,
        workspace_id: i64,
        runtime_name: &str,
        cancel: &Arc<AtomicBool>,
    ) -> Self {
        let done = Arc::new(AtomicBool::new(false));
        let handle = {
            let processes = Arc::clone(processes);
            let cancel = Arc::clone(cancel);
            let done = Arc::clone(&done);
            let runtime_name = runtime_name.to_string();
            std::thread::spawn(move || loop {
                if done.load(Ordering::Relaxed) {
                    break;
                }
                if cancel.load(Ordering::Relaxed) {
                    processes.signal_build_cancel(workspace_id, &runtime_name);
                    if let Err(e) =
                        processes.stop_runtime(workspace_id, &runtime_name, Some(CANCEL_STOP_GRACE))
                    {
                        log::warn!("R-12: cancel-stop of '{runtime_name}' failed: {e}");
                    }
                }
                std::thread::sleep(CANCEL_WATCH_INTERVAL);
            })
        };
        Self {
            done,
            handle: Some(handle),
        }
    }
}

impl Drop for CancelWatch {
    fn drop(&mut self) {
        self.done.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            // watcher 可能正阻塞在 stop_runtime 的 DB 锁上：构建取消快路径
            // 已保证构建尽快退出、锁尽快释放，join 有界。
            let _ = handle.join();
        }
    }
}
