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
  | { type: "stageFiles"; files: string[] }
  | { type: "restoreFiles"; files: string[] }
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

/**
 * TM-08：命令流事件条目（append-only）。每次状态迁移记一条，面板按时间序
 * 渲染；不上屏的瞬时迁移（queued→running 的高频刷新）也会落条目，但面板
 * 折叠连续同态事件只保留边界态（queued / 首个 running / 终态）。
 */
export interface TaskEventEntry {
  /** 单调递增序列号（同一事件重放去重 / 排序用）。 */
  seq: number;
  /** 任务 id；batch 行事件与子任务共享同一 batch id 以外的任务 id。 */
  taskId: string;
  at: number;
  repoPath: string;
  repoName: string;
  taskType: TaskType;
  status: TaskStatus;
  /** 状态迁移耗时（ms）；queued / running 首帧为 undefined。 */
  durationMs?: number;
  batchId?: string | null;
}

/** 命令流事件上限（防无限增长，超限丢最旧，同 terminal writeBuffer 策略）。 */
export const TASK_EVENT_LOG_MAX = 200;

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
