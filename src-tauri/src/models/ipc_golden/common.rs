//! Common domain (repository/workspace/group models, AI, error response, manifest).
//! Split from `models/ipc_golden_tests.rs` (B-01); merged in the parent module.

use crate::commands::ai;
use crate::core::manifest;
use crate::error::AppError;
use crate::models::{group, repository, workspace};
use serde_json::{json, Map, Value};

/// Domain portion of the IPC golden samples; merged into `super::samples()`.
pub(super) fn samples(m: &mut Map<String, Value>) {
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
            conflicted: 1,
            has_remote: true,
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
                conflicted: 0,
                has_remote: false,
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

    // error.rs — AppError serializes as the structured ErrorResponse.
    m.insert(
        "ErrorResponse".into(),
        json!(AppError::NotFound("thing".into())),
    );
    m.insert(
        "DependencyResolveFailedError".into(),
        json!(AppError::DependencyResolve(
            "missing effective model".into()
        )),
    );
    m.insert(
        "SourceMappingFailedError".into(),
        json!(AppError::SourceMapping("ambiguous workspace source".into())),
    );
    m.insert(
        "ProjectNotFoundError".into(),
        json!(AppError::ProjectNotFound("missing Maven module".into())),
    );

    // core/manifest.rs (T-33)
    m.insert(
        "ManifestRepo".into(),
        json!(manifest::ManifestRepo {
            path: "apps/web".into(),
            name: "web".into(),
            remote_url: Some("https://example.com/org/web.git".into()),
            default_branch: Some("main".into()),
            group: Some("前端".into()),
            tags: vec!["vue".into()],
        }),
    );
    m.insert(
        "WorkspaceManifest".into(),
        json!(manifest::WorkspaceManifest {
            version: 1,
            name: "ws".into(),
            exported_at: "2026-01-01T00:00:00Z".into(),
            repositories: vec![manifest::ManifestRepo {
                path: "apps/web".into(),
                name: "web".into(),
                remote_url: Some("https://example.com/org/web.git".into()),
                default_branch: Some("main".into()),
                group: Some("前端".into()),
                tags: vec!["vue".into()],
            }],
        }),
    );
    m.insert(
        "ClonePlanItem".into(),
        json!(manifest::ClonePlanItem {
            path: "apps/web".into(),
            name: "web".into(),
            remote_url: Some("https://example.com/org/web.git".into()),
            default_branch: Some("main".into()),
            group: Some("前端".into()),
            tags: vec!["vue".into()],
            dest_path: "/ws/apps/web".into(),
            action: manifest::CloneAction::Clone,
        }),
    );
    m.insert(
        "ClonePlan".into(),
        json!(manifest::ClonePlan {
            workspace_root: "/ws".into(),
            to_clone: 1,
            skip_existing: 0,
            no_url: 0,
            items: vec![manifest::ClonePlanItem {
                path: "apps/web".into(),
                name: "web".into(),
                remote_url: Some("https://example.com/org/web.git".into()),
                default_branch: Some("main".into()),
                group: Some("前端".into()),
                tags: vec!["vue".into()],
                dest_path: "/ws/apps/web".into(),
                action: manifest::CloneAction::Clone,
            }],
        }),
    );
}

/// Domain portion of `TS_TYPE_MAP`; merged in the parent module.
pub(super) const TS_TYPE_MAP: &[(&str, &str, &str)] = &[
    ("Repository", "types/repository.ts", "Repository"),
    ("RepoStatus", "types/repository.ts", "RepoStatus"),
    (
        "RepositoryWithStatus",
        "types/repository.ts",
        "RepositoryWithStatus",
    ),
    ("ScanProgress", "types/events.ts", "ScanProgress"),
    ("RepoStatusUpdate", "types/events.ts", "RepoStatusUpdate"),
    // T-33 manifest
    ("ManifestRepo", "types/manifest.ts", "ManifestRepo"),
    ("WorkspaceManifest", "types/manifest.ts", "WorkspaceManifest"),
    ("ClonePlanItem", "types/manifest.ts", "ClonePlanItem"),
    ("ClonePlan", "types/manifest.ts", "ClonePlan"),
];
