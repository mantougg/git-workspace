//! Runtime configuration IPC (R-07) + Runtime 控制面 IPC（R-12，§63）。

use std::collections::BTreeMap;
use std::sync::MutexGuard;

use rusqlite::Connection;
use tauri::{command, State};

use crate::error::{AppError, AppResult};
use crate::maven::{MavenProjectNode, RuntimeScope};
use crate::models::task::RuntimeOp;
use crate::runtime::{
    create_config, delete_config, get_config, get_workspace_environment, list_configs,
    resolve_environment, set_workspace_environment, update_config, ClosurePreview,
    CreateRuntimeConfigRequest, DependencyGraphView, LogExportOutcome, ProjectInspection,
    RuntimeApplicationConfig, RuntimeConfigSummary, RuntimeLogQuery, RuntimeOperationRequest,
    RuntimeProcessInfo, SchedulerConfig, ScriptApproval, UpdateRuntimeConfigRequest,
};
use crate::runtime::logs::LogEntry;
use crate::state::AppState;

fn lock_db<'a>(state: &'a State<'_, AppState>) -> AppResult<MutexGuard<'a, Connection>> {
    state
        .db
        .lock()
        .map_err(|error| AppError::Other(format!("DB lock error: {error}")))
}

#[command]
pub fn create_runtime_config(
    req: CreateRuntimeConfigRequest,
    state: State<'_, AppState>,
) -> AppResult<RuntimeApplicationConfig> {
    let conn = lock_db(&state)?;
    create_config(&conn, &req)
}

#[command]
pub fn update_runtime_config(
    req: UpdateRuntimeConfigRequest,
    state: State<'_, AppState>,
) -> AppResult<RuntimeApplicationConfig> {
    let conn = lock_db(&state)?;
    update_config(&conn, &req)
}

#[command]
pub fn delete_runtime_config(
    workspace_id: i64,
    name: String,
    state: State<'_, AppState>,
) -> AppResult<()> {
    let conn = lock_db(&state)?;
    delete_config(&conn, workspace_id, &name)
}

#[command]
pub fn list_runtime_configs(
    workspace_id: i64,
    state: State<'_, AppState>,
) -> AppResult<Vec<RuntimeConfigSummary>> {
    let conn = lock_db(&state)?;
    list_configs(&conn, workspace_id)
}

#[command]
pub fn get_runtime_config(
    workspace_id: i64,
    name: String,
    state: State<'_, AppState>,
) -> AppResult<RuntimeApplicationConfig> {
    let conn = lock_db(&state)?;
    get_config(&conn, workspace_id, &name)
}

#[command]
pub fn resolve_runtime_environment(
    workspace_id: i64,
    name: String,
    state: State<'_, AppState>,
) -> AppResult<BTreeMap<String, String>> {
    let conn = lock_db(&state)?;
    let values = resolve_environment(&conn, workspace_id, &name)?;
    // Never return sensitive values over IPC. Launcher internals should call
    // runtime::resolve_environment directly once process execution exists.
    Ok(values
        .into_iter()
        .map(|(key, value)| {
            let sensitive = crate::core::secret::is_sensitive_environment_key(&key);
            (
                key,
                if sensitive {
                    "••••••••".into()
                } else {
                    value
                },
            )
        })
        .collect())
}

#[command]
pub fn get_workspace_runtime_environment(
    workspace_id: i64,
    state: State<'_, AppState>,
) -> AppResult<BTreeMap<String, String>> {
    let conn = lock_db(&state)?;
    get_workspace_environment(&conn, workspace_id)
}

#[command]
pub fn set_workspace_runtime_environment(
    workspace_id: i64,
    environment: BTreeMap<String, String>,
    state: State<'_, AppState>,
) -> AppResult<()> {
    let conn = lock_db(&state)?;
    set_workspace_environment(&conn, workspace_id, environment)
}

// ---------------------------------------------------------------------------
// R-12 §63：Runtime 控制面命令
//
// 长操作（build / start / stop / restart / resolve_dependencies /
// start/stop_environment）一律提交 T-05 任务队列（返回任务 id，可取消、
// 有进度事件），不在 IPC 里同步执行；查询类命令直接走 RuntimeService 读侧。
// ---------------------------------------------------------------------------

