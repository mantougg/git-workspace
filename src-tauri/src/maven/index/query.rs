//! Maven Index 查询侧（R-02，B-04 拆分）：graph / project / dependency /
//! module / source mapping 的只读查询。查询不改缓存失效状态（§7）。

use std::path::PathBuf;

use rusqlite::Connection;

use crate::error::{AppError, AppResult};
use crate::maven::model::{DependencyScope, MavenDependency, PomCoordinates};
use crate::maven::resolver::{DependencySource, ResolutionReason};

use super::cache::graph_fingerprint;
use super::types::{
    DependencyEdge, DependencyGraph, MavenModuleLink, MavenProjectNode, SourceMapping,
};

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

pub(super) fn query_dependencies_with_filter(
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
