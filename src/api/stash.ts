import { invoke } from "@tauri-apps/api/core";
import type { StashEntry } from "@/types/stash";
import type { FileDiff } from "@/types/git";

/** List the stash stack (newest first; also persists a snapshot to SQLite). */
export function listStashes(repoPath: string): Promise<StashEntry[]> {
  return invoke<StashEntry[]>("list_stashes", { repoPath });
}

/** Stash working-tree changes; returns the stash commit oid. */
export function stashChanges(
  repoPath: string,
  message?: string,
  includeUntracked?: boolean,
): Promise<string> {
  return invoke<string>("stash_changes", {
    repoPath,
    message: message ?? null,
    includeUntracked: includeUntracked ?? null,
  });
}

/** Apply a stash entry, keeping it on the stack. */
export function applyStash(repoPath: string, index: number): Promise<void> {
  return invoke<void>("apply_stash", { repoPath, index });
}

/** Apply a stash entry and drop it from the stack. */
export function popStash(repoPath: string, index: number): Promise<void> {
  return invoke<void>("pop_stash", { repoPath, index });
}

/** Drop a stash entry (Warning-level, UI confirms first). */
export function dropStash(repoPath: string, index: number): Promise<void> {
  return invoke<void>("drop_stash", { repoPath, index });
}

/** Clear the whole stash stack (Warning-level). Returns the dropped count. */
export function clearStashes(repoPath: string): Promise<number> {
  return invoke<number>("clear_stashes", { repoPath });
}

/** Diff of a stash entry against its base commit (tracked changes). */
export function getStashDiff(repoPath: string, index: number): Promise<FileDiff[]> {
  return invoke<FileDiff[]>("get_stash_diff", { repoPath, index });
}

/** Create a branch from a stash entry (branch at base + apply + drop). */
export function branchFromStash(
  repoPath: string,
  branchName: string,
  index: number,
): Promise<void> {
  return invoke<void>("branch_from_stash", { repoPath, branchName, index });
}
