import { invoke } from "@tauri-apps/api/core";
import type {
  ClosurePreview,
  CreateRuntimeConfigRequest,
  DependencyGraphView,
  HealthSnapshot,
  LogEntry,
  LogExportOutcome,
  PortCheckResult,
  PortKillOutcome,
  ProjectInspection,
  RuntimeApplicationConfig,
  RuntimeConfigSummary,
  RuntimeEnvironment,
  RuntimeLogQuery,
  RuntimeOperationRequest,
  RuntimeProcessInfo,
  RuntimeRunningBrief,
  RuntimeTemplate,
  SchedulerConfig,
  ScriptApproval,
  UpdateRuntimeConfigRequest,
} from "@/types/runtime";
import type { MavenProjectNode, RuntimeScope } from "@/types/maven";

/** §64 Runtime 事件名（Tauri event）。F-15：Tauri listen 校验只允许
    字母数字/`-`/`/`/`:`/`_`——不能用 `.`，带点会被拒绝并阻断订阅链。 */
export const RUNTIME_EVENTS = {
  projectDiscovered: "runtime_project_discovered",
  dependencyResolved: "runtime_dependency_resolved",
  buildStarted: "runtime_build_started",
  buildProgress: "runtime_build_progress",
  buildCompleted: "runtime_build_completed",
  processStarted: "runtime_process_started",
  processOutput: "runtime_process_output",
  processStopped: "runtime_process_stopped",
  processFailed: "runtime_process_failed",
  healthChanged: "runtime_health_changed",
  fileChanged: "runtime_file_changed",
  restartStarted: "runtime_restart_started",
  restartCompleted: "runtime_restart_completed",
  environmentProgress: "runtime_environment_progress",
  environmentCompleted: "runtime_environment_completed",
  dependencyChanged: "runtime_dependency_changed",
} as const;

export function createRuntimeConfig(
  req: CreateRuntimeConfigRequest,
): Promise<RuntimeApplicationConfig> {
  return invoke<RuntimeApplicationConfig>("create_runtime_config", { req });
}

export function updateRuntimeConfig(
  req: UpdateRuntimeConfigRequest,
): Promise<RuntimeApplicationConfig> {
  return invoke<RuntimeApplicationConfig>("update_runtime_config", { req });
}

export function deleteRuntimeConfig(
  workspaceId: number,
  name: string,
): Promise<void> {
  return invoke<void>("delete_runtime_config", { workspaceId, name });
}

/** Metadata-only list; the backend intentionally does not open JSON files here. */
export function listRuntimeConfigs(
  workspaceId: number,
): Promise<RuntimeConfigSummary[]> {
  return invoke<RuntimeConfigSummary[]>("list_runtime_configs", { workspaceId });
}

export function getRuntimeConfig(
  workspaceId: number,
  name: string,
): Promise<RuntimeApplicationConfig> {
  return invoke<RuntimeApplicationConfig>("get_runtime_config", {
    workspaceId,
    name,
  });
}

export function resolveRuntimeEnvironment(
  workspaceId: number,
  name: string,
): Promise<Record<string, string>> {
  return invoke<Record<string, string>>("resolve_runtime_environment", {
    workspaceId,
    name,
  });
}

export function getWorkspaceRuntimeEnvironment(
  workspaceId: number,
): Promise<Record<string, string>> {
  return invoke<Record<string, string>>("get_workspace_runtime_environment", {
    workspaceId,
  });
}

export function setWorkspaceRuntimeEnvironment(
  workspaceId: number,
  environment: Record<string, string>,
): Promise<void> {
  return invoke<void>("set_workspace_runtime_environment", {
    workspaceId,
    environment,
  });
}

// ---------------------------------------------------------------------------
// R-12 §63：Runtime 控制面
// 长操作返回任务 id（T-05 任务队列，可取消；进度经 §64 事件流出）。
// ---------------------------------------------------------------------------

export function runtimeListProjects(
  workspaceId: number,
): Promise<MavenProjectNode[]> {
  return invoke<MavenProjectNode[]>("runtime_list_projects", { workspaceId });
}

export function runtimeInspectProject(
  workspaceId: number,
  project: string,
): Promise<ProjectInspection> {
  return invoke<ProjectInspection>("runtime_inspect_project", {
    workspaceId,
    project,
  });
}

export function runtimeResolveDependencies(workspaceId: number): Promise<string> {
  return invoke<string>("runtime_resolve_dependencies", { workspaceId });
}

export function runtimeGetDependencyGraph(
  workspaceId: number,
  projectId?: number,
  maxEdges?: number,
): Promise<DependencyGraphView> {
  return invoke<DependencyGraphView>("runtime_get_dependency_graph", {
    workspaceId,
    projectId: projectId ?? null,
    maxEdges: maxEdges ?? null,
  });
}

