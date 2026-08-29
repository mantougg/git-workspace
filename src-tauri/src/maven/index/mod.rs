//! Persistent Workspace Maven Index and dependency graph (R-02)。
//!
//! 文件布局（B-04 拆分，设计文档 §4.4）：
//! - 本文件（`mod.rs`）：公共入口和 re-export；
//! - `types`：领域类型（IndexSyncResult、MavenProjectNode、MavenModuleLink、
//!   SourceMapping、DependencyEdge、DependencyGraph、GraphCacheLookup）；
//! - `path`：`path_key`、Windows verbatim 前缀清理、lexical normalize；
//! - `query`：graph / project / dependency / module / source mapping 查询；
//! - `cache`：DependencyGraphCache 与 graph fingerprint；
//! - `mapping`：Source Mapping 与 Artifact 刷新/清理；
//! - `sync`：`sync_workspace_index` 事务同步（项目、父子模块、依赖、
//!   artifact、source mapping 同一事务语义更新，失败不留半套索引）。
//!
//! 公共路径兼容（§5.3）：`maven::sync_workspace_index`、
//! `maven::query_dependency_graph`、`maven::query_project_dependencies`、
//! `maven::refresh_dependency_sources`、`maven::DependencyGraphCache` 等
//! 类型与函数经本模块 re-export 保持可用，调用方零修改。

mod cache;
mod mapping;
mod path;
mod query;
mod sync;
mod types;

pub use cache::DependencyGraphCache;
pub use mapping::refresh_dependency_sources;
pub use query::{query_dependency_graph, query_project_dependencies};
pub use sync::sync_workspace_index;
pub use types::{
    DependencyEdge, DependencyGraph, GraphCacheLookup, IndexSyncResult, MavenModuleLink,
    MavenProjectNode, SourceMapping,
};

#[cfg(test)]
mod tests;
