import { invoke } from "@tauri-apps/api/core";
import type { WorktreeInfo } from "@/types/worktree";

/** List worktrees (main + linked) of a repository. */
export function listWorktrees(repoPath: string): Promise<WorktreeInfo[]> {
  return invoke<WorktreeInfo[]>("list_worktrees", { repoPath });
}

/**
 * Create a linked worktree. `newBranch` creates a branch at HEAD and checks
 * it out; `branch` checks out an existing branch; neither = detached HEAD.
 */
export function createWorktree(
  repoPath: string,
  path: string,
  branch: string | null,
  newBranch: string | null,
): Promise<void> {
  return invoke<void>("create_worktree", { repoPath, path, branch, newBranch });
}

/** Remove a linked worktree; dirty worktrees need `force` (§46 confirm). */
export function removeWorktree(
  repoPath: string,
  name: string,
  force: boolean,
): Promise<void> {
  return invoke<void>("remove_worktree", { repoPath, name, force });
}
