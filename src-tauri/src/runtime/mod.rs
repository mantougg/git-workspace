//! Runtime-specific discovery and orchestration primitives.

pub mod build;
pub mod config;
pub mod environment;
pub mod events;
pub mod git_link;
pub mod guard;
pub mod health;
pub mod launch;
pub mod logs;
pub mod port_manager;
pub mod script_approval;
pub mod service;
pub mod spring_boot;
pub mod templates;
pub mod watch;

pub use config::{
    create_config, delete_config, get_config, get_workspace_environment, list_configs,
    merge_environment, resolve_environment, set_workspace_environment, update_config,
    CreateRuntimeConfigRequest, EnvironmentLayers, RuntimeApplicationConfig, RuntimeConfigSummary,
    UpdateRuntimeConfigRequest,
};

pub use launch::{
    LifecycleStatus, RuntimeEvent, RuntimeEventSink, RuntimeProcessInfo, RuntimeProcessManager,
    StartOptions,
};

pub use service::{
    ClosurePreview, DependencyGraphView, ProjectInspection, RuntimeLogQuery, RuntimeOperationRequest,
    RuntimeService, SchedulerConfig,
};

pub use logs::{
    LogEntry, LogExportOutcome, LogFilter, LogLevel, LogLine, LogPhase, RuntimeLogEngine,
};

pub use script_approval::{script_approvals_path, ScriptApproval, ScriptApprovalStore};

pub use health::{
    evaluate_check, parse_http_response, tcp_probe, HealthCheckConfig, HealthCheckKind,
    HealthEngine, HealthSnapshot,
};

pub use port_manager::{kill_external_process, PortCheckResult, PortKillOutcome};

pub use environment::{
    delete_environment, get_environment, list_environments, save_environment,
    topo_sort_services, RuntimeEnvironment,
};

pub use templates::{
    delete_template, get_template, list_templates, save_config_as_template, save_template,
    RuntimeTemplate,
};

pub use spring_boot::{
    detect_spring_boot_workspace, SpringBootCandidate, SpringBootDetectionCache, SpringBootProject,
    SpringBootWorkspaceResult,
};

pub use watch::{ignore_path, RuntimeWatchEngine};

pub use git_link::{GitLinkEngine, RuntimeRunningBrief};
