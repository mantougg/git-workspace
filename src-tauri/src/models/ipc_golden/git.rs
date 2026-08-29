//! Git domain (diff, graph, branch, stash, merge/rebase, conflict, reflog, history, health, worktree, workspace stash, change set).
//! Split from `models/ipc_golden_tests.rs` (B-01); merged in the parent module.

use crate::commands::{batch as batch_cmd, diff as diff_cmd, git_ops};
use crate::core::{
    branch as branch_core, change_set, conflict as conflict_core, diff, graph,
    health as health_core, history as history_core, merge as merge_core, rebase as rebase_core,
    reflog as reflog_core, stash as stash_core, workspace_stash, worktree as worktree_core,
};
use crate::models::commit;
use serde_json::{json, Map, Value};

/// Domain portion of the IPC golden samples; merged into `super::samples()`.
pub(super) fn samples(m: &mut Map<String, Value>) {
    // core/diff.rs
    m.insert(
        "FileDiff".into(),
        json!(diff::FileDiff {
            old_path: "a.rs".into(),
            new_path: "a.rs".into(),
            status: "modified".into(),
            hunks: vec![],
        }),
    );
    m.insert(
        "Hunk".into(),
        json!(diff::Hunk {
            old_start: 1,
            old_lines: 2,
            new_start: 3,
            new_lines: 4,
            lines: vec![],
        }),
    );
    m.insert(
        "DiffLine".into(),
        json!(diff::DiffLine {
            line_type: "add".into(),
            content: "let x = 1;".into(),
            old_line: None,
            new_line: Some(3),
        }),
    );

    // core/graph.rs
    m.insert(
        "CommitInfo".into(),
        json!(graph::CommitInfo {
            oid: "abc123".into(),
            short_oid: "abc123".into(),
            message: "msg".into(),
            author: "A U Thor".into(),
            email: "a@example.com".into(),
            time: "2026-01-01T00:00:00Z".into(),
            parents: vec!["def456".into()],
            refs: vec!["main".into()],
        }),
    );
    m.insert(
        "BranchInfo".into(),
        json!(graph::BranchInfo {
            name: "main".into(),
            is_remote: false,
            is_current: true,
            last_commit_oid: "abc123".into(),
            last_commit_message: "msg".into(),
        }),
    );

    // core/branch.rs (T-09)
    m.insert(
        "BranchEntry".into(),
        json!(branch_core::BranchEntry {
            name: "main".into(),
            is_current: true,
            last_commit_oid: "abc123".into(),
            last_commit_message: "msg".into(),
            upstream: Some("origin/main".into()),
            ahead: 1,
            behind: 2,
        }),
    );
    m.insert(
        "RemoteBranchEntry".into(),
        json!(branch_core::RemoteBranchEntry {
            name: "origin/main".into(),
            last_commit_oid: "abc123".into(),
            last_commit_message: "msg".into(),
        }),
    );
    m.insert(
        "TagEntry".into(),
        json!(branch_core::TagEntry {
            name: "v1.0".into(),
            target_oid: "abc123".into(),
            message: Some("release".into()),
        }),
    );
    m.insert(
        "BranchOverview".into(),
        json!(branch_core::BranchOverview {
            current: Some("main".into()),
            locals: vec![],
            remotes: vec![],
            tags: vec![],
        }),
    );
    m.insert(
        "CompareResult".into(),
        json!(branch_core::CompareResult {
            base: "main".into(),
            other: "feature".into(),
            ahead: vec![],
            behind: vec![],
            files: vec![],
        }),
    );

    // commands/diff.rs (TS name: DiffOptions)
    m.insert(
        "DiffOptionsParam".into(),
        json!(diff_cmd::DiffOptionsParam {
            ignore_whitespace: true,
            ignore_whitespace_eol: false,
            ignore_case: true,
        }),
    );

    // commands/git_ops.rs
    m.insert(
        "CommitRequest".into(),
        json!(git_ops::CommitRequest {
            repo_path: "/ws/repo".into(),
            repo_name: "repo".into(),
            message: "msg".into(),
            files: vec!["a.rs".into()],
            amend: false,
            no_edit: false,
            index_only: false,
            then_push: false,
            allow_unsafe: false,
        }),
    );
    m.insert(
        "CommitScanFinding".into(),
        json!(commit::CommitScanFinding {
            path: "a.rs".into(),
            kind: "secret".into(),
            detail: "疑似 Secret（AWS Key）".into(),
        }),
    );
    m.insert(
        "CommitIdentity".into(),
        json!(commit::CommitIdentity {
            name: "alice".into(),
            email: "alice@example.com".into(),
            source: "repo".into(),
        }),
    );
    m.insert(
        "WorktreeInfo".into(),
        json!(worktree_core::WorktreeInfo {
            name: "feature-x".into(),
            path: "/ws/repo-wt".into(),
            branch: Some("feature/x".into()),
            is_main: false,
            is_locked: false,
            is_dirty: true,
        }),
    );
    m.insert(
        "DryRunItem".into(),
        json!(batch_cmd::DryRunItem {
            repo_path: "/ws/repo".into(),
            repo_name: "repo".into(),
            category: "fast_forward".into(),
            ahead: 1,
            behind: 2,
            detail: "可快进 2 个提交".into(),
        }),
    );
    m.insert(
        "AddRequest".into(),
        json!(git_ops::AddRequest {
            repo_path: "/ws/repo".into(),
            repo_name: "repo".into(),
            files: vec!["a.rs".into()],
        }),
    );
    m.insert(
        "RestoreRequest".into(),
        json!(git_ops::RestoreRequest {
            repo_path: "/ws/repo".into(),
            repo_name: "repo".into(),
            files: vec!["a.rs".into()],
        }),
    );

    // core/stash.rs (T-10)
    m.insert(
        "StashEntry".into(),
        json!(stash_core::StashEntry {
            index: 0,
            oid: "abc123".into(),
            message: "On master: work".into(),
            time: "2026-01-01 00:00:00 +0000".into(),
        }),
    );

    // core/merge.rs + core/rebase.rs (T-15)
    m.insert(
        "MergeOutcome".into(),
        json!([
            merge_core::MergeOutcome::UpToDate,
            merge_core::MergeOutcome::FastForward {
                to: "abc123".into(),
            },
            merge_core::MergeOutcome::Merged {
                commit_oid: "abc123".into(),
            },
            merge_core::MergeOutcome::Squashed,
            merge_core::MergeOutcome::Conflict {
                files: vec!["a.rs".into()],
                base_oid: Some("def456".into()),
            },
        ]),
    );
    m.insert(
        "RebaseOp".into(),
        json!(rebase_core::RebaseOp {
            action: "pick".into(),
            oid: "abc123".into(),
            message: Some("msg".into()),
            subject: "subject".into(),
        }),
    );
    m.insert(
        "RebaseState".into(),
        json!(rebase_core::RebaseState {
            original_head: "abc123".into(),
            onto: "main".into(),
            ops: vec![],
            position: 0,
            prev_commit: "def456".into(),
        }),
    );
    m.insert(
        "RebaseOutcome".into(),
        json!([
            rebase_core::RebaseOutcome::Success { rewritten: 3 },
            rebase_core::RebaseOutcome::Conflict {
                files: vec!["a.rs".into()],
                position: 1,
                total: 3,
                current: "abc123".into(),
            },
        ]),
    );

    // core/conflict.rs (T-16)
    m.insert(
        "ConflictFile".into(),
        json!(conflict_core::ConflictFile {
            path: "a.rs".into(),
            conflict_type: "both-modified".into(),
        }),
    );
    m.insert(
        "OperationState".into(),
        json!(conflict_core::OperationState {
            merge: true,
            cherry_pick: false,
            revert: false,
            rebase: None,
            conflicts: vec![],
        }),
    );
    m.insert(
        "ConflictContent".into(),
        json!(conflict_core::ConflictContent {
            base: Some("base".into()),
            ours: Some("ours".into()),
            theirs: Some("theirs".into()),
            worktree: Some("<<<<<<< x".into()),
            truncated: false,
        }),
    );

    // core/reflog.rs (T-14)
    m.insert(
        "ReflogEntry".into(),
        json!(reflog_core::ReflogEntry {
            index: 0,
            selector: "HEAD@{0}".into(),
            old_oid: "abc123".into(),
            new_oid: "def456".into(),
            summary: "commit: msg".into(),
            commit_message: "msg".into(),
            time: "2026-01-01 00:00:00 +0000".into(),
        }),
    );

    // core/history.rs (T-13)
    m.insert(
        "PickOutcome".into(),
        json!([
            history_core::PickOutcome::Success { picked: 2 },
            history_core::PickOutcome::Conflict {
                files: vec!["a.rs".into()],
                current: "abc123".into(),
                done: 1,
                total: 2,
                base_oid: Some("def456".into()),
            },
        ]),
    );
    m.insert(
        "ResetResult".into(),
        json!(history_core::ResetResult {
            previous_head: Some("abc123".into()),
            target: "def456".into(),
            mode: "hard".into(),
        }),
    );

    // core/health.rs (T-19)
    m.insert(
        "HealthWeights".into(),
        json!(health_core::HealthWeights::default()),
    );
    m.insert(
        "RepoHealth".into(),
        json!(health_core::RepoHealth {
            repo_path: "/ws/repo".into(),
            repo_name: "repo".into(),
            branch: "main".into(),
            anomalies: vec!["dirty".into(), "conflict".into()],
            score: 60,
        }),
    );
    m.insert(
        "WorkspaceHealth".into(),
        json!(health_core::WorkspaceHealth {
            score: 91,
            total: 2,
            anomalous: 1,
            repos: vec![],
            weights: health_core::HealthWeights::default(),
        }),
    );
    m.insert(
        "RepoHealthExtra".into(),
        json!(health_core::RepoHealthExtra {
            repo_path: "/ws/repo".into(),
            large_files: 1,
            largest_file_bytes: 11 * 1024 * 1024,
            lfs_error: true,
            submodule_error: false,
        }),
    );

    // core/workspace_stash.rs (T-21)
    m.insert(
        "WorkspaceStashRepoOutcome".into(),
        json!(workspace_stash::WorkspaceStashRepoOutcome {
            repo_path: "/ws/repo".into(),
            repo_name: "repo".into(),
            status: "stashed".into(),
            stash_oid: Some("abc123".into()),
            detail: String::new(),
        }),
    );
    m.insert(
        "SaveWorkspaceStashResult".into(),
        json!(workspace_stash::SaveWorkspaceStashResult {
            id: Some(1),
            name: "Workspace Stash #1".into(),
            items: vec![workspace_stash::WorkspaceStashRepoOutcome {
                repo_path: "/ws/repo".into(),
                repo_name: "repo".into(),
                status: "stashed".into(),
                stash_oid: Some("abc123".into()),
                detail: String::new(),
            }],
        }),
    );
    m.insert(
        "WorkspaceStashSummary".into(),
        json!(workspace_stash::WorkspaceStashSummary {
            id: 1,
            name: "Workspace Stash #1".into(),
            message: Some("sprint work".into()),
            created_at: "2026-01-01T00:00:00Z".into(),
            repo_count: 2,
        }),
    );
    m.insert(
        "WorkspaceStashItemEntry".into(),
        json!(workspace_stash::WorkspaceStashItemEntry {
            repo_path: "/ws/repo".into(),
            stash_oid: "abc123".into(),
            stash_index: 0,
            branch: "main".into(),
        }),
    );
    m.insert(
        "WorkspaceStashCheckItem".into(),
        json!(workspace_stash::WorkspaceStashCheckItem {
            repo_path: "/ws/repo".into(),
            repo_name: "repo".into(),
            branch: "main".into(),
            current_branch: Some("main".into()),
            status: "ok".into(),
            detail: String::new(),
        }),
    );

    // core/change_set.rs (T-22)
    m.insert(
        "ChangeSet".into(),
        json!(change_set::ChangeSet {
            id: 1,
            workspace_id: 2,
            name: "Feature: AI Review".into(),
            description: Some("desc".into()),
            created_at: "2026-01-01T00:00:00Z".into(),
            updated_at: "2026-01-01T00:00:00Z".into(),
        }),
    );
    m.insert(
        "ChangeSetRepo".into(),
        json!(change_set::ChangeSetRepo {
            change_set_id: 1,
            repo_id: 3,
            repo_path: "/ws/repo".into(),
            repo_name: "repo".into(),
            relative_path: "repo".into(),
            target_branch: Some("feature/ai".into()),
        }),
    );
    m.insert(
        "ChangeSetRepoInput".into(),
        json!(change_set::ChangeSetRepoInput {
            repo_id: 3,
            target_branch: Some("feature/ai".into()),
        }),
    );
    m.insert(
        "CreateChangeSetRequest".into(),
        json!(change_set::CreateChangeSetRequest {
            workspace_id: 2,
            name: "Feature: AI Review".into(),
            description: Some("desc".into()),
            repos: vec![change_set::ChangeSetRepoInput {
                repo_id: 3,
                target_branch: Some("feature/ai".into()),
            }],
        }),
    );
    m.insert(
        "UpdateChangeSetRequest".into(),
        json!(change_set::UpdateChangeSetRequest {
            id: 1,
            name: Some("renamed".into()),
            description: Some("new desc".into()),
        }),
    );
    m.insert(
        "ChangeSetRepoSummary".into(),
        json!(change_set::ChangeSetRepoSummary {
            repo: change_set::ChangeSetRepo {
                change_set_id: 1,
                repo_id: 3,
                repo_path: "/ws/repo".into(),
                repo_name: "repo".into(),
                relative_path: "repo".into(),
                target_branch: Some("feature/ai".into()),
            },
            current_branch: Some("feature/ai".into()),
            ahead: 1,
            behind: 2,
            files: 3,
            added: 4,
            deleted: 5,
            error: None,
        }),
    );
    m.insert(
        "ChangeSetSummary".into(),
        json!(change_set::ChangeSetSummary {
            change_set: change_set::ChangeSet {
                id: 1,
                workspace_id: 2,
                name: "Feature: AI Review".into(),
                description: Some("desc".into()),
                created_at: "2026-01-01T00:00:00Z".into(),
                updated_at: "2026-01-01T00:00:00Z".into(),
            },
            repositories: 1,
            files: 3,
            added: 4,
            deleted: 5,
            commits: 1,
            repos: vec![change_set::ChangeSetRepoSummary {
                repo: change_set::ChangeSetRepo {
                    change_set_id: 1,
                    repo_id: 3,
                    repo_path: "/ws/repo".into(),
                    repo_name: "repo".into(),
                    relative_path: "repo".into(),
                    target_branch: Some("feature/ai".into()),
                },
                current_branch: Some("feature/ai".into()),
                ahead: 1,
                behind: 2,
                files: 3,
                added: 4,
                deleted: 5,
                error: None,
            }],
        }),
    );
}

