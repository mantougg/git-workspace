use std::path::Path;

use crate::db::dao;
use crate::error::{AppError, AppResult};
use crate::maven::{self, MavenProjectNode, RuntimeScope};
use crate::runtime::launch::RuntimeProcessInfo;
use crate::runtime::logs::{LogEntry, LogExportOutcome};

use super::*;

impl RuntimeService {
    /// `runtime_list_projects`：workspace 的 Maven 项目索引（DB 视角，
    /// 热路径；未同步过时为空，由 UI 引导触发 `runtime_resolve_dependencies`）。
    pub fn list_projects(&self, workspace_id: i64) -> AppResult<Vec<MavenProjectNode>> {
        let conn = self.db.lock().unwrap();
        Ok(maven::query_dependency_graph(&conn, workspace_id)?.projects)
    }

    /// `runtime_inspect_project`：按 path / artifactId / groupId:artifactId
    /// 三级匹配定位项目（与 R-09 `find_root_project` 同口径）。
    pub fn inspect_project(
        &self,
        workspace_id: i64,
        project: &str,
    ) -> AppResult<ProjectInspection> {
        let conn = self.db.lock().unwrap();
        let graph = maven::query_dependency_graph(&conn, workspace_id)?;
        let node = find_project(&graph.projects, project).ok_or_else(|| {
            AppError::ProjectNotFound(format!(
                "项目 '{project}' 不在 workspace #{workspace_id} 的 Maven 索引中；\
                 请先执行依赖解析（runtime.resolve_dependencies）"
            ))
        })?;
        let project_id = node.project_id;
        Ok(ProjectInspection {
            project: node.clone(),
            modules: graph
                .modules
                .iter()
                .filter(|m| m.parent_project_id == project_id)
                .cloned()
                .collect(),
            parent_project_id: graph
                .modules
                .iter()
                .find(|m| m.module_project_id == Some(project_id))
                .map(|m| m.parent_project_id),
            dependencies: graph
                .dependencies
                .iter()
                .filter(|e| e.from_project_id == project_id)
                .cloned()
                .collect(),
            source_mappings: graph
                .source_mappings
                .iter()
                .filter(|m| m.project_id == project_id)
                .cloned()
                .collect(),
        })
    }

    /// `runtime_get_dependency_graph`：全量图（默认截断保护）或单项目下钻。
    pub fn dependency_graph(
        &self,
        workspace_id: i64,
        project_id: Option<i64>,
        max_edges: Option<usize>,
    ) -> AppResult<DependencyGraphView> {
        let conn = self.db.lock().unwrap();
        let graph = maven::query_dependency_graph(&conn, workspace_id)?;
        let (dependencies, total, truncated) = match project_id {
            Some(pid) => {
                let edges = maven::query_project_dependencies(&conn, pid)?;
                let total = edges.len();
                (edges, total, false)
            }
            None => {
                let cap = max_edges.unwrap_or(DEFAULT_MAX_GRAPH_EDGES);
                let total = graph.dependencies.len();
                let truncated = total > cap;
                let edges = graph.dependencies.into_iter().take(cap).collect();
                (edges, total, truncated)
            }
        };
        Ok(DependencyGraphView {
            workspace_id,
            fingerprint: graph.fingerprint,
            projects: graph.projects,
            modules: graph.modules,
            dependencies,
            source_mappings: graph.source_mappings,
            total_dependencies: total,
            truncated,
        })
    }

    /// `runtime_get_closure`（R-13）：按给定 Scope 计算闭包预览，供
    /// Runtime Scope 视图使用（R-03 fingerprint 缓存热路径）。
    pub fn closure_preview(
        &self,
        workspace_id: i64,
        project: &str,
        scope: &RuntimeScope,
    ) -> AppResult<ClosurePreview> {
        let conn = self.db.lock().unwrap();
        let graph = self.graph_cache.get_or_load(&conn, workspace_id)?.graph;
        let node = find_project(&graph.projects, project).ok_or_else(|| {
            AppError::ProjectNotFound(format!(
                "项目 '{project}' 不在 workspace #{workspace_id} 的 Maven 索引中；\
                 请先执行依赖解析（runtime.resolve_dependencies）"
            ))
        })?;
        let lookup = self
            .closure_cache
            .get_or_compute(&graph, node.project_id, scope)?;
        Ok(ClosurePreview {
            closure: lookup.closure,
            cache_hit: lookup.cache_hit,
        })
    }

