import { invoke } from "@tauri-apps/api/core";
import type { MergeOutcome } from "@/types/merge";

/** Merge `branch` into HEAD. `mode`: "normal" | "no-ff" | "squash". */
export function mergeBranch(
  repoPath: string,
  branch: string,
  mode: string,
): Promise<MergeOutcome> {
  return invoke<MergeOutcome>("merge_branch", { repoPath, branch, mode });
}

/** Finalize a conflicted merge after resolving the index. Returns commit oid. */
export function mergeContinue(repoPath: string, message?: string): Promise<string> {
  return invoke<string>("merge_continue", { repoPath, message: message ?? null });
}

/** Abort a conflicted merge, restoring the pre-merge state. */
export function mergeAbort(repoPath: string): Promise<void> {
  return invoke<void>("merge_abort", { repoPath });
}

/** Whether a merge is in progress (MERGE_HEAD exists). */
export function getMergeInProgress(repoPath: string): Promise<boolean> {
  return invoke<boolean>("get_merge_in_progress", { repoPath });
}
