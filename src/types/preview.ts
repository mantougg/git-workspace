/** One commit in a destructive-op preview list (GF-17). */
export interface PreviewCommit {
  oid: string;
  shortOid: string;
  /** First line of the commit message. */
  summary: string;
  author: string;
  /** Formatted like graph-view commit times. */
  time: string;
}

/** One tracked change a hard reset would discard (GF-17). */
export interface PreviewFileChange {
  path: string;
  /** "conflicted" | "staged" | "unstaged" | "staged+unstaged" */
  status: string;
}

/**
 * What a `reset` will do (GF-17, Roadmap §46 structured preview).
 * Computed by the read-only `preview_reset` command; shown inside the
 * Dangerous confirm before `reset_to` runs.
 */
export interface ResetPreview {
  repoPath: string;
  /** Current branch shorthand; "detached HEAD" when HEAD is detached. */
  branch: string;
  detached: boolean;
  headOid: string;
  /** Resolved target oid the reset will point HEAD at. */
  targetOid: string;
  targetSummary: string;
  /** "soft" | "mixed" | "hard" */
  mode: string;
  /** Commits the branch will drop (target..HEAD), newest first, capped. */
  discardedCommits: PreviewCommit[];
  /** Authoritative count of discarded commits (list may be capped). */
  discardedCount: number;
  /** Tracked changes dropped from the index (worktree too when hard). */
  lostFileChanges: PreviewFileChange[];
  /** Authoritative count of lost changes (list may be capped). */
  lostChangesCount: number;
  /** hard + uncommitted changes: content no reflog entry can restore. */
  unrecoverable: boolean;
}

/**
 * What a `merge` will do (GF-17, Roadmap §46 structured preview).
 * Computed by the read-only `preview_merge` command; shown inside the merge
 * confirm before `merge_branch` runs.
 */
export interface MergePreview {
  repoPath: string;
  /** Current branch (merge target). */
  branch: string;
  /** Source ref being merged in. */
  source: string;
  headOid: string;
  sourceOid: string;
  /** "up_to_date" | "fast_forward" | "merge" */
  kind: string;
  /** "normal" | "no-ff" | "squash" */
  mode: string;
  /** Commits the merge brings in (source side, not in HEAD), newest first, capped. */
  incomingCommits: PreviewCommit[];
  /** Authoritative count of incoming commits (list may be capped). */
  incomingCount: number;
  /** Files the merge can change in the worktree (their side vs merge base), capped. */
  affectedFiles: string[];
  /** Authoritative count of affected files (list may be capped). */
  affectedFilesCount: number;
  /** Predicted via an in-memory merge; always false for up_to_date / fast_forward. */
  conflictPredicted: boolean;
  conflictFiles: string[];
  /** Uncommitted tracked changes — the merge itself would refuse (PAF-10). */
  dirtyBlocked: boolean;
  dirtyFiles: string[];
  dirtyFilesCount: number;
}
