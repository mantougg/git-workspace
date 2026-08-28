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

use crate::commands::{ai, batch as batch_cmd, diff as diff_cmd, git_ops, logs};
use crate::core::{branch as branch_core, change_set, conflict as conflict_core, diff, graph, health as health_core, history as history_core, manifest, merge as merge_core, operation_log, pipeline, rebase as rebase_core, reflog as reflog_core, stash as stash_core, worktree as worktree_core, workspace_stash};
use crate::error::AppError;
use crate::java::model as jdk_model;
use crate::maven::{
    closure as maven_closure, exec_model as maven_exec_model, index as maven_index,
    model as maven_model, reactor as maven_reactor, resolver as maven_resolver,
};
use crate::models::{commit, group, repository, task, workspace};
use crate::runtime::{
    config as runtime_config, events as runtime_events, launch as runtime_launch,
    logs as runtime_logs, script_approval as runtime_script_approval, service as runtime_service,
    spring_boot as spring_boot_model,
};

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

    // R-01 Maven model
    m.insert(
        "MavenProject".into(),
        json!(maven_model::MavenProject {
            path: PathBuf::from("/ws/repo/pom.xml"),
            group_id: "com.example".into(),
            artifact_id: "app".into(),
            version: "1.0.0".into(),
            packaging: "jar".into(),
            parent: Some(maven_model::MavenParent {
                group_id: "com.example".into(),
                artifact_id: "parent".into(),
                version: "1.0.0".into(),
                relative_path: Some("../pom.xml".into()),
            }),
            modules: vec![maven_model::MavenModule {
                path: "module-a".into(),
            }],
            dependencies: vec![maven_model::MavenDependency {
                group_id: "com.example".into(),
                artifact_id: "library".into(),
                version: Some("2.0.0".into()),
                scope: maven_model::DependencyScope::Runtime,
                optional: false,
                dep_type: "jar".into(),
                classifier: None,
                exclusions: vec![],
            }],
            dependency_management: vec![],
            profiles: vec![maven_model::MavenProfile {
                id: "dev".into(),
                properties: BTreeMap::from([("spring.profiles.active".into(), "dev".into())]),
                dependencies: vec![],
            }],
            properties: BTreeMap::from([("java.version".into(), "21".into())]),
            plugins: vec![maven_model::MavenPlugin {
                group_id: "org.springframework.boot".into(),
                artifact_id: "spring-boot-maven-plugin".into(),
                version: Some("3.3.0".into()),
            }],
            file_hash: "0123456789abcdef".into(),
        }),
    );
    // R-06 Spring Boot application discovery
    m.insert(
        "SpringBootCandidate".into(),
        json!(spring_boot_model::SpringBootCandidate {
            class_name: "com.example.Application".into(),
            simple_name: "Application".into(),
            module: "app".into(),
            source_path: PathBuf::from("/ws/repo/src/main/java/com/example/Application.java"),
        }),
    );
    m.insert(
        "SpringBootProject".into(),
        json!(spring_boot_model::SpringBootProject {
            project_path: PathBuf::from("/ws/repo/pom.xml"),
            module: "app".into(),
            spring_boot_plugin: true,
            spring_boot_dependency: true,
            is_spring_boot: true,
            candidates: vec![spring_boot_model::SpringBootCandidate {
                class_name: "com.example.Application".into(),
                simple_name: "Application".into(),
                module: "app".into(),
                source_path: PathBuf::from(
                    "/ws/repo/src/main/java/com/example/Application.java",
                ),
            }],
            default_main_class: Some("com.example.Application".into()),
            source_files_scanned: 1,
            source_scan_truncated: false,
        }),
    );
    m.insert(
        "SpringBootWorkspaceResult".into(),
        json!(spring_boot_model::SpringBootWorkspaceResult {
            projects: vec![],
            elapsed_ms: 12,
        }),
    );
    // R-07 Runtime configuration
    let runtime_sample = runtime_config::RuntimeApplicationConfig {
        schema_version: 1,
        name: "boot".into(),
        project: "repo-boot".into(),
        main_class: Some("com.example.Application".into()),
        jdk: Some("21".into()),
        profile: Some("dev".into()),
        vm_options: vec!["-Xmx1g".into()],
        program_arguments: vec!["--server.port=8080".into()],
        environment: BTreeMap::from([("SERVER_PORT".into(), "8080".into())]),
        runtime_environment: BTreeMap::from([("RUNTIME_FLAG".into(), "on".into())]),
        build_engine: Some("maven".into()),
        scope: maven_closure::RuntimeScope::Auto,
        pre_build_script: None,
        post_build_script: None,
        health_check: Some(crate::runtime::health::HealthCheckConfig {
            kind: crate::runtime::health::HealthCheckKind::Auto,
            host: None,
            port: Some(8080),
            path: None,
            interval_ms: Some(5000),
            timeout_ms: Some(2000),
            healthy_after: Some(1),
            unhealthy_after: Some(3),
        }),
    };
    m.insert(
        "RuntimeApplicationConfig".into(),
        json!(runtime_sample.clone()),
    );
    m.insert(
        "RuntimeConfigSummary".into(),
        json!(runtime_config::RuntimeConfigSummary {
            id: 1,
            workspace_id: 2,
            name: "boot".into(),
            project: "repo-boot".into(),
            main_class: Some("com.example.Application".into()),
            jdk: Some("21".into()),
            profile: Some("dev".into()),
            build_engine: Some("maven".into()),
            config_path: "/ws/.gitworkspace/runtimes/boot.json".into(),
            created_at: "2026-08-18T00:00:00Z".into(),
            updated_at: "2026-08-18T00:00:00Z".into(),
        }),
    );
    m.insert(
        "CreateRuntimeConfigRequest".into(),
        json!(runtime_config::CreateRuntimeConfigRequest {
            workspace_id: 2,
            config: runtime_sample.clone(),
        }),
    );
    m.insert(
        "UpdateRuntimeConfigRequest".into(),
        json!(runtime_config::UpdateRuntimeConfigRequest {
            workspace_id: 2,
            name: "boot".into(),
            config: runtime_sample,
        }),
    );
    m.insert(
        "DependencyGraph".into(),
        json!(maven_index::DependencyGraph {
            workspace_id: 1,
            fingerprint: "graph-hash".into(),
            projects: vec![maven_index::MavenProjectNode {
                project_id: 10,
                repository_id: Some(2),
                path: PathBuf::from("/ws/repo/pom.xml"),
                coordinates: maven_model::PomCoordinates {
                    group_id: "com.example".into(),
                    artifact_id: "app".into(),
                    version: "1.0.0".into(),
                },
                packaging: "jar".into(),
                pom_hash: "pom-hash".into(),
            }],
            dependencies: vec![maven_index::DependencyEdge {
                dependency_id: 20,
                from_project_id: 10,
                dependency: maven_model::MavenDependency {
                    group_id: "com.example".into(),
                    artifact_id: "library".into(),
                    version: Some("1.0.0".into()),
                    scope: maven_model::DependencyScope::Compile,
                    optional: false,
                    dep_type: "jar".into(),
                    classifier: None,
                    exclusions: vec![],
                },
                source: maven_resolver::DependencySource::WorkspaceSource,
                source_project_id: Some(11),
                resolved_path: Some(PathBuf::from("/ws/library")),
                reason: maven_resolver::ResolutionReason::WorkspaceExactMatch,
            }],
            modules: vec![maven_index::MavenModuleLink {
                parent_project_id: 10,
                module_project_id: Some(11),
                declared_path: "library".into(),
            }],
            source_mappings: vec![maven_index::SourceMapping {
                coordinates: maven_model::PomCoordinates {
                    group_id: "com.example".into(),
                    artifact_id: "library".into(),
                    version: "1.0.0".into(),
                },
                repository_id: Some(3),
                project_id: 11,
                project_path: PathBuf::from("/ws/library"),
            }],
        }),
    );
    m.insert(
        "RuntimeScope".into(),
        json!([
            maven_closure::RuntimeScope::Auto,
            maven_closure::RuntimeScope::Manual {
                project_ids: vec![10, 11],
            },
            maven_closure::RuntimeScope::Hybrid {
                include_project_ids: vec![12],
                exclude_project_ids: vec![13],
            },
        ]),
    );
    m.insert(
        "RuntimeClosure".into(),
        json!(maven_closure::RuntimeClosure {
            workspace_id: 1,
            root_project_id: 10,
            graph_fingerprint: "graph-hash".into(),
            mode: maven_closure::RuntimeScopeMode::Hybrid,
            projects: vec![maven_index::MavenProjectNode {
                project_id: 10,
                repository_id: Some(2),
                path: PathBuf::from("/ws/repo/pom.xml"),
                coordinates: maven_model::PomCoordinates {
                    group_id: "com.example".into(),
                    artifact_id: "app".into(),
                    version: "1.0.0".into(),
                },
                packaging: "jar".into(),
                pom_hash: "pom-hash".into(),
            }],
        }),
    );
    m.insert(
        "RuntimeReactorPlan".into(),
        json!(maven_reactor::RuntimeReactorPlan {
            kind: maven_reactor::RuntimeReactorKind::Synthetic,
            pom_path: PathBuf::from("/ws/.gitworkspace/runtime/app/pom.xml"),
            module_paths: vec![PathBuf::from("/ws/repo")],
            arguments: vec!["-f".into(), "/ws/.gitworkspace/runtime/app/pom.xml".into(),],
        }),
    );
    // R-13 Runtime Scope 预览（runtime_get_closure）
    m.insert(
        "ClosurePreview".into(),
        json!(runtime_service::ClosurePreview {
            closure: maven_closure::RuntimeClosure {
                workspace_id: 1,
                root_project_id: 10,
                graph_fingerprint: "graph-hash".into(),
                mode: maven_closure::RuntimeScopeMode::Auto,
                projects: vec![maven_index::MavenProjectNode {
                    project_id: 10,
                    repository_id: Some(2),
                    path: PathBuf::from("/ws/repo/pom.xml"),
                    coordinates: maven_model::PomCoordinates {
                        group_id: "com.example".into(),
                        artifact_id: "app".into(),
                        version: "1.0.0".into(),
                    },
                    packaging: "jar".into(),
                    pom_hash: "pom-hash".into(),
                }],
            },
            cache_hit: true,
        }),
    );
    // R-13 日志导出（runtime_export_logs）
    m.insert(
        "LogExportOutcome".into(),
        json!(runtime_logs::LogExportOutcome {
            path: "/ws/.gitworkspace/logs/app/export.txt".into(),
            lines: 42,
        }),
    );
    // R-14 §75 Command Safety：脚本确认记录（runtime_script_approvals IPC）
    m.insert(
        "ScriptApproval".into(),
        json!(runtime_script_approval::ScriptApproval {
            workspace_id: 2,
            runtime_name: "boot".into(),
            script_type: "pre".into(),
            script_hash: "abc123".into(),
            preview: "#!/bin/sh".into(),
            approved_at: "2026-08-26T00:00:00Z".into(),
            last_executed_at: Some("2026-08-26T00:01:00Z".into()),
        }),
    );

    // R-12 Runtime IPC / Event API（§63/§64/§66）
    let sample_node = maven_index::MavenProjectNode {
        project_id: 10,
        repository_id: Some(2),
        path: PathBuf::from("/ws/repo/pom.xml"),
        coordinates: maven_model::PomCoordinates {
            group_id: "com.example".into(),
            artifact_id: "app".into(),
            version: "1.0.0".into(),
        },
        packaging: "jar".into(),
        pom_hash: "pom-hash".into(),
    };
    let sample_module_link = maven_index::MavenModuleLink {
        parent_project_id: 10,
        module_project_id: Some(11),
        declared_path: "library".into(),
    };
    let sample_source_mapping = maven_index::SourceMapping {
        coordinates: maven_model::PomCoordinates {
            group_id: "com.example".into(),
            artifact_id: "library".into(),
            version: "1.0.0".into(),
        },
        repository_id: Some(3),
        project_id: 11,
        project_path: PathBuf::from("/ws/library"),
    };
    let sample_edge = maven_index::DependencyEdge {
        dependency_id: 20,
        from_project_id: 10,
        dependency: maven_model::MavenDependency {
            group_id: "com.example".into(),
            artifact_id: "library".into(),
            version: Some("1.0.0".into()),
            scope: maven_model::DependencyScope::Compile,
            optional: false,
            dep_type: "jar".into(),
            classifier: None,
            exclusions: vec![],
        },
        source: maven_resolver::DependencySource::WorkspaceSource,
        source_project_id: Some(11),
        resolved_path: Some(PathBuf::from("/ws/library")),
        reason: maven_resolver::ResolutionReason::WorkspaceExactMatch,
    };
    m.insert("MavenProjectNode".into(), json!(sample_node.clone()));
    m.insert("MavenModuleLink".into(), json!(sample_module_link.clone()));
    m.insert("SourceMapping".into(), json!(sample_source_mapping.clone()));
    m.insert("DependencyEdge".into(), json!(sample_edge.clone()));
    m.insert(
        "LifecycleStatus".into(),
        json!([
            runtime_launch::LifecycleStatus::Created,
            runtime_launch::LifecycleStatus::Preparing,
            runtime_launch::LifecycleStatus::Resolving,
            runtime_launch::LifecycleStatus::Building,
            runtime_launch::LifecycleStatus::Starting,
            runtime_launch::LifecycleStatus::Running,
            runtime_launch::LifecycleStatus::Stopping,
            runtime_launch::LifecycleStatus::Stopped,
            runtime_launch::LifecycleStatus::Failed,
        ]),
    );
    m.insert(
        "RuntimeProcessInfo".into(),
        json!(runtime_launch::RuntimeProcessInfo {
            process_id: 31,
            workspace_id: 1,
            runtime_name: "app".into(),
            pid: Some(4242),
            status: runtime_launch::LifecycleStatus::Running,
            run_strategy: Some(crate::runtime::build::RunStrategy::ClasspathRun),
            command_preview: Some("java -cp ... com.example.Application".into()),
            working_dir: Some("/ws/repo/app".into()),
            ports: vec![8080],
            exit_code: None,
            adopted: false,
            started_at: "2026-08-25T00:00:00Z".into(),
            stopped_at: None,
            uptime_seconds: Some(42),
            cpu_percent: Some(3.5),
            memory_bytes: Some(268_435_456),
        }),
    );
    m.insert(
        "LogLevel".into(),
        json!([
            runtime_logs::LogLevel::Trace,
            runtime_logs::LogLevel::Debug,
            runtime_logs::LogLevel::Info,
            runtime_logs::LogLevel::Warn,
            runtime_logs::LogLevel::Error,
        ]),
    );
    m.insert(
        "LogPhase".into(),
        json!([
            runtime_logs::LogPhase::Build,
            runtime_logs::LogPhase::Run,
        ]),
    );
    m.insert(
        "OutputStream".into(),
        json!([
            crate::process::streaming::OutputStream::Stdout,
            crate::process::streaming::OutputStream::Stderr,
        ]),
    );
    let sample_log_line = runtime_logs::LogLine {
        seq: 1,
        at: "2026-08-25T00:00:00Z".into(),
        phase: runtime_logs::LogPhase::Run,
        stream: crate::process::streaming::OutputStream::Stdout,
        level: Some(runtime_logs::LogLevel::Info),
        line: "Started Application in 3.2 seconds".into(),
    };
    m.insert("LogLine".into(), json!(sample_log_line.clone()));
    m.insert(
        "LogEntry".into(),
        json!(runtime_logs::LogEntry {
            line_number: 7,
            level: Some(runtime_logs::LogLevel::Warn),
            text: "slow query".into(),
        }),
    );
    m.insert(
        "LogFilter".into(),
        json!(runtime_logs::LogFilter {
            query: Some("error".into()),
            min_level: Some(runtime_logs::LogLevel::Warn),
            limit: Some(200),
        }),
    );
    m.insert(
        "RuntimeStage".into(),
        json!([
            runtime_events::RuntimeStage::Preparing,
            runtime_events::RuntimeStage::Resolving,
            runtime_events::RuntimeStage::Building,
            runtime_events::RuntimeStage::Starting,
        ]),
    );
    m.insert(
        "HealthStatus".into(),
        json!([
            runtime_events::HealthStatus::Up,
            runtime_events::HealthStatus::Down,
            // R-16 探针状态机取值
            runtime_events::HealthStatus::Starting,
            runtime_events::HealthStatus::Healthy,
            runtime_events::HealthStatus::Unhealthy,
            runtime_events::HealthStatus::Stopped,
        ]),
    );
    // R-16 健康快照 / 端口管理
    m.insert(
        "HealthSnapshot".into(),
        json!(crate::runtime::health::HealthSnapshot {
            process_id: 7,
            workspace_id: 1,
            runtime_name: "boot".into(),
            phase: runtime_events::HealthStatus::Healthy,
            last_checked_at: Some("2026-08-29T00:00:00Z".into()),
            last_detail: Some("Actuator /actuator/health UP".into()),
        }),
    );
    m.insert(
        "HealthCheckConfig".into(),
        json!(crate::runtime::health::HealthCheckConfig {
            kind: crate::runtime::health::HealthCheckKind::Auto,
            host: None,
            port: Some(8080),
            path: None,
            interval_ms: Some(5000),
            timeout_ms: Some(2000),
            healthy_after: Some(1),
            unhealthy_after: Some(3),
        }),
    );
    m.insert(
        "HealthCheckKind".into(),
        json!([
            crate::runtime::health::HealthCheckKind::Auto,
            crate::runtime::health::HealthCheckKind::Port,
            crate::runtime::health::HealthCheckKind::Http,
            crate::runtime::health::HealthCheckKind::Tcp,
            crate::runtime::health::HealthCheckKind::Actuator,
        ]),
    );
    m.insert(
        "PortCheckResult".into(),
        json!(crate::runtime::port_manager::PortCheckResult {
            port: 8080,
            in_use: true,
            occupier: Some(crate::process::port::PortOccupier {
                pid: Some(4242),
                process_name: Some("java".into()),
            }),
        }),
    );
    m.insert(
        "PortOccupier".into(),
        json!(crate::process::port::PortOccupier {
            pid: Some(4242),
            process_name: Some("java".into()),
        }),
    );
    m.insert(
        "PortKillOutcome".into(),
        json!(crate::runtime::port_manager::PortKillOutcome {
            pid: 4242,
            process_name: Some("java".into()),
            killed: true,
        }),
    );
    // R-15 环境编排
    m.insert(
        "RuntimeEnvironment".into(),
        json!(crate::runtime::environment::RuntimeEnvironment {
            schema_version: 1,
            name: "Development".into(),
            description: Some("联调环境".into()),
            services: vec![crate::runtime::environment::EnvironmentService {
                runtime_name: "gateway".into(),
                depends_on: vec!["auth".into()],
                jdk: Some("21".into()),
                profile: Some("dev".into()),
                environment: BTreeMap::from([("GATEWAY_UPSTREAM".into(), "http://auth:8081".into())]),
                port: Some(8080),
                external_notes: Some("依赖外部 MySQL".into()),
                ready_timeout_seconds: Some(90),
            }],
        }),
    );
    m.insert(
        "EnvironmentService".into(),
        json!(crate::runtime::environment::EnvironmentService {
            runtime_name: "auth".into(),
            depends_on: vec![],
            jdk: None,
            profile: None,
            environment: BTreeMap::new(),
            port: None,
            external_notes: None,
            ready_timeout_seconds: None,
        }),
    );
    m.insert(
        "ServiceExecState".into(),
        json!([
            crate::runtime::events::ServiceExecState::Skipped,
            crate::runtime::events::ServiceExecState::Starting,
            crate::runtime::events::ServiceExecState::Ready,
            crate::runtime::events::ServiceExecState::Failed,
            crate::runtime::events::ServiceExecState::Stopped,
        ]),
    );
    m.insert(
        "EnvironmentServiceOutcome".into(),
        json!(runtime_events::EnvironmentServiceOutcome {
            service: "gateway".into(),
            state: runtime_events::ServiceExecState::Ready,
            detail: Some("Healthy（5231ms）".into()),
        }),
    );
    m.insert(
        "EnvironmentProgressPayload".into(),
        json!(runtime_events::EnvironmentProgressPayload {
            workspace_id: 1,
            environment: "Development".into(),
            service: "gateway".into(),
            state: runtime_events::ServiceExecState::Ready,
            detail: Some("Healthy（5231ms）".into()),
            at: "2026-08-29T00:00:00Z".into(),
        }),
    );
    m.insert(
        "EnvironmentCompletedPayload".into(),
        json!(runtime_events::EnvironmentCompletedPayload {
            workspace_id: 1,
            environment: "Development".into(),
            success: true,
            services: vec![runtime_events::EnvironmentServiceOutcome {
                service: "gateway".into(),
                state: runtime_events::ServiceExecState::Ready,
                detail: None,
            }],
            at: "2026-08-29T00:00:00Z".into(),
        }),
    );
    // §64 事件 payload（runtime.<event> 一一对应）
    m.insert(
        "ProjectDiscoveredPayload".into(),
        json!(runtime_events::ProjectDiscoveredPayload {
            workspace_id: 1,
            path: "repo/app".into(),
            coordinates: "com.example:app:1.0.0".into(),
            packaging: "jar".into(),
            at: "2026-08-25T00:00:00Z".into(),
        }),
    );
    m.insert(
        "DependencyResolvedPayload".into(),
        json!(runtime_events::DependencyResolvedPayload {
            workspace_id: 1,
            projects: 3,
            dependencies: 2,
            source_mappings: 2,
            inserted: 3,
            updated: 0,
            removed: 0,
            elapsed_ms: 120,
            at: "2026-08-25T00:00:00Z".into(),
        }),
    );
    m.insert(
        "BuildStartedPayload".into(),
        json!(runtime_events::BuildStartedPayload {
            workspace_id: 1,
            runtime_name: "app".into(),
            op: task::RuntimeOp::Build,
            at: "2026-08-25T00:00:00Z".into(),
        }),
    );
    m.insert(
        "BuildProgressPayload".into(),
        json!(runtime_events::BuildProgressPayload {
            workspace_id: 1,
            runtime_name: "app".into(),
            process_id: Some(31),
            stage: runtime_events::RuntimeStage::Building,
            at: "2026-08-25T00:00:00Z".into(),
        }),
    );
    m.insert(
        "BuildCompletedPayload".into(),
        json!(runtime_events::BuildCompletedPayload {
            workspace_id: 1,
            runtime_name: "app".into(),
            process_id: Some(31),
            success: true,
            duration_ms: Some(12_345),
            error: None,
            at: "2026-08-25T00:00:00Z".into(),
        }),
    );
    m.insert(
        "ProcessStartedPayload".into(),
        json!(runtime_events::ProcessStartedPayload {
            workspace_id: 1,
            process_id: 31,
            runtime_name: "app".into(),
            at: "2026-08-25T00:00:00Z".into(),
        }),
    );
    m.insert(
        "ProcessOutputPayload".into(),
        json!(runtime_events::ProcessOutputPayload {
            process_id: 31,
            runtime_name: "app".into(),
            lines: vec![sample_log_line],
        }),
    );
    m.insert(
        "ProcessStoppedPayload".into(),
        json!(runtime_events::ProcessStoppedPayload {
            workspace_id: 1,
            process_id: 31,
            runtime_name: "app".into(),
            exit_code: Some(0),
            at: "2026-08-25T00:00:00Z".into(),
        }),
    );
    m.insert(
        "ProcessFailedPayload".into(),
        json!(runtime_events::ProcessFailedPayload {
            workspace_id: 1,
            process_id: 31,
            runtime_name: "app".into(),
            exit_code: Some(1),
            at: "2026-08-25T00:00:00Z".into(),
        }),
    );
    m.insert(
        "HealthChangedPayload".into(),
        json!(runtime_events::HealthChangedPayload {
            workspace_id: 1,
            process_id: 31,
            runtime_name: "app".into(),
            health: runtime_events::HealthStatus::Up,
            at: "2026-08-25T00:00:00Z".into(),
        }),
    );
    m.insert(
        "FileChangedPayload".into(),
        json!(runtime_events::FileChangedPayload {
            workspace_id: 1,
            paths: vec!["repo/app/src/main/java/com/example/App.java".into()],
            at: "2026-08-25T00:00:00Z".into(),
        }),
    );
    m.insert(
        "RestartStartedPayload".into(),
        json!(runtime_events::RestartStartedPayload {
            workspace_id: 1,
            runtime_name: "app".into(),
            at: "2026-08-25T00:00:00Z".into(),
        }),
    );
    m.insert(
        "RestartCompletedPayload".into(),
        json!(runtime_events::RestartCompletedPayload {
            workspace_id: 1,
            runtime_name: "app".into(),
            success: true,
            error: None,
            at: "2026-08-25T00:00:00Z".into(),
        }),
    );
    // §63 请求 / 视图类型 + §66 调度配置
    m.insert(
        "RuntimeOperationRequest".into(),
        json!(runtime_service::RuntimeOperationRequest {
            workspace_id: 1,
            runtime_name: "app".into(),
            options: task::RuntimeTaskOptions::default(),
        }),
    );
    m.insert(
        "RuntimeLogQuery".into(),
        json!(runtime_service::RuntimeLogQuery {
            workspace_id: 1,
            runtime_name: "app".into(),
            process_id: 31,
            filter: runtime_logs::LogFilter::default(),
        }),
    );
    m.insert(
        "ProjectInspection".into(),
        json!(runtime_service::ProjectInspection {
            project: sample_node.clone(),
            modules: vec![sample_module_link.clone()],
            parent_project_id: None,
            dependencies: vec![sample_edge.clone()],
            source_mappings: vec![sample_source_mapping.clone()],
        }),
    );
    m.insert(
        "DependencyGraphView".into(),
        json!(runtime_service::DependencyGraphView {
            workspace_id: 1,
            fingerprint: "graph-hash".into(),
            projects: vec![sample_node],
            modules: vec![sample_module_link],
            dependencies: vec![sample_edge],
            source_mappings: vec![sample_source_mapping],
            total_dependencies: 1,
            truncated: false,
        }),
    );
    m.insert(
        "SchedulerConfig".into(),
        json!(runtime_service::SchedulerConfig {
            max_concurrent_builds: 2,
            max_concurrent_resolves: 4,
        }),
    );

    // R-04 JDK Manager model
    m.insert(
        "JdkInstallation".into(),
        json!(jdk_model::JdkInstallation {
            id: Some(1),
            home_path: "/Library/Java/JavaVirtualMachines/temurin-17".into(),
            major_version: Some(17),
            full_version: Some("17.0.12".into()),
            vendor: Some(jdk_model::JdkVendor::Temurin),
            architecture: Some("x86_64".into()),
            bitness: Some(64),
            source: jdk_model::JdkDiscoverySource::System,
            java_exec: Some("/Library/Java/JavaVirtualMachines/temurin-17/bin/java".into()),
            javac_exec: Some("/Library/Java/JavaVirtualMachines/temurin-17/bin/javac".into()),
            is_valid: true,
            last_checked: "2026-08-18T00:00:00Z".into(),
            raw_version: Some("openjdk version \"17.0.12\"".into()),
            created_at: Some("2026-08-18T00:00:00Z".into()),
            updated_at: Some("2026-08-18T00:00:00Z".into()),
        }),
    );
    m.insert(
        "JdkDiscoverySource".into(),
        json!([
            jdk_model::JdkDiscoverySource::System,
            jdk_model::JdkDiscoverySource::JavaHome,
            jdk_model::JdkDiscoverySource::Path,
            jdk_model::JdkDiscoverySource::Mise,
            jdk_model::JdkDiscoverySource::Jenv,
            jdk_model::JdkDiscoverySource::Sdkman,
            jdk_model::JdkDiscoverySource::Manual,
        ]),
    );
    m.insert(
        "JdkVendor".into(),
        json!([
            jdk_model::JdkVendor::Oracle,
            jdk_model::JdkVendor::OpenJdk,
            jdk_model::JdkVendor::Temurin,
            jdk_model::JdkVendor::Corretto,
            jdk_model::JdkVendor::GraalVm,
            jdk_model::JdkVendor::Zulu,
            jdk_model::JdkVendor::Liberica,
            jdk_model::JdkVendor::Other,
        ]),
    );
    m.insert(
        "JdkNotFoundError".into(),
        json!(AppError::JdkNotFound("未在 /opt/jdk/bin 下找到 java 可执行文件".into())),
    );

    // R-05 Maven 检测与执行策略 model
    m.insert(
        "MavenSource".into(),
        json!([
            maven_exec_model::MavenSource::ProjectWrapper,
            maven_exec_model::MavenSource::Configured,
            maven_exec_model::MavenSource::System,
        ]),
    );
    m.insert(
        "MavenVersionInfo".into(),
        json!(maven_exec_model::MavenVersionInfo {
            major_version: Some(3),
            full_version: Some("3.9.6".into()),
            raw: "Apache Maven 3.9.6 (bc0240f3c744)".into(),
        }),
    );
    m.insert(
        "MavenExecutable".into(),
        json!(maven_exec_model::MavenExecutable {
            id: Some(1),
            executable_path: "/proj/mvnw".into(),
            source: maven_exec_model::MavenSource::ProjectWrapper,
            project_path: Some("/proj".into()),
            major_version: Some(3),
            full_version: Some("3.9.6".into()),
            is_valid: true,
            last_checked: "2026-08-18T00:00:00Z".into(),
            raw_version: Some("Apache Maven 3.9.6".into()),
            created_at: Some("2026-08-18T00:00:00Z".into()),
            updated_at: Some("2026-08-18T00:00:00Z".into()),
        }),
    );
    m.insert(
        "ResolvedMaven".into(),
        json!(maven_exec_model::ResolvedMaven {
            executable: maven_exec_model::MavenExecutable {
                id: None,
                executable_path: "/usr/bin/mvn".into(),
                source: maven_exec_model::MavenSource::System,
                project_path: None,
                major_version: Some(3),
                full_version: Some("3.9.6".into()),
                is_valid: true,
                last_checked: "2026-08-18T00:00:00Z".into(),
                raw_version: Some("Apache Maven 3.9.6".into()),
                created_at: None,
                updated_at: None,
            },
            local_repository: PathBuf::from("/home/alice/.m2/repository"),
            uses_wrapper: false,
        }),
    );
    m.insert(
        "MavenExecutionRequest".into(),
        json!(maven_exec_model::MavenExecutionRequest {
            working_dir: PathBuf::from("/proj"),
            executable: "/proj/mvnw".into(),
            goals: vec!["clean".into(), "install".into()],
            extra_args: vec!["-DskipTests".into()],
            via_cmd_c: true,
            local_repository: Some(PathBuf::from("/home/alice/.m2/repository")),
        }),
    );
    m.insert(
        "MavenNotFoundError".into(),
        json!(AppError::MavenNotFound(
            "未在项目 /proj 找到可用的 Maven（wrapper / 配置 / 系统三者皆缺）".into()
        )),
    );
    m.insert(
        "BuildFailedError".into(),
        json!(AppError::BuildFailed {
            module: "com.example:app".into(),
            exit_code: Some(1),
            log_tail: "[ERROR] Failed to execute goal ... compile".into(),
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
                },
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
    m.insert(
        "NodeCondition".into(),
        json!([task::NodeCondition::RepoClean]),
    );
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
    // R-01 Maven model
    ("MavenProject", "types/maven.ts", "MavenProject"),
    // R-06 Spring Boot application discovery
    (
        "SpringBootCandidate",
        "types/springBoot.ts",
        "SpringBootCandidate",
    ),
    (
        "SpringBootProject",
        "types/springBoot.ts",
        "SpringBootProject",
    ),
    (
        "SpringBootWorkspaceResult",
        "types/springBoot.ts",
        "SpringBootWorkspaceResult",
    ),
    // R-07 Runtime configuration
    (
        "RuntimeApplicationConfig",
        "types/runtime.ts",
        "RuntimeApplicationConfig",
    ),
    (
        "RuntimeConfigSummary",
        "types/runtime.ts",
        "RuntimeConfigSummary",
    ),
    (
        "CreateRuntimeConfigRequest",
        "types/runtime.ts",
        "CreateRuntimeConfigRequest",
    ),
    (
        "UpdateRuntimeConfigRequest",
        "types/runtime.ts",
        "UpdateRuntimeConfigRequest",
    ),
    // R-02 dependency graph
    ("DependencyGraph", "types/maven.ts", "DependencyGraph"),
    ("MavenProjectNode", "types/maven.ts", "MavenProjectNode"),
    ("MavenModuleLink", "types/maven.ts", "MavenModuleLink"),
    ("SourceMapping", "types/maven.ts", "SourceMapping"),
    ("DependencyEdge", "types/maven.ts", "DependencyEdge"),
    // R-12 Runtime IPC / Event API（§63/§64/§66）
    (
        "RuntimeProcessInfo",
        "types/runtime.ts",
        "RuntimeProcessInfo",
    ),
    ("LogLine", "types/runtime.ts", "LogLine"),
    ("LogEntry", "types/runtime.ts", "LogEntry"),
    ("LogExportOutcome", "types/runtime.ts", "LogExportOutcome"),
    ("LogFilter", "types/runtime.ts", "LogFilter"),
    (
        "ProjectDiscoveredPayload",
        "types/runtime.ts",
        "ProjectDiscoveredPayload",
    ),
    (
        "DependencyResolvedPayload",
        "types/runtime.ts",
        "DependencyResolvedPayload",
    ),
    (
        "BuildStartedPayload",
        "types/runtime.ts",
        "BuildStartedPayload",
    ),
    (
        "BuildProgressPayload",
        "types/runtime.ts",
        "BuildProgressPayload",
    ),
    (
        "BuildCompletedPayload",
        "types/runtime.ts",
        "BuildCompletedPayload",
    ),
    (
        "ProcessStartedPayload",
        "types/runtime.ts",
        "ProcessStartedPayload",
    ),
    (
        "ProcessOutputPayload",
        "types/runtime.ts",
        "ProcessOutputPayload",
    ),
    (
        "ProcessStoppedPayload",
        "types/runtime.ts",
        "ProcessStoppedPayload",
    ),
    (
        "ProcessFailedPayload",
        "types/runtime.ts",
        "ProcessFailedPayload",
    ),
    (
        "HealthChangedPayload",
        "types/runtime.ts",
        "HealthChangedPayload",
    ),
    (
        "FileChangedPayload",
        "types/runtime.ts",
        "FileChangedPayload",
    ),
    (
        "RestartStartedPayload",
        "types/runtime.ts",
        "RestartStartedPayload",
    ),
    (
        "RestartCompletedPayload",
        "types/runtime.ts",
        "RestartCompletedPayload",
    ),
    (
        "RuntimeOperationRequest",
        "types/runtime.ts",
        "RuntimeOperationRequest",
    ),
    ("RuntimeLogQuery", "types/runtime.ts", "RuntimeLogQuery"),
    ("ProjectInspection", "types/runtime.ts", "ProjectInspection"),
    (
        "DependencyGraphView",
        "types/runtime.ts",
        "DependencyGraphView",
    ),
    ("SchedulerConfig", "types/runtime.ts", "SchedulerConfig"),
    // R-16 §41/§81 健康检查 + 端口管理
    ("HealthSnapshot", "types/runtime.ts", "HealthSnapshot"),
    ("HealthCheckConfig", "types/runtime.ts", "HealthCheckConfig"),
    ("PortCheckResult", "types/runtime.ts", "PortCheckResult"),
    ("PortKillOutcome", "types/runtime.ts", "PortKillOutcome"),
    ("PortOccupier", "types/runtime.ts", "PortOccupier"),
    // R-15 环境编排
    ("RuntimeEnvironment", "types/runtime.ts", "RuntimeEnvironment"),
    ("EnvironmentService", "types/runtime.ts", "EnvironmentService"),
    (
        "EnvironmentProgressPayload",
        "types/runtime.ts",
        "EnvironmentProgressPayload",
    ),
    (
        "EnvironmentCompletedPayload",
        "types/runtime.ts",
        "EnvironmentCompletedPayload",
    ),
    ("EnvironmentServiceOutcome", "types/runtime.ts", "EnvironmentServiceOutcome"),
    // R-13 Runtime Scope 预览
    ("ClosurePreview", "types/runtime.ts", "ClosurePreview"),
    // R-14 §75 Command Safety 脚本确认
    ("ScriptApproval", "types/runtime.ts", "ScriptApproval"),
    // R-03 Runtime Closure and Reactor
    ("RuntimeScope", "types/maven.ts", "RuntimeScope"),
    ("RuntimeClosure", "types/maven.ts", "RuntimeClosure"),
    ("RuntimeReactorPlan", "types/maven.ts", "RuntimeReactorPlan"),
    // R-04 JDK Manager
    ("JdkInstallation", "types/jdk.ts", "JdkInstallation"),
    // R-05 Maven 检测与执行策略
    // (MavenSource 是纯字符串 union，与 JdkDiscoverySource 一样被 parse_ts_file
    // 跳过，不注册；样本仍写入 golden 快照守卫序列化稳定性)
    ("MavenVersionInfo", "types/maven.ts", "MavenVersionInfo"),
    ("MavenExecutable", "types/maven.ts", "MavenExecutable"),
    ("ResolvedMaven", "types/maven.ts", "ResolvedMaven"),
    ("MavenExecutionRequest", "types/maven.ts", "MavenExecutionRequest"),
    ("TaskType", "types/task.ts", "TaskType"),
    ("RuntimeTaskOptions", "types/task.ts", "RuntimeTaskOptions"),
    ("TaskStatus", "types/task.ts", "TaskStatus"),
    ("Task", "types/task.ts", "Task"),
    ("TaskRequest", "types/task.ts", "TaskRequest"),
    ("TaskProgress", "types/task.ts", "TaskProgress"),
    ("GitCommandResult", "types/task.ts", "GitCommandResult"),
    ("CommitRequest", "types/task.ts", "CommitRequest"),
    ("CommitScanFinding", "types/commit.ts", "CommitScanFinding"),
    ("CommitIdentity", "types/commit.ts", "CommitIdentity"),
    ("WorktreeInfo", "types/worktree.ts", "WorktreeInfo"),
    ("DryRunItem", "types/batch.ts", "DryRunItem"),
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
    ("HealthWeights", "types/health.ts", "HealthWeights"),
    ("RepoHealth", "types/health.ts", "RepoHealth"),
    ("WorkspaceHealth", "types/health.ts", "WorkspaceHealth"),
    ("RepoHealthExtra", "types/health.ts", "RepoHealthExtra"),
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
    // T-33 manifest
    ("ManifestRepo", "types/manifest.ts", "ManifestRepo"),
    ("WorkspaceManifest", "types/manifest.ts", "WorkspaceManifest"),
    ("ClonePlanItem", "types/manifest.ts", "ClonePlanItem"),
    ("ClonePlan", "types/manifest.ts", "ClonePlan"),
    // T-34 operation log
    ("OperationLogPage", "types/operationLog.ts", "OperationLogPage"),
    (
        "OperationLogSummary",
        "types/operationLog.ts",
        "OperationLogSummary",
    ),
    ("OperationLogItem", "types/operationLog.ts", "OperationLogItem"),
    (
        "OperationLogDetail",
        "types/operationLog.ts",
        "OperationLogDetail",
    ),
    ("UndoPreviewItem", "types/operationLog.ts", "UndoPreviewItem"),
    ("UndoItemResult", "types/operationLog.ts", "UndoItemResult"),
    ("UndoOutcome", "types/operationLog.ts", "UndoOutcome"),
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
    // Discriminant field: `type`, `status`, or domain-specific `mode`.
    let tag_re = regex::Regex::new(r#"(?:type|status|mode)\s*:\s*"([^"]+)""#).unwrap();

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
            // Pure string-literal unions (e.g. `FailurePolicy`, `CloneAction`)
            // carry no `{...}` object variants, so the parser cannot validate
            // them as tagged unions; skip them so the reverse coverage check
            // does not demand their registration.
            if !body.contains('{') {
                continue;
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

/// Discriminant of a tagged-union sample variant.
fn variant_tag(v: &Value) -> String {
    v["type"]
        .as_str()
        .or_else(|| v["status"].as_str())
        .or_else(|| v["mode"].as_str())
        .expect("tagged-union variant must carry `type`, `status`, or `mode`")
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
