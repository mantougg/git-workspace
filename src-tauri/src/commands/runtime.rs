//! Runtime configuration IPC (R-07) + Runtime 控制面 IPC（R-12，§63）。

use std::collections::BTreeMap;
use std::sync::MutexGuard;

use rusqlite::Connection;
use tauri::{command, State};

use crate::error::{AppError, AppResult};
use crate::maven::{MavenProjectNode, RuntimeScope};
use crate::models::task::RuntimeOp;
use crate::runtime::logs::LogEntry;
use crate::runtime::{
    create_config, delete_config, get_config, get_workspace_environment, list_configs, resolve_environment,
    set_workspace_environment, update_config, ClosurePreview, CreateRuntimeConfigRequest, DependencyGraphView,
    LogExportOutcome, ProjectInspection, RuntimeApplicationConfig, RuntimeConfigSummary, RuntimeLogQuery,
    RuntimeOperationRequest, RuntimeProcessInfo, RuntimeRunningBrief, SchedulerConfig, ScriptApproval,
    UpdateRuntimeConfigRequest,
};
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
pub fn delete_runtime_config(workspace_id: i64, name: String, state: State<'_, AppState>) -> AppResult<()> {
    let conn = lock_db(&state)?;
    delete_config(&conn, workspace_id, &name)
}

