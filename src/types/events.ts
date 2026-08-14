import type { RepoStatus } from "./repository";

/** Payload of the `scan_progress` Tauri event (repository scan). */
export interface ScanProgress {
  workspaceId: number;
  found: number;
  current: number;
  total: number | null;
}

/** Payload item of the `repo_status_changed_batch` Tauri event (file watcher). */
export interface RepoStatusUpdate {
  repoPath: string;
  status: RepoStatus;
}
