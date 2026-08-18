//! Runtime-specific discovery and orchestration primitives.

pub mod config;
pub mod spring_boot;

pub use config::{
    create_config, delete_config, get_config, get_workspace_environment, list_configs,
    merge_environment, resolve_environment, set_workspace_environment, update_config,
    CreateRuntimeConfigRequest, EnvironmentLayers, RuntimeApplicationConfig, RuntimeConfigSummary,
    UpdateRuntimeConfigRequest,
};

pub use spring_boot::{
    detect_spring_boot_workspace, SpringBootCandidate, SpringBootDetectionCache, SpringBootProject,
    SpringBootWorkspaceResult,
};
