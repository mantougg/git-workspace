import { invoke } from "@tauri-apps/api/core";
import type { PickOutcome, ResetResult } from "@/types/history";

/** Cherry-pick one or more commits onto HEAD (applied in order). */
export function cherryPick(repoPath: string, oids: string[]): Promise<PickOutcome> {
  return invoke<PickOutcome>("cherry_pick", { repoPath, oids });
}

/** Revert a single commit (creates a revert commit on success). */
export function revertCommit(repoPath: string, oid: string): Promise<PickOutcome> {
  return invoke<PickOutcome>("revert_commit", { repoPath, oid });
}

/** Reset HEAD to `target` with soft / mixed / hard semantics. */
export function resetTo(
  repoPath: string,
  target: string | undefined,
  mode: "soft" | "mixed" | "hard",
): Promise<ResetResult> {
  return invoke<ResetResult>("reset_to", {
    repoPath,
    target: target ?? null,
    mode,
  });
}

/** Abort an in-progress cherry-pick / revert (restores pre-operation state). */
export function abortPick(repoPath: string, baseOid?: string): Promise<void> {
  return invoke<void>("abort_pick", { repoPath, baseOid: baseOid ?? null });
}

/** Continue an in-progress cherry-pick / revert after resolving (T-16). */
export function pickContinue(repoPath: string): Promise<string> {
  return invoke<string>("pick_continue", { repoPath });
}

/** Currently conflicted files (empty when no conflict is in progress). */
export function getConflictFiles(repoPath: string): Promise<string[]> {
  return invoke<string[]>("get_conflict_files", { repoPath });
}
