use std::collections::BTreeSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::error::{AppError, AppResult};
use crate::models::task::{RuntimeOp, RuntimeTaskOptions, TaskRequest, TaskType};
use crate::runtime::config;
use crate::runtime::events::{
    EnvironmentCompletedPayload, EnvironmentProgressPayload, EnvironmentServiceOutcome, ServiceExecState,
    EVENT_ENVIRONMENT_COMPLETED, EVENT_ENVIRONMENT_PROGRESS,
};
use crate::runtime::launch::StartOptions;

use super::*;

impl RuntimeService {
    /// `runtime_start_environment`：为 workspace 下全部 Runtime 配置各组装
    /// 一个 Start 任务（批量提交共享 batch id，T-20 聚合）。
    ///
    /// Phase 1 口径（R-15 之前的 environment = 「该 workspace 的全部配置」）：
    /// 不含服务依赖排序与并行编排（R-15），并发由 Task Scheduler 限流。
    pub fn start_environment_requests(&self, workspace_id: i64) -> AppResult<Vec<TaskRequest>> {
        let conn = self.db.lock().unwrap();
        let configs = config::list_configs(&conn, workspace_id)?;
        drop(conn);
        Ok(configs
            .into_iter()
            .map(|summary| {
                self.operation_task_request(
                    &RuntimeOperationRequest {
                        workspace_id,
                        runtime_name: summary.name,
                        options: RuntimeTaskOptions::default(),
                    },
                    RuntimeOp::Start,
                )
            })
            .collect())
    }

    /// `runtime_stop_environment`：只为当前有活跃进程的配置组装 Stop 任务
    /// （没有活跃进程的配置不需要 Stop 任务，避免空转）。
    pub fn stop_environment_requests(&self, workspace_id: i64) -> AppResult<Vec<TaskRequest>> {
        let running: BTreeSet<String> = self
            .processes
            .list_processes(workspace_id)?
            .into_iter()
            .filter(|p| p.status.is_active())
            .map(|p| p.runtime_name)
            .collect();
        let conn = self.db.lock().unwrap();
        let configs = config::list_configs(&conn, workspace_id)?;
        drop(conn);
        Ok(configs
            .into_iter()
            .filter(|summary| running.contains(&summary.name))
            .map(|summary| {
                self.operation_task_request(
                    &RuntimeOperationRequest {
                        workspace_id,
                        runtime_name: summary.name,
                        options: RuntimeTaskOptions::default(),
                    },
                    RuntimeOp::Stop,
                )
            })
            .collect())
    }

    /// `runtime_start_named_environment` 的任务组装（环境名放 `runtime_name`
    /// 字段；任务面板显示为「环境 <name>」）。
    pub fn named_environment_task_request(&self, workspace_id: i64, environment: &str, op: RuntimeOp) -> TaskRequest {
        TaskRequest {
            task_type: TaskType::Runtime {
                op,
                workspace_id,
                runtime_name: environment.to_string(),
                options: RuntimeTaskOptions::default(),
            },
            repo_path: String::new(),
            repo_name: format!("环境 {environment}"),
        }
    }

    fn emit_environment_progress(
        &self,
        workspace_id: i64,
        environment: &str,
        service: &str,
        state: ServiceExecState,
        detail: Option<String>,
    ) {
        self.emit(
            EVENT_ENVIRONMENT_PROGRESS,
            &EnvironmentProgressPayload {
                workspace_id,
                environment: environment.to_string(),
                service: service.to_string(),
                state,
                detail,
                at: Self::now(),
            },
        );
    }

