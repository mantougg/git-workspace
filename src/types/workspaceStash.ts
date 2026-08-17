/** Per-repo outcome of a workspace stash save / restore run (T-21). */
export interface WorkspaceStashRepoOutcome {
  repoPath: string;
  repoName: string;
  /**
   * Save: "stashed" | "skipped_clean" | "failed".
   * Restore: "applied" | "skipped" | "failed".
   */
  status: string;
  stashOid: string | null;
  detail: string;
}

/**
 * Result of a workspace stash save. `id` is null when nothing was stashed
 * (all repos clean/failed), in which case no record was written.
 */
export interface SaveWorkspaceStashResult {
  id: number | null;
  name: string;
  items: WorkspaceStashRepoOutcome[];
}

/** List row for one workspace stash record. */
export interface WorkspaceStashSummary {
  id: number;
  name: string;
  message: string | null;
  createdAt: string;
  repoCount: number;
}

/** One repo member of a workspace stash record. */
export interface WorkspaceStashItemEntry {
  repoPath: string;
  stashOid: string;
  /** Stash stack index at save time (informational; restore re-resolves by oid). */
  stashIndex: number;
  /** Branch the repo was on when stashed. */
  branch: string;
}

/** Pre-restore safety check for one repo (T-21 §46 Warning flow input). */
export interface WorkspaceStashCheckItem {
  repoPath: string;
  repoName: string;
  /** Branch recorded at stash time. */
  branch: string;
  currentBranch: string | null;
  /** "ok" | "branch_mismatch" | "stash_missing" | "repo_missing" | "error" */
  status: string;
  detail: string;
}
