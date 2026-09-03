//! Task domain (task model, DAG/pipeline, operation log).
//! Split from `models/ipc_golden_tests.rs` (B-01); merged in the parent module.

use crate::core::{operation_log, pipeline};
use crate::models::task;
use serde_json::{json, Map, Value};

/// Domain portion of the IPC golden samples; merged into `super::samples()`.
pub(super) fn samples(m: &mut Map<String, Value>) {
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
            task::TaskType::ConflictApply {
                path: "src/main.rs".into(),
                strategy: "both".into(),
                content: None,
            },
            task::TaskType::BranchOp {
                op: task::BranchOpKind::Checkout,
                name: "feature".into(),
                force: false,
            },
            task::TaskType::Clone {
                url: "https://example.com/repo.git".into(),
                branch: Some("main".into()),
            },
            task::TaskType::ShellCommand {
                command: "make build".into(),
                timeout_secs: Some(600),
            },
            task::TaskType::Runtime {
                op: task::RuntimeOp::Start,
                workspace_id: 1,
                runtime_name: "app".into(),
                options: task::RuntimeTaskOptions {
                    strategy: Some(crate::runtime::build::RunStrategy::MavenRun),
                    skip_build: false,
                    skip_tests: Some(true),
                    offline: false,
                    affected_modules: vec![],
                },
            },
            task::TaskType::RuntimeUpdateConfig {
                workspace_id: 1,
                name: "app".into(),
                config_json: "{}".into(),
            },
            task::TaskType::NodeInstall {
                project_dir: "/home/user/web".into(),
                package_manager: crate::node::PackageManager::Pnpm,
            },
        ]),
    );
    m.insert(
        "RuntimeTaskOptions".into(),
        json!(task::RuntimeTaskOptions {
            strategy: Some(crate::runtime::build::RunStrategy::PackageRun),
            skip_build: true,
            skip_tests: Some(false),
            offline: true,
            affected_modules: vec!["com.example:auth".into()],
        }),
    );
    m.insert(
        "RuntimeOp".into(),
        json!([
            task::RuntimeOp::Build,
            task::RuntimeOp::Start,
            task::RuntimeOp::Stop,
            task::RuntimeOp::Restart,
            task::RuntimeOp::ResolveDependencies,
            // R-15/R-17 扩展
            task::RuntimeOp::StartEnvironment,
            task::RuntimeOp::StopEnvironment,
            task::RuntimeOp::RebuildRestart,
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
            task::TaskStatus::Failed { error: "boom".into() },
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
            batch_id: Some("b-1".into()),
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
            batch_id: Some("b-1".into()),
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

    // models/task.rs (T-24 DAG) + core/pipeline.rs (T-23)
    m.insert(
        "DagNodeRequest".into(),
        json!(task::DagNodeRequest {
            task: task::TaskRequest {
                task_type: task::TaskType::Fetch,
                repo_path: "/ws/repo".into(),
                repo_name: "repo".into(),
            },
            depends_on: vec![0],
            max_attempts: 1,
            condition: Some(task::NodeCondition::RepoClean),
            group: Some("fetch-all".into()),
            label: Some("Fetch All · repo".into()),
        }),
    );
    m.insert(
        "DagSubmitRequest".into(),
        json!(task::DagSubmitRequest {
            name: "示例流水线".into(),
            nodes: vec![task::DagNodeRequest {
                task: task::TaskRequest {
                    task_type: task::TaskType::Fetch,
                    repo_path: "/ws/repo".into(),
                    repo_name: "repo".into(),
                },
                depends_on: vec![],
                max_attempts: 1,
                condition: None,
                group: Some("fetch-all".into()),
                label: Some("Fetch All · repo".into()),
            }],
            on_failure: task::FailurePolicy::Continue,
        }),
    );
    m.insert("NodeCondition".into(), json!([task::NodeCondition::RepoClean]));
    m.insert(
        "DagNodeInfo".into(),
        json!(task::DagNodeInfo {
            task_id: "t-1".into(),
            label: "Fetch All · repo".into(),
            group: Some("fetch-all".into()),
            repo_path: "/ws/repo".into(),
            repo_name: "repo".into(),
            depends_on: vec!["t-0".into()],
            status: task::TaskStatus::Success,
            skipped: false,
            attempts: 1,
            output: Some("ok".into()),
            started_at: Some("2026-01-01T00:00:00Z".into()),
            finished_at: Some("2026-01-01T00:00:02Z".into()),
        }),
    );
    m.insert(
        "DagEdge".into(),
        json!(task::DagEdge {
            from: "t-0".into(),
            to: "t-1".into(),
        }),
    );
    m.insert(
        "DagGraph".into(),
        json!(task::DagGraph {
            dag_id: "dag-1".into(),
            name: "示例流水线".into(),
            on_failure: task::FailurePolicy::Continue,
            nodes: vec![task::DagNodeInfo {
                task_id: "t-1".into(),
                label: "Fetch All · repo".into(),
                group: Some("fetch-all".into()),
                repo_path: "/ws/repo".into(),
                repo_name: "repo".into(),
                depends_on: vec![],
                status: task::TaskStatus::Success,
                skipped: false,
                attempts: 1,
                output: Some("ok".into()),
                started_at: Some("2026-01-01T00:00:00Z".into()),
                finished_at: Some("2026-01-01T00:00:02Z".into()),
            }],
            edges: vec![task::DagEdge {
                from: "t-0".into(),
                to: "t-1".into(),
            }],
        }),
    );
    m.insert(
        "Pipeline".into(),
        json!(pipeline::Pipeline {
            id: "p-1".into(),
            name: "示例流水线".into(),
            description: "内置示例流".into(),
            steps: vec![pipeline::PipelineStep {
                id: "fetch-all".into(),
                name: "Fetch All".into(),
                kind: pipeline::StepKind::Fetch,
                depends_on: vec![],
                condition: None,
                retries: 0,
                timeout_secs: None,
            }],
            created_at: "2026-01-01T00:00:00Z".into(),
            updated_at: "2026-01-01T00:00:00Z".into(),
        }),
    );
    m.insert(
        "PipelineStep".into(),
        json!(pipeline::PipelineStep {
            id: "build".into(),
            name: "Build".into(),
            kind: pipeline::StepKind::Build {
                command: "cargo build".into(),
            },
            depends_on: vec!["fetch-all".into()],
            condition: Some(task::NodeCondition::RepoClean),
            retries: 1,
            timeout_secs: Some(600),
        }),
    );
    m.insert(
        "StepKind".into(),
        json!([
            pipeline::StepKind::Fetch,
            pipeline::StepKind::CheckStatus,
            pipeline::StepKind::Pull,
            pipeline::StepKind::Build {
                command: "cargo build".into(),
            },
            pipeline::StepKind::Test {
                command: "cargo test".into(),
            },
            pipeline::StepKind::Report,
        ]),
    );
    m.insert(
        "RepoSelection".into(),
        json!(pipeline::RepoSelection {
            repo_path: "/ws/repo".into(),
            repo_name: "repo".into(),
        }),
    );
    m.insert(
        "StepItemReport".into(),
        json!(pipeline::StepItemReport {
            task_id: "t-1".into(),
            repo_path: "/ws/repo".into(),
            repo_name: "repo".into(),
            status: "success".into(),
            message: None,
            output: Some("ok".into()),
            attempts: 1,
            started_at: Some("2026-01-01T00:00:00Z".into()),
            finished_at: Some("2026-01-01T00:00:02Z".into()),
            duration_ms: Some(2000),
        }),
    );
    m.insert(
        "StepReport".into(),
        json!(pipeline::StepReport {
            step_id: "fetch-all".into(),
            name: "Fetch All".into(),
            kind: "fetch".into(),
            status: "success".into(),
            total: 1,
            succeeded: 1,
            failed: 0,
            skipped: 0,
            cancelled: 0,
            items: vec![pipeline::StepItemReport {
                task_id: "t-1".into(),
                repo_path: "/ws/repo".into(),
                repo_name: "repo".into(),
                status: "success".into(),
                message: None,
                output: Some("ok".into()),
                attempts: 1,
                started_at: Some("2026-01-01T00:00:00Z".into()),
                finished_at: Some("2026-01-01T00:00:02Z".into()),
                duration_ms: Some(2000),
            }],
            started_at: Some("2026-01-01T00:00:00Z".into()),
            finished_at: Some("2026-01-01T00:00:02Z".into()),
            duration_ms: Some(2000),
        }),
    );
    m.insert(
        "PipelineRunReport".into(),
        json!(pipeline::PipelineRunReport {
            run_id: "run-1".into(),
            pipeline_name: "示例流水线".into(),
            status: "success".into(),
            total: 1,
            succeeded: 1,
            failed: 0,
            skipped: 0,
            cancelled: 0,
            steps: vec![pipeline::StepReport {
                step_id: "fetch-all".into(),
                name: "Fetch All".into(),
                kind: "fetch".into(),
                status: "success".into(),
                total: 1,
                succeeded: 1,
                failed: 0,
                skipped: 0,
                cancelled: 0,
                items: vec![pipeline::StepItemReport {
                    task_id: "t-1".into(),
                    repo_path: "/ws/repo".into(),
                    repo_name: "repo".into(),
                    status: "success".into(),
                    message: None,
                    output: Some("ok".into()),
                    attempts: 1,
                    started_at: Some("2026-01-01T00:00:00Z".into()),
                    finished_at: Some("2026-01-01T00:00:02Z".into()),
                    duration_ms: Some(2000),
                }],
                started_at: Some("2026-01-01T00:00:00Z".into()),
                finished_at: Some("2026-01-01T00:00:02Z".into()),
                duration_ms: Some(2000),
            }],
            started_at: Some("2026-01-01T00:00:00Z".into()),
            finished_at: Some("2026-01-01T00:00:02Z".into()),
            duration_ms: Some(2000),
        }),
    );

    // core/operation_log.rs (T-34)
    m.insert(
        "OperationLogPage".into(),
        json!(operation_log::OperationLogPage {
            total: 1,
            logs: vec![operation_log::OperationLogSummary {
                id: 1,
                workspace_id: Some(2),
                op_type: "reset".into(),
                summary: "reset --hard".into(),
                created_at: "2026-01-01T00:00:00Z".into(),
                undone_at: None,
                repo_count: 1,
                undone_count: 0,
            }],
        }),
    );
    m.insert(
        "OperationLogSummary".into(),
        json!(operation_log::OperationLogSummary {
            id: 1,
            workspace_id: Some(2),
            op_type: "reset".into(),
            summary: "reset --hard".into(),
            created_at: "2026-01-01T00:00:00Z".into(),
            undone_at: None,
            repo_count: 1,
            undone_count: 0,
        }),
    );
    m.insert(
        "OperationLogItem".into(),
        json!(operation_log::OperationLogItem {
            id: 1,
            log_id: 1,
            repo_path: "/ws/repo".into(),
            ref_name: "main".into(),
            before_oid: "abc123".into(),
            after_oid: Some("def456".into()),
            detail: Some("mode:hard".into()),
            undone_at: None,
        }),
    );
    m.insert(
        "OperationLogDetail".into(),
        json!(operation_log::OperationLogDetail {
            id: 1,
            workspace_id: Some(2),
            op_type: "reset".into(),
            summary: "reset --hard".into(),
            created_at: "2026-01-01T00:00:00Z".into(),
            undone_at: None,
            items: vec![operation_log::OperationLogItem {
                id: 1,
                log_id: 1,
                repo_path: "/ws/repo".into(),
                ref_name: "main".into(),
                before_oid: "abc123".into(),
                after_oid: Some("def456".into()),
                detail: Some("mode:hard".into()),
                undone_at: None,
            }],
        }),
    );
    m.insert(
        "UndoPreviewItem".into(),
        json!(operation_log::UndoPreviewItem {
            item_id: 1,
            repo_path: "/ws/repo".into(),
            repo_name: "repo".into(),
            action: "将分支 'main' 回退到 abc123".into(),
            ok: true,
            message: String::new(),
            undone: false,
        }),
    );
    m.insert(
        "UndoItemResult".into(),
        json!(operation_log::UndoItemResult {
            item_id: 1,
            repo_path: "/ws/repo".into(),
            repo_name: "repo".into(),
            success: true,
            message: "已回退".into(),
        }),
    );
    m.insert(
        "UndoOutcome".into(),
        json!(operation_log::UndoOutcome {
            log_id: 1,
            fully_undone: true,
            results: vec![operation_log::UndoItemResult {
                item_id: 1,
                repo_path: "/ws/repo".into(),
                repo_name: "repo".into(),
                success: true,
                message: "已回退".into(),
            }],
        }),
    );
}

/// Domain portion of `TS_TYPE_MAP`; merged in the parent module.
pub(super) const TS_TYPE_MAP: &[(&str, &str, &str)] = &[
    // T-23/T-24 pipeline + DAG
    ("DagNodeRequest", "types/pipeline.ts", "DagNodeRequest"),
    ("DagSubmitRequest", "types/pipeline.ts", "DagSubmitRequest"),
    ("NodeCondition", "types/pipeline.ts", "NodeCondition"),
    ("DagNodeInfo", "types/pipeline.ts", "DagNodeInfo"),
    ("DagEdge", "types/pipeline.ts", "DagEdge"),
    ("DagGraph", "types/pipeline.ts", "DagGraph"),
    ("Pipeline", "types/pipeline.ts", "Pipeline"),
    ("PipelineStep", "types/pipeline.ts", "PipelineStep"),
    ("StepKind", "types/pipeline.ts", "StepKind"),
    ("RepoSelection", "types/pipeline.ts", "RepoSelection"),
    ("StepItemReport", "types/pipeline.ts", "StepItemReport"),
    ("StepReport", "types/pipeline.ts", "StepReport"),
    ("PipelineRunReport", "types/pipeline.ts", "PipelineRunReport"),
    // T-34 operation log
    ("OperationLogPage", "types/operationLog.ts", "OperationLogPage"),
    ("OperationLogSummary", "types/operationLog.ts", "OperationLogSummary"),
    ("OperationLogItem", "types/operationLog.ts", "OperationLogItem"),
    ("OperationLogDetail", "types/operationLog.ts", "OperationLogDetail"),
    ("UndoPreviewItem", "types/operationLog.ts", "UndoPreviewItem"),
    ("UndoItemResult", "types/operationLog.ts", "UndoItemResult"),
    ("UndoOutcome", "types/operationLog.ts", "UndoOutcome"),
];