    /// R-16 就绪门限：等待服务 Healthy（或就绪超时放行）。
    ///
    /// - 有探针：轮询健康快照，`Healthy` 即就绪；超时（默认 60s，可按服务
    ///   覆盖）按警告放行（应用仍在运行，只是未达 Healthy）。
    /// - 无探针：`processes.start` 返回时进程已确认 Running，视为就绪
    ///   （快照缺位时的首个轮询窗口即返回超时放行语义，秒级）。
    fn wait_service_ready(
        &self,
        _workspace_id: i64,
        _runtime_name: &str,
        process_id: i64,
        timeout: Duration,
    ) -> Result<String, String> {
        let deadline = Instant::now() + timeout;
        let started = Instant::now();
        // 无探针服务：processes.start 返回时已确认 Running，立即就绪
        // （R-15 第一版「固定顺序占位」的退化路径；探针门限归 R-16 引擎）。
        if !self.health.has_monitor(process_id) {
            // 给探针注册留一个小窗口（Running 迁移与 monitor spawn 同线程序，
            // 此处只是防御性二次确认）。
            std::thread::sleep(Duration::from_millis(150));
            if !self.health.has_monitor(process_id) {
                return Ok("进程 Running（未配置探针，跳过就绪等待）".into());
            }
        }
        loop {
            // 进程先死 → 就绪等待失败（编排按失败处理，依赖分支跳过）。
            if let Ok(Some(info)) = self.processes.get_process(process_id) {
                if info.status.is_terminal() {
                    return Err(format!("服务在就绪等待期间退出（状态 {}）", info.status.as_str()));
                }
            }
            if let Some(snapshot) = self.health.snapshot(process_id) {
                match snapshot.phase {
                    crate::runtime::events::HealthStatus::Healthy => {
                        return Ok(format!("Healthy（{}ms）", started.elapsed().as_millis()));
                    }
                    crate::runtime::events::HealthStatus::Stopped => {
                        return Err("探针判定服务已停止".into());
                    }
                    _ => {}
                }
            }
            if Instant::now() >= deadline {
                return Ok(format!("就绪等待超时（{:?}），进程仍在运行，放行依赖分支", timeout));
            }
            std::thread::sleep(Duration::from_millis(250));
        }
    }

    /// §38 Start Environment：拓扑分波，波内并行启动（构建并发受 §66
    /// Build permit 池约束），波间严格串行；依赖失败的服务及其下游标记
    /// Skipped（部分失败语义：不影响无依赖分支）。
    pub(super) fn exec_start_environment(
        &self,
        workspace_id: i64,
        environment_name: &str,
        cancel: &Arc<AtomicBool>,
    ) -> AppResult<Option<String>> {
        let environment = {
            let conn = self.db.lock().unwrap();
            let root = config::workspace_root(&conn, workspace_id)?;
            let environment = crate::runtime::environment::get_environment(&root, environment_name)?;
            crate::runtime::environment::validate_environment_configs(&conn, workspace_id, &environment)?;
            environment
        };
        let env_name: &str = &environment.name;
        let waves = crate::runtime::environment::topo_sort_services(&environment)?;
        log::info!(
            "R-15: starting environment '{}' ({} services, {} waves)",
            environment.name,
            environment.services.len(),
            waves.len()
        );

        // 服务终态收集（state + detail）。
        let mut outcomes: std::collections::BTreeMap<String, (ServiceExecState, Option<String>)> = environment
            .services
            .iter()
            .map(|s| (s.runtime_name.clone(), (ServiceExecState::Starting, None)))
            .collect();

        for wave in &waves {
            if cancel.load(Ordering::Relaxed) {
                // 取消：剩余服务标记 Skipped，汇总后返回。
                for name in waves.iter().flatten() {
                    if let Some(entry) = outcomes.get_mut(name) {
                        if entry.0 == ServiceExecState::Starting && entry.1.is_none() {
                            *entry = (ServiceExecState::Skipped, Some("环境启动已取消".into()));
                        }
                    }
                }
                break;
            }
            // 波内并行：scoped threads（波结束即 join，可安全借用 &self）。
            // 构建阶段受 §66 Build permit 池约束（排队调度而非无脑并发）。
            // 依赖状态在进入本波前已定稿（依赖都在更早的波次），提前检查。
            let plans: Vec<(crate::runtime::environment::EnvironmentService, Vec<String>)> = wave
                .iter()
                .filter_map(|service_name| {
                    let service = environment.services.iter().find(|s| &s.runtime_name == service_name)?;
                    let failed_deps: Vec<String> = service
                        .depends_on
                        .iter()
                        .filter(|dep| {
                            outcomes
                                .get(*dep)
                                .map(|(state, _)| *state != ServiceExecState::Ready)
                                .unwrap_or(true)
                        })
                        .cloned()
                        .collect();
                    Some((service.clone(), failed_deps))
                })
                .collect();
            let results: Vec<(String, ServiceExecState, Option<String>)> = std::thread::scope(|scope| {
                let mut handles = Vec::new();
                for (service, failed_deps) in plans {
                    let cancel = Arc::clone(cancel);
                    handles.push(scope.spawn(move || {
                        start_environment_service(self, workspace_id, env_name, &service, &failed_deps, &cancel)
                    }));
                }
                handles
                    .into_iter()
                    .map(|handle| handle.join().expect("environment service thread"))
                    .collect()
            });
            for (name, state, detail) in results {
                outcomes.insert(name, (state, detail));
            }
        }

        // 汇总事件 + 任务结果。
        let service_outcomes: Vec<EnvironmentServiceOutcome> = outcomes
            .iter()
            .map(|(name, (state, detail))| EnvironmentServiceOutcome {
                service: name.clone(),
                state: *state,
                detail: detail.clone(),
            })
            .collect();
        let ready = service_outcomes
            .iter()
            .filter(|o| o.state == ServiceExecState::Ready)
            .count();
        let skipped = service_outcomes
            .iter()
            .filter(|o| o.state == ServiceExecState::Skipped)
            .count();
        let failed: Vec<String> = service_outcomes
            .iter()
            .filter(|o| o.state == ServiceExecState::Failed)
            .map(|o| o.service.clone())
            .collect();
        let success = failed.is_empty() && ready > 0;
        let summary = format!(
            "环境 '{}' 编排完成：{} Ready / {} Failed / {} Skipped / 共 {} 服务",
            environment.name,
            ready,
            failed.len(),
            skipped,
            service_outcomes.len()
        );
        self.emit(
            EVENT_ENVIRONMENT_COMPLETED,
            &EnvironmentCompletedPayload {
                workspace_id,
                environment: environment.name.clone(),
                success,
                services: service_outcomes,
                at: Self::now(),
            },
        );
        if success {
            Ok(Some(summary))
        } else if ready == 0 {
            Err(AppError::Task(format!("{summary}；失败服务：{}", failed.join(", "))))
        } else {
            // 部分成功：任务成功收尾，失败明细在汇总与事件中可见。
            Ok(Some(format!("{summary}；失败服务：{}", failed.join(", "))))
        }
    }

