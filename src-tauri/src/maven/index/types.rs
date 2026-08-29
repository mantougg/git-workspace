//! Maven Index 领域类型（R-02，B-04 拆分）。

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::maven::model::{DependencyScope, MavenDependency, PomCoordinates};
use crate::maven::resolver::{DependencySource, ResolutionReason};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexSyncResult {
    pub inserted: usize,
    pub updated: usize,
    pub unchanged: usize,
    pub removed: usize,
    pub recomputed_projects: usize,
    pub mapping_changed: bool,
    pub graph_fingerprint: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MavenProjectNode {
    pub project_id: i64,
    pub repository_id: Option<i64>,
    pub path: PathBuf,
    pub coordinates: PomCoordinates,
    pub packaging: String,
    pub pom_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MavenModuleLink {
    pub parent_project_id: i64,
    pub module_project_id: Option<i64>,
    pub declared_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceMapping {
    pub coordinates: PomCoordinates,
    pub repository_id: Option<i64>,
    pub project_id: i64,
    pub project_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DependencyEdge {
    pub dependency_id: i64,
    pub from_project_id: i64,
    pub dependency: MavenDependency,
    pub source: DependencySource,
    pub source_project_id: Option<i64>,
    pub resolved_path: Option<PathBuf>,
    pub reason: ResolutionReason,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DependencyGraph {
    pub workspace_id: i64,
    pub fingerprint: String,
    pub projects: Vec<MavenProjectNode>,
    pub dependencies: Vec<DependencyEdge>,
    pub modules: Vec<MavenModuleLink>,
    pub source_mappings: Vec<SourceMapping>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphCacheLookup {
    pub graph: DependencyGraph,
    pub cache_hit: bool,
}

impl std::fmt::Display for DependencyScope {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Compile => "compile",
            Self::Provided => "provided",
            Self::Runtime => "runtime",
            Self::Test => "test",
            Self::System => "system",
            Self::Import => "import",
        })
    }
}