/** R-13：按给定 Scope 计算闭包预览（R-03 fingerprint 缓存热路径）。 */
export function runtimeGetClosure(
  workspaceId: number,
  project: string,
  scope: RuntimeScope,
): Promise<ClosurePreview> {
  return invoke<ClosurePreview>("runtime_get_closure", {
    workspaceId,
    project,
    scope,
  });
}

export function runtimeBuild(req: RuntimeOperationRequest): Promise<string> {
  return invoke<string>("runtime_build", { req });
}

export function runtimeStart(req: RuntimeOperationRequest): Promise<string> {
  return invoke<string>("runtime_start", { req });
}

export function runtimeStop(
  workspaceId: number,
  runtimeName: string,
): Promise<string> {
  return invoke<string>("runtime_stop", { workspaceId, runtimeName });
}

/** R-21 §49：全部工作区「运行中应用」摘要（Checkout 保护确认查询）。 */
export function runtimeRunningBriefs(): Promise<RuntimeRunningBrief[]> {
  return invoke<RuntimeRunningBrief[]>("runtime_running_briefs");
}

/** R-21 §49 Stop & Switch：同步优雅停止指定 Runtime。 */
export function runtimeStopBlocking(
  workspaceId: number,
  runtimeName: string,
): Promise<RuntimeProcessInfo | null> {
  return invoke<RuntimeProcessInfo | null>("runtime_stop_blocking", {
    workspaceId,
    runtimeName,
  });
}

export function runtimeRestart(req: RuntimeOperationRequest): Promise<string> {
  return invoke<string>("runtime_restart", { req });
}

/** R-17/R-21：Stop → 完整构建 → Start（区别于 restart 的 skip-build 复用）。 */
export function runtimeRebuildRestart(req: RuntimeOperationRequest): Promise<string> {
  return invoke<string>("runtime_rebuild_restart", { req });
}

export function runtimeListProcesses(
  workspaceId: number,
): Promise<RuntimeProcessInfo[]> {
  return invoke<RuntimeProcessInfo[]>("runtime_list_processes", { workspaceId });
}

export function runtimeProcessStatus(
  processId: number,
): Promise<RuntimeProcessInfo | null> {
  return invoke<RuntimeProcessInfo | null>("runtime_process_status", { processId });
}

export function runtimeGetLogs(query: RuntimeLogQuery): Promise<LogEntry[]> {
  return invoke<LogEntry[]>("runtime_get_logs", { query });
}

export function runtimeClearLogs(query: RuntimeLogQuery): Promise<void> {
  return invoke<void>("runtime_clear_logs", { query });
}

/** R-13：按过滤条件全量导出日志到目标文件（R-11 §36 同管道）。 */
export function runtimeExportLogs(
  query: RuntimeLogQuery,
  destPath: string,
): Promise<LogExportOutcome> {
  return invoke<LogExportOutcome>("runtime_export_logs", {
    query,
    destPath,
  });
}

export function runtimeStartEnvironment(workspaceId: number): Promise<string[]> {
  return invoke<string[]>("runtime_start_environment", { workspaceId });
}

export function runtimeStopEnvironment(workspaceId: number): Promise<string[]> {
  return invoke<string[]>("runtime_stop_environment", { workspaceId });
}

export function runtimeGetSchedulerConfig(): Promise<SchedulerConfig> {
  return invoke<SchedulerConfig>("runtime_get_scheduler_config");
}

export function runtimeSetSchedulerConfig(config: SchedulerConfig): Promise<void> {
  return invoke<void>("runtime_set_scheduler_config", { config });
}

// ---------------------------------------------------------------------------
// R-19 §83：Runtime Templates
// ---------------------------------------------------------------------------

/** 用户模板 + 未被遮蔽的内置模板。 */
export function runtimeListTemplates(
  workspaceId: number,
): Promise<RuntimeTemplate[]> {
  return invoke<RuntimeTemplate[]>("runtime_list_templates", { workspaceId });
}

/** 创建 / 覆盖用户模板（builtin 标记被忽略；同名用户文件遮蔽内置）。 */
export function runtimeSaveTemplate(
  workspaceId: number,
  template: RuntimeTemplate,
): Promise<RuntimeTemplate> {
  return invoke<RuntimeTemplate>("runtime_save_template", {
    workspaceId,
    template,
  });
}

export function runtimeDeleteTemplate(
  workspaceId: number,
  name: string,
): Promise<void> {
  return invoke<void>("runtime_delete_template", { workspaceId, name });
}

/** 另存为模板：从现有 Runtime 配置生成模板（身份字段剥离）。 */
export function runtimeSaveConfigAsTemplate(
  workspaceId: number,
  configName: string,
  templateName: string,
  description?: string | null,
): Promise<RuntimeTemplate> {
  return invoke<RuntimeTemplate>("runtime_save_config_as_template", {
    workspaceId,
    configName,
    templateName,
    description: description ?? null,
  });
}

