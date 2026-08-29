//! Persistent Workspace Maven Index and dependency graph (R-02).

use std::collections::{BTreeSet, HashMap, HashSet};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use moka::sync::Cache;
use rusqlite::{params, Connection, Transaction};
use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use crate::maven::discovery::MavenDiscoveryResult;
use crate::maven::effective::EffectiveProject;
use crate::maven::model::{DependencyScope, MavenDependency, MavenProject, PomCoordinates};
use crate::maven::parser::hex_hash;
use crate::maven::resolver::{
    local_artifact_path, resolve_dependency, DependencySource, IndexedMavenProject,
    ResolutionReason, WorkspaceMavenIndex,
};

const GRAPH_CACHE_CAPACITY: u64 = 64;

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

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct GraphCacheKey {
    workspace_id: i64,
    fingerprint: String,
}

pub struct DependencyGraphCache {
    inner: Cache<GraphCacheKey, DependencyGraph>,
}

impl DependencyGraphCache {
    pub fn new() -> Self {
        Self {
            inner: Cache::builder().max_capacity(GRAPH_CACHE_CAPACITY).build(),
        }
    }

    pub fn get_or_load(&self, conn: &Connection, workspace_id: i64) -> AppResult<GraphCacheLookup> {
        let fingerprint = graph_fingerprint(conn, workspace_id)?;
        let key = GraphCacheKey {
            workspace_id,
            fingerprint,
        };
        if let Some(graph) = self.inner.get(&key) {
            return Ok(GraphCacheLookup {
                graph,
                cache_hit: true,
            });
        }

        let graph = query_dependency_graph(conn, workspace_id)?;
        self.inner.insert(key, graph.clone());
        Ok(GraphCacheLookup {
            graph,
            cache_hit: false,
        })
    }

    pub fn invalidate_all(&self) {
        self.inner.invalidate_all();
    }
}

impl Default for DependencyGraphCache {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone)]
struct ProjectInput {
    project: MavenProject,
    effective: EffectiveProject,
    path: String,
    project_path: PathBuf,
    repository_id: Option<i64>,
    model_hash: String,
}

#[derive(Clone)]
struct ProjectRecord {
    project_id: i64,
    repository_id: Option<i64>,
    coordinates: PomCoordinates,
    project_path: PathBuf,
}

pub fn sync_workspace_index(
    conn: &mut Connection,
    workspace_id: i64,
    discovery: &MavenDiscoveryResult,
    local_repository: &Path,
) -> AppResult<IndexSyncResult> {
    let repository_roots = repository_roots(conn, workspace_id)?;
    let effective_by_path: HashMap<String, &EffectiveProject> = discovery
        .effective
        .iter()
        .map(|project| (path_key(&project.path), project))
        .collect();

    let mut inputs = Vec::with_capacity(discovery.projects.len());
    for project in &discovery.projects {
        let path = path_key(&project.path);
        let effective = effective_by_path.get(&path).ok_or_else(|| {
            AppError::DependencyResolve(format!(
                "effective Maven model missing for {}",
                project.path.display()
            ))
        })?;
        let repository_id = find_repository_id(&project.path, &repository_roots);
        inputs.push(ProjectInput {
            project: project.clone(),
            effective: (*effective).clone(),
            path,
            project_path: project
                .path
                .parent()
                .unwrap_or_else(|| Path::new(""))
                .to_path_buf(),
            repository_id,
            model_hash: effective_model_hash(project, effective)?,
        });
    }
    inputs.sort_by(|left, right| left.path.cmp(&right.path));

    let existing = existing_projects(conn, workspace_id)?;
    let old_mappings = mapping_fingerprint_rows(conn, workspace_id)?;
    let new_mappings: BTreeSet<String> = inputs.iter().map(mapping_row_key).collect();
    let mapping_changed = old_mappings != new_mappings;

    let changed_paths: HashSet<String> = inputs
        .iter()
        .filter(|input| {
            existing
                .get(&input.path)
                .map(|(_, model_hash)| model_hash != &input.model_hash)
                .unwrap_or(true)
        })
        .map(|input| input.path.clone())
        .collect();
    let inserted = inputs
        .iter()
        .filter(|input| !existing.contains_key(&input.path))
        .count();
    let updated = changed_paths.len().saturating_sub(inserted);
    let unchanged = inputs.len().saturating_sub(changed_paths.len());
    let input_paths: HashSet<&str> = inputs.iter().map(|input| input.path.as_str()).collect();
    let stale_paths: Vec<String> = existing
        .keys()
        .filter(|path| !input_paths.contains(path.as_str()))
        .cloned()
        .collect();
    let removed = stale_paths.len();

    let now = chrono::Utc::now().to_rfc3339();
    let tx = conn.transaction()?;
    delete_stale_projects(&tx, workspace_id, &stale_paths)?;
    upsert_projects(&tx, workspace_id, &inputs, &now)?;

    let records = load_project_records(&tx, workspace_id)?;
    update_parent_links(&tx, &inputs, &records)?;

    let recompute_paths: HashSet<String> = if mapping_changed {
        inputs.iter().map(|input| input.path.clone()).collect()
    } else {
        changed_paths
    };
    replace_source_mappings(
        &tx,
        workspace_id,
        &inputs,
        &records,
        mapping_changed,
        &recompute_paths,
    )?;
    replace_graph_rows(
        &tx,
        workspace_id,
        &inputs,
        &records,
        &recompute_paths,
        local_repository,
        &now,
    )?;
    tx.commit()?;

    // Changed projects were resolved above. Unchanged projects still need a
    // cheap local-artifact existence refresh because a prior/parallel Maven
    // build may have populated or cleaned `~/.m2` without touching any POM.
    if recompute_paths.len() < inputs.len() {
        refresh_dependency_sources(conn, workspace_id, local_repository)?;
    }

    Ok(IndexSyncResult {
        inserted,
        updated,
        unchanged,
        removed,
        recomputed_projects: recompute_paths.len(),
        mapping_changed,
        graph_fingerprint: graph_fingerprint(conn, workspace_id)?,
    })
}

