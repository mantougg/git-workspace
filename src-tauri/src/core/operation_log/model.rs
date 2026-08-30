//! Data model of the operation log: the op_type vocabulary, the IPC DTOs
//! (serde camelCase; mirrored in src/types/operationLog.ts), and the
//! per-repo snapshot rows supplied by instrumented commands.

use serde::Serialize;

/// op_type of a batch checkout (T-20 Checkout All).
pub const OP_CHECKOUT_ALL: &str = "checkout_all";
/// op_type of a batch branch delete (T-20 Delete Branch All).
pub const OP_DELETE_BRANCH_ALL: &str = "delete_branch_all";
/// op_type of a `reset_to` (soft/mixed/hard; the mode is kept in the item's
/// detail so undo can mirror it).
pub const OP_RESET: &str = "reset";
/// op_type of a completed rebase.
pub const OP_REBASE: &str = "rebase";
/// op_type of a conflicted file resolved through the Conflict Resolver.
///
/// The log records the confirmed Apply action but deliberately contains no
/// file content. Conflict text may contain secrets and cannot be restored
/// safely from the ref-snapshot-based Undo model.
pub const OP_CONFLICT_RESOLUTION: &str = "conflict_resolution";

/// One page of operation log summaries plus the total matching count.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationLogPage {
    pub total: i64,
    pub logs: Vec<OperationLogSummary>,
}

/// List-row view of one logged operation (items aggregated).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationLogSummary {
    pub id: i64,
    pub workspace_id: Option<i64>,
    pub op_type: String,
    pub summary: String,
    pub created_at: String,
    pub undone_at: Option<String>,
    /// How many per-repo items the log has.
    pub repo_count: i64,
    /// How many of them are already undone.
    pub undone_count: i64,
}

/// One per-repo ref snapshot of a logged operation.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationLogItem {
    pub id: i64,
    pub log_id: i64,
    pub repo_path: String,
    /// Short branch name (e.g. "main"); empty when HEAD was detached.
    pub ref_name: String,
    pub before_oid: String,
    /// Tip after the operation; None when unknown (async batch ops) or the
    /// ref ceased to exist (branch delete).
    pub after_oid: Option<String>,
    /// Op-specific extra (e.g. "mode:hard" for reset, "onto:x" for rebase).
    pub detail: Option<String>,
    pub undone_at: Option<String>,
}

/// Full detail of one logged operation including all per-repo items.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationLogDetail {
    pub id: i64,
    pub workspace_id: Option<i64>,
    pub op_type: String,
    pub summary: String,
    pub created_at: String,
    pub undone_at: Option<String>,
    pub items: Vec<OperationLogItem>,
}

/// One repo's undo plan row for the §46 confirmation dialog.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UndoPreviewItem {
    pub item_id: i64,
    pub repo_path: String,
    pub repo_name: String,
    /// Human-readable reverse action, e.g. "重建分支 'feature' → a1b2c3d".
    pub action: String,
    /// Whether the reverse op can run safely right now.
    pub ok: bool,
    /// Safety-check detail (why not ok, or a note); empty when ok.
    pub message: String,
    /// Already undone (reported for completeness; skipped on execution).
    pub undone: bool,
}

/// Per-repo outcome of an undo run.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UndoItemResult {
    pub item_id: i64,
    pub repo_path: String,
    pub repo_name: String,
    pub success: bool,
    pub message: String,
}

/// Aggregate outcome of an undo run over one operation log.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UndoOutcome {
    pub log_id: i64,
    /// True when every item of the log is undone (log marked undone).
    pub fully_undone: bool,
    pub results: Vec<UndoItemResult>,
}

/// One per-repo snapshot row to insert together with a new operation log.
#[derive(Debug, Clone)]
pub struct NewOperationLogItem {
    pub repo_path: String,
    /// Short branch name; empty when HEAD was detached.
    pub ref_name: String,
    pub before_oid: String,
    pub after_oid: Option<String>,
    pub detail: Option<String>,
}