/** 从模板创建 Runtime 配置（后端校验模板存在性 + R-07 全量校验）。 */
export function runtimeApplyTemplate(
  workspaceId: number,
  templateName: string,
  config: RuntimeApplicationConfig,
): Promise<RuntimeApplicationConfig> {
  return invoke<RuntimeApplicationConfig>("runtime_apply_template", {
    workspaceId,
    templateName,
    config,
  });
}

// ---------------------------------------------------------------------------
// R-14 §75 Command Safety：Pre/Post Build Script 确认状态
// ---------------------------------------------------------------------------

export function runtimeGetScriptApprovals(): Promise<ScriptApproval[]> {
  return invoke<ScriptApproval[]>("runtime_get_script_approvals");
}

/** 确认一条脚本（后端从配置读内容计算哈希，与流水线校验口径一致）。 */
export function runtimeApproveScript(
  workspaceId: number,
  runtimeName: string,
  scriptType: string,
): Promise<ScriptApproval> {
  return invoke<ScriptApproval>("runtime_approve_script", {
    workspaceId,
    runtimeName,
    scriptType,
  });
}

/** 按范围撤销确认（workspace / runtime 可空 = 匹配任意；返回删除条数）。 */
export function runtimeResetScriptApprovals(
  workspaceId?: number | null,
  runtimeName?: string | null,
): Promise<number> {
  return invoke<number>("runtime_reset_script_approvals", {
    workspaceId: workspaceId ?? null,
    runtimeName: runtimeName ?? null,
  });
}

// ---------------------------------------------------------------------------
// R-16 §41/§81：健康检查 + 端口管理
// ---------------------------------------------------------------------------

/** 单进程健康快照（无探针 / 未启动为 null）。 */
export function runtimeGetHealth(
  processId: number,
): Promise<HealthSnapshot | null> {
  return invoke<HealthSnapshot | null>("runtime_get_health", { processId });
}

/** workspace 下全部探针快照（Dashboard 汇总）。 */
export function runtimeListHealth(
  workspaceId: number,
): Promise<HealthSnapshot[]> {
  return invoke<HealthSnapshot[]>("runtime_list_health", { workspaceId });
}

/** 端口占用检测（bind 实测 + 占用方识别）。 */
export function runtimeCheckPort(port: number): Promise<PortCheckResult> {
  return invoke<PortCheckResult>("runtime_check_port", { port });
}

/** 终止占用端口的进程（危险操作，UI 必须二次确认后传 confirmed=true）。 */
export function runtimeKillPortProcess(
  pid: number,
  confirmed: boolean,
): Promise<PortKillOutcome> {
  return invoke<PortKillOutcome>("runtime_kill_port_process", {
    pid,
    confirmed,
  });
}

/** 改写 Runtime 配置端口（注入 --server.port=；只改 GitWorkspace 配置）。 */
export function runtimeChangeRuntimePort(
  workspaceId: number,
  name: string,
  port: number,
): Promise<RuntimeApplicationConfig> {
  return invoke<RuntimeApplicationConfig>("runtime_change_runtime_port", {
    workspaceId,
    name,
    port,
  });
}

// ---------------------------------------------------------------------------
// R-15 §38/§39/§40：Multi-Service Runtime Environment
// ---------------------------------------------------------------------------

export function runtimeListEnvironments(
  workspaceId: number,
): Promise<RuntimeEnvironment[]> {
  return invoke<RuntimeEnvironment[]>("runtime_list_environments", {
    workspaceId,
  });
}

/** 创建 / 覆盖环境（后端校验依赖图 + Runtime 配置存在性）。 */
export function runtimeSaveEnvironment(
  workspaceId: number,
  environment: RuntimeEnvironment,
): Promise<RuntimeEnvironment> {
  return invoke<RuntimeEnvironment>("runtime_save_environment", {
    workspaceId,
    environment,
  });
}

export function runtimeDeleteEnvironment(
  workspaceId: number,
  name: string,
): Promise<void> {
  return invoke<void>("runtime_delete_environment", { workspaceId, name });
}

/** 提交 Start Environment 任务（拓扑分波编排，返回任务 id）。 */
export function runtimeStartNamedEnvironment(
  workspaceId: number,
  environment: string,
): Promise<string> {
  return invoke<string>("runtime_start_named_environment", {
    workspaceId,
    environment,
  });
}

/** 提交 Stop Environment 任务（逆拓扑序停止）。 */
export function runtimeStopNamedEnvironment(
  workspaceId: number,
  environment: string,
): Promise<string> {
  return invoke<string>("runtime_stop_named_environment", {
    workspaceId,
    environment,
  });
}
