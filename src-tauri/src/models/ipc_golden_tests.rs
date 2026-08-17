//! IPC type single-source-of-truth tests (T-03, global constraint §6).
//!
//! Rust serde structs are the source of truth for every payload crossing IPC
//! (command args/returns and Tauri event payloads). Two tests guard against
//! drift between the Rust definitions and the hand-written TS types:
//!
//! 1. `golden_samples_match_snapshot` — serializes a representative sample of
//!    every IPC type and compares it against `golden/ipc_samples.json`.
//!    Regenerate after intentional changes with
//!    `GW_UPDATE_GOLDEN=1 cargo test ipc_golden` and review the git diff.
//! 2. `ts_types_match_rust_samples` — parses the TS type files and asserts
//!    each TS type's field set matches the keys of its Rust sample (and, for
//!    tagged enums, each union variant). Every exported type in the mapped
//!    files must be registered in `TS_TYPE_MAP`.
//!
//! ts-rs codegen remains a later evaluation item; until then this is the
//! automated backstop against `camelCase` renames and field add/remove drift.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::PathBuf;

use serde_json::{json, Map, Value};

use crate::commands::{ai, diff as diff_cmd, git_ops, logs};
use crate::core::{branch as branch_core, conflict as conflict_core, diff, graph, history as history_core, merge as merge_core, rebase as rebase_core, reflog as reflog_core, stash as stash_core, worktree as worktree_core};
use crate::error::AppError;
use crate::models::{commit, group, repository, task, workspace};

