import { invoke } from "@tauri-apps/api/core";
import type { RebaseOp, RebaseOutcome, RebaseState } from "@/types/rebase";

/** Default rebase todo: commits of `branch` not in `upstream`, oldest first. */
export function listRebaseCommits(
  repoPath: string,
  upstream: string,
  branch?: string,
): Promise<RebaseOp[]> {
  return invoke<RebaseOp[]>("list_rebase_commits", {
    repoPath,
    upstream,
    branch: branch ?? null,
  });
}

/** Start a rebase: replay `ops` onto `onto`. */
export function startRebase(
  repoPath: string,
  onto: string,
  ops: RebaseOp[],
): Promise<RebaseOutcome> {
  return invoke<RebaseOutcome>("start_rebase", { repoPath, onto, ops });
}

/** Continue after the conflicted op was resolved. */
export function rebaseContinue(repoPath: string): Promise<RebaseOutcome> {
  return invoke<RebaseOutcome>("rebase_continue", { repoPath });
}

/** Skip the current (conflicting) op and replay the rest. */
export function rebaseSkip(repoPath: string): Promise<RebaseOutcome> {
  return invoke<RebaseOutcome>("rebase_skip", { repoPath });
}

/** Abort the rebase, restoring the pre-rebase HEAD. */
export function rebaseAbort(repoPath: string): Promise<void> {
  return invoke<void>("rebase_abort", { repoPath });
}

/** Current rebase progress (restart detection), if any. */
export function getRebaseState(repoPath: string): Promise<RebaseState | null> {
  return invoke<RebaseState | null>("get_rebase_state", { repoPath });
}
