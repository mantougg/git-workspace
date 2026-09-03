//! Watch 任务提交（R-17，B-07 拆分）：RebuildRestart / Resolve 任务提交
//! 与 `WatchTaskSubmitter` 端口。提交走 TaskManager 现有通道，任务类型与
//! payload 不变（§4.7——提交路径集中在本模块，便于审计）。

use crate::models::task::{RuntimeOp, RuntimeTaskOptions, TaskRequest, TaskType};

/// 自动重启任务提交端（解耦 TaskManager，测试注入假提交器）。
pub trait WatchTaskSubmitter: Send + Sync {
    fn submit(&self, request: TaskRequest) -> crate::error::AppResult<String>;
}

impl WatchTaskSubmitter for crate::task::manager::TaskManager {
    fn submit(&self, request: TaskRequest) -> crate::error::AppResult<String> {
        let mut ids = self.submit(&[request])?;
        ids.pop()
            .ok_or_else(|| crate::error::AppError::Task("任务提交失败：未返回任务 id".into()))
    }
}

impl super::RuntimeWatchEngine {
    /// 提交 RebuildRestart 任务。返回是否成功提交；失败时调用方负责把
    /// `modules` 放回 pending（本方法不再动 in_flight 状态）。
    pub(super) fn submit_rebuild(&self, workspace_id: i64, runtime_name: &str, modules: &[String]) -> bool {
        let Some(submitter) = self.task_manager.lock().unwrap().clone() else {
            log::debug!("R-17: task manager not attached yet; rebuild for '{runtime_name}' kept pending");
            return false;
        };
        let request = TaskRequest {
            task_type: TaskType::Runtime {
                op: RuntimeOp::RebuildRestart,
                workspace_id,
                runtime_name: runtime_name.to_string(),
                options: RuntimeTaskOptions {
                    affected_modules: modules.to_vec(),
                    ..Default::default()
                },
            },
            repo_path: String::new(),
            repo_name: format!("自动重建重启 · {runtime_name}"),
        };
        match submitter.submit(request) {
            Ok(task_id) => {
                log::info!("R-17: auto rebuild+restart submitted for '{runtime_name}' (task {task_id})");
                true
            }
            Err(e) => {
                log::warn!("R-17: failed to submit auto rebuild for '{runtime_name}': {e}");
                false
            }
        }
    }

    pub(super) fn submit_resolve(&self, workspace_id: i64) {
        let Some(submitter) = self.task_manager.lock().unwrap().clone() else {
            return;
        };
        let request = TaskRequest {
            task_type: TaskType::Runtime {
                op: RuntimeOp::ResolveDependencies,
                workspace_id,
                runtime_name: String::new(),
                options: RuntimeTaskOptions::default(),
            },
            repo_path: String::new(),
            repo_name: format!("watch: workspace #{workspace_id} pom 变化 → 依赖重算"),
        };
        if let Err(e) = submitter.submit(request) {
            log::warn!("R-17: failed to submit resolve after pom change: {e}");
        }
    }
}
