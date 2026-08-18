//! Runtime-specific discovery and orchestration primitives.

pub mod spring_boot;

pub use spring_boot::{
    detect_spring_boot_workspace, SpringBootCandidate, SpringBootDetectionCache, SpringBootProject,
    SpringBootWorkspaceResult,
};
