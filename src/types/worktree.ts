/** One worktree of a repository, main working tree included (T-17). */
export interface WorktreeInfo {
  /** Worktree name; the main tree uses the repo directory name. */
  name: string;
  path: string;
  /** Checked-out branch (null = detached HEAD). */
  branch: string | null;
  isMain: boolean;
  isLocked: boolean;
  /** Uncommitted changes present (drives the remove confirmation). */
  isDirty: boolean;
}