/// 提交单个 Runtime 任务，返回任务 id。
fn submit_one(state: &State<'_, AppState>, req: crate::models::task::TaskRequest) -> AppResult<String> {
    let mut ids = state.task_manager.submit(&[req])?;
    ids.pop()
        .ok_or_else(|| AppError::Task("任务提交失败：未返回任务 id".into()))
}

/// §63 `runtime.list_projects`：workspace 的 Maven 项目索引（DB 热路径）。
#[command]
pub fn runtime_list_projects(
    workspace_id: i64,
    state: State<'_, AppState>,
) -> AppResult<Vec<MavenProjectNode>> {
    state.runtime.list_projects(workspace_id)
}

/// §63 `runtime.inspect_project`：单项目详情（模块 / 依赖边 / 源码映射）。
#[command]
pub fn runtime_inspect_project(
    workspace_id: i64,
    project: String,
    state: State<'_, AppState>,
) -> AppResult<ProjectInspection> {
    state.runtime.inspect_project(workspace_id, &project)
}

/// §63 `runtime.resolve_dependencies`：提交依赖解析同步任务（§66 限流 4）。
#[command]
pub fn runtime_resolve_dependencies(
    workspace_id: i64,
    state: State<'_, AppState>,
) -> AppResult<String> {
    let req = state.runtime.resolve_task_request(workspace_id);
    submit_one(&state, req)
}

/// §63 `runtime.get_dependency_graph`：依赖图（默认截断保护；`project_id`
/// 下钻单项目出边；`max_edges` 调截断上限）。
#[command]
pub fn runtime_get_dependency_graph(
    workspace_id: i64,
    project_id: Option<i64>,
    max_edges: Option<usize>,
    state: State<'_, AppState>,
) -> AppResult<DependencyGraphView> {
    state
        .runtime
        .dependency_graph(workspace_id, project_id, max_edges)
}

/// R-13：`runtime.get_closure`——按给定 Scope 计算闭包预览（R-03 §15，
/// fingerprint 缓存热路径），供 Runtime Scope 视图展示模块勾选结果。
#[command]
pub fn runtime_get_closure(
    workspace_id: i64,
    project: String,
    scope: RuntimeScope,
    state: State<'_, AppState>,
) -> AppResult<ClosurePreview> {
    state.runtime.closure_preview(workspace_id, &project, &scope)
}

/// §63 `runtime.build`：提交 Build 任务（§66 限流 2，排队可取消）。
#[command]
pub fn runtime_build(
    req: RuntimeOperationRequest,
    state: State<'_, AppState>,
) -> AppResult<String> {
    let req = state.runtime.operation_task_request(&req, RuntimeOp::Build);
    submit_one(&state, req)
}

/// §63 `runtime.start`：提交 Start 任务（Validate JDK/Maven → Resolve →
/// Reactor → Build → Start 子任务进度经 §64 事件流出）。
#[command]
pub fn runtime_start(
    req: RuntimeOperationRequest,
    state: State<'_, AppState>,
) -> AppResult<String> {
    let req = state.runtime.operation_task_request(&req, RuntimeOp::Start);
    submit_one(&state, req)
}

/// §63 `runtime.stop`：提交 Stop 任务（SIGTERM 优雅优先，幂等）。
#[command]
pub fn runtime_stop(
    workspace_id: i64,
    runtime_name: String,
    state: State<'_, AppState>,
) -> AppResult<String> {
    let req = state.runtime.operation_task_request(
        &RuntimeOperationRequest {
            workspace_id,
            runtime_name,
            options: Default::default(),
        },
        RuntimeOp::Stop,
    );
    submit_one(&state, req)
}

/// §63 `runtime.restart`：提交 Restart 任务（复用最近构建产物）。
#[command]
pub fn runtime_restart(
    req: RuntimeOperationRequest,
    state: State<'_, AppState>,
) -> AppResult<String> {
    let req = state.runtime.operation_task_request(&req, RuntimeOp::Restart);
    submit_one(&state, req)
}

