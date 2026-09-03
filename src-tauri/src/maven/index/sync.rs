//! Workspace Maven 索引事务同步（R-02，B-04 拆分）。
//!
//! [`sync_workspace_index`] 是风险最高的入口，必须继续保持（§4.4）：
//! 1. 项目、父子模块、依赖、artifact、source mapping 在同一事务语义下更新；
//! 2. 同步失败不留半套索引（事务回滚）；
//! 3. 索引变化后 `graph_cache` 与 `closure_cache` 失效时机不变；
//! 4. Windows 路径比较统一经 `path.rs` 归一化。
//!
//! DB 写操作保持短事务，不在同步期间持有锁执行外部命令（§7）。

use std::collections::{BTreeSet, HashMap, HashSet};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use rusqlite::{params, Connection, Transaction};

use crate::error::{AppError, AppResult};
use crate::maven::discovery::MavenDiscoveryResult;
use crate::maven::effective::EffectiveProject;
use crate::maven::model::{MavenProject, PomCoordinates};
use crate::maven::parser::hex_hash;
use crate::maven::resolver::{local_artifact_path, resolve_dependency, IndexedMavenProject, WorkspaceMavenIndex};

use super::cache::graph_fingerprint;
use super::mapping::{mapping_fingerprint_rows, mapping_row_key, prune_artifacts, replace_source_mappings};
use super::path::path_key;
use super::types::IndexSyncResult;

#[derive(Clone)]
pub(super) struct ProjectInput {
    pub(super) project: MavenProject,
    pub(super) effective: EffectiveProject,
    pub(super) path: String,
    pub(super) project_path: PathBuf,
    pub(super) repository_id: Option<i64>,
    pub(super) model_hash: String,
}

#[derive(Clone)]
pub(super) struct ProjectRecord {
    pub(super) project_id: i64,
    pub(super) repository_id: Option<i64>,
    pub(super) coordinates: PomCoordinates,
    pub(super) project_path: PathBuf,
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
            AppError::DependencyResolve(format!("effective Maven model missing for {}", project.path.display()))
        })?;
        let repository_id = find_repository_id(&project.path, &repository_roots);
        inputs.push(ProjectInput {
            project: project.clone(),
            effective: (*effective).clone(),
            path,
            project_path: project.path.parent().unwrap_or_else(|| Path::new("")).to_path_buf(),
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
    replace_source_mappings(&tx, workspace_id, &inputs, &records, mapping_changed, &recompute_paths)?;
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
        super::mapping::refresh_dependency_sources(conn, workspace_id, local_repository)?;
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

fn existing_projects(conn: &Connection, workspace_id: i64) -> AppResult<HashMap<String, (String, String)>> {
    let mut statement =
        conn.prepare("SELECT path, pom_hash, model_hash FROM maven_projects WHERE workspace_id = ?1")?;
    let rows = statement.query_map([workspace_id], |row| {
        Ok((
            row.get::<_, String>(0)?,
            (row.get::<_, String>(1)?, row.get::<_, String>(2)?),
        ))
    })?;
    Ok(rows.collect::<Result<HashMap<_, _>, _>>()?)
}

fn delete_stale_projects(tx: &Transaction<'_>, workspace_id: i64, stale_paths: &[String]) -> AppResult<()> {
    let mut statement = tx.prepare("DELETE FROM maven_projects WHERE workspace_id = ?1 AND path = ?2")?;
    for path in stale_paths {
        statement.execute(params![workspace_id, path])?;
    }
    Ok(())
}

fn upsert_projects(tx: &Transaction<'_>, workspace_id: i64, inputs: &[ProjectInput], now: &str) -> AppResult<()> {
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

pub(super) fn load_project_records(
    tx: &Transaction<'_>,
    workspace_id: i64,
) -> AppResult<HashMap<String, ProjectRecord>> {
    let mut statement = tx.prepare(
        "SELECT id, repository_id, path, group_id, artifact_id, version
         FROM maven_projects WHERE workspace_id = ?1",
    )?;
    let rows = statement.query_map([workspace_id], |row| {
        let path = row.get::<_, String>(2)?;
        let project_path = Path::new(&path).parent().unwrap_or_else(|| Path::new("")).to_path_buf();
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
            let gav = format!("{}:{}:{}", parent.group_id, parent.artifact_id, parent.version);
            by_gav
                .get(&gav)
                .filter(|matches| matches.len() == 1)
                .map(|matches| matches[0])
        });
        statement.execute(params![parent_id, record.project_id])?;
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
    let workspace_index = WorkspaceMavenIndex::new(records.values().map(|record| IndexedMavenProject {
        project_id: record.project_id,
        repository_id: record.repository_id,
        coordinates: record.coordinates.clone(),
        project_path: record.project_path.clone(),
    }));

    let mut delete_dependencies = tx.prepare("DELETE FROM maven_dependencies WHERE project_id = ?1")?;
    let mut delete_modules = tx.prepare("DELETE FROM maven_modules WHERE parent_project_id = ?1")?;
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
        let record = records
            .get(&input.path)
            .ok_or_else(|| AppError::DependencyResolve(format!("indexed Maven project missing for {}", input.path)))?;
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

            if let Some(version) = dependency.version.as_deref().filter(|value| !value.is_empty()) {
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
            let module_id = records.get(&path_key(&pom_path)).map(|item| item.project_id);
            insert_module.execute(params![record.project_id, module_id, module.path])?;
        }
    }

    prune_artifacts(tx, workspace_id)?;
    Ok(())
}

fn effective_model_hash(project: &MavenProject, effective: &EffectiveProject) -> AppResult<String> {
    let bytes = serde_json::to_vec(&(project.file_hash.as_str(), effective))?;
    Ok(hex_hash(&bytes))
}
