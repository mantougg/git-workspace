//! notify 事件收集与去抖（R-17，B-07 拆分）。
//!
//! OS 回调线程只做事件路径收集（不做任何重活）；防抖 worker 按静默窗口
//! [`DEBOUNCE_WINDOW`](super::DEBOUNCE_WINDOW) 收集突发后一次性处理，
//! 避免文件保存风暴触发重复构建。均为纯函数（可单测）。

use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use std::time::Duration;

/// 从单个 notify 事件收集值得处理的文件路径：只保留文件或带扩展名的
/// 路径（目录创建等噪音不进通道）。
pub(super) fn collect_event_paths(event: notify::Event) -> Vec<PathBuf> {
    event
        .paths
        .into_iter()
        .filter(|p| p.is_file() || p.extension().is_some())
        .collect()
}

/// 静默窗口收集：以 `first` 为起点，在 `window` 内继续收积后续突发，
/// 窗口耗尽或通道关闭即返回。返回合并后的全部路径。
pub(super) fn collect_batch(rx: &Receiver<Vec<PathBuf>>, first: Vec<PathBuf>, window: Duration) -> Vec<PathBuf> {
    let mut paths = first;
    let deadline = std::time::Instant::now() + window;
    loop {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            break;
        }
        match rx.recv_timeout(remaining) {
            Ok(more) => paths.extend(more),
            Err(_) => break,
        }
    }
    paths
}
