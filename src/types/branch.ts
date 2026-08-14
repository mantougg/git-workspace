import type { CommitInfo } from "./graph";
import type { FileDiff } from "./git";

/** A local branch with its upstream tracking state. */
export interface BranchEntry {
  name: string;
  isCurrent: boolean;
  lastCommitOid: string;
  lastCommitMessage: string;
  /** Upstream remote branch, e.g. "origin/main"; null if not tracking. */
  upstream: string | null;
  /** Commits ahead of the upstream (local-only). */
  ahead: number;
  /** Commits behind the upstream (local-only). */
  behind: number;
}

/** A remote-tracking branch (local snapshot of `refs/remotes/*`). */
export interface RemoteBranchEntry {
  name: string;
  lastCommitOid: string;
  lastCommitMessage: string;
}

/** A tag with its (peeled) target commit. */
export interface TagEntry {
  name: string;
  targetOid: string;
  /** Annotated tag message, if any. */
  message: string | null;
}

/** The three branch-manager sections of one repository. */
export interface BranchOverview {
  /** Current branch name; null when HEAD is detached. */
  current: string | null;
  locals: BranchEntry[];
  remotes: RemoteBranchEntry[];
  tags: TagEntry[];
}

/** Result of comparing two revisions (Branch Compare). */
export interface CompareResult {
  base: string;
  other: string;
  /** Commits reachable from `other` but not from `base`. */
  ahead: CommitInfo[];
  /** Commits reachable from `base` but not from `other`. */
  behind: CommitInfo[];
  /** File diff from `base` to `other`. */
  files: FileDiff[];
}
