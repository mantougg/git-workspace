export type TaskType =
  | { type: "fetch" }
  | { type: "pull" }
  | { type: "push" }
  | { type: "commit"; message: string; files: string[] };

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
}

export interface CommitRequest {
  repoPath: string;
  repoName: string;
  message: string;
  files: string[];
}

/** Payload of the `git_command_result` event (IDE-style git console). */
export interface GitCommandResult {
  repoName: string;
  repoPath: string;
  command: string;
  success: boolean;
  output: string;
}