pub fn query_dependency_graph(conn: &Connection, workspace_id: i64) -> AppResult<DependencyGraph> {
    let projects = query_projects(conn, workspace_id)?;
    let dependencies = query_dependencies(conn, workspace_id)?;
    let modules = query_modules(conn, workspace_id)?;
    let source_mappings = query_source_mappings(conn, workspace_id)?;
    Ok(DependencyGraph {
        workspace_id,
        fingerprint: graph_fingerprint(conn, workspace_id)?,
        projects,
        dependencies,
        modules,
        source_mappings,
    })
}

pub fn query_project_dependencies(
    conn: &Connection,
    project_id: i64,
) -> AppResult<Vec<DependencyEdge>> {
    query_dependencies_with_filter(conn, Some(project_id), None)
}

/// Re-evaluate only the local-repository portion of dependency resolution.
///
/// POM/model cache hits remain untouched. Build completion (or an explicit
/// local-repository refresh) can call this after `~/.m2` changes so persisted
/// Local/Remote classifications and graph-cache fingerprints stay current.
pub fn refresh_dependency_sources(
    conn: &mut Connection,
    workspace_id: i64,
    local_repository: &Path,
) -> AppResult<usize> {
    let now = chrono::Utc::now().to_rfc3339();
    let tx = conn.transaction()?;
    let records = load_project_records(&tx, workspace_id)?;
    let workspace_index =
        WorkspaceMavenIndex::new(records.values().map(|record| IndexedMavenProject {
            project_id: record.project_id,
            repository_id: record.repository_id,
            coordinates: record.coordinates.clone(),
            project_path: record.project_path.clone(),
        }));
    let dependencies = query_dependencies_with_filter(&tx, None, Some(workspace_id))?;

    let mut changed = 0usize;
    {
        let mut update = tx.prepare(
            "UPDATE maven_dependencies SET
                source_kind = ?1,
                source_project_id = ?2,
                resolved_path = ?3,
                resolution_reason = ?4
             WHERE id = ?5",
        )?;
        let mut upsert_artifact = tx.prepare(
            "INSERT INTO maven_artifacts (
                workspace_id, group_id, artifact_id, version, dep_type,
                classifier, local_path, exists_local, last_checked_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(workspace_id, group_id, artifact_id, version, dep_type, classifier)
             DO UPDATE SET
                local_path = excluded.local_path,
                exists_local = excluded.exists_local,
                last_checked_at = excluded.last_checked_at",
        )?;

        for edge in dependencies {
            let resolved = resolve_dependency(&workspace_index, &edge.dependency, local_repository);
            if edge.source != resolved.source
                || edge.source_project_id != resolved.source_project_id
                || edge.resolved_path != resolved.resolved_path
                || edge.reason != resolved.reason
            {
                changed += 1;
                update.execute(params![
                    resolved.source.as_db_str(),
                    resolved.source_project_id,
                    resolved.resolved_path.as_ref().map(|path| path_key(path)),
                    resolved.reason.as_db_str(),
                    edge.dependency_id,
                ])?;
            }

            if let Some(version) = edge
                .dependency
                .version
                .as_deref()
                .filter(|value| !value.is_empty())
            {
                let artifact_path =
                    local_artifact_path(local_repository, &edge.dependency, version);
                upsert_artifact.execute(params![
                    workspace_id,
                    edge.dependency.group_id,
                    edge.dependency.artifact_id,
                    version,
                    edge.dependency.dep_type,
                    edge.dependency.classifier.as_deref().unwrap_or_default(),
                    path_key(&artifact_path),
                    artifact_path.is_file(),
                    now,
                ])?;
            }
        }
    }
    prune_artifacts(&tx, workspace_id)?;
    tx.commit()?;
    Ok(changed)
}

