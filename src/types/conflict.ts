import type { RebaseState } from "./rebase";

/** A conflicted file with its conflict shape (T-16). */
export interface ConflictFile {
  path: string;
  /** "both-modified" | "both-added" | "deleted-by-us" | "deleted-by-them" */
  conflictType: string;
}

/** The operation driving the repo's conflict state. */
export interface OperationState {
  merge: boolean;
  cherryPick: boolean;
  revert: boolean;
  rebase: RebaseState | null;
  conflicts: ConflictFile[];
}

/** Three-way + worktree content of one conflicted file. */
export interface ConflictContent {
  /** BASE (common ancestor); null when the file has no ancestor. */
  base: string | null;
  /** OURS (current HEAD side); null when deleted on our side. */
  ours: string | null;
  /** THEIRS (incoming side); null when deleted on their side. */
  theirs: string | null;
  /** Current worktree content (with conflict markers), if readable. */
  worktree: string | null;
  /** True when any side was truncated. */
  truncated: boolean;
}
