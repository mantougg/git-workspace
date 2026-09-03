use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use crate::error::{AppError, AppResult};
use crate::models::task::{RuntimeOp, TaskType};
use crate::task::runtime::RuntimeTaskHandler;

use super::*;

impl RuntimeTaskHandler for RuntimeService {
    fn execute(&self, task_type: &TaskType, cancel: Arc<AtomicBool>) -> AppResult<Option<String>> {
        let TaskType::Runtime {
            op,
            workspace_id,
            runtime_name,
            options,
        } = task_type
        else {
            return Err(AppError::Task(format!(
                "RuntimeTaskHandler 收到非 Runtime 任务：{task_type:?}"
            )));
        };
        log::info!(
            "R-12: runtime task {:?} '{}' (workspace #{}) started",
            op,
            runtime_name,
            workspace_id
        );
        match op {
            RuntimeOp::Build => self.exec_build(*workspace_id, runtime_name, options, &cancel),
            RuntimeOp::Start => self.exec_start(*workspace_id, runtime_name, options, &cancel),
            RuntimeOp::Stop => self.exec_stop(*workspace_id, runtime_name),
            RuntimeOp::Restart => self.exec_restart(*workspace_id, runtime_name, options, &cancel),
            RuntimeOp::ResolveDependencies => self.exec_resolve(*workspace_id, &cancel),
            RuntimeOp::StartEnvironment => self.exec_start_environment(*workspace_id, runtime_name, &cancel),
            RuntimeOp::StopEnvironment => self.exec_stop_environment(*workspace_id, runtime_name),
            RuntimeOp::RebuildRestart => self.exec_rebuild_restart(*workspace_id, runtime_name, options, &cancel),
        }
    }
}