#[command]
pub fn list_runtime_configs(workspace_id: i64, state: State<'_, AppState>) -> AppResult<Vec<RuntimeConfigSummary>> {
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
pub fn runtime_list_projects(workspace_id: i64, state: State<'_, AppState>) -> AppResult<Vec<MavenProjectNode>> {
    state.runtime.list_projects(workspace_id)
}

/// N-09 统一项目视图：Maven 与 Node 项目合并列表（§4.8 开放问题的用户决策）。
/// node 侧与 `node_list_projects` 同源（发现 + 索引同步）；maven 侧与
/// `runtime_list_projects` 同源（DB 索引热路径）。各自的专属字段打包在
/// `node` / `maven` payload 里，公共字段平铺。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnifiedProjectNode {
    /// `"maven"` 或 `"node"`。
    pub source: String,
    pub project_id: i64,
    pub repository_id: Option<i64>,
    pub path: String,
    pub name: String,
    pub version: String,
    /// node 独有（maven 项目为 `None`）。
    pub node: Option<UnifiedNodeProjectPayload>,
    /// maven 独有（node 项目为 `None`）。
    pub maven: Option<UnifiedMavenProjectPayload>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnifiedNodeProjectPayload {
    pub package_manager: Option<String>,
    pub scripts_json: String,
    pub workspace_root: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnifiedMavenProjectPayload {
    pub coordinates: crate::maven::model::PomCoordinates,
    pub packaging: String,
}

/// N-09 `runtime.list_unified_projects`：Maven/Node 合并项目列表。
///
/// 修复（应用无响应）：本命令此前在持有 `state.db` 锁的闭包内调用
/// `state.runtime.list_projects()`，后者内部再次 `self.db.lock()`——
/// 同一个不可重入 Mutex 二次加锁 = 自死锁，切换到「前端工程」触发本
/// 命令后整个应用永久无响应。Maven 列表改为直接用已持有的 conn 走
/// `query_dependency_graph`，全程单次持锁；文件系统扫描（第二阶段）
/// 保持在锁外，扫描期间不阻塞其他 IPC。
#[command]
pub fn runtime_list_unified_projects(
    workspace_id: i64,
    state: State<'_, AppState>,
) -> AppResult<Vec<UnifiedProjectNode>> {
    // Phase 1: 读 Maven 列表 + workspace 配置（DB 锁内，快照）。
    // 注意：这里不能调 state.runtime.list_projects()——RuntimeService
    // 内部会再锁 self.db，与已持有的 state.db 锁形成自死锁。
    let (mut unified, node_root, scan_depth) = {
        let conn = state
            .db
            .lock()
            .map_err(|error| AppError::Other(format!("DB lock error: {error}")))?;
        let mut unified = Vec::new();
        // 只在取 Vec<MavenProjectNode> 后 unwrap_or_default（DependencyGraph
        // 本身无 Default，不能对其整体 unwrap_or_default）。
        let maven_projects = crate::maven::query_dependency_graph(&conn, workspace_id)
            .map(|graph| graph.projects)
            .unwrap_or_default();
        for project in maven_projects {
            unified.push(UnifiedProjectNode {
                source: "maven".into(),
                project_id: project.project_id,
                repository_id: project.repository_id,
                path: project.path.to_string_lossy().into_owned(),
                name: project.coordinates.artifact_id.clone(),
                version: project.coordinates.version.clone(),
                node: None,
                maven: Some(UnifiedMavenProjectPayload {
                    coordinates: project.coordinates,
                    packaging: project.packaging,
                }),
            });
        }
        let root = crate::runtime::config::workspace_root(&conn, workspace_id)?;
        let depth: i64 = conn.query_row(
            "SELECT scan_depth FROM workspaces WHERE id = ?1",
            [workspace_id],
            |row| row.get(0),
        )?;
        (unified, root, depth.max(1) as usize)
        // conn 被 drop，DB 锁释放——其他 IPC 可正常访问 DB。
    };

    // Phase 2: 文件系统扫描（DB 锁外，不阻塞其他 IPC）。
    let discovery = crate::node::discovery::discover_package_jsons(
        &node_root,
        scan_depth,
        Some(crate::node::discovery::global_package_cache()),
        None,
    );

    // Phase 3: 写入 node_projects 索引（短暂获取 DB 锁）。
    let node_projects = {
        let mut conn = state
            .db
            .lock()
            .map_err(|error| AppError::Other(format!("DB lock error: {error}")))?;
        crate::node::discovery::sync_node_projects(&mut conn, workspace_id, &discovery).unwrap_or_default()
    };
    for project in node_projects {
        unified.push(UnifiedProjectNode {
            source: "node".into(),
            project_id: project.project_id,
            repository_id: project.repository_id,
            path: project.path.to_string_lossy().into_owned(),
            name: project.name,
            version: project.version,
            node: Some(UnifiedNodeProjectPayload {
                package_manager: project.package_manager,
                scripts_json: project.scripts_json,
                workspace_root: project.workspace_root,
            }),
            maven: None,
        });
    }
    Ok(unified)
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
pub fn runtime_resolve_dependencies(workspace_id: i64, state: State<'_, AppState>) -> AppResult<String> {
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
    state.runtime.dependency_graph(workspace_id, project_id, max_edges)
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
pub fn runtime_build(req: RuntimeOperationRequest, state: State<'_, AppState>) -> AppResult<String> {
    let req = state.runtime.operation_task_request(&req, RuntimeOp::Build);
    submit_one(&state, req)
}

/// §63 `runtime.start`：提交 Start 任务（Validate JDK/Maven → Resolve →
/// Reactor → Build → Start 子任务进度经 §64 事件流出）。
#[command]
pub fn runtime_start(req: RuntimeOperationRequest, state: State<'_, AppState>) -> AppResult<String> {
    let req = state.runtime.operation_task_request(&req, RuntimeOp::Start);
    submit_one(&state, req)
}

/// §63 `runtime.stop`：提交 Stop 任务（SIGTERM 优雅优先，幂等）。
#[command]
pub fn runtime_stop(workspace_id: i64, runtime_name: String, state: State<'_, AppState>) -> AppResult<String> {
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
pub fn runtime_restart(req: RuntimeOperationRequest, state: State<'_, AppState>) -> AppResult<String> {
    let req = state.runtime.operation_task_request(&req, RuntimeOp::Restart);
    submit_one(&state, req)
}

/// R-17/R-21：Stop → 完整构建 → Start（源码/POM 变化后的重建重启入口，
/// 区别于 `runtime_restart` 的 skip-build 复用）。
#[command]
pub fn runtime_rebuild_restart(req: RuntimeOperationRequest, state: State<'_, AppState>) -> AppResult<String> {
    let req = state.runtime.operation_task_request(&req, RuntimeOp::RebuildRestart);
    submit_one(&state, req)
}

/// §63 `runtime.list_processes`。
#[command]
pub fn runtime_list_processes(workspace_id: i64, state: State<'_, AppState>) -> AppResult<Vec<RuntimeProcessInfo>> {
    state.runtime.list_processes(workspace_id)
}

/// §63 `runtime.process_status`。
#[command]
pub fn runtime_process_status(process_id: i64, state: State<'_, AppState>) -> AppResult<Option<RuntimeProcessInfo>> {
    state.runtime.process_status(process_id)
}

/// §63 `runtime.get_logs`（R-11：跨滚动段搜索 / tail，`filter.limit` 分页）。
#[command]
pub fn runtime_get_logs(query: RuntimeLogQuery, state: State<'_, AppState>) -> AppResult<Vec<LogEntry>> {
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
pub fn runtime_start_environment(workspace_id: i64, state: State<'_, AppState>) -> AppResult<Vec<String>> {
    let requests = state.runtime.start_environment_requests(workspace_id)?;
    state.task_manager.submit(&requests)
}

/// §63 `runtime.stop_environment`：停止 workspace 下全部活跃 Runtime 进程。
#[command]
pub fn runtime_stop_environment(workspace_id: i64, state: State<'_, AppState>) -> AppResult<Vec<String>> {
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
pub fn runtime_set_scheduler_config(config: SchedulerConfig, state: State<'_, AppState>) -> AppResult<()> {
    state.runtime.set_scheduler_config(&config)
}

// ---------------------------------------------------------------------------
// R-16 §41/§81：健康检查查询 + 端口管理
// ---------------------------------------------------------------------------

/// R-16 `runtime.get_health`：单进程健康快照（无探针 / 未启动为 null）。
#[command]
pub fn runtime_get_health(
    process_id: i64,
    state: State<'_, AppState>,
) -> AppResult<Option<crate::runtime::health::HealthSnapshot>> {
    Ok(state.runtime.get_health(process_id))
}

/// R-16 `runtime.list_health`：workspace 下全部探针快照（Dashboard 汇总）。
#[command]
pub fn runtime_list_health(
    workspace_id: i64,
    state: State<'_, AppState>,
) -> AppResult<Vec<crate::runtime::health::HealthSnapshot>> {
    Ok(state.runtime.list_health(workspace_id))
}

/// R-16 §81 `runtime.check_port`：端口占用检测（bind 实测 + 占用方识别）。
#[command]
pub fn runtime_check_port(port: u16) -> AppResult<crate::runtime::port_manager::PortCheckResult> {
    Ok(crate::runtime::port_manager::check_port(port))
}

/// R-16 §81 `runtime.kill_port_process`：终止占用端口的进程。**危险操作**：
/// 必须带 `confirmed=true`（全局约束 §3 二次确认），确认文案需明确进程身份。
#[command]
pub fn runtime_kill_port_process(
    pid: u32,
    confirmed: bool,
) -> AppResult<crate::runtime::port_manager::PortKillOutcome> {
    if !confirmed {
        return Err(AppError::Permission(format!(
            "终止 PID {pid} 会直接结束该进程（TERM 优雅优先，3s 未退出升级 KILL）。\
             请确认这是你了解的进程后，带 confirmed=true 重试"
        )));
    }
    Ok(crate::runtime::port_manager::kill_external_process(pid))
}

/// R-16 §81 `runtime.change_runtime_port`：改写 Runtime 配置的端口
/// （`program_arguments` 注入 `--server.port=`；只改 GitWorkspace 配置，
/// 不触碰用户项目文件）。返回更新后的配置（秘密已掩码）。
#[command]
pub fn runtime_change_runtime_port(
    workspace_id: i64,
    name: String,
    port: u16,
    state: State<'_, AppState>,
) -> AppResult<RuntimeApplicationConfig> {
    let conn = lock_db(&state)?;
    let mut config = crate::runtime::config::get_config(&conn, workspace_id, &name)?;
    // 端口注入三处形态统一收敛到 --server.port=<port>（Spring Boot 标准形）。
    config
        .program_arguments
        .retain(|arg| !arg.starts_with("--server.port=") && !arg.starts_with("--server.port "));
    config.vm_options.retain(|arg| !arg.starts_with("-Dserver.port="));
    config.program_arguments.push(format!("--server.port={port}"));
    crate::runtime::config::update_config(
        &conn,
        &UpdateRuntimeConfigRequest {
            workspace_id,
            name: name.clone(),
            config,
        },
    )
}

// ---------------------------------------------------------------------------
// R-15 §38/§39/§40：Multi-Service Runtime Environment
// ---------------------------------------------------------------------------

/// R-15 `runtime.list_environments`：workspace 全部环境（读目录）。
#[command]
pub fn runtime_list_environments(
    workspace_id: i64,
    state: State<'_, AppState>,
) -> AppResult<Vec<crate::runtime::environment::RuntimeEnvironment>> {
    let conn = lock_db(&state)?;
    let root = crate::runtime::config::workspace_root(&conn, workspace_id)?;
    Ok(crate::runtime::environment::list_environments(&root))
}

/// R-15 `runtime.save_environment`：创建 / 覆盖环境（校验依赖图 + Runtime
/// 配置存在性后原子写盘，可 Git 版本化共享）。
#[command]
pub fn runtime_save_environment(
    workspace_id: i64,
    environment: crate::runtime::environment::RuntimeEnvironment,
    state: State<'_, AppState>,
) -> AppResult<crate::runtime::environment::RuntimeEnvironment> {
    let conn = lock_db(&state)?;
    let root = crate::runtime::config::workspace_root(&conn, workspace_id)?;
    crate::runtime::environment::validate_environment_configs(&conn, workspace_id, &environment)?;
    crate::runtime::environment::save_environment(&root, &environment)
}

/// R-15 `runtime.delete_environment`。
#[command]
pub fn runtime_delete_environment(workspace_id: i64, name: String, state: State<'_, AppState>) -> AppResult<()> {
    let conn = lock_db(&state)?;
    let root = crate::runtime::config::workspace_root(&conn, workspace_id)?;
    crate::runtime::environment::delete_environment(&root, &name)
}

/// R-15 §38 `runtime.start_named_environment`：提交 Start Environment 任务
/// （拓扑分波编排；波内并行受构建 permit 池约束）。
#[command]
pub fn runtime_start_named_environment(
    workspace_id: i64,
    environment: String,
    state: State<'_, AppState>,
) -> AppResult<String> {
    let req = state
        .runtime
        .named_environment_task_request(workspace_id, &environment, RuntimeOp::StartEnvironment);
    submit_one(&state, req)
}

/// R-15 §38 `runtime.stop_named_environment`：提交 Stop Environment 任务
/// （逆拓扑序停止）。
#[command]
pub fn runtime_stop_named_environment(
    workspace_id: i64,
    environment: String,
    state: State<'_, AppState>,
) -> AppResult<String> {
    let req = state
        .runtime
        .named_environment_task_request(workspace_id, &environment, RuntimeOp::StopEnvironment);
    submit_one(&state, req)
}

// ---------------------------------------------------------------------------
// R-19 §83：Runtime Templates
// ---------------------------------------------------------------------------

/// R-19 `runtime.list_templates`：用户模板 + 未被遮蔽的内置模板。
#[command]
pub fn runtime_list_templates(
    workspace_id: i64,
    state: State<'_, AppState>,
) -> AppResult<Vec<crate::runtime::templates::RuntimeTemplate>> {
    let conn = lock_db(&state)?;
    let root = crate::runtime::config::workspace_root(&conn, workspace_id)?;
    Ok(crate::runtime::templates::list_templates(&root))
}

/// R-19 `runtime.save_template`：创建 / 覆盖用户模板（`builtin` 标记被
/// 忽略——内置模板只由代码提供；用户同名文件自动遮蔽内置）。
#[command]
pub fn runtime_save_template(
    workspace_id: i64,
    template: crate::runtime::templates::RuntimeTemplate,
    state: State<'_, AppState>,
) -> AppResult<crate::runtime::templates::RuntimeTemplate> {
    let conn = lock_db(&state)?;
    let root = crate::runtime::config::workspace_root(&conn, workspace_id)?;
    crate::runtime::templates::save_template(&root, &template)
}

/// R-19 `runtime.delete_template`：删除用户模板（内置模板拒绝）。
#[command]
pub fn runtime_delete_template(workspace_id: i64, name: String, state: State<'_, AppState>) -> AppResult<()> {
    let conn = lock_db(&state)?;
    let root = crate::runtime::config::workspace_root(&conn, workspace_id)?;
    crate::runtime::templates::delete_template(&root, &name)
}

/// R-19「另存为模板」：从现有 Runtime 配置生成模板（身份字段剥离）。
#[command]
pub fn runtime_save_config_as_template(
    workspace_id: i64,
    config_name: String,
    template_name: String,
    description: Option<String>,
    state: State<'_, AppState>,
) -> AppResult<crate::runtime::templates::RuntimeTemplate> {
    let conn = lock_db(&state)?;
    let root = crate::runtime::config::workspace_root(&conn, workspace_id)?;
    let config = crate::runtime::config::load_config_unredacted(&conn, workspace_id, &config_name)?;
    crate::runtime::templates::save_config_as_template(&root, config, &template_name, description)
}

/// R-19 `runtime.apply_template`：从模板创建 Runtime 配置。前端负责把
/// 模板载荷预填进向导表单（`get_runtime_config` 同构字段），本命令校验
/// 模板存在性并经 R-07 `create_config` 全量校验落盘。返回创建后的配置
/// （秘密已掩码）。
#[command]
pub fn runtime_apply_template(
    workspace_id: i64,
    template_name: String,
    config: RuntimeApplicationConfig,
    state: State<'_, AppState>,
) -> AppResult<RuntimeApplicationConfig> {
    let conn = lock_db(&state)?;
    let root = crate::runtime::config::workspace_root(&conn, workspace_id)?;
    // 模板必须存在（含内置），防止拼错名字静默落盘。
    let _template = crate::runtime::templates::get_template(&root, &template_name)?;
    if config.name.trim().is_empty() || config.project.trim().is_empty() {
        return Err(AppError::RuntimeConfig(
            "应用模板前请填写应用名称并选择 Maven 项目".into(),
        ));
    }
    crate::runtime::config::create_config(&conn, &CreateRuntimeConfigRequest { workspace_id, config })
}

// ---------------------------------------------------------------------------
// R-14 §75 Command Safety：Pre/Post Build Script 确认状态
// ---------------------------------------------------------------------------

/// 列出全部脚本确认记录（UI 管理列表；「不再询问」可重置）。
#[command]
pub fn runtime_get_script_approvals(state: State<'_, AppState>) -> AppResult<Vec<ScriptApproval>> {
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
    state.runtime.approve_script(workspace_id, &runtime_name, &script_type)
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

/// R-21 §49：全部工作区「运行中应用」摘要（轻量 DB 读，供 Checkout 前
/// 的保护确认；不做任何 git 操作、不拖慢正常流程）。
#[command]
pub fn runtime_running_briefs(state: State<'_, AppState>) -> Vec<RuntimeRunningBrief> {
    state.runtime.running_briefs()
}

/// R-21 §49 Stop & Switch：同步优雅停止指定 Runtime（默认宽限后升级
/// 整树终止），供保护确认后、切换分支前调用。
#[command]
pub fn runtime_stop_blocking(
    workspace_id: i64,
    runtime_name: String,
    state: State<'_, AppState>,
) -> AppResult<Option<RuntimeProcessInfo>> {
    state.runtime.stop_blocking(workspace_id, &runtime_name)
}
