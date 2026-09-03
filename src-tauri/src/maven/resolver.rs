//! Maven dependency source resolution (R-02).
//!
//! Resolution is deliberately local-only: exact workspace source first,
//! followed by an existence check in the local Maven repository, then a
//! remote marker. No artifact is downloaded here.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::maven::model::{MavenDependency, PomCoordinates};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DependencySource {
    WorkspaceSource,
    LocalRepository,
    RemoteRepository,
}

impl DependencySource {
    pub(crate) fn as_db_str(self) -> &'static str {
        match self {
            Self::WorkspaceSource => "workspaceSource",
            Self::LocalRepository => "localRepository",
            Self::RemoteRepository => "remoteRepository",
        }
    }

    pub(crate) fn from_db_str(value: &str) -> Option<Self> {
        match value {
            "workspaceSource" => Some(Self::WorkspaceSource),
            "localRepository" => Some(Self::LocalRepository),
            "remoteRepository" => Some(Self::RemoteRepository),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ResolutionReason {
    WorkspaceExactMatch,
    LocalArtifactExists,
    RemoteArtifactMissingLocally,
    VersionNotExactForSource,
    WorkspaceVersionMismatch,
    AmbiguousWorkspaceCoordinate,
    MissingVersion,
}

impl ResolutionReason {
    pub(crate) fn as_db_str(self) -> &'static str {
        match self {
            Self::WorkspaceExactMatch => "workspaceExactMatch",
            Self::LocalArtifactExists => "localArtifactExists",
            Self::RemoteArtifactMissingLocally => "remoteArtifactMissingLocally",
            Self::VersionNotExactForSource => "versionNotExactForSource",
            Self::WorkspaceVersionMismatch => "workspaceVersionMismatch",
            Self::AmbiguousWorkspaceCoordinate => "ambiguousWorkspaceCoordinate",
            Self::MissingVersion => "missingVersion",
        }
    }

    pub(crate) fn from_db_str(value: &str) -> Option<Self> {
        match value {
            "workspaceExactMatch" => Some(Self::WorkspaceExactMatch),
            "localArtifactExists" => Some(Self::LocalArtifactExists),
            "remoteArtifactMissingLocally" => Some(Self::RemoteArtifactMissingLocally),
            "versionNotExactForSource" => Some(Self::VersionNotExactForSource),
            "workspaceVersionMismatch" => Some(Self::WorkspaceVersionMismatch),
            "ambiguousWorkspaceCoordinate" => Some(Self::AmbiguousWorkspaceCoordinate),
            "missingVersion" => Some(Self::MissingVersion),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexedMavenProject {
    pub project_id: i64,
    pub repository_id: Option<i64>,
    pub coordinates: PomCoordinates,
    /// Directory containing the project's `pom.xml`.
    pub project_path: PathBuf,
}

#[derive(Debug, Clone, Default)]
pub struct WorkspaceMavenIndex {
    by_gav: BTreeMap<String, Vec<IndexedMavenProject>>,
    by_ga: BTreeMap<String, Vec<IndexedMavenProject>>,
}

impl WorkspaceMavenIndex {
    pub fn new(projects: impl IntoIterator<Item = IndexedMavenProject>) -> Self {
        let mut index = Self::default();
        for project in projects {
            index
                .by_gav
                .entry(project.coordinates.gav())
                .or_default()
                .push(project.clone());
            index
                .by_ga
                .entry(ga(&project.coordinates.group_id, &project.coordinates.artifact_id))
                .or_default()
                .push(project);
        }
        index
    }

    pub fn exact(&self, coordinates: &PomCoordinates) -> &[IndexedMavenProject] {
        self.by_gav
            .get(&coordinates.gav())
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    pub fn has_group_artifact(&self, group_id: &str, artifact_id: &str) -> bool {
        self.by_ga.contains_key(&ga(group_id, artifact_id))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedDependency {
    pub dependency: MavenDependency,
    pub source: DependencySource,
    pub source_project_id: Option<i64>,
    pub resolved_path: Option<PathBuf>,
    pub reason: ResolutionReason,
}

pub fn resolve_dependency(
    index: &WorkspaceMavenIndex,
    dependency: &MavenDependency,
    local_repository: &Path,
) -> ResolvedDependency {
    let Some(version) = dependency.version.as_deref().filter(|value| !value.is_empty()) else {
        return resolved(
            dependency,
            DependencySource::RemoteRepository,
            None,
            None,
            ResolutionReason::MissingVersion,
        );
    };

    let coordinates = PomCoordinates {
        group_id: dependency.group_id.clone(),
        artifact_id: dependency.artifact_id.clone(),
        version: version.to_string(),
    };
    let source_eligible = is_exact_source_version(version);
    let exact = index.exact(&coordinates);
    if source_eligible && exact.len() == 1 {
        let project = &exact[0];
        return resolved(
            dependency,
            DependencySource::WorkspaceSource,
            Some(project.project_id),
            Some(project.project_path.clone()),
            ResolutionReason::WorkspaceExactMatch,
        );
    }

    let fallback_reason = if !source_eligible {
        ResolutionReason::VersionNotExactForSource
    } else if exact.len() > 1 {
        ResolutionReason::AmbiguousWorkspaceCoordinate
    } else if index.has_group_artifact(&dependency.group_id, &dependency.artifact_id) {
        ResolutionReason::WorkspaceVersionMismatch
    } else {
        ResolutionReason::RemoteArtifactMissingLocally
    };

    let artifact = local_artifact_path(local_repository, dependency, version);
    if artifact.is_file() {
        let reason = if matches!(fallback_reason, ResolutionReason::RemoteArtifactMissingLocally) {
            ResolutionReason::LocalArtifactExists
        } else {
            fallback_reason
        };
        resolved(
            dependency,
            DependencySource::LocalRepository,
            None,
            Some(artifact),
            reason,
        )
    } else {
        resolved(
            dependency,
            DependencySource::RemoteRepository,
            None,
            None,
            fallback_reason,
        )
    }
}

pub fn default_local_repository() -> PathBuf {
    dirs::home_dir().unwrap_or_default().join(".m2").join("repository")
}

pub fn local_artifact_path(local_repository: &Path, dependency: &MavenDependency, version: &str) -> PathBuf {
    let mut directory = local_repository.to_path_buf();
    for segment in dependency.group_id.split('.') {
        directory.push(segment);
    }
    directory.push(&dependency.artifact_id);
    directory.push(version);

    let classifier = dependency
        .classifier
        .as_deref()
        .filter(|value| !value.is_empty())
        .map(|value| format!("-{value}"))
        .unwrap_or_default();
    let extension = match dependency.dep_type.as_str() {
        "test-jar" | "bundle" | "maven-plugin" => "jar",
        other => other,
    };
    directory.join(format!(
        "{}-{}{}.{}",
        dependency.artifact_id, version, classifier, extension
    ))
}

fn is_exact_source_version(version: &str) -> bool {
    let upper = version.to_ascii_uppercase();
    !upper.contains("SNAPSHOT")
        && !version.contains("${")
        && !version
            .chars()
            .any(|character| matches!(character, '[' | ']' | '(' | ')' | ','))
}

fn resolved(
    dependency: &MavenDependency,
    source: DependencySource,
    source_project_id: Option<i64>,
    resolved_path: Option<PathBuf>,
    reason: ResolutionReason,
) -> ResolvedDependency {
    ResolvedDependency {
        dependency: dependency.clone(),
        source,
        source_project_id,
        resolved_path,
        reason,
    }
}

fn ga(group_id: &str, artifact_id: &str) -> String {
    format!("{group_id}:{artifact_id}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::maven::model::DependencyScope;

    fn dependency(version: &str) -> MavenDependency {
        MavenDependency {
            group_id: "com.example".into(),
            artifact_id: "common".into(),
            version: Some(version.into()),
            scope: DependencyScope::Compile,
            optional: false,
            dep_type: "jar".into(),
            classifier: None,
            exclusions: vec![],
        }
    }

    fn source(version: &str, project_id: i64) -> IndexedMavenProject {
        IndexedMavenProject {
            project_id,
            repository_id: Some(project_id),
            coordinates: PomCoordinates {
                group_id: "com.example".into(),
                artifact_id: "common".into(),
                version: version.into(),
            },
            project_path: PathBuf::from(format!("/workspace/common-{version}")),
        }
    }

    #[test]
    fn exact_workspace_source_wins_over_local_repository() {
        let index = WorkspaceMavenIndex::new([source("1.0.0", 7)]);
        let local = std::env::temp_dir().join("gw_resolver_source_wins");
        let result = resolve_dependency(&index, &dependency("1.0.0"), &local);
        assert_eq!(result.source, DependencySource::WorkspaceSource);
        assert_eq!(result.source_project_id, Some(7));
        assert_eq!(result.reason, ResolutionReason::WorkspaceExactMatch);
    }

    #[test]
    fn version_mismatch_never_maps_workspace_source() {
        let index = WorkspaceMavenIndex::new([source("2.0.0", 7)]);
        let local = std::env::temp_dir().join("gw_resolver_version_mismatch");
        let result = resolve_dependency(&index, &dependency("1.0.0"), &local);
        assert_eq!(result.source, DependencySource::RemoteRepository);
        assert_eq!(result.source_project_id, None);
        assert_eq!(result.reason, ResolutionReason::WorkspaceVersionMismatch);
    }

    #[test]
    fn local_artifact_wins_when_no_source_exists() {
        let local = std::env::temp_dir().join(format!(
            "gw_resolver_local_{}",
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        let dep = dependency("1.0.0");
        let artifact = local_artifact_path(&local, &dep, "1.0.0");
        std::fs::create_dir_all(artifact.parent().unwrap()).unwrap();
        std::fs::write(&artifact, b"jar").unwrap();

        let result = resolve_dependency(&WorkspaceMavenIndex::default(), &dep, &local);
        assert_eq!(result.source, DependencySource::LocalRepository);
        assert_eq!(result.resolved_path.as_deref(), Some(artifact.as_path()));
        assert_eq!(result.reason, ResolutionReason::LocalArtifactExists);

        let _ = std::fs::remove_dir_all(local);
    }

    #[test]
    fn snapshots_and_ranges_do_not_map_workspace_source() {
        for version in ["1.0-SNAPSHOT", "[1.0,2.0)"] {
            let index = WorkspaceMavenIndex::new([source(version, 7)]);
            let result = resolve_dependency(
                &index,
                &dependency(version),
                &std::env::temp_dir().join("gw_resolver_non_exact"),
            );
            assert_ne!(result.source, DependencySource::WorkspaceSource);
            assert_eq!(result.reason, ResolutionReason::VersionNotExactForSource);
        }
    }

    #[test]
    fn duplicate_gav_is_ambiguous_instead_of_arbitrarily_mapped() {
        let index = WorkspaceMavenIndex::new([source("1.0.0", 7), source("1.0.0", 8)]);
        let result = resolve_dependency(
            &index,
            &dependency("1.0.0"),
            &std::env::temp_dir().join("gw_resolver_ambiguous"),
        );
        assert_eq!(result.source, DependencySource::RemoteRepository);
        assert_eq!(result.reason, ResolutionReason::AmbiguousWorkspaceCoordinate);
    }
}
