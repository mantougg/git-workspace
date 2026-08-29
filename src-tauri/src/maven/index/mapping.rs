//! Source Mapping 与 Artifact 刷新/清理（R-02，B-04 拆分）。
//!
//! - Source Mapping：映射指纹（变更检测）、行键、事务内替换；
//! - Artifact：`~/.m2` 变化后的 source 重解析刷新与孤儿 artifact 清理。

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::Path;

use rusqlite::{params, Connection, Transaction};

use crate::error::{AppError, AppResult};
use crate::maven::resolver::{
    local_artifact_path, resolve_dependency, IndexedMavenProject, WorkspaceMavenIndex,
};

use super::path::path_key;
use super::query::query_dependencies_with_filter;
use super::sync::{load_project_records, ProjectInput, ProjectRecord};

pub(super) fn mapping_fingerprint_rows(
    conn: &Connection,
    workspace_id: i64,
) -> AppResult<BTreeSet<String>> {
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

pub(super) fn mapping_row_key(input: &ProjectInput) -> String {
    format!(
        "{}:{}:{}|{:?}|{}",
        input.effective.group_id,
        input.effective.artifact_id,
        input.effective.version,
        input.repository_id,
        path_key(&input.project_path)
    )
}

pub(super) fn replace_source_mappings(
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

pub(super) fn prune_artifacts(tx: &Transaction<'_>, workspace_id: i64) -> AppResult<()> {
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
