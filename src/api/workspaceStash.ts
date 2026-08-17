import { invoke } from "@tauri-apps/api/core";
import type {
  SaveWorkspaceStashResult,
  WorkspaceStashCheckItem,
  WorkspaceStashItemEntry,
  WorkspaceStashRepoOutcome,
  WorkspaceStashSummary,
} from "@/types/workspaceStash";

/**
 * Save a workspace stash (T-21): stash every selected repo and persist the
 * `Workspace Stash #N` association record. Clean repos are skipped; when
 * nothing was stashed no record is written (`id` is null).
 */
export function saveWorkspaceStash(
  workspaceId: number,
  repoPaths: string[],
  message?: string,
  includeUntracked?: boolean,
): Promise<SaveWorkspaceStashResult> {
  return invoke<SaveWorkspaceStashResult>("save_workspace_stash", {
    workspaceId,
    repoPaths,
    message: message ?? null,
    includeUntracked: includeUntracked ?? null,
  });
}

/** List the workspace stash records of a workspace, newest first. */
export function listWorkspaceStashes(
  workspaceId: number,
): Promise<WorkspaceStashSummary[]> {
  return invoke<WorkspaceStashSummary[]>("list_workspace_stashes", {
    workspaceId,
  });
}

/** Per-repo items of one workspace stash record. */
export function getWorkspaceStashItems(
  workspaceStashId: number,
): Promise<WorkspaceStashItemEntry[]> {
  return invoke<WorkspaceStashItemEntry[]>("get_workspace_stash_items", {
    workspaceStashId,
  });
}

/**
 * Pre-restore safety check (§46): per repo, is the stash still on the stack
 * and is the current branch the recorded one?
 */
export function checkWorkspaceStash(
  workspaceStashId: number,
): Promise<WorkspaceStashCheckItem[]> {
  return invoke<WorkspaceStashCheckItem[]>("check_workspace_stash", {
    workspaceStashId,
  });
}

/**
 * Restore a workspace stash: apply each repo's stash (kept on the stack).
 * Repos failing the check are skipped; a branch mismatch applies only with
 * `allowBranchMismatch`. Per-repo failures are collected, not thrown.
 */
export function restoreWorkspaceStash(
  workspaceStashId: number,
  allowBranchMismatch?: boolean,
): Promise<WorkspaceStashRepoOutcome[]> {
  return invoke<WorkspaceStashRepoOutcome[]>("restore_workspace_stash", {
    workspaceStashId,
    allowBranchMismatch: allowBranchMismatch ?? null,
  });
}

/**
 * Delete a workspace stash record. The per-repo stashes stay on each repo's
 * stack (manageable in the single-repo Stash view).
 */
export function deleteWorkspaceStash(workspaceStashId: number): Promise<void> {
  return invoke<void>("delete_workspace_stash", { workspaceStashId });
}
