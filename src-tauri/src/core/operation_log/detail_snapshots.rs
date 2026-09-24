//! GF-16: op-specific snapshots carried in the operation-log item `detail`.
//!
//! The undo model stores a per-repo ref snapshot (before/after oid) in
//! dedicated columns, but some reversible ops need a little more state:
//! a removed worktree's path/branch, or the stash stack behind a drop/clear.
//! Those live in `detail` behind an explicit prefix so older rows (plain
//! strings like `mode:hard`) keep parsing as free text, and the UI can
//! translate the known prefixes for display.

use serde::{Deserialize, Serialize};

const WORKTREE_PREFIX: &str = "wt:";
const STASH_PREFIX: &str = "stashstack:";

/// Snapshot of a removed linked worktree (`wt:` + JSON).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorktreeSnapshot {
    /// Worktree name under `.git/worktrees/`.
    pub name: String,
    /// Working directory of the worktree.
    pub path: String,
    /// Branch checked out in the worktree (None = detached HEAD).
    pub branch: Option<String>,
    /// Detached-HEAD commit oid (None when `branch` is set).
    pub oid: Option<String>,
}

/// One recorded stash-stack entry: (stash commit oid, reflog message),
/// newest first — exactly the shape `stash_foreach` yields.
pub type StashSnapshot = Vec<(String, String)>;

pub fn encode_worktree_snapshot(snap: &WorktreeSnapshot) -> String {
    format!("{WORKTREE_PREFIX}{}", serde_json::to_string(snap).unwrap_or_default())
}

pub fn decode_worktree_snapshot(detail: Option<&str>) -> Option<WorktreeSnapshot> {
    let raw = detail?.strip_prefix(WORKTREE_PREFIX)?;
    serde_json::from_str(raw).ok()
}

pub fn encode_stash_snapshot(entries: &[(String, String)]) -> String {
    format!("{STASH_PREFIX}{}", serde_json::to_string(entries).unwrap_or_default())
}

pub fn decode_stash_snapshot(detail: Option<&str>) -> StashSnapshot {
    match detail.and_then(|d| d.strip_prefix(STASH_PREFIX)) {
        Some(raw) => serde_json::from_str(raw).unwrap_or_default(),
        None => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worktree_snapshot_roundtrip() {
        let snap = WorktreeSnapshot {
            name: "wt-a".into(),
            path: "D:/repos/wt-a".into(),
            branch: Some("feature".into()),
            oid: None,
        };
        let encoded = encode_worktree_snapshot(&snap);
        assert!(encoded.starts_with("wt:"));
        let back = decode_worktree_snapshot(Some(&encoded)).expect("decode");
        assert_eq!(back.name, "wt-a");
        assert_eq!(back.branch.as_deref(), Some("feature"));

        // Detached form.
        let snap = WorktreeSnapshot {
            name: "wt-b".into(),
            path: "D:/repos/wt-b".into(),
            branch: None,
            oid: Some("a".repeat(40)),
        };
        let back = decode_worktree_snapshot(Some(&encode_worktree_snapshot(&snap))).expect("decode");
        assert_eq!(back.branch, None);
        assert_eq!(back.oid.as_deref(), Some("a".repeat(40).as_str()));

        // Unknown / legacy details decode to None, never panic.
        assert!(decode_worktree_snapshot(Some("mode:hard")).is_none());
        assert!(decode_worktree_snapshot(None).is_none());
        assert!(decode_worktree_snapshot(Some("wt:{broken")).is_none());
    }

    #[test]
    fn stash_snapshot_roundtrip() {
        let entries = vec![
            ("b".repeat(40), "WIP on main: second".to_string()),
            ("a".repeat(40), "WIP on main: first".to_string()),
        ];
        let encoded = encode_stash_snapshot(&entries);
        assert!(encoded.starts_with("stashstack:"));
        assert_eq!(decode_stash_snapshot(Some(&encoded)), entries);
        assert!(decode_stash_snapshot(Some("mode:hard")).is_empty());
        assert!(decode_stash_snapshot(None).is_empty());
        assert!(decode_stash_snapshot(Some("stashstack:not-json")).is_empty());
    }
}