    /// §38 Stop Environment：逆拓扑序分波并行停止（先停下游，再停上游）。
    pub(super) fn exec_stop_environment(&self, workspace_id: i64, environment_name: &str) -> AppResult<Option<String>> {
        let environment = {
            let conn = self.db.lock().unwrap();
            let root = config::workspace_root(&conn, workspace_id)?;
            crate::runtime::environment::get_environment(&root, environment_name)?
        };
        let mut waves = crate::runtime::environment::topo_sort_services(&environment)?;
        waves.reverse();
        let env_name: &str = &environment.name;

        let mut outcomes: std::collections::BTreeMap<String, (ServiceExecState, Option<String>)> = environment
            .services
            .iter()
            .map(|s| (s.runtime_name.clone(), (ServiceExecState::Stopped, None)))
            .collect();

        for wave in &waves {
            let results: Vec<(String, ServiceExecState, Option<String>)> = std::thread::scope(|scope| {
                let mut handles = Vec::new();
                for service_name in wave {
                    let service_name = service_name.clone();
                    handles.push(scope.spawn(move || {
                        let result = self.processes.stop_runtime(workspace_id, &service_name, None);
                        let (state, detail) = match result {
                            Ok(Some(info)) => {
                                (ServiceExecState::Stopped, Some(format!("已停止（pid {:?}）", info.pid)))
                            }
                            Ok(None) => (ServiceExecState::Stopped, Some("未在运行".to_string())),
                            Err(error) => (ServiceExecState::Failed, Some(error.to_string())),
                        };
                        self.emit_environment_progress(workspace_id, env_name, &service_name, state, detail.clone());
                        (service_name, state, detail)
                    }));
                }
                handles
                    .into_iter()
                    .map(|handle| handle.join().expect("environment stop thread"))
                    .collect()
            });
            for (name, state, detail) in results {
                outcomes.insert(name, (state, detail));
            }
        }

        let service_outcomes: Vec<EnvironmentServiceOutcome> = outcomes
            .iter()
            .map(|(name, (state, detail))| EnvironmentServiceOutcome {
                service: name.clone(),
                state: *state,
                detail: detail.clone(),
            })
            .collect();
        let stopped = service_outcomes
            .iter()
            .filter(|o| o.state == ServiceExecState::Stopped)
            .count();
        let failed: Vec<String> = service_outcomes
            .iter()
            .filter(|o| o.state == ServiceExecState::Failed)
            .map(|o| o.service.clone())
            .collect();
        let summary = format!(
            "环境 '{}' 停止完成：{}/{} 已停止；失败：[{}]",
            environment.name,
            stopped,
            service_outcomes.len(),
            failed.join(", ")
        );
        self.emit(
            EVENT_ENVIRONMENT_COMPLETED,
            &EnvironmentCompletedPayload {
                workspace_id,
                environment: environment.name.clone(),
                success: failed.is_empty(),
                services: service_outcomes,
                at: Self::now(),
            },
        );
        if failed.is_empty() {
            Ok(Some(summary))
        } else {
            Err(AppError::Task(summary))
        }
    }
}

