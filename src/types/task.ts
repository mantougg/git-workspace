import type { RunStrategy, RuntimeOp } from "./runtime";

export type TaskType =
  | { type: "fetch" }
  | { type: "pull" }
  | { type: "push" }
  | {
      type: "commit";
      message: string;
      files: string[];
      amend?: boolean;
      noEdit?: boolean;
      indexOnly?: boolean;
      thenPush?: boolean;
      allowUnsafe?: boolean;
      authorName?: string | null;
      authorEmail?: string | null;
    }
  | { type: "conflictApply"; path: string; strategy: string; content?: string | null }
  | {
      type: "branchOp";
      op: "checkout" | "create" | "delete";
      name: string;
      force?: boolean;
    }
  | { type: "clone"; url: string; branch?: string | null }
  | { type: "shellCommand"; command: string; timeoutSecs?: number | null }
  | {
      /** Runtime Workspace 操作（R-12）：build / start / stop / restart 一个
       * Runtime 配置，或刷新 workspace 依赖索引。 */
      type: "runtime";
      op: RuntimeOp;
      workspaceId: number;
      runtimeName?: string;
      options?: RuntimeTaskOptions;
    }
  | { type: "runtimeUpdateConfig"; workspaceId: number; name: string; configJson: string }
  | {
      /** 显式依赖安装（N-08）：仅由确认后的 node_install IPC 创建。 */
      type: "nodeInstall";
      projectDir: string;
      packageManager: string;
    };

/** Runtime 任务的用户可调选项（R-12）；未指定项由后端跟随
 * BuildOptions / StartOptions 默认（对齐 IDEA Build 语义）。 */
export interface RuntimeTaskOptions {
  strategy?: RunStrategy | null;
  skipBuild?: boolean;
  skipTests?: boolean | null;
  offline?: boolean;
  /** R-17 §44：watch 影响分析给出的必建模块 GA 子集（增量 -pl 下限）。 */
  affectedModules?: string[];
}

export type TaskStatus =
  | { type: "queued" }
  | { type: "running"; progress: number }
  | { type: "success" }
  | { type: "partialSuccess"; succeeded: number; failed: number }
  | { type: "failed"; error: string }
  | { type: "cancelled" };

export interface Task {
  id: string;
  taskType: TaskType;
  repoPath: string;
  repoName: string;
  status: TaskStatus;
  createdAt: string;
  /** Batch this task belongs to (T-20); null for standalone/batch rows. */
  batchId?: string | null;
}

export interface TaskRequest {
  taskType: TaskType;
  repoPath: string;
  repoName: string;
}

export interface TaskProgress {
  taskId: string;
  taskType: TaskType;
  repoPath: string;
  repoName: string;
  status: TaskStatus;
  /** Batch grouping key (T-20); null for the batch row itself. */
  batchId?: string | null;
}

export interface CommitRequest {
  repoPath: string;
  repoName: string;
  message: string;
  files: string[];
  amend?: boolean;
  noEdit?: boolean;
  indexOnly?: boolean;
  thenPush?: boolean;
  allowUnsafe?: boolean;
}

/** Payload of the `git_command_result` event (IDE-style git console). */
export interface GitCommandResult {
  repoName: string;
  repoPath: string;
  command: string;
  success: boolean;
  output: string;
}