/// Domain portion of `TS_TYPE_MAP`; merged in the parent module.
pub(super) const TS_TYPE_MAP: &[(&str, &str, &str)] = &[
    ("FileChange", "types/changes.ts", "FileChange"),
    ("RepoChanges", "types/changes.ts", "RepoChanges"),
    // T-21 workspace stash
    (
        "WorkspaceStashRepoOutcome",
        "types/workspaceStash.ts",
        "WorkspaceStashRepoOutcome",
    ),
    (
        "SaveWorkspaceStashResult",
        "types/workspaceStash.ts",
        "SaveWorkspaceStashResult",
    ),
    (
        "WorkspaceStashSummary",
        "types/workspaceStash.ts",
        "WorkspaceStashSummary",
    ),
    (
        "WorkspaceStashItemEntry",
        "types/workspaceStash.ts",
        "WorkspaceStashItemEntry",
    ),
    (
        "WorkspaceStashCheckItem",
        "types/workspaceStash.ts",
        "WorkspaceStashCheckItem",
    ),
    // T-22 change set
    ("ChangeSet", "types/changeSet.ts", "ChangeSet"),
    ("ChangeSetRepo", "types/changeSet.ts", "ChangeSetRepo"),
    (
        "ChangeSetRepoInput",
        "types/changeSet.ts",
        "ChangeSetRepoInput",
    ),
    (
        "CreateChangeSetRequest",
        "types/changeSet.ts",
        "CreateChangeSetRequest",
    ),
    (
        "UpdateChangeSetRequest",
        "types/changeSet.ts",
        "UpdateChangeSetRequest",
    ),
    (
        "ChangeSetRepoSummary",
        "types/changeSet.ts",
        "ChangeSetRepoSummary",
    ),
    ("ChangeSetSummary", "types/changeSet.ts", "ChangeSetSummary"),
];
