/** R-07 Runtime configuration IPC types. */

import type {
  DependencyEdge,
  MavenModuleLink,
  MavenProjectNode,
  MavenCoordinates,
  RuntimeClosure,
  RuntimeScope,
  SourceMapping,
} from "./maven";
import type { RuntimeTaskOptions } from "./task";

export type RuntimeKind = "springBoot" | "node";

/** N-09 统一项目视图：node 项目专属字段。 */
export interface UnifiedNodeProjectPayload {
  packageManager: string | null;
  /** JSON object text preserving package.json script order. */
  scriptsJson: string;
  /** workspace 根目录；独立工程为 null。 */
  workspaceRoot: string | null
}

/** N-09 统一项目视图：maven 项目专属字段。 */
export interface UnifiedMavenProjectPayload {
  coordinates: MavenCoordinates;
  packaging: string;
}

/** N-09 统一项目视图：Maven/Node 合并列表项（source 区分）。 */
export interface UnifiedProjectNode {
  source: string;
  projectId: number;
  repositoryId: number | null
  path: string;
  name: string;
  version: string;
  node: UnifiedNodeProjectPayload | null;
  maven: UnifiedMavenProjectPayload | null;
}

export interface RuntimeApplicationConfig {
  schemaVersion: number;
  name: string;
  project: string;
  /** Runtime 技术栈；缺省值为 springBoot 以兼容历史配置。 */
  kind?: RuntimeKind;
  nodeScript?: string | null;
  nodePackageManager?: string | null;
  mainClass: string | null;
  jdk: string | null;
  profile: string | null;
  vmOptions: string[];
  programArguments: string[];
  environment: Record<string, string>;
  runtimeEnvironment: Record<string, string>;
  buildEngine: string | null;
  /** Runtime Scope（R-03 §15）；缺省 Auto，R-13 Scope 视图可调。 */
  scope: RuntimeScope;
  /** R-14 §75：构建前执行的用户脚本（首次执行必须确认）。 */
  preBuildScript: string | null;
  /** R-14 §75：构建成功后执行的用户脚本（同上确认规则）。 */
  postBuildScript: string | null;
  /** R-16 §41 健康检查配置；null = 不探针（保持生命周期推导语义）。 */
  healthCheck: HealthCheckConfig | null;
  /** R-17 §42 自动重启开关（File Watch → 增量重建 → 自动重启）；缺省关。 */
  autoRestart: boolean | null;
}

