//! Runtime 任务的执行挂接点（R-12，§65 Task Engine 集成）。
//!
//! Task Engine（T-05）的执行器原本只会跑 git / shell 操作；Runtime 的
//! Build / Start / Stop / Restart / ResolveDependencies 通过
//! [`RuntimeTaskHandler`] 分发给 Runtime 模块，保持 `task/` 不反向依赖
//! `runtime/` 的具体实现（依赖倒置，测试可注入假实现）。

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use crate::error::AppResult;
use crate::models::task::TaskType;

/// Runtime 任务执行端。worker 在 `spawn_blocking` 里同步调用：
/// 实现方负责把 `cancel` 置位翻译成尽快返回（杀 Maven 进程树 / 停止正在
/// 启动的应用），返回的 `Option<String>` 作为任务输出摘要（DAG 报告用）。
pub trait RuntimeTaskHandler: Send + Sync {
    fn execute(&self, task_type: &TaskType, cancel: Arc<AtomicBool>) -> AppResult<Option<String>>;
}
