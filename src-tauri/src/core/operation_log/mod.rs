//! Unified operation log + undo (T-34): the third Safety First layer
//! (Roadmap §46/§47) next to pre-op confirmation and the reflog — record
//! what a high-risk operation changed, then undo it with the reverse op.
//!
//! Initial scope (per task spec): Checkout All / Delete Branch All (T-20
//! batch branch ops), Reset, Rebase. Before the op runs, a per-repo ref
//! snapshot is captured (ref name + tip oid — pure data, never libgit2
//! handles, global constraint §3); after the op succeeds, the log row plus
//! all per-repo items are written in one transaction on the single-writer
//! connection (T-03), so failed ops leave no fake records. Undo applies the
//! reverse operation per repo with local libgit2 only (Offline First — no
//! network), is gated by a §46 Dangerous-level confirmation in the UI, and
//! refuses any repo whose state has moved on since the operation.
//!
//! Layout: `model` (DTOs + op_type vocabulary), `record` (snapshots + log
//! writes), `query` (paged listing + detail), `undo_plan` (reverse-action
//! planning + read-only preview), `undo_execute` (execution + result
//! write-back). This module is the facade — callers go through the
//! re-exports below.

mod model;
mod query;
mod record;
mod undo_execute;
mod undo_plan;

pub use model::{
    NewOperationLogItem, OperationLogDetail, OperationLogItem, OperationLogPage, OperationLogSummary, UndoItemResult,
    UndoOutcome, UndoPreviewItem, OP_AI_COMMIT, OP_CHECKOUT_ALL, OP_CONFLICT_RESOLUTION, OP_DELETE_BRANCH_ALL,
    OP_REBASE, OP_RESET,
};
pub(crate) use query::{get_operation_log, query_operation_logs, LogFilter};
pub use record::{record_operation_best_effort, snapshot_branch, snapshot_head};
pub(crate) use undo_execute::persist_undo_results;
pub use undo_execute::run_undo;
pub use undo_plan::preview_undo;

#[cfg(test)]
mod tests;
