/** Outcome of a cherry-pick / revert operation (T-13). */
export type PickOutcome =
  | { status: "success"; picked: number }
  | {
      status: "conflict";
      files: string[];
      current: string;
      done: number;
      total: number;
      baseOid: string | null;
    };

/** Result of a reset operation. */
export interface ResetResult {
  /** HEAD before the reset (recovery hint; reflog comes with T-14). */
  previousHead: string | null;
  /** Resolved target oid HEAD/index/worktree now points at. */
  target: string;
  mode: string;
}
