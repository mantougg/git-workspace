import { invoke } from "@tauri-apps/api/core";
import type { RepoChanges } from "@/types/changes";

/** Get the file-level change list for every repository in a workspace. */
export function getWorkspaceChanges(
  workspaceId: number,
): Promise<RepoChanges[]> {
  return invoke<RepoChanges[]>("get_workspace_changes", { workspaceId });
}

/** Request payload for staging files (git add). */
export interface AddRequest {
  repoPath: string;
  repoName: string;
  files: string[];
}

/** Stage (git add) the given files in each repository. */
export function batchAdd(requests: AddRequest[]): Promise<string[]> {
  return invoke<string[]>("batch_add", { requests });
}

/** Request payload for reverting working-tree changes. */
export interface RestoreRequest {
  repoPath: string;
  repoName: string;
  files: string[];
}

/** Revert working-tree changes for the given files in each repository. */
export function batchRestore(requests: RestoreRequest[]): Promise<string[]> {
  return invoke<string[]>("batch_restore", { requests });
}