fn repository_roots(conn: &Connection, workspace_id: i64) -> AppResult<Vec<(i64, PathBuf)>> {
    let mut statement = conn.prepare(
        "SELECT id, path FROM repositories
         WHERE workspace_id = ?1 AND is_deleted = 0
         ORDER BY length(path) DESC",
    )?;
    let rows = statement.query_map([workspace_id], |row| {
        Ok((row.get(0)?, PathBuf::from(row.get::<_, String>(1)?)))
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

fn find_repository_id(path: &Path, roots: &[(i64, PathBuf)]) -> Option<i64> {
    let project = comparable_path(path);
    roots
        .iter()
        .find(|(_, root)| {
            let root = comparable_path(root);
            let root = root.trim_end_matches('/');
            project == root || project.starts_with(&format!("{root}/"))
        })
        .map(|(id, _)| *id)
}

fn comparable_path(path: &Path) -> String {
    let normalized = path_key(path);
    if cfg!(windows) {
        normalized.to_ascii_lowercase()
    } else {
        normalized
    }
}

fn existing_projects(
    conn: &Connection,
    workspace_id: i64,
) -> AppResult<HashMap<String, (String, String)>> {
    let mut statement = conn
        .prepare("SELECT path, pom_hash, model_hash FROM maven_projects WHERE workspace_id = ?1")?;
    let rows = statement.query_map([workspace_id], |row| {
        Ok((
            row.get::<_, String>(0)?,
            (row.get::<_, String>(1)?, row.get::<_, String>(2)?),
        ))
    })?;
    Ok(rows.collect::<Result<HashMap<_, _>, _>>()?)
}

fn mapping_fingerprint_rows(conn: &Connection, workspace_id: i64) -> AppResult<BTreeSet<String>> {
    let mut statement = conn.prepare(
        "SELECT group_id, artifact_id, version, repository_id, project_path
         FROM maven_source_mappings WHERE workspace_id = ?1",
    )?;
    let rows = statement.query_map([workspace_id], |row| {
        let repository_id: Option<i64> = row.get(3)?;
        Ok(format!(
            "{}:{}:{}|{:?}|{}",
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            repository_id,
            path_key(Path::new(&row.get::<_, String>(4)?))
        ))
    })?;
    Ok(rows.collect::<Result<BTreeSet<_>, _>>()?)
}

fn mapping_row_key(input: &ProjectInput) -> String {
    format!(
        "{}:{}:{}|{:?}|{}",
        input.effective.group_id,
        input.effective.artifact_id,
        input.effective.version,
        input.repository_id,
        path_key(&input.project_path)
    )
}

fn delete_stale_projects(
    tx: &Transaction<'_>,
    workspace_id: i64,
    stale_paths: &[String],
) -> AppResult<()> {
    let mut statement =
        tx.prepare("DELETE FROM maven_projects WHERE workspace_id = ?1 AND path = ?2")?;
    for path in stale_paths {
        statement.execute(params![workspace_id, path])?;
    }
    Ok(())
}

fn upsert_projects(
    tx: &Transaction<'_>,
    workspace_id: i64,
    inputs: &[ProjectInput],
    now: &str,
) -> AppResult<()> {
    let mut statement = tx.prepare(
        "INSERT INTO maven_projects (
            workspace_id, repository_id, path, group_id, artifact_id, version,
            packaging, pom_hash, model_hash, last_scanned_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
         ON CONFLICT(workspace_id, path) DO UPDATE SET
            repository_id = excluded.repository_id,
            group_id = excluded.group_id,
            artifact_id = excluded.artifact_id,
            version = excluded.version,
            packaging = excluded.packaging,
            pom_hash = excluded.pom_hash,
            model_hash = excluded.model_hash,
            last_scanned_at = excluded.last_scanned_at",
    )?;
    for input in inputs {
        statement.execute(params![
            workspace_id,
            input.repository_id,
            input.path,
            input.effective.group_id,
            input.effective.artifact_id,
            input.effective.version,
            input.effective.packaging,
            input.project.file_hash,
            input.model_hash,
            now,
        ])?;
    }
    Ok(())
}

fn load_project_records(
    tx: &Transaction<'_>,
    workspace_id: i64,
) -> AppResult<HashMap<String, ProjectRecord>> {
    let mut statement = tx.prepare(
        "SELECT id, repository_id, path, group_id, artifact_id, version
         FROM maven_projects WHERE workspace_id = ?1",
    )?;
    let rows = statement.query_map([workspace_id], |row| {
        let path = row.get::<_, String>(2)?;
        let project_path = Path::new(&path)
            .parent()
            .unwrap_or_else(|| Path::new(""))
            .to_path_buf();
        Ok((
            path,
            ProjectRecord {
                project_id: row.get(0)?,
                repository_id: row.get(1)?,
                coordinates: PomCoordinates {
                    group_id: row.get(3)?,
                    artifact_id: row.get(4)?,
                    version: row.get(5)?,
                },
                project_path,
            },
        ))
    })?;
    Ok(rows.collect::<Result<HashMap<_, _>, _>>()?)
}

fn update_parent_links(
    tx: &Transaction<'_>,
    inputs: &[ProjectInput],
    records: &HashMap<String, ProjectRecord>,
) -> AppResult<()> {
    let mut by_gav: HashMap<String, Vec<i64>> = HashMap::new();
    for record in records.values() {
        by_gav
            .entry(record.coordinates.gav())
            .or_default()
            .push(record.project_id);
    }
    let mut statement = tx.prepare("UPDATE maven_projects SET parent_id = ?1 WHERE id = ?2")?;
    for input in inputs {
        let Some(record) = records.get(&input.path) else {
            continue;
        };
        let parent_id = input.project.parent.as_ref().and_then(|parent| {
            let gav = format!(
                "{}:{}:{}",
                parent.group_id, parent.artifact_id, parent.version
            );
            by_gav
                .get(&gav)
                .filter(|matches| matches.len() == 1)
                .map(|matches| matches[0])
        });
        statement.execute(params![parent_id, record.project_id])?;
    }
    Ok(())
}

fn replace_source_mappings(
    tx: &Transaction<'_>,
    workspace_id: i64,
    inputs: &[ProjectInput],
    records: &HashMap<String, ProjectRecord>,
    replace_all: bool,
    recompute_paths: &HashSet<String>,
) -> AppResult<()> {
    if replace_all {
        tx.execute(
            "DELETE FROM maven_source_mappings WHERE workspace_id = ?1",
            [workspace_id],
        )?;
    } else {
        let mut delete = tx.prepare("DELETE FROM maven_source_mappings WHERE project_id = ?1")?;
        for path in recompute_paths {
            if let Some(record) = records.get(path) {
                delete.execute([record.project_id])?;
            }
        }
    }

    let mut insert = tx.prepare(
        "INSERT INTO maven_source_mappings (
            workspace_id, group_id, artifact_id, version, repository_id,
            project_id, project_path
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
    )?;
    for input in inputs {
        if !replace_all && !recompute_paths.contains(&input.path) {
            continue;
        }
        let record = records.get(&input.path).ok_or_else(|| {
            AppError::SourceMapping(format!("indexed Maven project missing for {}", input.path))
        })?;
        insert.execute(params![
            workspace_id,
            input.effective.group_id,
            input.effective.artifact_id,
            input.effective.version,
            record.repository_id,
            record.project_id,
            path_key(&record.project_path),
        ])?;
    }
    Ok(())
}

fn replace_graph_rows(
    tx: &Transaction<'_>,
    workspace_id: i64,
    inputs: &[ProjectInput],
    records: &HashMap<String, ProjectRecord>,
    recompute_paths: &HashSet<String>,
    local_repository: &Path,
    now: &str,
) -> AppResult<()> {
    let workspace_index =
        WorkspaceMavenIndex::new(records.values().map(|record| IndexedMavenProject {
            project_id: record.project_id,
            repository_id: record.repository_id,
            coordinates: record.coordinates.clone(),
            project_path: record.project_path.clone(),
        }));

    let mut delete_dependencies =
        tx.prepare("DELETE FROM maven_dependencies WHERE project_id = ?1")?;
    let mut delete_modules =
        tx.prepare("DELETE FROM maven_modules WHERE parent_project_id = ?1")?;
    let mut insert_dependency = tx.prepare(
        "INSERT INTO maven_dependencies (
            project_id, group_id, artifact_id, version, scope, optional,
            dep_type, classifier, exclusions_json, source_kind,
            source_project_id, resolved_path, resolution_reason, model_hash,
            sort_order
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
    )?;
    let mut insert_module = tx.prepare(
        "INSERT INTO maven_modules (parent_project_id, module_project_id, declared_path)
         VALUES (?1, ?2, ?3)",
    )?;
    let mut upsert_artifact = tx.prepare(
        "INSERT INTO maven_artifacts (
            workspace_id, group_id, artifact_id, version, dep_type,
            classifier, local_path, exists_local, last_checked_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
         ON CONFLICT(workspace_id, group_id, artifact_id, version, dep_type, classifier)
         DO UPDATE SET
            local_path = excluded.local_path,
            exists_local = excluded.exists_local,
            last_checked_at = excluded.last_checked_at",
    )?;

    for input in inputs {
        if !recompute_paths.contains(&input.path) {
            continue;
        }
        let record = records.get(&input.path).ok_or_else(|| {
            AppError::DependencyResolve(format!("indexed Maven project missing for {}", input.path))
        })?;
        delete_dependencies.execute([record.project_id])?;
        delete_modules.execute([record.project_id])?;

        for (sort_order, dependency) in input.effective.effective_dependencies.iter().enumerate() {
            let resolved = resolve_dependency(&workspace_index, dependency, local_repository);
            let exclusions = serde_json::to_string(&dependency.exclusions)?;
            insert_dependency.execute(params![
                record.project_id,
                dependency.group_id,
                dependency.artifact_id,
                dependency.version,
                dependency.scope.to_string(),
                dependency.optional,
                dependency.dep_type,
                dependency.classifier,
                exclusions,
                resolved.source.as_db_str(),
                resolved.source_project_id,
                resolved.resolved_path.as_ref().map(|path| path_key(path)),
                resolved.reason.as_db_str(),
                input.model_hash,
                sort_order as i64,
            ])?;

            if let Some(version) = dependency
                .version
                .as_deref()
                .filter(|value| !value.is_empty())
            {
                let artifact_path = local_artifact_path(local_repository, dependency, version);
                upsert_artifact.execute(params![
                    workspace_id,
                    dependency.group_id,
                    dependency.artifact_id,
                    version,
                    dependency.dep_type,
                    dependency.classifier.as_deref().unwrap_or_default(),
                    path_key(&artifact_path),
                    artifact_path.is_file(),
                    now,
                ])?;
            }
        }

        let parent_dir = input.project.path.parent().unwrap_or_else(|| Path::new(""));
        for module in &input.project.modules {
            let declared = parent_dir.join(&module.path);
            let pom_path = if declared.file_name() == Some(OsStr::new("pom.xml")) {
                declared
            } else {
                declared.join("pom.xml")
            };
            let pom_path = std::fs::canonicalize(&pom_path).unwrap_or(pom_path);
            let module_id = records
                .get(&path_key(&pom_path))
                .map(|item| item.project_id);
            insert_module.execute(params![record.project_id, module_id, module.path])?;
        }
    }

    prune_artifacts(tx, workspace_id)?;
    Ok(())
}

fn prune_artifacts(tx: &Transaction<'_>, workspace_id: i64) -> AppResult<()> {
    tx.execute(
        "DELETE FROM maven_artifacts
         WHERE workspace_id = ?1
           AND NOT EXISTS (
               SELECT 1
               FROM maven_dependencies d
               JOIN maven_projects p ON p.id = d.project_id
               WHERE p.workspace_id = maven_artifacts.workspace_id
                 AND d.group_id = maven_artifacts.group_id
                 AND d.artifact_id = maven_artifacts.artifact_id
                 AND d.version = maven_artifacts.version
                 AND d.dep_type = maven_artifacts.dep_type
                 AND COALESCE(d.classifier, '') = maven_artifacts.classifier
           )",
        [workspace_id],
    )?;
    Ok(())
}

fn query_projects(conn: &Connection, workspace_id: i64) -> AppResult<Vec<MavenProjectNode>> {
    let mut statement = conn.prepare(
        "SELECT id, repository_id, path, group_id, artifact_id, version, packaging, pom_hash
         FROM maven_projects WHERE workspace_id = ?1 ORDER BY path",
    )?;
    let rows = statement.query_map([workspace_id], |row| {
        Ok(MavenProjectNode {
            project_id: row.get(0)?,
            repository_id: row.get(1)?,
            path: PathBuf::from(row.get::<_, String>(2)?),
            coordinates: PomCoordinates {
                group_id: row.get(3)?,
                artifact_id: row.get(4)?,
                version: row.get(5)?,
            },
            packaging: row.get(6)?,
            pom_hash: row.get(7)?,
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

fn query_dependencies(conn: &Connection, workspace_id: i64) -> AppResult<Vec<DependencyEdge>> {
    query_dependencies_with_filter(conn, None, Some(workspace_id))
}

fn query_dependencies_with_filter(
    conn: &Connection,
    project_id: Option<i64>,
    workspace_id: Option<i64>,
) -> AppResult<Vec<DependencyEdge>> {
    let (where_clause, value) = if let Some(project_id) = project_id {
        ("d.project_id = ?1", project_id)
    } else if let Some(workspace_id) = workspace_id {
        ("p.workspace_id = ?1", workspace_id)
    } else {
        return Ok(Vec::new());
    };
    let sql = format!(
        "SELECT d.id, d.project_id, d.group_id, d.artifact_id, d.version,
                d.scope, d.optional, d.dep_type, d.classifier,
                d.exclusions_json, d.source_kind, d.source_project_id,
                d.resolved_path, d.resolution_reason
         FROM maven_dependencies d
         JOIN maven_projects p ON p.id = d.project_id
         WHERE {where_clause}
         ORDER BY d.project_id, d.sort_order"
    );
    let mut statement = conn.prepare(&sql)?;
    let rows = statement.query_map([value], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, Option<String>>(4)?,
            row.get::<_, String>(5)?,
            row.get::<_, i64>(6)? != 0,
            row.get::<_, String>(7)?,
            row.get::<_, Option<String>>(8)?,
            row.get::<_, String>(9)?,
            row.get::<_, String>(10)?,
            row.get::<_, Option<i64>>(11)?,
            row.get::<_, Option<String>>(12)?,
            row.get::<_, String>(13)?,
        ))
    })?;

    let mut result = Vec::new();
    for row in rows {
        let (
            dependency_id,
            from_project_id,
            group_id,
            artifact_id,
            version,
            scope,
            optional,
            dep_type,
            classifier,
            exclusions_json,
            source,
            source_project_id,
            resolved_path,
            reason,
        ) = row?;
        result.push(DependencyEdge {
            dependency_id,
            from_project_id,
            dependency: MavenDependency {
                group_id,
                artifact_id,
                version,
                scope: DependencyScope::parse(&scope),
                optional,
                dep_type,
                classifier,
                exclusions: serde_json::from_str(&exclusions_json)?,
            },
            source: DependencySource::from_db_str(&source)
                .ok_or_else(|| AppError::Index(format!("unknown dependency source `{source}`")))?,
            source_project_id,
            resolved_path: resolved_path.map(PathBuf::from),
            reason: ResolutionReason::from_db_str(&reason)
                .ok_or_else(|| AppError::Index(format!("unknown resolution reason `{reason}`")))?,
        });
    }
    Ok(result)
}

fn query_modules(conn: &Connection, workspace_id: i64) -> AppResult<Vec<MavenModuleLink>> {
    let mut statement = conn.prepare(
        "SELECT m.parent_project_id, m.module_project_id, m.declared_path
         FROM maven_modules m
         JOIN maven_projects p ON p.id = m.parent_project_id
         WHERE p.workspace_id = ?1
         ORDER BY m.parent_project_id, m.declared_path",
    )?;
    let rows = statement.query_map([workspace_id], |row| {
        Ok(MavenModuleLink {
            parent_project_id: row.get(0)?,
            module_project_id: row.get(1)?,
            declared_path: row.get(2)?,
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

fn query_source_mappings(conn: &Connection, workspace_id: i64) -> AppResult<Vec<SourceMapping>> {
    let mut statement = conn.prepare(
        "SELECT group_id, artifact_id, version, repository_id, project_id, project_path
         FROM maven_source_mappings WHERE workspace_id = ?1
         ORDER BY group_id, artifact_id, version, project_path",
    )?;
    let rows = statement.query_map([workspace_id], |row| {
        Ok(SourceMapping {
            coordinates: PomCoordinates {
                group_id: row.get(0)?,
                artifact_id: row.get(1)?,
                version: row.get(2)?,
            },
            repository_id: row.get(3)?,
            project_id: row.get(4)?,
            project_path: PathBuf::from(row.get::<_, String>(5)?),
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

fn effective_model_hash(project: &MavenProject, effective: &EffectiveProject) -> AppResult<String> {
    let bytes = serde_json::to_vec(&(project.file_hash.as_str(), effective))?;
    Ok(hex_hash(&bytes))
}

fn graph_fingerprint(conn: &Connection, workspace_id: i64) -> AppResult<String> {
    let mut bytes = Vec::new();
    {
        let mut statement = conn.prepare(
            "SELECT path, model_hash, COALESCE(repository_id, -1)
             FROM maven_projects WHERE workspace_id = ?1 ORDER BY path",
        )?;
        let rows = statement.query_map([workspace_id], |row| {
            Ok(format!(
                "P\0{}\0{}\0{}\n",
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?
            ))
        })?;
        for row in rows {
            bytes.extend_from_slice(row?.as_bytes());
        }
    }
    {
        let mut statement = conn.prepare(
            "SELECT p.path, d.sort_order, d.source_kind,
                    COALESCE(d.source_project_id, -1),
                    COALESCE(d.resolved_path, ''), d.resolution_reason
             FROM maven_dependencies d
             JOIN maven_projects p ON p.id = d.project_id
             WHERE p.workspace_id = ?1
             ORDER BY p.path, d.sort_order, d.id",
        )?;
        let rows = statement.query_map([workspace_id], |row| {
            Ok(format!(
                "D\0{}\0{}\0{}\0{}\0{}\0{}\n",
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?
            ))
        })?;
        for row in rows {
            bytes.extend_from_slice(row?.as_bytes());
        }
    }
    Ok(hex_hash(&bytes))
}

fn path_key(path: &Path) -> String {
    let normalized = std::fs::canonicalize(path).unwrap_or_else(|_| lexical_normalize(path));
    strip_windows_verbatim_prefix(&normalized.to_string_lossy()).replace('\\', "/")
}

fn strip_windows_verbatim_prefix(path: &str) -> String {
    if let Some(rest) = path.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{rest}")
    } else if let Some(rest) = path.strip_prefix(r"\\?\") {
        rest.to_string()
    } else {
        path.to_string()
    }
}

fn lexical_normalize(path: &Path) -> PathBuf {
    use std::path::Component;

    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() && !path.has_root() {
                    normalized.push("..");
                }
            }
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => {
                normalized.push(component.as_os_str());
            }
        }
    }
    normalized
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

#[cfg(test)]
mod tests;
