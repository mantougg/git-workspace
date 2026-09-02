//! 子进程生命周期管理（R-09 / R-10，全局约束 §3 / §6）。
//!
//! - `kill_tree`：按 parent 链枚举并终止整棵进程树（取消传播用），
//!   sysinfo 枚举方案天然跨平台（Windows 无 pgid）。R-10 起补充
//!   `terminate_process`（SIGTERM 优雅停止）与存活/start_time 探测
//!   （防 PID 复用）。
//! - `streaming`：spawn 子进程并把 stdout/stderr 按行实时转发给回调
//!   （无上限缓冲，直接管道转发），支持取消与超时，触发时杀整棵进程树；
//!   `spawn_streaming_ext` 额外在 spawn 后发布 pid（R-10 进程托管）。

pub mod kill_tree;
pub mod port;
pub mod streaming;

pub use kill_tree::{kill_process_tree, process_alive, process_start_time, terminate_process};
pub use port::{detect_port_occupier, is_port_in_use, PortOccupier};
pub use streaming::{spawn_streaming, spawn_streaming_ext, OutputStream, StreamingExit};
