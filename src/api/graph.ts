import { invoke } from "@tauri-apps/api/core";
import type { CommitInfo, BranchInfo } from "@/types/graph";

/**
 * Load a page of commit history (newest first).
 *
 * `offset` skips the newest `offset` commits and `limit` caps the page size —
 * paging transfers only the page instead of re-fetching the whole prefix.
 * `maxCount` is the legacy alias for the page size (kept for backward
 * compatibility: with `offset`/`limit` omitted it returns the first
 * `maxCount` commits from HEAD).
 */
export function getCommitHistory(
  repoPath: string,
  maxCount?: number,
  offset?: number,
  limit?: number,
): Promise<CommitInfo[]> {
  // 可选参数统一 ?? null（同 create_branch / ai_get_session 惯例）：
  // Rust 侧 Option<usize> 收到 null / 缺省均为 None。
  return invoke<CommitInfo[]>("get_commit_history", {
    repoPath,
    maxCount: maxCount ?? null,
    offset: offset ?? null,
    limit: limit ?? null,
  });
}

export function getBranches(repoPath: string): Promise<BranchInfo[]> {
  return invoke<BranchInfo[]>("get_branches", { repoPath });
}
