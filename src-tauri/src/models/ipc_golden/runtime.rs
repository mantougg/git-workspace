//! Runtime / toolchain domain (runtime, maven, jdk, spring boot, log export).
//! Split from `models/ipc_golden_tests.rs` (B-01); merged in the parent module.

use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::commands::logs;
use crate::error::AppError;
use crate::java::model as jdk_model;
use crate::maven::{
    closure as maven_closure, exec_model as maven_exec_model, index as maven_index,
    model as maven_model, reactor as maven_reactor, resolver as maven_resolver,
};
use crate::models::task;
use crate::runtime::{
    config as runtime_config, events as runtime_events, launch as runtime_launch,
    logs as runtime_logs, script_approval as runtime_script_approval, service as runtime_service,
    spring_boot as spring_boot_model,
};
use serde_json::{json, Map, Value};

/// Domain portion of the IPC golden samples; merged into `super::samples()`.
pub(super) fn samples(m: &mut Map<String, Value>) {
    // R-01 Maven model
    m.insert(
        "PomCoordinates".into(),
        json!(maven_model::PomCoordinates {
            group_id: "com.example".into(),
            artifact_id: "app".into(),
            version: "1.0.0".into(),
        }),
    );
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
                source_path: PathBuf::from("/ws/repo/src/main/java/com/example/Application.java",),
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
        kind: runtime_config::RuntimeKind::SpringBoot,
        node_script: None,
        node_package_manager: None,
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
        auto_restart: Some(true),
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
            kind: runtime_config::RuntimeKind::SpringBoot,
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
    // N-09 统一项目视图。
    m.insert(
        "UnifiedNodeProjectPayload".into(),
        json!(crate::commands::runtime::UnifiedNodeProjectPayload {
            package_manager: Some("npm".into()),
            scripts_json: r#"{"dev":"vite"}"#.into(),
            workspace_root: Some("/ws".into()),
        }),
    );
    m.insert(
        "UnifiedMavenProjectPayload".into(),
        json!(crate::commands::runtime::UnifiedMavenProjectPayload {
            coordinates: maven_model::PomCoordinates {
                group_id: "com.example".into(),
                artifact_id: "app".into(),
                version: "1.0.0".into(),
            },
            packaging: "jar".into(),
        }),
    );
    m.insert(
        "UnifiedProjectNode".into(),
        json!(crate::commands::runtime::UnifiedProjectNode {
            source: "node".into(),
            project_id: 7,
            repository_id: Some(3),
            path: "/ws/web".into(),
            name: "web".into(),
            version: "1.2.3".into(),
            node: Some(crate::commands::runtime::UnifiedNodeProjectPayload {
                package_manager: Some("npm".into()),
                scripts_json: r#"{"dev":"vite"}"#.into(),
                workspace_root: Some("/ws".into()),
            }),
            maven: None,
        }),
    );
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
        json!([runtime_logs::LogPhase::Build, runtime_logs::LogPhase::Run,]),
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
                environment: BTreeMap::from([(
                    "GATEWAY_UPSTREAM".into(),
                    "http://auth:8081".into()
                )]),
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
    // R-19 模板
    m.insert(
        "RuntimeTemplate".into(),
        json!(crate::runtime::templates::RuntimeTemplate {
            schema_version: 1,
            name: "Spring Boot Development".into(),
            description: Some("Spring Boot 开发默认".into()),
            applies_to: Some("spring-boot".into()),
            builtin: true,
            config: runtime_config::RuntimeApplicationConfig {
                schema_version: 1,
                name: String::new(),
                project: String::new(),
                kind: runtime_config::RuntimeKind::SpringBoot,
                node_script: None,
                node_package_manager: None,
                main_class: None,
                jdk: Some("21".into()),
                profile: Some("dev".into()),
                vm_options: vec!["-Xms512m".into(), "-Xmx2048m".into()],
                program_arguments: vec![],
                environment: BTreeMap::new(),
                runtime_environment: BTreeMap::new(),
                build_engine: Some("maven".into()),
                scope: maven_closure::RuntimeScope::Auto,
                pre_build_script: None,
                post_build_script: None,
                health_check: None,
                auto_restart: None,
            },
        }),
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
    // R-21 §47/§48/§49 Git 联动
    m.insert(
        "DependencyChangedPayload".into(),
        json!(runtime_events::DependencyChangedPayload {
            workspace_id: 1,
            runtime_name: "boot".into(),
            reason: "filesModified".into(),
            repos: vec!["/ws/repo-auth".into(), "/ws/repo-common".into()],
            affected_modules: vec!["com.example:auth".into(), "com.example:common".into()],
            at: "2026-08-29T00:00:00Z".into(),
        }),
    );
    m.insert(
        "RuntimeRunningBrief".into(),
        json!(crate::runtime::git_link::RuntimeRunningBrief {
            workspace_id: 1,
            runtime_name: "boot".into(),
            status: "running".into(),
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
        json!(AppError::JdkNotFound(
            "未在 /opt/jdk/bin 下找到 java 可执行文件".into()
        )),
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

    // commands/logs.rs
    m.insert(
        "LogFileInfo".into(),
        json!(logs::LogFileInfo {
            name: "app.log".into(),
            path: "/logs/app.log".into(),
            size_bytes: 42,
        }),
    );
}

/// Domain portion of `TS_TYPE_MAP`; merged in the parent module.
pub(super) const TS_TYPE_MAP: &[(&str, &str, &str)] = &[
    // N-09 统一项目视图
    (
        "UnifiedProjectNode",
        "types/runtime.ts",
        "UnifiedProjectNode",
    ),
    (
        "UnifiedNodeProjectPayload",
        "types/runtime.ts",
        "UnifiedNodeProjectPayload",
    ),
    (
        "UnifiedMavenProjectPayload",
        "types/runtime.ts",
        "UnifiedMavenProjectPayload",
    ),
    // R-01 Maven model
    ("MavenProject", "types/maven.ts", "MavenProject"),
    ("PomCoordinates", "types/maven.ts", "MavenCoordinates"),
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
    (
        "RuntimeEnvironment",
        "types/runtime.ts",
        "RuntimeEnvironment",
    ),
    (
        "EnvironmentService",
        "types/runtime.ts",
        "EnvironmentService",
    ),
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
    (
        "EnvironmentServiceOutcome",
        "types/runtime.ts",
        "EnvironmentServiceOutcome",
    ),
    ("RuntimeTemplate", "types/runtime.ts", "RuntimeTemplate"),
    // R-21 §47/§48/§49 Git 联动
    (
        "DependencyChangedPayload",
        "types/runtime.ts",
        "DependencyChangedPayload",
    ),
    (
        "RuntimeRunningBrief",
        "types/runtime.ts",
        "RuntimeRunningBrief",
    ),
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
    (
        "MavenExecutionRequest",
        "types/maven.ts",
        "MavenExecutionRequest",
    ),
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
    ("RemoteBranchEntry", "types/branch.ts", "RemoteBranchEntry"),
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
];
