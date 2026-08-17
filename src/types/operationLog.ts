/**
 * Unified operation log + undo (T-34). Mirrors the serde camelCase payloads
 * of `src-tauri/src/core/operation_log.rs`.
 */

/** One page of operation log summaries plus the total matching count. */
export interface OperationLogPage {
  total: number;
  logs: OperationLogSummary[];
}

/** List-row view of one logged operation (items aggregated). */
export interface OperationLogSummary {
  id: number;
  workspaceId: number | null;
  /** "checkout_all" | "delete_branch_all" | "reset" | "rebase" */
  opType: string;
  summary: string;
  createdAt: string;
  undoneAt: string | null;
  /** How many per-repo items the log has. */
  repoCount: number;
  /** How many of them are already undone. */
  undoneCount: number;
}

/** One per-repo ref snapshot of a logged operation. */
export interface OperationLogItem {
  id: number;
  logId: number;
  repoPath: string;
  /** Short branch name; empty when HEAD was detached. */
  refName: string;
  beforeOid: string;
  /** Tip after the op; null when unknown (async batch) or ref is gone. */
  afterOid: string | null;
  /** Op-specific extra, e.g. "mode:hard" (reset) / "onto:x" (rebase). */
  detail: string | null;
  undoneAt: string | null;
}

/** Full detail of one logged operation including all per-repo items. */
export interface OperationLogDetail {
  id: number;
  workspaceId: number | null;
  opType: string;
  summary: string;
  createdAt: string;
  undoneAt: string | null;
  items: OperationLogItem[];
}

/** One repo's undo plan row for the §46 confirmation dialog. */
export interface UndoPreviewItem {
  itemId: number;
  repoPath: string;
  repoName: string;
  /** Human-readable reverse action. */
  action: string;
  /** Whether the reverse op can run safely right now. */
  ok: boolean;
  /** Safety-check detail (why not ok); empty when ok. */
  message: string;
  /** Already undone (skipped on execution). */
  undone: boolean;
}

/** Per-repo outcome of an undo run. */
export interface UndoItemResult {
  itemId: number;
  repoPath: string;
  repoName: string;
  success: boolean;
  message: string;
}

/** Aggregate outcome of an undo run over one operation log. */
export interface UndoOutcome {
  logId: number;
  /** True when every item of the log is undone. */
  fullyUndone: boolean;
  results: UndoItemResult[];
}
