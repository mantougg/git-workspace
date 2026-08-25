import { invoke } from "@tauri-apps/api/core";
import type {
  CreateRuntimeConfigRequest,
  DependencyGraphView,
  LogEntry,
  ProjectInspection,
  RuntimeApplicationConfig,
  RuntimeConfigSummary,
  RuntimeLogQuery,
  RuntimeOperationRequest,
  RuntimeProcessInfo,
  SchedulerConfig,
  UpdateRuntimeConfigRequest,
} from "@/types/runtime";
import type { MavenProjectNode } from "@/types/maven";

/** §64 Runtime 事件名（Tauri event）。 */
export const RUNTIME_EVENTS = {
  projectDiscovered: "runtime.project_discovered",
  dependencyResolved: "runtime.dependency_resolved",
  buildStarted: "runtime.build_started",
  buildProgress: "runtime.build_progress",
  buildCompleted: "runtime.build_completed",
  processStarted: "runtime.process_started",
  processOutput: "runtime.process_output",
  processStopped: "runtime.process_stopped",
  processFailed: "runtime.process_failed",
  healthChanged: "runtime.health_changed",
  fileChanged: "runtime.file_changed",
  restartStarted: "runtime.restart_started",
  restartCompleted: "runtime.restart_completed",
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

export function runtimeRestart(req: RuntimeOperationRequest): Promise<string> {
  return invoke<string>("runtime_restart", { req });
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
