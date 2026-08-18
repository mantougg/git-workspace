//! Runtime dependency closure and scope selection (R-03).

use std::collections::{HashMap, HashSet};

use moka::sync::Cache;
use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use crate::maven::index::{DependencyGraph, MavenProjectNode};
use crate::maven::resolver::DependencySource;

const CLOSURE_CACHE_CAPACITY: u64 = 128;

/// User-selectable Runtime Scope behavior.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "camelCase")]
pub enum RuntimeScope {
    #[default]
    Auto,
    Manual {
        #[serde(rename = "projectIds")]
        project_ids: Vec<i64>,
    },
    Hybrid {
        #[serde(rename = "includeProjectIds")]
        include_project_ids: Vec<i64>,
        #[serde(rename = "excludeProjectIds")]
        exclude_project_ids: Vec<i64>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RuntimeScopeMode {
    Auto,
    Manual,
    Hybrid,
}

impl RuntimeScope {
    pub fn mode(&self) -> RuntimeScopeMode {
        match self {
            Self::Auto => RuntimeScopeMode::Auto,
            Self::Manual { .. } => RuntimeScopeMode::Manual,
            Self::Hybrid { .. } => RuntimeScopeMode::Hybrid,
        }
    }

    fn canonicalized(&self) -> Self {
        fn sorted_unique(values: &[i64]) -> Vec<i64> {
            let mut values = values.to_vec();
            values.sort_unstable();
            values.dedup();
            values
        }

        match self {
            Self::Auto => Self::Auto,
            Self::Manual { project_ids } => Self::Manual {
                project_ids: sorted_unique(project_ids),
            },
            Self::Hybrid {
                include_project_ids,
                exclude_project_ids,
            } => Self::Hybrid {
                include_project_ids: sorted_unique(include_project_ids),
                exclude_project_ids: sorted_unique(exclude_project_ids),
            },
        }
    }
}

/// Dependency-first project order for one runtime application.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeClosure {
    pub workspace_id: i64,
    pub root_project_id: i64,
    pub graph_fingerprint: String,
    pub mode: RuntimeScopeMode,
    pub projects: Vec<MavenProjectNode>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClosureCacheLookup {
    pub closure: RuntimeClosure,
    pub cache_hit: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ClosureCacheKey {
    workspace_id: i64,
    graph_fingerprint: String,
    root_project_id: i64,
    scope: RuntimeScope,
}

/// Bounded cache keyed by the R-02 graph fingerprint and normalized scope.
pub struct RuntimeClosureCache {
    inner: Cache<ClosureCacheKey, RuntimeClosure>,
}

impl RuntimeClosureCache {
    pub fn new() -> Self {
        Self {
            inner: Cache::builder()
                .max_capacity(CLOSURE_CACHE_CAPACITY)
                .build(),
        }
    }

    pub fn get_or_compute(
        &self,
        graph: &DependencyGraph,
        root_project_id: i64,
        scope: &RuntimeScope,
    ) -> AppResult<ClosureCacheLookup> {
        let scope = scope.canonicalized();
        let key = ClosureCacheKey {
            workspace_id: graph.workspace_id,
            graph_fingerprint: graph.fingerprint.clone(),
            root_project_id,
            scope: scope.clone(),
        };
        if let Some(closure) = self.inner.get(&key) {
            return Ok(ClosureCacheLookup {
                closure,
                cache_hit: true,
            });
        }

        let closure = compute_runtime_closure(graph, root_project_id, &scope)?;
        self.inner.insert(key, closure.clone());
        Ok(ClosureCacheLookup {
            closure,
            cache_hit: false,
        })
    }

    pub fn invalidate_all(&self) {
        self.inner.invalidate_all();
    }
}

impl Default for RuntimeClosureCache {
    fn default() -> Self {
        Self::new()
    }
}

/// Compute a minimal source-only closure. Local and remote artifacts never enter it.
pub fn compute_runtime_closure(
    graph: &DependencyGraph,
    root_project_id: i64,
    scope: &RuntimeScope,
) -> AppResult<RuntimeClosure> {
    let view = GraphView::new(graph)?;
    view.require_project(root_project_id)?;
    let scope = scope.canonicalized();

    let active = match &scope {
        RuntimeScope::Auto => view.reachable_from(&[root_project_id], &HashSet::new())?,
        RuntimeScope::Manual { project_ids } => {
            let mut selected: HashSet<i64> = project_ids.iter().copied().collect();
            selected.insert(root_project_id);
            view.require_projects(selected.iter().copied())?;
            selected
        }
        RuntimeScope::Hybrid {
            include_project_ids,
            exclude_project_ids,
        } => {
            let excluded: HashSet<i64> = exclude_project_ids.iter().copied().collect();
            view.require_projects(excluded.iter().copied())?;
            view.require_projects(include_project_ids.iter().copied())?;
            if excluded.contains(&root_project_id) {
                return Err(AppError::DependencyResolve(
                    "the root project cannot be excluded from Runtime Scope".into(),
                ));
            }
            if let Some(project_id) = include_project_ids
                .iter()
                .find(|project_id| excluded.contains(project_id))
            {
                return Err(AppError::DependencyResolve(format!(
                    "project {project_id} is both included and excluded from Runtime Scope"
                )));
            }

            let mut roots = Vec::with_capacity(include_project_ids.len() + 1);
            roots.push(root_project_id);
            roots.extend(include_project_ids.iter().copied());
            view.reachable_from(&roots, &excluded)?
        }
    };

    let ordered_ids = view.dependency_first_order(&active)?;
    let projects = ordered_ids
        .into_iter()
        .map(|project_id| view.projects[&project_id].clone())
        .collect();

    Ok(RuntimeClosure {
        workspace_id: graph.workspace_id,
        root_project_id,
        graph_fingerprint: graph.fingerprint.clone(),
        mode: scope.mode(),
        projects,
    })
}

struct GraphView<'a> {
    projects: HashMap<i64, &'a MavenProjectNode>,
    source_dependencies: HashMap<i64, Vec<i64>>,
}

impl<'a> GraphView<'a> {
    fn new(graph: &'a DependencyGraph) -> AppResult<Self> {
        let mut projects = HashMap::with_capacity(graph.projects.len());
        for project in &graph.projects {
            if projects.insert(project.project_id, project).is_some() {
                return Err(AppError::Index(format!(
                    "duplicate Maven project id {} in dependency graph",
                    project.project_id
                )));
            }
        }

        let mut source_dependencies: HashMap<i64, Vec<i64>> = HashMap::new();
        for edge in &graph.dependencies {
            if edge.source != DependencySource::WorkspaceSource {
                continue;
            }
            if !projects.contains_key(&edge.from_project_id) {
                return Err(AppError::ProjectNotFound(format!(
                    "dependency graph references missing source project {}",
                    edge.from_project_id
                )));
            }
            let dependency_project_id = edge.source_project_id.ok_or_else(|| {
                AppError::DependencyResolve(format!(
                    "Workspace Source dependency {}:{} from project {} has no mapped project",
                    edge.dependency.group_id, edge.dependency.artifact_id, edge.from_project_id
                ))
            })?;
            if !projects.contains_key(&dependency_project_id) {
                return Err(AppError::ProjectNotFound(format!(
                    "Workspace Source dependency {}:{} maps to missing project {}",
                    edge.dependency.group_id, edge.dependency.artifact_id, dependency_project_id
                )));
            }
            source_dependencies
                .entry(edge.from_project_id)
                .or_default()
                .push(dependency_project_id);
        }
        for dependencies in source_dependencies.values_mut() {
            dependencies.sort_unstable();
            dependencies.dedup();
        }

        Ok(Self {
            projects,
            source_dependencies,
        })
    }

    fn require_project(&self, project_id: i64) -> AppResult<()> {
        if self.projects.contains_key(&project_id) {
            Ok(())
        } else {
            Err(AppError::ProjectNotFound(format!(
                "Maven project {project_id} is not present in the workspace dependency graph"
            )))
        }
    }

    fn require_projects(&self, project_ids: impl Iterator<Item = i64>) -> AppResult<()> {
        for project_id in project_ids {
            self.require_project(project_id)?;
        }
        Ok(())
    }

    fn reachable_from(&self, roots: &[i64], excluded: &HashSet<i64>) -> AppResult<HashSet<i64>> {
        let mut active = HashSet::new();
        let mut pending = roots.to_vec();
        while let Some(project_id) = pending.pop() {
            self.require_project(project_id)?;
            if excluded.contains(&project_id) || !active.insert(project_id) {
                continue;
            }
            if let Some(dependencies) = self.source_dependencies.get(&project_id) {
                pending.extend(dependencies.iter().rev().copied());
            }
        }
        Ok(active)
    }

    fn dependency_first_order(&self, active: &HashSet<i64>) -> AppResult<Vec<i64>> {
        let mut states = HashMap::<i64, VisitState>::with_capacity(active.len());
        let mut stack = Vec::new();
        let mut ordered = Vec::with_capacity(active.len());
        let mut project_ids: Vec<i64> = active.iter().copied().collect();
        project_ids.sort_unstable();

        for project_id in project_ids {
            self.visit(project_id, active, &mut states, &mut stack, &mut ordered)?;
        }
        Ok(ordered)
    }

    fn visit(
        &self,
        project_id: i64,
        active: &HashSet<i64>,
        states: &mut HashMap<i64, VisitState>,
        stack: &mut Vec<i64>,
        ordered: &mut Vec<i64>,
    ) -> AppResult<()> {
        match states.get(&project_id) {
            Some(VisitState::Visited) => return Ok(()),
            Some(VisitState::Visiting) => {
                let cycle_start = stack
                    .iter()
                    .position(|candidate| *candidate == project_id)
                    .unwrap_or(0);
                let mut cycle = stack[cycle_start..]
                    .iter()
                    .map(|id| self.project_label(*id))
                    .collect::<Vec<_>>();
                cycle.push(self.project_label(project_id));
                return Err(AppError::DependencyResolve(format!(
                    "cycle detected in Runtime Closure: {}",
                    cycle.join(" -> ")
                )));
            }
            None => {}
        }

        states.insert(project_id, VisitState::Visiting);
        stack.push(project_id);
        if let Some(dependencies) = self.source_dependencies.get(&project_id) {
            for dependency_project_id in dependencies {
                if active.contains(dependency_project_id) {
                    self.visit(*dependency_project_id, active, states, stack, ordered)?;
                }
            }
        }
        stack.pop();
        states.insert(project_id, VisitState::Visited);
        ordered.push(project_id);
        Ok(())
    }

    fn project_label(&self, project_id: i64) -> String {
        self.projects
            .get(&project_id)
            .map(|project| project.coordinates.gav())
            .unwrap_or_else(|| project_id.to_string())
    }
}

#[derive(Clone, Copy)]
enum VisitState {
    Visiting,
    Visited,
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::maven::index::{DependencyEdge, DependencyGraph};
    use crate::maven::model::{DependencyScope, MavenDependency, PomCoordinates};
    use crate::maven::resolver::ResolutionReason;

    fn project(id: i64) -> MavenProjectNode {
        MavenProjectNode {
            project_id: id,
            repository_id: Some(id),
            path: PathBuf::from(format!("/workspace/repo-{id}/pom.xml")),
            coordinates: PomCoordinates {
                group_id: "com.example".into(),
                artifact_id: format!("project-{id}"),
                version: "1.0.0".into(),
            },
            packaging: "jar".into(),
            pom_hash: format!("hash-{id}"),
        }
    }

    fn edge(id: i64, from: i64, to: i64) -> DependencyEdge {
        DependencyEdge {
            dependency_id: id,
            from_project_id: from,
            dependency: MavenDependency {
                group_id: "com.example".into(),
                artifact_id: format!("project-{to}"),
                version: Some("1.0.0".into()),
                scope: DependencyScope::Compile,
                optional: false,
                dep_type: "jar".into(),
                classifier: None,
                exclusions: vec![],
            },
            source: DependencySource::WorkspaceSource,
            source_project_id: Some(to),
            resolved_path: Some(PathBuf::from(format!("/workspace/repo-{to}"))),
            reason: ResolutionReason::WorkspaceExactMatch,
        }
    }

    fn graph(project_count: i64, edges: &[(i64, i64)]) -> DependencyGraph {
        DependencyGraph {
            workspace_id: 1,
            fingerprint: "graph-v1".into(),
            projects: (1..=project_count).map(project).collect(),
            dependencies: edges
                .iter()
                .enumerate()
                .map(|(index, (from, to))| edge(index as i64 + 1, *from, *to))
                .collect(),
            modules: vec![],
            source_mappings: vec![],
        }
    }

    fn ids(closure: &RuntimeClosure) -> Vec<i64> {
        closure
            .projects
            .iter()
            .map(|project| project.project_id)
            .collect()
    }

    #[test]
    fn auto_scope_keeps_only_the_five_project_chain_in_a_hundred_repo_workspace() {
        let graph = graph(100, &[(5, 4), (4, 3), (3, 2), (2, 1)]);
        let closure = compute_runtime_closure(&graph, 5, &RuntimeScope::Auto).unwrap();

        assert_eq!(ids(&closure), vec![1, 2, 3, 4, 5]);
    }

    #[test]
    fn manual_and_hybrid_scope_apply_explicit_selection() {
        let graph = graph(7, &[(5, 4), (4, 3), (3, 2), (2, 1), (6, 7)]);

        let manual = compute_runtime_closure(
            &graph,
            5,
            &RuntimeScope::Manual {
                project_ids: vec![1, 3],
            },
        )
        .unwrap();
        assert_eq!(ids(&manual), vec![1, 3, 5]);

        let hybrid = compute_runtime_closure(
            &graph,
            5,
            &RuntimeScope::Hybrid {
                include_project_ids: vec![6],
                exclude_project_ids: vec![2],
            },
        )
        .unwrap();
        assert_eq!(ids(&hybrid), vec![3, 4, 5, 7, 6]);
    }

    #[test]
    fn reports_cycles_and_missing_source_projects_as_structured_errors() {
        let cyclic = graph(3, &[(1, 2), (2, 3), (3, 1)]);
        let error = compute_runtime_closure(&cyclic, 1, &RuntimeScope::Auto).unwrap_err();
        assert_eq!(error.code(), "DependencyResolveFailed");
        assert!(error.to_string().contains("cycle detected"));

        let mut missing = graph(2, &[(1, 2)]);
        missing.dependencies[0].source_project_id = Some(99);
        let error = compute_runtime_closure(&missing, 1, &RuntimeScope::Auto).unwrap_err();
        assert_eq!(error.code(), "ProjectNotFound");
    }

    #[test]
    fn graph_fingerprint_and_normalized_scope_drive_cache_reuse() {
        let graph = graph(3, &[(3, 2), (2, 1)]);
        let cache = RuntimeClosureCache::new();
        assert_eq!(
            cache.inner.policy().max_capacity(),
            Some(CLOSURE_CACHE_CAPACITY)
        );

        let scope = RuntimeScope::Manual {
            project_ids: vec![2, 1, 2],
        };
        assert!(!cache.get_or_compute(&graph, 3, &scope).unwrap().cache_hit);
        let normalized = RuntimeScope::Manual {
            project_ids: vec![1, 2],
        };
        assert!(
            cache
                .get_or_compute(&graph, 3, &normalized)
                .unwrap()
                .cache_hit
        );

        let mut changed = graph.clone();
        changed.fingerprint = "graph-v2".into();
        assert!(
            !cache
                .get_or_compute(&changed, 3, &normalized)
                .unwrap()
                .cache_hit
        );
    }
}
