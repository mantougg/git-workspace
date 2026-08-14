import { invoke } from "@tauri-apps/api/core";
import type { ConflictContent, OperationState } from "@/types/conflict";

/** The repo's current operation + conflict state. */
export function getOperationState(repoPath: string): Promise<OperationState> {
  return invoke<OperationState>("get_operation_state", { repoPath });
}

/** Load BASE / OURS / THEIRS + worktree content of one conflicted file. */
export function getConflictContent(
  repoPath: string,
  path: string,
): Promise<ConflictContent> {
  return invoke<ConflictContent>("get_conflict_content", { repoPath, path });
}

/** Resolve one conflicted file: "ours" | "theirs" | "both". */
export function resolveConflict(
  repoPath: string,
  path: string,
  strategy: string,
): Promise<void> {
  return invoke<void>("resolve_conflict", { repoPath, path, strategy });
}

/** Resolve one conflicted file with manually edited content (null = delete). */
export function resolveConflictWithContent(
  repoPath: string,
  path: string,
  content: string | null,
): Promise<void> {
  return invoke<void>("resolve_conflict_with_content", { repoPath, path, content });
}
