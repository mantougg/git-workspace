import type { TaskRequest, TaskStatus } from "@/types/task";

// ---------------------------------------------------------------------------
// T-24 Task DAG
// ---------------------------------------------------------------------------

/** Failure propagation policy of a DAG run. */
export type FailurePolicy = "failFast" | "continue";

/** Dispatch condition of a DAG node (evaluated in memory, T-23 Conditional). */
export type NodeCondition = { type: "repoClean" };

/** One node of a DAG submission. */
export interface DagNodeRequest {
  task: TaskRequest;
  /** Indices into the same submission's `nodes` array. */
  dependsOn?: number[];
  /** Scheduler-level attempts (1 = no retry). */
  maxAttempts?: number;
  condition?: NodeCondition | null;
  group?: string | null;
  label?: string | null;
}

/** Submit a dependency DAG (T-24). */
export interface DagSubmitRequest {
  name: string;
  nodes: DagNodeRequest[];
  onFailure?: FailurePolicy;
}

/** One node in the DAG visualization / report query. */
export interface DagNodeInfo {
  taskId: string;
  label: string;
  group?: string | null;
  repoPath: string;
  repoName: string;
  dependsOn: string[];
  status: TaskStatus;
  /** Skipped (dependency failed / condition false), not executed. */
  skipped: boolean;
  attempts: number;
  output?: string | null;
  startedAt?: string | null;
  finishedAt?: string | null;
}

export interface DagEdge {
  from: string;
  to: string;
}

/** DAG visualization payload: nodes + edges + live states. */
export interface DagGraph {
  dagId: string;
  name: string;
  onFailure: FailurePolicy;
  nodes: DagNodeInfo[];
  edges: DagEdge[];
}

// ---------------------------------------------------------------------------
// T-23 Workspace Pipeline
// ---------------------------------------------------------------------------

/** Built-in step kinds. */
export type StepKind =
  | { type: "fetch" }
  | { type: "checkStatus" }
  | { type: "pull" }
  | { type: "build"; command: string }
  | { type: "test"; command: string }
  | { type: "report" };

/** One orchestration step (Sequential by default; dependsOn for branches). */
export interface PipelineStep {
  id: string;
  name: string;
  kind: StepKind;
  /** Explicit upstream step ids; empty = chain on the previous step. */
  dependsOn?: string[];
  condition?: NodeCondition | null;
  retries?: number;
  timeoutSecs?: number | null;
}

/** Pipeline definition / template. */
export interface Pipeline {
  id: string;
  name: string;
  description: string;
  steps: PipelineStep[];
  createdAt: string;
  updatedAt: string;
}

/** Repository a pipeline run applies to. */
export interface RepoSelection {
  repoPath: string;
  repoName: string;
}

/** Per-repository outcome of one step. */
export interface StepItemReport {
  taskId: string;
  repoPath: string;
  repoName: string;
  /** queued / running / success / failed / cancelled / skipped */
  status: string;
  message?: string | null;
  output?: string | null;
  attempts: number;
  startedAt?: string | null;
  finishedAt?: string | null;
  durationMs?: number | null;
}

/** Aggregate + per-repo detail of one step. */
export interface StepReport {
  stepId: string;
  name: string;
  kind: string;
  /** pending / running / success / partialSuccess / failed / cancelled / skipped */
  status: string;
  total: number;
  succeeded: number;
  failed: number;
  skipped: number;
  cancelled: number;
  items: StepItemReport[];
  startedAt?: string | null;
  finishedAt?: string | null;
  durationMs?: number | null;
}

/** Full execution report of one pipeline run. */
export interface PipelineRunReport {
  runId: string;
  pipelineName: string;
  status: string;
  total: number;
  succeeded: number;
  failed: number;
  skipped: number;
  cancelled: number;
  steps: StepReport[];
  startedAt?: string | null;
  finishedAt?: string | null;
  durationMs?: number | null;
}