/// Representative sample of every IPC type, keyed by Rust type name.
/// Enum (tagged-union) types serialize as an array of all variants.
fn samples() -> Map<String, Value> {
    let mut m = Map::new();

    // models/repository.rs
    m.insert(
        "Repository".into(),
        json!(repository::Repository {
            id: Some(1),
            workspace_id: 2,
            path: "/ws/repo".into(),
            name: "repo".into(),
            relative_path: "repo".into(),
            is_favorite: true,
            tags: vec!["tag".into()],
            group_id: Some(3),
        }),
    );
    m.insert(
        "RepoStatus".into(),
        json!(repository::RepoStatus {
            branch: "main".into(),
            is_detached: false,
            ahead: 1,
            behind: 2,
            modified: 3,
            added: 4,
            deleted: 5,
            untracked: 6,
            staged: 7,
            is_clean: false,
        }),
    );
    m.insert(
        "RepositoryWithStatus".into(),
        json!(repository::RepositoryWithStatus {
            repository: repository::Repository {
                id: None,
                workspace_id: 2,
                path: "/ws/repo".into(),
                name: "repo".into(),
                relative_path: "repo".into(),
                is_favorite: false,
                tags: vec![],
                group_id: None,
            },
            status: None,
            last_error: Some("err".into()),
        }),
    );
    m.insert(
        "ScanProgress".into(),
        json!(repository::ScanProgress {
            workspace_id: 1,
            found: 5,
            current: 3,
            total: Some(10),
        }),
    );
    m.insert(
        "RepoStatusUpdate".into(),
        json!(repository::RepoStatusUpdate {
            repo_path: "/ws/repo".into(),
            status: repository::RepoStatus {
                branch: "main".into(),
                is_detached: false,
                ahead: 0,
                behind: 0,
                modified: 0,
                added: 0,
                deleted: 0,
                untracked: 0,
                staged: 0,
                is_clean: true,
            },
        }),
    );
    m.insert(
        "FileChange".into(),
        json!(repository::FileChange {
            path: "src/a.rs".into(),
            status: "modified".into(),
        }),
    );
    m.insert(
        "RepoChanges".into(),
        json!(repository::RepoChanges {
            repo_path: "/ws/repo".into(),
            repo_name: "repo".into(),
            relative_path: "repo".into(),
            branch: "main".into(),
            is_detached: false,
            ahead: 0,
            behind: 0,
            changes: vec![],
        }),
    );

    // models/task.rs
    m.insert(
        "TaskType".into(),
        json!([
            task::TaskType::Fetch,
            task::TaskType::Pull,
            task::TaskType::Push,
            task::TaskType::Commit {
                message: "msg".into(),
                files: vec!["a.rs".into()],
                amend: false,
                no_edit: false,
                index_only: false,
                then_push: false,
                allow_unsafe: false,
                author_name: Some("alice".into()),
                author_email: Some("alice@example.com".into()),
            },
        ]),
    );
    m.insert(
        "TaskStatus".into(),
        json!([
            task::TaskStatus::Queued,
            task::TaskStatus::Running { progress: 0.5 },
            task::TaskStatus::Success,
            task::TaskStatus::PartialSuccess {
                succeeded: 2,
                failed: 1,
            },
            task::TaskStatus::Failed {
                error: "boom".into(),
            },
            task::TaskStatus::Cancelled,
        ]),
    );
    m.insert(
        "Task".into(),
        json!(task::Task {
            id: "t-1".into(),
            task_type: task::TaskType::Fetch,
            repo_path: "/ws/repo".into(),
            repo_name: "repo".into(),
            status: task::TaskStatus::Queued,
            created_at: "2026-01-01T00:00:00Z".into(),
        }),
    );
    m.insert(
        "TaskRequest".into(),
        json!(task::TaskRequest {
            task_type: task::TaskType::Pull,
            repo_path: "/ws/repo".into(),
            repo_name: "repo".into(),
        }),
    );
    m.insert(
        "TaskProgress".into(),
        json!(task::TaskProgress {
            task_id: "t-1".into(),
            task_type: task::TaskType::Push,
            repo_path: "/ws/repo".into(),
            repo_name: "repo".into(),
            status: task::TaskStatus::Running { progress: 1.0 },
        }),
    );
    m.insert(
        "GitCommandResult".into(),
        json!(task::GitCommandResult {
            repo_name: "repo".into(),
            repo_path: "/ws/repo".into(),
            command: "git fetch origin".into(),
            success: true,
            output: "ok".into(),
        }),
    );

    // models/workspace.rs
    m.insert(
        "Workspace".into(),
        json!(workspace::Workspace {
            id: 1,
            name: "ws".into(),
            path: "/ws".into(),
            scan_depth: 5,
            created_at: "2026-01-01T00:00:00Z".into(),
            updated_at: "2026-01-01T00:00:00Z".into(),
        }),
    );
    m.insert(
        "CreateWorkspaceRequest".into(),
        json!(workspace::CreateWorkspaceRequest {
            name: "ws".into(),
            path: "/ws".into(),
            scan_depth: Some(5),
        }),
    );
    m.insert(
        "UpdateWorkspaceRequest".into(),
        json!(workspace::UpdateWorkspaceRequest {
            name: Some("ws2".into()),
            scan_depth: None,
        }),
    );

    // models/group.rs
    m.insert(
        "RepoGroup".into(),
        json!(group::RepoGroup {
            id: 1,
            workspace_id: 2,
            name: "g".into(),
            parent_id: Some(3),
            sort_order: 4,
        }),
    );
    m.insert(
        "CreateGroupRequest".into(),
        json!(group::CreateGroupRequest {
            workspace_id: 2,
            name: "g".into(),
            parent_id: None,
        }),
    );

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

    // commands/ai.rs
    m.insert(
        "ReviewResult".into(),
        json!(ai::ReviewResult {
            summary: "ok".into(),
            issues: vec![],
        }),
    );
    m.insert(
        "ReviewIssue".into(),
        json!(ai::ReviewIssue {
            severity: "high".into(),
            category: "bug".into(),
            file: "a.rs".into(),
            description: "desc".into(),
        }),
    );
    m.insert(
        "SearchResult".into(),
        json!(ai::SearchResult {
            repo_path: "/ws/repo".into(),
            file_path: "a.rs".into(),
            snippet: "let x".into(),
            rank: 0.5,
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

    // commands/logs.rs
    m.insert(
        "LogFileInfo".into(),
        json!(logs::LogFileInfo {
            name: "app.log".into(),
            path: "/logs/app.log".into(),
            size_bytes: 42,
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

    // error.rs — AppError serializes as the structured ErrorResponse.
    m.insert(
        "ErrorResponse".into(),
        json!(AppError::NotFound("thing".into())),
    );

    m
}

/// (golden key, TS file relative to the frontend `src/` dir, TS type name).
/// Every payload crossing IPC must be registered here.
const TS_TYPE_MAP: &[(&str, &str, &str)] = &[
    ("Repository", "types/repository.ts", "Repository"),
    ("RepoStatus", "types/repository.ts", "RepoStatus"),
    (
        "RepositoryWithStatus",
        "types/repository.ts",
        "RepositoryWithStatus",
    ),
    ("ScanProgress", "types/events.ts", "ScanProgress"),
    ("RepoStatusUpdate", "types/events.ts", "RepoStatusUpdate"),
    ("FileChange", "types/changes.ts", "FileChange"),
    ("RepoChanges", "types/changes.ts", "RepoChanges"),
    ("TaskType", "types/task.ts", "TaskType"),
    ("TaskStatus", "types/task.ts", "TaskStatus"),
    ("Task", "types/task.ts", "Task"),
    ("TaskRequest", "types/task.ts", "TaskRequest"),
    ("TaskProgress", "types/task.ts", "TaskProgress"),
    ("GitCommandResult", "types/task.ts", "GitCommandResult"),
    ("CommitRequest", "types/task.ts", "CommitRequest"),
    ("CommitScanFinding", "types/commit.ts", "CommitScanFinding"),
    ("CommitIdentity", "types/commit.ts", "CommitIdentity"),
    ("WorktreeInfo", "types/worktree.ts", "WorktreeInfo"),
    ("Workspace", "types/workspace.ts", "Workspace"),
    (
        "CreateWorkspaceRequest",
        "types/workspace.ts",
        "CreateWorkspaceRequest",
    ),
    (
        "UpdateWorkspaceRequest",
        "types/workspace.ts",
        "UpdateWorkspaceRequest",
    ),
    ("RepoGroup", "types/group.ts", "RepoGroup"),
    ("CreateGroupRequest", "types/group.ts", "CreateGroupRequest"),
    ("FileDiff", "types/git.ts", "FileDiff"),
    ("Hunk", "types/git.ts", "Hunk"),
    ("DiffLine", "types/git.ts", "DiffLine"),
    ("CommitInfo", "types/graph.ts", "CommitInfo"),
    ("BranchInfo", "types/graph.ts", "BranchInfo"),
    ("BranchEntry", "types/branch.ts", "BranchEntry"),
    (
        "RemoteBranchEntry",
        "types/branch.ts",
        "RemoteBranchEntry",
    ),
    ("TagEntry", "types/branch.ts", "TagEntry"),
    ("BranchOverview", "types/branch.ts", "BranchOverview"),
    ("CompareResult", "types/branch.ts", "CompareResult"),
    ("PickOutcome", "types/history.ts", "PickOutcome"),
    ("ResetResult", "types/history.ts", "ResetResult"),
    ("ReflogEntry", "types/reflog.ts", "ReflogEntry"),
    ("StashEntry", "types/stash.ts", "StashEntry"),
    ("MergeOutcome", "types/merge.ts", "MergeOutcome"),
    ("RebaseOp", "types/rebase.ts", "RebaseOp"),
    ("RebaseState", "types/rebase.ts", "RebaseState"),
    ("RebaseOutcome", "types/rebase.ts", "RebaseOutcome"),
    ("ConflictFile", "types/conflict.ts", "ConflictFile"),
    ("OperationState", "types/conflict.ts", "OperationState"),
    ("ConflictContent", "types/conflict.ts", "ConflictContent"),
    ("ReviewResult", "types/ai.ts", "ReviewResult"),
    ("ReviewIssue", "types/ai.ts", "ReviewIssue"),
    ("SearchResult", "types/ai.ts", "SearchResult"),
    ("DiffOptionsParam", "api/git.ts", "DiffOptions"),
    ("LogFileInfo", "api/logs.ts", "LogFileInfo"),
    ("AddRequest", "api/changes.ts", "AddRequest"),
    ("RestoreRequest", "api/changes.ts", "RestoreRequest"),
    ("ErrorResponse", "utils/error.ts", "ErrorResponse"),
];

fn golden_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("golden/ipc_samples.json")
}

fn frontend_src_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("src-tauri must have a parent dir")
        .join("src")
}

fn normalize(s: &str) -> String {
    s.replace("\r\n", "\n")
}

/// Snapshot test: serialized samples must match the committed golden file.
/// Regenerate with `GW_UPDATE_GOLDEN=1 cargo test ipc_golden`.
#[test]
fn golden_samples_match_snapshot() {
    let path = golden_path();
    let actual = serde_json::to_string_pretty(&Value::Object(samples())).unwrap() + "\n";

    if std::env::var("GW_UPDATE_GOLDEN").is_ok() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &actual).unwrap();
        return;
    }

    let expected = std::fs::read_to_string(&path).unwrap_or_else(|_| {
        panic!(
            "golden file missing at {}; create it with `GW_UPDATE_GOLDEN=1 cargo test ipc_golden`",
            path.display()
        )
    });
    assert_eq!(
        normalize(&expected),
        normalize(&actual),
        "IPC serialization drift vs golden/ipc_samples.json; if intentional, \
         regenerate with `GW_UPDATE_GOLDEN=1 cargo test ipc_golden` and review the git diff"
    );
}

#[derive(Default)]
struct TsFileTypes {
    /// interface name -> field names
    interfaces: HashMap<String, BTreeSet<String>>,
    /// tagged-union name -> (tag value -> field names, including `type`)
    unions: HashMap<String, BTreeMap<String, BTreeSet<String>>>,
}

/// Remove `/* ... */` blocks and `//` line comments so comment text cannot
/// produce false field matches. Not string-literal aware; sufficient for the
/// type-declaration files parsed here.
fn strip_ts_comments(content: &str) -> String {
    let block = regex::Regex::new(r"(?s)/\*.*?\*/").unwrap();
    let line = regex::Regex::new(r"//[^\n]*").unwrap();
    line.replace_all(&block.replace_all(content, ""), "")
        .to_string()
}

/// Parse the exported interfaces and tagged-union type aliases of a TS file.
/// Only handles the simple declaration shapes used under `src/` (one field
/// per line; union variants as object literals).
fn parse_ts_file(content: &str) -> TsFileTypes {
    let interface_start = regex::Regex::new(r"^export interface (\w+)\s*\{").unwrap();
    let union_start = regex::Regex::new(r"^export type (\w+)\s*=").unwrap();
    // Interface fields: one per line, anchored.
    let iface_field_re = regex::Regex::new(r"^\s*([A-Za-z_]\w*)\??\s*:").unwrap();
    // Union variant fields: multiple per line, unanchored.
    let variant_field_re = regex::Regex::new(r"([A-Za-z_]\w*)\??\s*:").unwrap();
    // Discriminant field: `type` (TaskType/TaskStatus) or `status` (PickOutcome).
    let tag_re = regex::Regex::new(r#"(?:type|status)\s*:\s*"([^"]+)""#).unwrap();

    let mut result = TsFileTypes::default();
    let stripped = strip_ts_comments(content);
    let mut lines = stripped.lines().peekable();

    while let Some(line) = lines.next() {
        let trimmed = line.trim_start();

        if let Some(cap) = interface_start.captures(trimmed) {
            let name = cap[1].to_string();
            let mut fields = BTreeSet::new();
            for body in lines.by_ref() {
                let b = body.trim();
                if b == "}" {
                    break;
                }
                if let Some(f) = iface_field_re.captures(b) {
                    fields.insert(f[1].to_string());
                }
            }
            result.interfaces.insert(name, fields);
            continue;
        }

        if let Some(cap) = union_start.captures(trimmed) {
            let name = cap[1].to_string();
            // Accumulate the union body until the line that terminates the
            // type alias: braces balanced and line ends with `;` (multi-line
            // variants have `;`-terminated field lines inside the braces).
            let mut body = String::new();
            let mut rest = trimmed[cap.get(0).unwrap().end()..].to_string();
            let mut depth = 0i32;
            loop {
                for ch in rest.chars() {
                    match ch {
                        '{' => depth += 1,
                        '}' => depth -= 1,
                        _ => {}
                    }
                }
                let done = depth <= 0 && rest.trim_end().ends_with(';');
                body.push_str(&rest);
                body.push('\n');
                if done {
                    break;
                }
                match lines.next() {
                    Some(next) => rest = next.to_string(),
                    None => break,
                }
            }
            // Each top-level `{ ... }` group is one variant.
            let mut variants = BTreeMap::new();
            let mut depth = 0i32;
            let mut current = String::new();
            for ch in body.chars() {
                match ch {
                    '{' => {
                        depth += 1;
                        if depth == 1 {
                            current.clear();
                            continue;
                        }
                    }
                    '}' => {
                        depth -= 1;
                        if depth == 0 {
                            let tag = tag_re
                                .captures(&current)
                                .map(|c| c[1].to_string())
                                .unwrap_or_default();
                            let fields: BTreeSet<String> = variant_field_re
                                .captures_iter(&current)
                                .map(|c| c[1].to_string())
                                .collect();
                            variants.insert(tag, fields);
                            continue;
                        }
                    }
                    _ => {}
                }
                if depth >= 1 {
                    current.push(ch);
                }
            }
            result.unions.insert(name, variants);
        }
    }

    result
}

fn rust_keys(sample: &Value) -> BTreeSet<String> {
    sample
        .as_object()
        .expect("struct sample must be a JSON object")
        .keys()
        .cloned()
        .collect()
}

/// Discriminant of a tagged-union sample variant: `type` (TaskType/TaskStatus)
/// or `status` (PickOutcome).
fn variant_tag(v: &Value) -> String {
    v["type"]
        .as_str()
        .or_else(|| v["status"].as_str())
        .expect("tagged-union variant must carry `type` or `status`")
        .to_string()
}

/// Alignment test: TS field sets must match the Rust sample keys exactly.
#[test]
fn ts_types_match_rust_samples() {
    let samples = Value::Object(samples());
    let root = frontend_src_dir();

    // Parse each mapped TS file once.
    let mut parsed: HashMap<&str, TsFileTypes> = HashMap::new();
    for (_, file, _) in TS_TYPE_MAP {
        if parsed.contains_key(file) {
            continue;
        }
        let path = root.join(file);
        let content = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("failed to read {}: {}", path.display(), e));
        parsed.insert(file, parse_ts_file(&content));
    }

    for (golden_key, file, ts_name) in TS_TYPE_MAP {
        let sample = samples
            .get(golden_key)
            .unwrap_or_else(|| panic!("no Rust sample registered for `{}`", golden_key));
        let types = &parsed[file];

        if let Value::Array(variants) = sample {
            // Tagged union: every Rust variant must have a TS counterpart
            // with the same tag and field set, and vice versa.
            let union = types.unions.get(*ts_name).unwrap_or_else(|| {
                panic!(
                    "TS union type `{}` not found in src/{} — drift or rename",
                    ts_name, file
                )
            });
            let rust_tags: BTreeSet<String> = variants.iter().map(variant_tag).collect();
            let ts_tags: BTreeSet<String> = union.keys().cloned().collect();
            assert_eq!(
                rust_tags, ts_tags,
                "`{}` variant tags drifted (src/{})",
                ts_name, file
            );
            for v in variants {
                let tag = variant_tag(v);
                assert_eq!(
                    rust_keys(v),
                    union[&tag],
                    "`{}` variant `{}` fields drifted (src/{})",
                    ts_name,
                    tag,
                    file
                );
            }
        } else {
            let fields = types.interfaces.get(*ts_name).unwrap_or_else(|| {
                panic!(
                    "TS interface `{}` not found in src/{} — drift or rename",
                    ts_name, file
                )
            });
            assert_eq!(
                &rust_keys(sample),
                fields,
                "`{}` (src/{}) fields drifted from Rust `{}`",
                ts_name,
                file,
                golden_key
            );
        }
    }

    // Reverse coverage: every exported type in the mapped files must be
    // registered in TS_TYPE_MAP, so new TS-only types cannot slip through.
    for (file, types) in &parsed {
        let mapped: BTreeSet<&str> = TS_TYPE_MAP
            .iter()
            .filter(|(_, f, _)| f == file)
            .map(|(_, _, ts)| *ts)
            .collect();
        for name in types.interfaces.keys().chain(types.unions.keys()) {
            assert!(
                mapped.contains(name.as_str()),
                "TS type `{}` in src/{} is not registered in TS_TYPE_MAP (ipc_golden_tests.rs)",
                name,
                file
            );
        }
    }
}