    /// `runtime_list_processes`。
    pub fn list_processes(&self, workspace_id: i64) -> AppResult<Vec<RuntimeProcessInfo>> {
        self.processes.list_processes(workspace_id)
    }

    /// `runtime_process_status`。
    pub fn process_status(&self, process_id: i64) -> AppResult<Option<RuntimeProcessInfo>> {
        self.processes.get_process(process_id)
    }

    /// R-21 §49 操作保护：全部工作区「运行中应用」摘要（轻量 DB 读，
    /// 供前端 Checkout 前的确认弹窗；不做任何 git 操作）。
    pub fn running_briefs(&self) -> Vec<crate::runtime::git_link::RuntimeRunningBrief> {
        let conn = self.db.lock().unwrap();
        let mut briefs = Vec::new();
        if let Ok(workspaces) = dao::list_workspaces(&conn) {
            for ws in workspaces {
                if let Ok(rows) = crate::runtime::launch::store::list_processes(&conn, ws.id) {
                    for row in rows {
                        if row.status.is_active() {
                            briefs.push(crate::runtime::git_link::RuntimeRunningBrief {
                                workspace_id: ws.id,
                                runtime_name: row.runtime_name,
                                status: row.status.as_str().to_string(),
                            });
                        }
                    }
                }
            }
        }
        briefs
    }

    /// R-16 `runtime_get_health`：单进程健康快照（无探针为 None）。
    pub fn get_health(&self, process_id: i64) -> Option<crate::runtime::health::HealthSnapshot> {
        self.health.snapshot(process_id)
    }

    /// R-16 `runtime_list_health`：workspace 下全部探针快照。
    pub fn list_health(&self, workspace_id: i64) -> Vec<crate::runtime::health::HealthSnapshot> {
        self.health.snapshots(workspace_id)
    }

    /// `runtime_get_logs`（R-11 引擎 search：跨滚动段、时间序、脱敏在写入侧已完成）。
    pub fn get_logs(&self, query: &RuntimeLogQuery) -> AppResult<Vec<LogEntry>> {
        let root = self.workspace_root(query.workspace_id)?;
        self.logs
            .search(&root, &query.runtime_name, query.process_id, &query.filter)
    }

    /// 日志 tail（R-11 引擎 tail：活跃会话读环形缓冲，否则文件尾部）。
    /// AI 上下文（AI-03「日志尾部」）等需要最近 N 行的场景用。
    pub fn tail_logs(&self, query: &RuntimeLogQuery, n: usize) -> AppResult<Vec<LogEntry>> {
        let root = self.workspace_root(query.workspace_id)?;
        self.logs
            .tail(&root, &query.runtime_name, query.process_id, n)
    }

    /// 过滤 + tail：最近 n 行匹配项（如「最近错误日志」，AI-03 错误诊断上下文）。
    pub fn search_logs_tail(&self, query: &RuntimeLogQuery, n: usize) -> AppResult<Vec<LogEntry>> {
        let root = self.workspace_root(query.workspace_id)?;
        self.logs.search_tail(
            &root,
            &query.runtime_name,
            query.process_id,
            &query.filter,
            n,
        )
    }

    /// `runtime_clear_logs`。
    pub fn clear_logs(&self, query: &RuntimeLogQuery) -> AppResult<()> {
        let root = self.workspace_root(query.workspace_id)?;
        self.logs
            .clear(&root, &query.runtime_name, query.process_id)
    }

    /// R-13 `runtime_export_logs`：导出到用户选择的目标文件（R-11 §36，
    /// 与 `search` 同一过滤管道，导出内容与显示一致）。
    pub fn export_logs(&self, query: &RuntimeLogQuery, dest: &str) -> AppResult<LogExportOutcome> {
        let root = self.workspace_root(query.workspace_id)?;
        self.logs.export(
            &root,
            &query.runtime_name,
            query.process_id,
            &query.filter,
            Path::new(dest),
        )
    }

    /// 当前生效的调度并发上限（§66 可配置的读侧）。
    pub fn scheduler_config(&self) -> SchedulerConfig {
        SchedulerConfig {
            max_concurrent_builds: self.build_scheduler.max(),
            max_concurrent_resolves: self.resolve_scheduler.max(),
        }
    }
}
