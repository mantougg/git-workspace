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
  | {
      type: "branchOp";
      op: "checkout" | "create" | "delete";
      name: string;
      force?: boolean;
    };

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
