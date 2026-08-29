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

    // ai/provider.rs + ai/model.rs + commands/ai.rs（AI-01，设计文档 §6 / §12.2）
    m.insert(
        "AiProvider".into(),
        json!(crate::ai::AiProvider {
            id: "p1".into(),
            name: "Team OpenAI".into(),
            kind: crate::ai::ProviderKind::OpenaiCompatible,
            base_url: "https://api.openai.com/v1".into(),
            credential_ref: Some("ai-provider:p1".into()),
            has_credential: true,
            session_only_credential: false,
            enabled: true,
            network_policy: crate::ai::NetworkPolicy::OnlineOnly,
            created_at: "2026-01-01T00:00:00Z".into(),
            updated_at: "2026-01-01T00:00:00Z".into(),
        }),
    );
    m.insert(
        "SaveAiProviderRequest".into(),
        json!(crate::ai::SaveAiProviderRequest {
            id: Some("p1".into()),
            name: "Team OpenAI".into(),
            kind: crate::ai::ProviderKind::OpenaiCompatible,
            base_url: "https://api.openai.com/v1".into(),
            enabled: true,
            network_policy: crate::ai::NetworkPolicy::OnlineOnly,
        }),
    );
    m.insert(
        "AiProviderTestResult".into(),
        json!(crate::ai::AiProviderTestResult {
            success: true,
            message: "连接成功，发现 2 个模型".into(),
            models: vec!["gpt-4o".into(), "gpt-4o-mini".into()],
            latency_ms: 120,
        }),
    );
    m.insert(
        "AiModel".into(),
        json!(crate::ai::AiModel {
            provider_id: "p1".into(),
            id: "gpt-4o-mini".into(),
            display_name: "GPT-4o mini".into(),
            capabilities: vec![
                crate::ai::ModelCapability::Chat,
                crate::ai::ModelCapability::StructuredOutput,
            ],
            max_context_tokens: 128000,
            defaults: crate::ai::AiModelDefaults {
                temperature: Some(0.2),
            },
            enabled: true,
            created_at: "2026-01-01T00:00:00Z".into(),
            updated_at: "2026-01-01T00:00:00Z".into(),
        }),
    );
    m.insert(
        "AiModelDefaults".into(),
        json!(crate::ai::AiModelDefaults {
            temperature: Some(0.2),
        }),
    );
    m.insert(
        "SaveAiModelRequest".into(),
        json!(crate::ai::SaveAiModelRequest {
            provider_id: "p1".into(),
            id: "gpt-4o-mini".into(),
            display_name: "GPT-4o mini".into(),
            capabilities: vec![crate::ai::ModelCapability::Chat],
            max_context_tokens: 128000,
            defaults: crate::ai::AiModelDefaults {
                temperature: Some(0.2),
            },
            enabled: true,
        }),
    );
    m.insert(
        "AiTaskDefault".into(),
        json!(crate::ai::AiTaskDefault {
            task_kind: crate::ai::AiTaskKind::GitReview,
            workspace_id: Some(1),
            provider_id: "p1".into(),
            model_id: "gpt-4o-mini".into(),
            updated_at: "2026-01-01T00:00:00Z".into(),
        }),
    );
    m.insert(
        "AiSettingsSummary".into(),
        json!(ai::AiSettingsSummary {
            provider_count: 1,
            enabled_provider_count: 1,
            model_count: 2,
            enabled_model_count: 2,
            task_defaults: vec![crate::ai::AiTaskDefault {
                task_kind: crate::ai::AiTaskKind::Chat,
                workspace_id: None,
                provider_id: "p1".into(),
                model_id: "gpt-4o-mini".into(),
                updated_at: "2026-01-01T00:00:00Z".into(),
            }],
            os_credential_store_available: true,
            session_credential_count: 0,
            legacy_review_count: 3,
            legacy_task_count: 4,
        }),
    );
    m.insert(
        "AiCredentialStatus".into(),
        json!(ai::AiCredentialStatus {
            provider_id: "p1".into(),
            has_credential: true,
            session_only: false,
            os_store_available: true,
        }),
    );
    // AI 结构化错误（§17）：details 内含 suggestedActions。
    m.insert(
        "AiNotConfiguredError".into(),
        json!(AppError::Ai(crate::ai::AiError::NotConfigured {
            message: "没有可用的 AI 模型".into(),
        })),
    );
    m.insert(
        "AiCredentialUnavailableError".into(),
        json!(AppError::Ai(crate::ai::AiError::CredentialUnavailable {
            message: "OS 凭证存储不可用".into(),
        })),
    );
    m.insert(
        "AiModelNotFoundError".into(),
        json!(AppError::Ai(crate::ai::AiError::ModelNotFound {
            provider_id: "p1".into(),
            model_id: "gone".into(),
        })),
    );
    m.insert(
        "AiModelCapabilityMismatchError".into(),
        json!(AppError::Ai(crate::ai::AiError::ModelCapabilityMismatch {
            provider_id: "p1".into(),
            model_id: "m1".into(),
            capability: "structuredOutput".into(),
        })),
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
    // AI（types/ai.ts：本文件注册的每个 interface 都必须有 Rust 样本）
    ("ReviewResult", "types/ai.ts", "ReviewResult"),
    ("ReviewIssue", "types/ai.ts", "ReviewIssue"),
    ("SearchResult", "types/ai.ts", "SearchResult"),
    ("AiProvider", "types/ai.ts", "AiProvider"),
    ("SaveAiProviderRequest", "types/ai.ts", "SaveAiProviderRequest"),
    ("AiProviderTestResult", "types/ai.ts", "AiProviderTestResult"),
    ("AiModel", "types/ai.ts", "AiModel"),
    ("AiModelDefaults", "types/ai.ts", "AiModelDefaults"),
    ("SaveAiModelRequest", "types/ai.ts", "SaveAiModelRequest"),
    ("AiTaskDefault", "types/ai.ts", "AiTaskDefault"),
    ("AiSettingsSummary", "types/ai.ts", "AiSettingsSummary"),
    ("AiCredentialStatus", "types/ai.ts", "AiCredentialStatus"),
];
