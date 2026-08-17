import { invoke } from "@tauri-apps/api/core";
import type {
  OperationLogDetail,
  OperationLogPage,
  UndoOutcome,
  UndoPreviewItem,
} from "@/types/operationLog";

/**
 * Query operation logs (T-34), newest first. Filters: workspace, repo-path
 * substring, op type ("checkout_all" | "delete_branch_all" | "reset" |
 * "rebase"), created-date bounds ("YYYY-MM-DD"). Paged via limit/offset.
 */
export function listOperationLogs(
  workspaceId: number | null,
  repoPath: string | null,
  opType: string | null,
  dateFrom: string | null,
  dateTo: string | null,
  limit: number,
  offset: number,
): Promise<OperationLogPage> {
  return invoke<OperationLogPage>("list_operation_logs", {
    workspaceId,
    repoPath,
    opType,
    dateFrom,
    dateTo,
    limit,
    offset,
  });
}

/** Full detail of one logged operation, incl. every per-repo ref snapshot. */
export function getOperationLogDetail(
  logId: number,
): Promise<OperationLogDetail> {
  return invoke<OperationLogDetail>("get_operation_log_detail", { logId });
}

/**
 * Per-repo undo plan with live safety checks — the impact list for the §46
 * Dangerous confirmation dialog shown before undoing.
 */
export function previewUndoOperation(
  logId: number,
): Promise<UndoPreviewItem[]> {
  return invoke<UndoPreviewItem[]>("preview_undo_operation", { logId });
}

/**
 * One-click undo (§46 Dangerous — confirm with the preview impact list
 * first): apply the reverse operation per repo and record the outcome.
 */
export function undoOperation(logId: number): Promise<UndoOutcome> {
  return invoke<UndoOutcome>("undo_operation", { logId });
}
