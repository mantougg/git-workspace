import { invoke } from "@tauri-apps/api/core";
import type { MergePreview, ResetPreview } from "@/types/preview";

/**
 * Structured preview of a `reset` (GF-17): commits the branch will drop
 * (oid + message) and the tracked worktree/index changes that will be
 * discarded. Read-only — never mutates the repository.
 */
export function previewReset(
  repoPath: string,
  target: string | null,
  mode: "soft" | "mixed" | "hard",
): Promise<ResetPreview> {
  return invoke<ResetPreview>("preview_reset", { repoPath, target, mode });
}

/**
 * Structured preview of a `merge` (GF-17): incoming commits, affected files
 * and a conflict prediction. Read-only — never mutates the repository.
 */
export function previewMerge(
  repoPath: string,
  branch: string,
  mode: "normal" | "no-ff" | "squash",
): Promise<MergePreview> {
  return invoke<MergePreview>("preview_merge", { repoPath, branch, mode });
}
