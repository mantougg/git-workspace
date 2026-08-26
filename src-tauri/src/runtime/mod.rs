//! Runtime-specific discovery and orchestration primitives.

pub mod build;
pub mod config;
pub mod events;
pub mod guard;
pub mod launch;
pub mod logs;
pub mod script_approval;
pub mod service;
pub mod spring_boot;

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

pub use spring_boot::{
    detect_spring_boot_workspace, SpringBootCandidate, SpringBootDetectionCache, SpringBootProject,
    SpringBootWorkspaceResult,
};
