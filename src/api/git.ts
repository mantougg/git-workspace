import { invoke } from "@tauri-apps/api/core";
import type { FileDiff } from "@/types/git";

/** Diff rendering options (Roadmap §9 diff settings). */
export interface DiffOptions {
  ignoreWhitespace: boolean;
  ignoreWhitespaceEol: boolean;
  ignoreCase: boolean;
}

export function getDiff(
  repoPath: string,
  options?: DiffOptions,
): Promise<FileDiff[]> {
  return invoke<FileDiff[]>("get_diff", { repoPath, options });
}

/** Unstaged changes only (index → workdir, includes untracked). */
export function getUnstagedDiff(
  repoPath: string,
  options?: DiffOptions,
): Promise<FileDiff[]> {
  return invoke<FileDiff[]>("get_unstaged_diff", { repoPath, options });
}

/** Staged changes only (HEAD tree → index), matching `git diff --cached`. */
export function getStagedDiff(
  repoPath: string,
  options?: DiffOptions,
): Promise<FileDiff[]> {
  return invoke<FileDiff[]>("get_staged_diff", { repoPath, options });
}

/** Diff between two revisions (branch / tag / commit specs), LRU-cached. */
export function getRevisionDiff(
  repoPath: string,
  base: string,
  other: string,
  options?: DiffOptions,
): Promise<FileDiff[]> {
  return invoke<FileDiff[]>("get_revision_diff", {
    repoPath,
    base,
    other,
    options,
  });
}

/** Diff of a single commit (parent → commit), LRU-cached. */
export function getCommitDiff(
  repoPath: string,
  oid: string,
  options?: DiffOptions,
): Promise<FileDiff[]> {
  return invoke<FileDiff[]>("get_commit_diff", { repoPath, oid, options });
}

/** Stage one hunk of a file's unstaged changes (T-12). */
export function stageHunk(
  repoPath: string,
  filePath: string,
  hunkIndex: number,
): Promise<void> {
  return invoke<void>("stage_hunk", { repoPath, filePath, hunkIndex });
}

/** Unstage one hunk of a file's staged changes (T-12). */
export function unstageHunk(
  repoPath: string,
  filePath: string,
  hunkIndex: number,
): Promise<void> {
  return invoke<void>("unstage_hunk", { repoPath, filePath, hunkIndex });
}

/** Stage only the selected lines of one hunk (T-12). */
export function stageLines(
  repoPath: string,
  filePath: string,
  hunkIndex: number,
  lineIndices: number[],
): Promise<void> {
  return invoke<void>("stage_lines", {
    repoPath,
    filePath,
    hunkIndex,
    lineIndices,
  });
}

/** Unstage only the selected lines of one staged hunk (T-12). */
export function unstageLines(
  repoPath: string,
  filePath: string,
  hunkIndex: number,
  lineIndices: number[],
): Promise<void> {
  return invoke<void>("unstage_lines", {
    repoPath,
    filePath,
    hunkIndex,
    lineIndices,
  });
}
