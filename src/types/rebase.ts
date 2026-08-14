/** One interactive-rebase todo entry (T-15). */
export interface RebaseOp {
  /** "pick" | "reword" | "squash" | "drop" */
  action: string;
  oid: string;
  /** Replacement message for reword; null keeps the original. */
  message: string | null;
  /** Original first line (display). */
  subject: string;
}

/** Persisted rebase progress (restart detection). */
export interface RebaseState {
  /** HEAD before the rebase started (abort target). */
  originalHead: string;
  /** Revision the branch is being replayed onto. */
  onto: string;
  ops: RebaseOp[];
  /** Index of the op currently being applied. */
  position: number;
  /** Last commit of the new chain. */
  prevCommit: string;
}

/** Outcome of a rebase run (start / continue / skip). */
export type RebaseOutcome =
  | { status: "success"; rewritten: number }
  | {
      status: "conflict";
      files: string[];
      position: number;
      total: number;
      /** Oid of the op being applied when the conflict occurred. */
      current: string;
    };