/// 启动环境内的一个服务：依赖未就绪 → Skipped；否则 Start（带环境覆盖项）
/// → R-16 就绪门限（Healthy / 超时放行 / 进程死亡即失败）。
#[allow(clippy::too_many_arguments)]
fn start_environment_service(
    service_runtime: &RuntimeService,
    workspace_id: i64,
    environment_name: &str,
    service: &crate::runtime::environment::EnvironmentService,
    failed_deps: &[String],
    cancel: &Arc<AtomicBool>,
) -> (String, ServiceExecState, Option<String>) {
    let runtime_name = &service.runtime_name;
    if !failed_deps.is_empty() {
        let detail = format!("依赖未就绪：{}（部分失败语义：跳过本服务）", failed_deps.join(", "));
        service_runtime.emit_environment_progress(
            workspace_id,
            environment_name,
            runtime_name,
            ServiceExecState::Skipped,
            Some(detail.clone()),
        );
        return (runtime_name.clone(), ServiceExecState::Skipped, Some(detail));
    }

    service_runtime.emit_environment_progress(
        workspace_id,
        environment_name,
        runtime_name,
        ServiceExecState::Starting,
        None,
    );
    // 每服务一个取消 watcher（构建取消快路径 + 停止收尾）。
    let _watch = CancelWatch::start(&service_runtime.processes, workspace_id, runtime_name, cancel);
    let options = StartOptions {
        overrides: Some(crate::runtime::launch::EnvironmentOverrides {
            jdk: service.jdk.clone(),
            profile: service.profile.clone(),
            environment: service.environment.clone(),
            port: service.port,
        }),
        ..Default::default()
    };
    match service_runtime.processes.start(workspace_id, runtime_name, options) {
        Ok(info) => {
            let timeout = Duration::from_secs(
                service
                    .ready_timeout_seconds
                    .unwrap_or(crate::runtime::environment::DEFAULT_READY_TIMEOUT_SECS),
            );
            match service_runtime.wait_service_ready(workspace_id, runtime_name, info.process_id, timeout) {
                Ok(detail) => {
                    service_runtime.emit_environment_progress(
                        workspace_id,
                        environment_name,
                        runtime_name,
                        ServiceExecState::Ready,
                        Some(detail.clone()),
                    );
                    (runtime_name.clone(), ServiceExecState::Ready, Some(detail))
                }
                Err(detail) => {
                    service_runtime.emit_environment_progress(
                        workspace_id,
                        environment_name,
                        runtime_name,
                        ServiceExecState::Failed,
                        Some(detail.clone()),
                    );
                    (runtime_name.clone(), ServiceExecState::Failed, Some(detail))
                }
            }
        }
        Err(error) => {
            let detail = error.to_string();
            service_runtime.emit_environment_progress(
                workspace_id,
                environment_name,
                runtime_name,
                ServiceExecState::Failed,
                Some(detail.clone()),
            );
            (runtime_name.clone(), ServiceExecState::Failed, Some(detail))
        }
    }
}
