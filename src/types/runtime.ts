/** R-07 Runtime configuration IPC types. */

import type {
  DependencyEdge,
  MavenModuleLink,
  MavenProjectNode,
  SourceMapping,
} from "./maven";
import type { RuntimeTaskOptions } from "./task";

export interface RuntimeApplicationConfig {
  schemaVersion: number;
  name: string;
  project: string;
  mainClass: string | null;
  jdk: string | null;
  profile: string | null;
  vmOptions: string[];
  programArguments: string[];
  environment: Record<string, string>;
  runtimeEnvironment: Record<string, string>;
  buildEngine: string | null;
}

export interface RuntimeConfigSummary {
  id: number;
  workspaceId: number;
  name: string;
  project: string;
  mainClass: string | null;
  jdk: string | null;
  profile: string | null;
  buildEngine: string | null;
  configPath: string;
  createdAt: string;
  updatedAt: string;
}

export interface CreateRuntimeConfigRequest {
  workspaceId: number;
  config: RuntimeApplicationConfig;
}

export interface UpdateRuntimeConfigRequest {
  workspaceId: number;
  name: string;
  config: RuntimeApplicationConfig;
}

// ---------------------------------------------------------------------------
// R-12 Runtime 控制面（§63 命令 / §64 事件 / §66 调度）
// ---------------------------------------------------------------------------

/** R-09 Run Strategy（§30）。 */
export type RunStrategy = "mavenRun" | "packageRun" | "classpathRun";

/** Runtime 任务操作（R-12，§63/§65）。 */
export type RuntimeOp = "build" | "start" | "stop" | "restart" | "resolveDependencies";

/** R-10 生命周期状态机（§27）。 */
export type LifecycleStatus =
  | "created"
  | "preparing"
  | "resolving"
  | "building"
  | "starting"
  | "running"
  | "stopping"
  | "stopped"
  | "failed";

/** Start 流水线的 UI 阶段（§65）。 */
export type RuntimeStage = "preparing" | "resolving" | "building" | "starting";

/** `runtime.health_changed` 的健康取值。 */
export type HealthStatus = "up" | "down";

/** 日志级别（R-11，序数语义：越大越严重）。 */
export type LogLevel = "trace" | "debug" | "info" | "warn" | "error";

/** 日志阶段（R-11）：构建期 / 运行期。 */
export type LogPhase = "build" | "run";

/** 进程输出流。 */
export type OutputStream = "stdout" | "stderr";

/** 运行中进程的信息快照（§33）。 */
export interface RuntimeProcessInfo {
  processId: number;
  workspaceId: number;
  runtimeName: string;
  pid: number | null;
  status: LifecycleStatus;
  runStrategy: RunStrategy | null;
  /** 启动命令预览（§75 可追溯；不含环境变量）。 */
  commandPreview: string | null;
  workingDir: string | null;
  ports: number[];
  exitCode: number | null;
  /** 是否为 GitWorkspace 重启后接管的孤儿进程。 */
  adopted: boolean;
  startedAt: string;
  stoppedAt: string | null;
  uptimeSeconds: number | null;
  cpuPercent: number | null;
  memoryBytes: number | null;
}

/** `runtime.process_output` 事件批次元素（已脱敏）。 */
export interface LogLine {
  seq: number;
  at: string;
  phase: LogPhase;
  stream: OutputStream;
  level: LogLevel | null;
  line: string;
}

/** `runtime_get_logs` 返回行（跨滚动段全局行号）。 */
export interface LogEntry {
  lineNumber: number;
  level: LogLevel | null;
  text: string;
}

/** 日志查询过滤器。 */
export interface LogFilter {
  query?: string | null;
  minLevel?: LogLevel | null;
  limit?: number | null;
}

// §64 事件 payload（事件名 `runtime.<event>` 见 api/runtime.ts 的常量）

export interface ProjectDiscoveredPayload {
  workspaceId: number;
  /** 相对 workspace 根的 POM 所在目录。 */
  path: string;
  /** `groupId:artifactId:version`。 */
  coordinates: string;
  packaging: string;
  at: string;
}

export interface DependencyResolvedPayload {
  workspaceId: number;
  projects: number;
  dependencies: number;
  sourceMappings: number;
  inserted: number;
  updated: number;
  removed: number;
  elapsedMs: number;
  at: string;
}

export interface BuildStartedPayload {
  workspaceId: number;
  runtimeName: string;
  op: RuntimeOp;
  at: string;
}

export interface BuildProgressPayload {
  workspaceId: number;
  runtimeName: string;
  processId: number | null;
  stage: RuntimeStage;
  at: string;
}

export interface BuildCompletedPayload {
  workspaceId: number;
  runtimeName: string;
  processId: number | null;
  success: boolean;
  durationMs: number | null;
  error: string | null;
  at: string;
}

export interface ProcessStartedPayload {
  workspaceId: number;
  processId: number;
  runtimeName: string;
  at: string;
}

export interface ProcessOutputPayload {
  processId: number;
  runtimeName: string;
  lines: LogLine[];
}

export interface ProcessStoppedPayload {
  workspaceId: number;
  processId: number;
  runtimeName: string;
  exitCode: number | null;
  at: string;
}

export interface ProcessFailedPayload {
  workspaceId: number;
  processId: number;
  runtimeName: string;
  exitCode: number | null;
  at: string;
}

export interface HealthChangedPayload {
  workspaceId: number;
  processId: number;
  runtimeName: string;
  health: HealthStatus;
  at: string;
}

export interface FileChangedPayload {
  workspaceId: number;
  paths: string[];
  at: string;
}

export interface RestartStartedPayload {
  workspaceId: number;
  runtimeName: string;
  at: string;
}

export interface RestartCompletedPayload {
  workspaceId: number;
  runtimeName: string;
  success: boolean;
  error: string | null;
  at: string;
}

// §63 请求 / 视图类型

/** `runtime_build` / `runtime_start` / `runtime_restart` 请求。 */
export interface RuntimeOperationRequest {
  workspaceId: number;
  runtimeName: string;
  options?: RuntimeTaskOptions;
}

/** `runtime_get_logs` / `runtime_clear_logs` 请求。 */
export interface RuntimeLogQuery {
  workspaceId: number;
  runtimeName: string;
  processId: number;
  filter?: LogFilter;
}

/** `runtime_inspect_project` 返回（DB 索引视角）。 */
export interface ProjectInspection {
  project: MavenProjectNode;
  modules: MavenModuleLink[];
  parentProjectId: number | null;
  dependencies: DependencyEdge[];
  sourceMappings: SourceMapping[];
}

/** `runtime_get_dependency_graph` 返回；`truncated` 标记依赖边截断。 */
export interface DependencyGraphView {
  workspaceId: number;
  fingerprint: string;
  projects: MavenProjectNode[];
  modules: MavenModuleLink[];
  dependencies: DependencyEdge[];
  sourceMappings: SourceMapping[];
  totalDependencies: number;
  truncated: boolean;
}

/** §66 调度并发上限（`runtime-scheduler.json` 持久化）。 */
export interface SchedulerConfig {
  maxConcurrentBuilds: number;
  maxConcurrentResolves: number;
}