/// §63 `runtime.list_processes`。
#[command]
pub fn runtime_list_processes(
    workspace_id: i64,
    state: State<'_, AppState>,
) -> AppResult<Vec<RuntimeProcessInfo>> {
    state.runtime.list_processes(workspace_id)
}

/// §63 `runtime.process_status`。
#[command]
pub fn runtime_process_status(
    process_id: i64,
    state: State<'_, AppState>,
) -> AppResult<Option<RuntimeProcessInfo>> {
    state.runtime.process_status(process_id)
}

/// §63 `runtime.get_logs`（R-11：跨滚动段搜索 / tail，`filter.limit` 分页）。
#[command]
pub fn runtime_get_logs(
    query: RuntimeLogQuery,
    state: State<'_, AppState>,
) -> AppResult<Vec<LogEntry>> {
    state.runtime.get_logs(&query)
}

/// §63 `runtime.clear_logs`。
#[command]
pub fn runtime_clear_logs(query: RuntimeLogQuery, state: State<'_, AppState>) -> AppResult<()> {
    state.runtime.clear_logs(&query)
}

/// R-13 `runtime.export_logs`：按当前过滤条件全量导出到 `dest_path`
/// （R-11 §36，与 `search` 同管道；`filter.limit` 被忽略）。
#[command]
pub fn runtime_export_logs(
    query: RuntimeLogQuery,
    dest_path: String,
    state: State<'_, AppState>,
) -> AppResult<LogExportOutcome> {
    state.runtime.export_logs(&query, &dest_path)
}

/// §63 `runtime.start_environment`：启动 workspace 下全部 Runtime 配置
/// （Phase 1 口径；依赖排序 / 并行编排由 R-15 引入）。返回任务 id 列表
/// （>1 个配置时共享 T-20 批量聚合行）。
#[command]
pub fn runtime_start_environment(
    workspace_id: i64,
    state: State<'_, AppState>,
) -> AppResult<Vec<String>> {
    let requests = state.runtime.start_environment_requests(workspace_id)?;
    state.task_manager.submit(&requests)
}

/// §63 `runtime.stop_environment`：停止 workspace 下全部活跃 Runtime 进程。
#[command]
pub fn runtime_stop_environment(
    workspace_id: i64,
    state: State<'_, AppState>,
) -> AppResult<Vec<String>> {
    let requests = state.runtime.stop_environment_requests(workspace_id)?;
    state.task_manager.submit(&requests)
}

/// §66 可配置：读取当前调度并发上限（Build / Dependency Resolve）。
#[command]
pub fn runtime_get_scheduler_config(state: State<'_, AppState>) -> AppResult<SchedulerConfig> {
    Ok(state.runtime.scheduler_config())
}

/// §66 可配置：调整调度并发上限（立即生效并持久化）。
#[command]
pub fn runtime_set_scheduler_config(
    config: SchedulerConfig,
    state: State<'_, AppState>,
) -> AppResult<()> {
    state.runtime.set_scheduler_config(&config)
}

// ---------------------------------------------------------------------------
// R-14 §75 Command Safety：Pre/Post Build Script 确认状态
// ---------------------------------------------------------------------------

/// 列出全部脚本确认记录（UI 管理列表；「不再询问」可重置）。
#[command]
pub fn runtime_get_script_approvals(
    state: State<'_, AppState>,
) -> AppResult<Vec<ScriptApproval>> {
    Ok(state.runtime.script_approval_list())
}

/// 确认一条脚本（pre / post）。后端从配置读脚本内容计算哈希与预览，
/// 与流水线校验口径一致；脚本内容变更后需重新确认。
#[command]
pub fn runtime_approve_script(
    workspace_id: i64,
    runtime_name: String,
    script_type: String,
    state: State<'_, AppState>,
) -> AppResult<ScriptApproval> {
    state
        .runtime
        .approve_script(workspace_id, &runtime_name, &script_type)
}

/// 按范围撤销脚本确认（workspace / runtime 可空 = 匹配任意）。
#[command]
pub fn runtime_reset_script_approvals(
    workspace_id: Option<i64>,
    runtime_name: Option<String>,
    state: State<'_, AppState>,
) -> AppResult<usize> {
    state
        .runtime
        .reset_script_approvals(workspace_id, runtime_name.as_deref())
}
