//! 子进程生命周期管理（R-09，全局约束 §3 / §6）。
//!
//! - `kill_tree`：按 parent 链枚举并终止整棵进程树（取消传播用），
//!   sysinfo 枚举方案天然跨平台（Windows 无 pgid）。
//! - `streaming`：spawn 子进程并把 stdout/stderr 按行实时转发给回调
//!   （无上限缓冲，直接管道转发），支持取消与超时，触发时杀整棵进程树。

pub mod kill_tree;
pub mod streaming;

pub use kill_tree::kill_process_tree;
pub use streaming::{spawn_streaming, OutputStream, StreamingExit};
