//! Maven 依赖图缓存（R-02，B-04 拆分）：`DependencyGraphCache` 与 graph
//! fingerprint 计算。失效由同步用例统一负责，查询侧只读（§7）。

use moka::sync::Cache;
use rusqlite::Connection;

use crate::error::AppResult;
use crate::maven::parser::hex_hash;

use super::query::query_dependency_graph;
use super::types::{DependencyGraph, GraphCacheLookup};

pub(super) const GRAPH_CACHE_CAPACITY: u64 = 64;

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
            return Ok(GraphCacheLookup { graph, cache_hit: true });
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

    #[cfg(test)]
    pub(super) fn max_capacity(&self) -> Option<u64> {
        self.inner.policy().max_capacity()
    }
}

impl Default for DependencyGraphCache {
    fn default() -> Self {
        Self::new()
    }
}

pub(super) fn graph_fingerprint(conn: &Connection, workspace_id: i64) -> AppResult<String> {
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