export interface RuntimeConfigSummary {
  id: number;
  workspaceId: number;
  name: string;
  project: string;
  kind?: RuntimeKind;
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
export type RunStrategy = "mavenRun" | "packageRun" | "classpathRun" | "nodeScript";

/** Runtime 任务操作（R-12，§63/§65；R-15/R-17 扩展）。 */
export type RuntimeOp =
  | "build"
  | "start"
  | "stop"
  | "restart"
  | "resolveDependencies"
  | "startEnvironment"
  | "stopEnvironment"
  | "rebuildRestart";

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

/** `runtime.health_changed` 的健康取值。R-12 生命周期推导产生
    up/down；R-16 探针状态机产生 starting/healthy/unhealthy/stopped。 */
export type HealthStatus =
  | "up"
  | "down"
  | "starting"
  | "healthy"
  | "unhealthy"
  | "stopped";

/** R-16 §41 健康检查方式；auto = Actuator 优先、失败回退 TCP。 */
export type HealthCheckKind = "auto" | "port" | "http" | "tcp" | "actuator";

/** R-16 §41 每应用健康检查配置（随 Runtime 配置持久化，全部字段可缺省）。 */
export interface HealthCheckConfig {
  kind: HealthCheckKind;
  host: string | null;
  port: number | null;
  path: string | null;
  intervalMs: number | null;
  timeoutMs: number | null;
  healthyAfter: number | null;
  unhealthyAfter: number | null;
}

/** R-16 健康快照（探针状态机当前状态）。 */
export interface HealthSnapshot {
  processId: number;
  workspaceId: number;
  runtimeName: string;
  phase: HealthStatus;
  lastCheckedAt: string | null;
  lastDetail: string | null;
}

/** R-16 §81 端口占用方信息。 */
export interface PortOccupier {
  pid: number | null;
  processName: string | null;
}

/** R-16 §81 端口检查结果。 */
export interface PortCheckResult {
  port: number;
  inUse: boolean;
  occupier: PortOccupier | null;
}

/** R-16 §81 跨进程 Kill 结果。 */
export interface PortKillOutcome {
  pid: number;
  processName: string | null;
  killed: boolean;
}

// ---------------------------------------------------------------------------
// R-15 §38/§39/§40：Multi-Service Runtime Environment
// ---------------------------------------------------------------------------

/** 环境内的一个服务（引用 Runtime 配置 + 覆盖项，§39/§82）。 */
export interface EnvironmentService {
  runtimeName: string;
  /** 依赖的其他服务（拓扑排序决定启动顺序）。 */
  dependsOn: string[];
  jdk: string | null;
  profile: string | null;
  environment: Record<string, string>;
  port: number | null;
  externalNotes: string | null;
  readyTimeoutSeconds: number | null;
}

/** 多服务环境（§82；持久化于 .gitworkspace/environments/<name>.json）。 */
export interface RuntimeEnvironment {
  schemaVersion: number;
  name: string;
  description: string | null;
  services: EnvironmentService[];
}

/** 环境内单服务的编排状态。 */
export type ServiceExecState =
  | "skipped"
  | "starting"
  | "ready"
  | "failed"
  | "stopped";

/** `runtime.environment_progress` 事件 payload。 */
export interface EnvironmentProgressPayload {
  workspaceId: number;
  environment: string;
  service: string;
  state: ServiceExecState;
  detail: string | null;
  at: string;
}

/** `runtime.environment_completed` 事件 payload。 */
export interface EnvironmentServiceOutcome {
  service: string;
  state: ServiceExecState;
  detail: string | null;
}

export interface EnvironmentCompletedPayload {
  workspaceId: number;
  environment: string;
  success: boolean;
  services: EnvironmentServiceOutcome[];
  at: string;
}

// ---------------------------------------------------------------------------
// R-19 §83：Runtime Templates
// ---------------------------------------------------------------------------

/** 一个 Runtime 配置模板（§83）：与 R-07 配置同构的预填载荷 + 元信息。 */
export interface RuntimeTemplate {
  schemaVersion: number;
  /** 模板名（workspace 内唯一）。 */
  name: string;
  description: string | null;
  /** 适用类型（如 spring-boot）。 */
  appliesTo: string | null;
  /** true = 内置模板（无用户文件时列出，不可删除；同名用户文件遮蔽）。 */
  builtin: boolean;
  /** 预填配置（name / project 通常为空，应用时填写）。 */
  config: RuntimeApplicationConfig;
}

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

/** `runtime_export_logs` 返回（R-13，R-11 §36）。 */
export interface LogExportOutcome {
  /** 实际写入的文件路径。 */
  path: string;
  /** 实际导出的行数（与同条件 search 一致）。 */
  lines: number;
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

// ---------------------------------------------------------------------------
// R-21 §47/§48/§49：Git 联动
// ---------------------------------------------------------------------------

/** `runtime_dependency_changed` 事件 payload。reason：`filesModified`
    （§47 Status 联动）/ `branchSwitched`（§48 分支切换 POM 变化）。 */
export interface DependencyChangedPayload {
  workspaceId: number;
  runtimeName: string;
  reason: "filesModified" | "branchSwitched";
  /** 发生变化的仓库路径（正斜杠归一化）。 */
  repos: string[];
  /** 受影响模块 GA 列表。 */
  affectedModules: string[];
  at: string;
}

/** §49 运行中应用摘要（Checkout 保护确认查询用）。 */
export interface RuntimeRunningBrief {
  workspaceId: number;
  runtimeName: string;
  status: string;
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

/** `runtime_get_closure` 返回（R-13）：给定 Scope 下的闭包预览。 */
export interface ClosurePreview {
  closure: RuntimeClosure;
  /** 是否命中 graph fingerprint 闭包缓存。 */
  cacheHit: boolean;
}

/** §66 调度并发上限（`runtime-scheduler.json` 持久化）。 */
export interface SchedulerConfig {
  maxConcurrentBuilds: number;
  maxConcurrentResolves: number;
}

/** R-14 §75：Pre/Post Build Script 确认记录（`script-approvals.json`）。 */
export interface ScriptApproval {
  workspaceId: number;
  runtimeName: string;
  /** "pre" | "post"。 */
  scriptType: string;
  /** 脚本内容哈希：内容变更后需重新确认。 */
  scriptHash: string;
  /** 脚本首行预览。 */
  preview: string;
  approvedAt: string;
  /** 最近一次实际执行时间（确认后执行且记录）。 */
  lastExecutedAt: string | null;
}
