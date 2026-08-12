import { invoke } from "@tauri-apps/api/core";
import type { CommitInfo, BranchInfo } from "@/types/graph";

export function getCommitHistory(
  repoPath: string,
  maxCount?: number,
): Promise<CommitInfo[]> {
  return invoke<CommitInfo[]>("get_commit_history", {
    repoPath,
    maxCount,
  });
}

export function getBranches(repoPath: string): Promise<BranchInfo[]> {
  return invoke<BranchInfo[]>("get_branches", { repoPath });
}
