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
            line: Some(42),
            description: "desc".into(),
        }),
    );
    m.insert(
        "CommitSuggestion".into(),
        json!({
            "title": "feat(ai): add git assistant scenarios",
            "body": ["Add structured Git Assistant outputs"],
            "type": "feat",
            "scope": "ai",
            "changedRepositories": ["/ws/repo"],
            "rationale": "The selected diff adds scenario handling."
        }),
    );
    m.insert(
        "CommitSummaryRepository".into(),
        json!({
            "path": "/ws/repo",
            "summary": "Adds AI scenario contracts.",
            "risk": "Review generated prompt changes."
        }),
    );
    m.insert(
        "CommitSummary".into(),
        json!({
            "summary": "One repository changes AI behavior.",
            "repositories": [{
                "path": "/ws/repo",
                "summary": "Adds AI scenario contracts.",
                "risk": "Review generated prompt changes."
            }],
            "risks": ["AI output quality needs user review."]
        }),
    );
    m.insert(
        "PrDescription".into(),
        json!({
            "title": "Add Git Assistant scenarios",
            "description": "Adds structured assistant results.",
            "summary": ["Adds preview-driven Git AI entry points."],
            "testing": ["cargo test", "pnpm build"],
            "risks": ["AI-derived guidance requires user review."]
        }),
    );
    m.insert(
        "ExplanationResult".into(),
        json!({
            "summary": "The commit introduces Git Assistant scenarios.",
            "details": ["It adds structured output schemas."],
            "riskNotes": ["Verify suggested changes before applying them."]
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
            api_type: crate::ai::ApiType::OpenaiChatCompletions,
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
            api_type: crate::ai::ApiType::OpenaiChatCompletions,
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
    // AI-02 Gateway（设计文档 §7 / §8.4 / §16.1）：请求模型 / 结果模型 /
    // 流式事件契约 / 状态快照。
    m.insert(
        "AiMessage".into(),
        json!(crate::ai::AiMessage {
            role: crate::ai::MessageRole::User,
            content: "请解释这段构建日志".into(),
        }),
    );
    m.insert(
        "ContextItem".into(),
        json!(crate::ai::ContextItem {
            kind: crate::ai::ContextKind::Log,
            source_id: "runtime/app:latest".into(),
            display_name: "应用启动日志（最近 200 行）".into(),
            char_count: 8192,
            estimated_tokens: 2048,
            redacted: true,
            truncated: false,
            excluded: false,
            exclusion_reason: None,
        }),
    );
    m.insert(
        "AiTokenUsage".into(),
        json!(crate::ai::AiTokenUsage {
            input_tokens: Some(1024),
            output_tokens: Some(256),
        }),
    );
    m.insert(
        "AiRequest".into(),
        json!(crate::ai::AiRequest {
            request_id: "req-1".into(),
            session_id: Some("sess-1".into()),
            task_kind: crate::ai::AiTaskKind::RuntimeDiagnostic,
            git_scenario: None,
            provider_id: None,
            model_id: None,
            system_instruction: "你是构建排障助手".into(),
            messages: vec![crate::ai::AiMessage {
                role: crate::ai::MessageRole::User,
                content: "启动失败，日志如下".into(),
            }],
            context_manifest: vec![crate::ai::ContextItem {
                kind: crate::ai::ContextKind::Log,
                source_id: "runtime/app:latest".into(),
                display_name: "应用启动日志（最近 200 行）".into(),
                char_count: 8192,
                estimated_tokens: 2048,
                redacted: true,
                truncated: false,
                excluded: false,
                exclusion_reason: None,
            }],
            response_format: crate::ai::ResponseFormat::Json,
            tool_policy: crate::ai::ToolPolicy::ReadOnlyWhitelist,
            token_budget: 32000,
            temperature: Some(0.2),
            stream: true,
            secret_warn_confirmed: false,
            use_cache: true,
        }),
    );
    m.insert(
        "AiToolDefinition".into(),
        json!(crate::ai::ToolDefinition {
            name: "runtime.getLogs".into(),
            version: "1.0".into(),
            input_schema: json!({"type": "object", "required": ["workspaceId"]}),
            allowed_roles: vec![
                crate::ai::ToolRole::RuntimeDiagnostician,
                crate::ai::ToolRole::ActionPlanner
            ],
            context_scope: crate::ai::ToolScope::Runtime,
            requires_workspace: true,
            may_contain_secrets: true,
            timeout_ms: 10000,
            max_result_bytes: 262144,
            read_only: true,
        }),
    );
    m.insert(
        "ToolCallRequest".into(),
        json!(crate::ai::ToolCallRequest {
            request_id: "req-1".into(),
            tool_name: "workspace.list".into(),
            role: crate::ai::ToolRole::WorkspaceAssistant,
            arguments: json!({}),
        }),
    );
    m.insert(
        "ToolInvocation".into(),
        json!(crate::ai::ToolInvocation {
            request_id: "req-1".into(),
            tool_name: "workspace.list".into(),
            role: crate::ai::ToolRole::WorkspaceAssistant,
            result: json!([]),
            truncated: false,
            result_bytes: 2,
            total_result_bytes: 2,
            duration_ms: 1,
            parameter_hash: "0000000000000000".into(),
        }),
    );
    m.insert(
        "ActionProposal".into(),
        json!(crate::ai::ActionProposal {
            proposal_id: "proposal-1".into(),
            request_id: Some("req-1".into()),
            action_kind: crate::ai::ActionKind::GitCreateCommit,
            risk_level: crate::ai::RiskLevel::Medium,
            target_scope: json!({"workspaceId": 1, "repoPath": "/ws/repo"}),
            affected_repositories: vec!["/ws/repo".into()],
            affected_files: vec!["src/main.rs".into()],
            before_summary: "working tree has changes".into(),
            after_summary: "one commit will be created".into(),
            diff: None,
            command_preview: Some("git add <files> && git commit -m <message>".into()),
            reversible: true,
            expires_at: "2026-08-31T12:00:00Z".into(),
            status: crate::ai::ProposalStatus::Pending,
            confirmed_at: None,
            executed_task_id: None,
            created_at: "2026-08-31T11:45:00Z".into(),
        }),
    );
    // 结果模型（§8.4）：枚举类型按 golden 约定序列化为全部变体数组。
    m.insert(
        "AiResult".into(),
        json!([
            crate::ai::AiResult::Answer {
                text: "直接的解释文本".into(),
            },
            crate::ai::AiResult::DiagnosticReport {
                payload: json!({"cause": "port occupied", "evidence": []}),
            },
            crate::ai::AiResult::ReviewReport {
                payload: json!({"summary": "ok", "issues": []}),
            },
            crate::ai::AiResult::GeneratedText {
                text: "feat: add gateway".into(),
            },
            crate::ai::AiResult::CommitSuggestion {
                payload: json!({"title": "feat: add gateway", "body": []}),
            },
            crate::ai::AiResult::CommitSummary {
                payload: json!({"summary": "one repository", "repositories": [], "risks": []}),
            },
            crate::ai::AiResult::PrDescription {
                payload: json!({"title": "AI-08", "description": "", "summary": [], "testing": [], "risks": []}),
            },
            crate::ai::AiResult::Explanation {
                payload: json!({"summary": "intent", "details": [], "riskNotes": []}),
            },
            crate::ai::AiResult::ConflictProposal {
                payload: json!({
                    "proposedContent": "merged\n",
                    "diff": "@@ -1 +1 @@",
                    "rationale": "keeps both intended changes",
                    "confidence": "medium"
                }),
            },
            crate::ai::AiResult::ActionProposal {
                payload: json!({"action": "none"}),
            },
        ]),
    );
    m.insert(
        "AiStreamChunk".into(),
        json!([
            crate::ai::events::AiStreamChunk::TextDelta {
                text: "启动".into(),
            },
            crate::ai::events::AiStreamChunk::End {
                finish_reason: Some("stop".into()),
            },
        ]),
    );
    m.insert(
        "AiRequestEvent".into(),
        json!(crate::ai::events::AiRequestEvent {
            request_id: "req-1".into(),
            phase: crate::ai::RequestPhase::Streaming,
            chunk: Some(crate::ai::events::AiStreamChunk::TextDelta {
                text: "启动".into(),
            }),
            output_chars: 42,
        }),
    );
    m.insert(
        "AiRequestSnapshot".into(),
        json!(crate::ai::AiRequestSnapshot {
            request_id: "req-1".into(),
            session_id: Some("sess-1".into()),
            task_kind: crate::ai::AiTaskKind::RuntimeDiagnostic,
            provider_id: "p1".into(),
            model_id: "gpt-4o-mini".into(),
            phase: crate::ai::RequestPhase::Succeeded,
            stream: true,
            estimated_prompt_tokens: 2048,
            output_chars: 42,
            attempts: 1,
            usage: Some(crate::ai::AiTokenUsage {
                input_tokens: Some(1024),
                output_tokens: Some(256),
            }),
            result: Some(crate::ai::AiResult::Answer {
                text: "端口被占用".into(),
            }),
            error: None,
            error_code: None,
            from_cache: false,
        }),
    );
    // AI-04 会话 / 消息 / 审计 / 缓存（§10.4 / §11.2 / §11.3）
    m.insert(
        "AiSession".into(),
        json!(crate::ai::session::AiSession {
            id: "sess-1".into(),
            title: "运行时排障".into(),
            role: crate::ai::session::AiSessionRole::RuntimeDiagnostician,
            workspace_id: Some(1),
            repository_scope: vec!["D:/ws/repo".into()],
            runtime_scope: json!({"runtimeName": "app", "processId": 12}),
            created_at: "2026-01-01T00:00:00Z".into(),
            updated_at: "2026-01-02T00:00:00Z".into(),
            archived_at: None,
            message_count: 2,
        }),
    );
    m.insert(
        "CreateAiSessionRequest".into(),
        json!(crate::ai::session::CreateAiSessionRequest {
            title: "运行时排障".into(),
            role: Some(crate::ai::session::AiSessionRole::GitReviewer),
            workspace_id: Some(1),
            repository_scope: vec!["D:/ws/repo".into()],
            runtime_scope: None,
        }),
    );
    m.insert(
        "AiSessionListQuery".into(),
        json!(crate::ai::session::AiSessionListQuery {
            workspace_id: Some(1),
            include_archived: false,
            limit: Some(20),
            offset: Some(0),
        }),
    );
    m.insert(
        "AiSessionList".into(),
        json!(crate::ai::session::AiSessionList {
            items: vec![crate::ai::session::AiSession {
                id: "sess-1".into(),
                title: "运行时排障".into(),
                role: crate::ai::session::AiSessionRole::WorkspaceAssistant,
                workspace_id: None,
                repository_scope: vec![],
                runtime_scope: json!({}),
                created_at: "2026-01-01T00:00:00Z".into(),
                updated_at: "2026-01-01T00:00:00Z".into(),
                archived_at: None,
                message_count: 0,
            }],
            total: 1,
        }),
    );
    m.insert(
        "AiSessionMessage".into(),
        json!(crate::ai::session::AiSessionMessage {
            id: 7,
            session_id: "sess-1".into(),
            role: crate::ai::MessageRole::Assistant,
            content: json!({"text": "端口被占用"}),
            sequence: 1,
            created_at: "2026-01-01T00:00:00Z".into(),
        }),
    );
    m.insert(
        "AiSessionDetail".into(),
        json!(crate::ai::session::AiSessionDetail {
            session: crate::ai::session::AiSession {
                id: "sess-1".into(),
                title: "运行时排障".into(),
                role: crate::ai::session::AiSessionRole::WorkspaceAssistant,
                workspace_id: None,
                repository_scope: vec![],
                runtime_scope: json!({}),
                created_at: "2026-01-01T00:00:00Z".into(),
                updated_at: "2026-01-01T00:00:00Z".into(),
                archived_at: None,
                message_count: 2,
            },
            messages: vec![crate::ai::session::AiSessionMessage {
                id: 7,
                session_id: "sess-1".into(),
                role: crate::ai::MessageRole::User,
                content: json!({"text": "启动失败"}),
                sequence: 0,
                created_at: "2026-01-01T00:00:00Z".into(),
            }],
            total_messages: 2,
        }),
    );
    m.insert(
        "AiSessionPersistence".into(),
        json!(crate::ai::session::AiSessionPersistence {
            persist_sessions: true,
            session_count: 3,
        }),
    );
    m.insert(
        "AiSessionExport".into(),
        json!(crate::ai::session::AiSessionExport {
            session_id: "sess-1".into(),
            title: "运行时排障".into(),
            path: "D:/ws/exports/session.md".into(),
            message_count: 2,
        }),
    );
    m.insert(
        "AiRequestAudit".into(),
        json!(crate::ai::audit::AiRequestAudit {
            id: "req-1".into(),
            session_id: Some("sess-1".into()),
            task_kind: crate::ai::AiTaskKind::RuntimeDiagnostic,
            provider_id: "p1".into(),
            model_id: "gpt-4o-mini".into(),
            input_hash: "0123456789abcdef".into(),
            context_manifest: vec![crate::ai::ContextItem {
                kind: crate::ai::ContextKind::Log,
                source_id: "runtime/app:latest".into(),
                display_name: "应用启动日志（最近 200 行）".into(),
                char_count: 8192,
                estimated_tokens: 2048,
                redacted: true,
                truncated: false,
                excluded: false,
                exclusion_reason: None,
            }],
            status: "succeeded".into(),
            error_code: None,
            secret_counts: std::collections::BTreeMap::from([("Password".to_string(), 2i64)]),
            input_tokens: Some(1024),
            output_tokens: Some(256),
            latency_ms: Some(1500),
            created_at: "2026-01-01T00:00:00Z".into(),
            finished_at: Some("2026-01-01T00:00:02Z".into()),
        }),
    );
    // AI-03 Context Builder / Preview（设计文档 §8 / §10.1 / §10.2）：
    // 预算策略 / Secret 管道 / Preview 契约。
    m.insert(
        "SupplementaryContext".into(),
        json!(crate::ai::preview::SupplementaryContext {
            role: crate::ai::context::ContextRole::StructuredError,
            kind: crate::ai::ContextKind::Error,
            source_id: "error:BuildFailed".into(),
            display_name: "结构化错误（BuildFailed）".into(),
            content: "code: BuildFailed\nmessage: 构建失败".into(),
            redacted: true,
        }),
    );
    m.insert(
        "ConflictPreviewTarget".into(),
        json!(crate::ai::preview::ConflictPreviewTarget {
            path: "src/main.rs".into(),
            hunk_index: 1,
            hunk_total: 3,
        }),
    );
    m.insert(
        "ConflictProposal".into(),
        json!(crate::ai::ConflictProposal {
            proposed_content: "merged\n".into(),
            diff: "@@ -1 +1 @@".into(),
            rationale: "keeps both intended changes".into(),
            confidence: crate::ai::ConflictConfidence::Medium,
        }),
    );
    m.insert(
        "ContextPreviewRequest".into(),
        json!(crate::ai::preview::ContextPreviewRequest {
            task_kind: crate::ai::AiTaskKind::RuntimeDiagnostic,
            git_scenario: None,
            provider_id: None,
            model_id: None,
            workspace_id: Some(1),
            repo_path: None,
            conflict: None,
            runtime_name: Some("app".into()),
            process_id: Some(12),
            project: None,
            user_instruction: "帮我诊断启动失败".into(),
            diff_scope: None,
            diff_selection: Some(crate::ai::GitDiffSelection {
                repositories: vec![crate::ai::DiffRepositorySelection {
                    repo_path: "/ws/repo".into(),
                    include_paths: vec!["src".into()],
                    exclude_paths: vec!["src/generated".into()],
                }],
            }),
            supplementary: vec![],
            exclusions: vec!["log:1:app:12:tail".into()],
            secret_policy: crate::ai::redact::SecretPolicyChoice {
                strategy: crate::ai::redact::SecretStrategyKind::Block,
                warn_confirmed: false,
            },
            budget_strategy: Some(crate::ai::policy::BudgetStrategy::ErrorDiagnosis),
            stream: true,
            token_estimate_factor: Some(1.2),
            log_tail_lines: Some(200),
            token_budget: Some(16000),
            include_runtime_logs: true,
        }),
    );
    m.insert(
        "DiffRepositorySelection".into(),
        json!(crate::ai::DiffRepositorySelection {
            repo_path: "/ws/repo".into(),
            include_paths: vec!["src".into()],
            exclude_paths: vec!["src/generated".into()],
        }),
    );
    m.insert(
        "GitDiffSelection".into(),
        json!(crate::ai::GitDiffSelection {
            repositories: vec![crate::ai::DiffRepositorySelection {
                repo_path: "/ws/repo".into(),
                include_paths: vec!["src".into()],
                exclude_paths: vec!["src/generated".into()],
            }],
        }),
    );
    m.insert(
        "DiagnosticErrorInput".into(),
        json!(crate::ai::diagnose::DiagnosticErrorInput {
            code: "PortOccupied".into(),
            message: "端口 8080 已被占用".into(),
            details: Some(json!({"port": 8080, "processName": "java"})),
            occurred_at: Some("2026-01-01T00:00:00Z".into()),
        }),
    );
    m.insert(
        "RuntimeDiagnosticRequest".into(),
        json!(crate::ai::diagnose::RuntimeDiagnosticRequest {
            workspace_id: 1,
            runtime_name: "app".into(),
            process_id: Some(12),
            error: Some(crate::ai::diagnose::DiagnosticErrorInput {
                code: "PortOccupied".into(),
                message: "端口 8080 已被占用".into(),
                details: Some(json!({"port": 8080})),
                occurred_at: Some("2026-01-01T00:00:00Z".into()),
            }),
            project: Some("app".into()),
            want_config_advice: true,
            user_instruction: "请给出排查步骤".into(),
            exclusions: vec!["runtime:app:12:log-tail".into()],
            secret_policy: crate::ai::redact::SecretPolicyChoice::default(),
            log_tail_lines: Some(100),
            selected_log: None,
            token_budget: Some(4096),
            stream: true,
            token_estimate_factor: Some(1.0),
        }),
    );
    m.insert(
        "DiagnosticReport".into(),
        json!({
            "headline": "端口可能被其他进程占用",
            "confidence": "high",
            "facts": ["Runtime 配置使用端口 8080"],
            "likelyCauses": ["其他 Java 进程正在监听该端口"],
            "suggestedActions": ["确认占用进程后再决定是否释放端口"],
            "needsUserCheck": ["确认占用进程是否属于当前项目"],
            "sourceContext": ["结构化错误", "Runtime 配置"],
        }),
    );
    m.insert(
        "PreviewTarget".into(),
        json!(crate::ai::preview::PreviewTarget {
            workspace_id: Some(1),
            workspace_name: Some("ws".into()),
            repo_path: Some("/ws/repo".into()),
            repository_paths: vec!["/ws/repo".into()],
            runtime_name: Some("app".into()),
            process_id: Some(12),
        }),
    );
    m.insert(
        "SecretFindingSummary".into(),
        json!(crate::ai::redact::SecretFindingSummary {
            source_id: "diff:workdir:/ws/repo:.env".into(),
            display_name: "diff: .env".into(),
            kinds: vec!["Password".into()],
            count: 1,
        }),
    );
    m.insert(
        "SecretPolicyChoice".into(),
        json!(crate::ai::redact::SecretPolicyChoice {
            strategy: crate::ai::redact::SecretStrategyKind::Warn,
            warn_confirmed: true,
        }),
    );
    m.insert(
        "SecretReport".into(),
        json!(crate::ai::redact::SecretReport {
            findings: vec![crate::ai::redact::SecretFindingSummary {
                source_id: "diff:workdir:/ws/repo:.env".into(),
                display_name: "diff: .env".into(),
                kinds: vec!["Password".into()],
                count: 1,
            }],
            masked_sources: vec![],
            blocked: true,
            block_kinds: vec!["Password".into()],
            warn_pending: false,
        }),
    );
    m.insert(
        "AiContextPreview".into(),
        json!(crate::ai::preview::AiContextPreview {
            request_id: "req-1".into(),
            task_kind: crate::ai::AiTaskKind::GitReview,
            git_scenario: Some(crate::ai::GitAssistantScenario::CodeReview),
            provider_id: "p1".into(),
            provider_name: "Team OpenAI".into(),
            model_id: "gpt-4o-mini".into(),
            model_name: "GPT-4o mini".into(),
            target: crate::ai::preview::PreviewTarget {
                workspace_id: Some(1),
                workspace_name: Some("ws".into()),
                repo_path: Some("/ws/repo".into()),
                repository_paths: vec!["/ws/repo".into()],
                runtime_name: None,
                process_id: None,
            },
            items: vec![crate::ai::ContextItem {
                kind: crate::ai::ContextKind::Diff,
                source_id: "diff:workdir:/ws/repo:summary".into(),
                display_name: "diff 摘要（workdir）".into(),
                char_count: 200,
                estimated_tokens: 50,
                redacted: false,
                truncated: false,
                excluded: false,
                exclusion_reason: None,
            }],
            total_chars: 200,
            total_estimated_tokens: 50,
            budget_tokens: 24000,
            budget_strategy: crate::ai::policy::BudgetStrategy::CodeReview,
            secret: crate::ai::redact::SecretReport::default(),
            truncated_sources: vec![],
            budget_excluded_sources: vec![],
            estimated_requests: 1,
            cost_estimate: None,
            uses_network: true,
            blocked: false,
            block_reasons: vec![],
            content_hash: "0123456789abcdef".into(),
            request: crate::ai::AiRequest {
                request_id: "req-1".into(),
                session_id: None,
                task_kind: crate::ai::AiTaskKind::GitReview,
                git_scenario: Some(crate::ai::GitAssistantScenario::CodeReview),
                provider_id: Some("p1".into()),
                model_id: Some("gpt-4o-mini".into()),
                system_instruction: "你是 Git Reviewer".into(),
                messages: vec![crate::ai::AiMessage {
                    role: crate::ai::MessageRole::User,
                    content: "<context-item kind=\"diff\" source=\"diff:workdir:/ws/repo:summary\" name=\"diff 摘要\">...</context-item>".into(),
                }],
                context_manifest: vec![],
                response_format: crate::ai::ResponseFormat::Json,
                tool_policy: crate::ai::ToolPolicy::Disabled,
                token_budget: 24000,
                temperature: None,
                stream: true,
                secret_warn_confirmed: false,
                use_cache: true,
            },
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
    (
        "WorkspaceManifest",
        "types/manifest.ts",
        "WorkspaceManifest",
    ),
    ("ClonePlanItem", "types/manifest.ts", "ClonePlanItem"),
    ("ClonePlan", "types/manifest.ts", "ClonePlan"),
    // AI（types/ai.ts：本文件注册的每个 interface 都必须有 Rust 样本）
    ("ReviewResult", "types/ai.ts", "ReviewResult"),
    ("ReviewIssue", "types/ai.ts", "ReviewIssue"),
    ("CommitSuggestion", "types/ai.ts", "CommitSuggestion"),
    (
        "CommitSummaryRepository",
        "types/ai.ts",
        "CommitSummaryRepository",
    ),
    ("CommitSummary", "types/ai.ts", "CommitSummary"),
    ("PrDescription", "types/ai.ts", "PrDescription"),
    ("ExplanationResult", "types/ai.ts", "ExplanationResult"),
    ("SearchResult", "types/ai.ts", "SearchResult"),
    ("AiProvider", "types/ai.ts", "AiProvider"),
    (
        "SaveAiProviderRequest",
        "types/ai.ts",
        "SaveAiProviderRequest",
    ),
    (
        "AiProviderTestResult",
        "types/ai.ts",
        "AiProviderTestResult",
    ),
    ("AiModel", "types/ai.ts", "AiModel"),
    ("AiModelDefaults", "types/ai.ts", "AiModelDefaults"),
    ("SaveAiModelRequest", "types/ai.ts", "SaveAiModelRequest"),
    ("AiTaskDefault", "types/ai.ts", "AiTaskDefault"),
    ("AiSettingsSummary", "types/ai.ts", "AiSettingsSummary"),
    ("AiCredentialStatus", "types/ai.ts", "AiCredentialStatus"),
    // AI-02 Gateway（§7 / §8.4 / 事件契约）
    ("AiMessage", "types/ai.ts", "AiMessage"),
    ("ContextItem", "types/ai.ts", "ContextItem"),
    (
        "DiffRepositorySelection",
        "types/ai.ts",
        "DiffRepositorySelection",
    ),
    ("GitDiffSelection", "types/ai.ts", "GitDiffSelection"),
    ("AiTokenUsage", "types/ai.ts", "AiTokenUsage"),
    ("AiRequest", "types/ai.ts", "AiRequest"),
    ("AiResult", "types/ai.ts", "AiResult"),
    ("AiStreamChunk", "types/ai.ts", "AiStreamChunk"),
    ("AiRequestEvent", "types/ai.ts", "AiRequestEvent"),
    ("AiRequestSnapshot", "types/ai.ts", "AiRequestSnapshot"),
    ("AiToolDefinition", "types/ai.ts", "AiToolDefinition"),
    ("ToolCallRequest", "types/ai.ts", "ToolCallRequest"),
    ("ToolInvocation", "types/ai.ts", "ToolInvocation"),
    ("ActionProposal", "types/ai.ts", "ActionProposal"),
    // AI-03 Context Builder / Preview（§8 / §10.1 / §10.2）
    (
        "SupplementaryContext",
        "types/ai.ts",
        "SupplementaryContext",
    ),
    (
        "ConflictPreviewTarget",
        "types/ai.ts",
        "ConflictPreviewTarget",
    ),
    ("ConflictProposal", "types/ai.ts", "ConflictProposal"),
    (
        "ContextPreviewRequest",
        "types/ai.ts",
        "ContextPreviewRequest",
    ),
    (
        "DiagnosticErrorInput",
        "types/ai.ts",
        "DiagnosticErrorInput",
    ),
    (
        "RuntimeDiagnosticRequest",
        "types/ai.ts",
        "RuntimeDiagnosticRequest",
    ),
    ("DiagnosticReport", "types/ai.ts", "DiagnosticReport"),
    ("PreviewTarget", "types/ai.ts", "PreviewTarget"),
    (
        "SecretFindingSummary",
        "types/ai.ts",
        "SecretFindingSummary",
    ),
    ("SecretPolicyChoice", "types/ai.ts", "SecretPolicyChoice"),
    ("SecretReport", "types/ai.ts", "SecretReport"),
    ("AiContextPreview", "types/ai.ts", "AiContextPreview"),
    // AI-04 会话 / 消息 / 审计（§10.4 / §11.2 / §16.1）
    ("AiSession", "types/ai.ts", "AiSession"),
    (
        "CreateAiSessionRequest",
        "types/ai.ts",
        "CreateAiSessionRequest",
    ),
    ("AiSessionListQuery", "types/ai.ts", "AiSessionListQuery"),
    ("AiSessionList", "types/ai.ts", "AiSessionList"),
    ("AiSessionMessage", "types/ai.ts", "AiSessionMessage"),
    ("AiSessionDetail", "types/ai.ts", "AiSessionDetail"),
    (
        "AiSessionPersistence",
        "types/ai.ts",
        "AiSessionPersistence",
    ),
    ("AiSessionExport", "types/ai.ts", "AiSessionExport"),
    ("AiRequestAudit", "types/ai.ts", "AiRequestAudit"),
];
