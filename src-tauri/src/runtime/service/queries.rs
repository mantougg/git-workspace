use std::path::Path;

use crate::db::dao;
use crate::error::{AppError, AppResult};
use crate::maven::{self, MavenProjectNode, RuntimeScope};
use crate::runtime::launch::store;
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
    pub fn inspect_project(&self, workspace_id: i64, project: &str) -> AppResult<ProjectInspection> {
        let conn = self.db.lock().unwrap();
        self.inspect_project_with_connection(&conn, workspace_id, project)
    }

    /// Read-only variant for callers that already hold the shared DB
    /// connection. This preserves the RuntimeService domain lookup while
    /// avoiding recursive locking from AI Context Builder.
    pub(crate) fn inspect_project_with_connection(
        &self,
        conn: &rusqlite::Connection,
        workspace_id: i64,
        project: &str,
    ) -> AppResult<ProjectInspection> {
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
    pub fn closure_preview(&self, workspace_id: i64, project: &str, scope: &RuntimeScope) -> AppResult<ClosurePreview> {
        let conn = self.db.lock().unwrap();
        let graph = self.graph_cache.get_or_load(&conn, workspace_id)?.graph;
        let node = find_project(&graph.projects, project).ok_or_else(|| {
            AppError::ProjectNotFound(format!(
                "项目 '{project}' 不在 workspace #{workspace_id} 的 Maven 索引中；\
                 请先执行依赖解析（runtime.resolve_dependencies）"
            ))
        })?;
        let lookup = self.closure_cache.get_or_compute(&graph, node.project_id, scope)?;
        Ok(ClosurePreview {
            closure: lookup.closure,
            cache_hit: lookup.cache_hit,
        })
    }

    /// `runtime_list_processes`。
    pub fn list_processes(&self, workspace_id: i64) -> AppResult<Vec<RuntimeProcessInfo>> {
        self.processes.list_processes(workspace_id)
    }

    pub(crate) fn list_processes_with_connection(
        &self,
        conn: &rusqlite::Connection,
        workspace_id: i64,
    ) -> AppResult<Vec<RuntimeProcessInfo>> {
        self.processes.list_processes_with_connection(conn, workspace_id)
    }

    /// `runtime_process_status`。
    pub fn process_status(&self, process_id: i64) -> AppResult<Option<RuntimeProcessInfo>> {
        self.processes.get_process(process_id)
    }

    pub(crate) fn process_status_with_connection(
        &self,
        conn: &rusqlite::Connection,
        process_id: i64,
    ) -> AppResult<Option<RuntimeProcessInfo>> {
        self.processes.get_process_with_connection(conn, process_id)
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
        self.logs.tail(&root, &query.runtime_name, query.process_id, n)
    }

    pub(crate) fn tail_logs_with_connection(
        &self,
        conn: &rusqlite::Connection,
        query: &RuntimeLogQuery,
        n: usize,
    ) -> AppResult<Vec<LogEntry>> {
        let root = config::workspace_root(conn, query.workspace_id)?;
        self.logs.tail(&root, &query.runtime_name, query.process_id, n)
    }

    /// 过滤 + tail：最近 n 行匹配项（如「最近错误日志」，AI-03 错误诊断上下文）。
    pub fn search_logs_tail(&self, query: &RuntimeLogQuery, n: usize) -> AppResult<Vec<LogEntry>> {
        let root = self.workspace_root(query.workspace_id)?;
        self.logs
            .search_tail(&root, &query.runtime_name, query.process_id, &query.filter, n)
    }

    pub(crate) fn search_logs_tail_with_connection(
        &self,
        conn: &rusqlite::Connection,
        query: &RuntimeLogQuery,
        n: usize,
    ) -> AppResult<Vec<LogEntry>> {
        let root = config::workspace_root(conn, query.workspace_id)?;
        self.logs
            .search_tail(&root, &query.runtime_name, query.process_id, &query.filter, n)
    }

    /// `runtime_clear_logs`。
    pub fn clear_logs(&self, query: &RuntimeLogQuery) -> AppResult<()> {
        let root = self.workspace_root(query.workspace_id)?;
        self.logs.clear(&root, &query.runtime_name, query.process_id)
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

    /// 注册终端启动的 Runtime 进程。
    ///
    /// 创建一个轻量级进程记录（状态=Running），关联 PTY 会话 ID。
    /// 若 `pty_pid` 有值，同步回填 `runtime_processes.pid`（供
    /// process_alive / metrics 采样使用），并启动后台端口扫描线程。
    pub fn register_terminal_process(
        &self,
        workspace_id: i64,
        runtime_name: &str,
        terminal_session_id: &str,
        pty_pid: Option<u32>,
    ) -> AppResult<i64> {
        let conn = self.db.lock().unwrap();
        let process_id = store::insert_terminal_process(
            &conn,
            workspace_id,
            runtime_name,
            terminal_session_id,
        )?;
        // 回填 PTY 子进程 PID（供 process_alive / metrics 采样）。
        if let Some(pid) = pty_pid {
            let _ = store::set_pid(&conn, process_id, pid, None);
        }
        // 发射 process_started 事件，让前端刷新进程列表
        self.emit(crate::runtime::events::EVENT_PROCESS_STARTED, &serde_json::json!({
            "workspaceId": workspace_id,
            "processId": process_id,
            "runtimeName": runtime_name,
            "terminalSessionId": terminal_session_id,
        }));
        // 若 PID 已知，启动后台端口扫描线程（等待进程启动后枚举监听端口）。
        if let Some(root_pid) = pty_pid {
            let db = Arc::clone(&self.db);
            let emitter = Arc::clone(&self.emitter);
            let rn = runtime_name.to_string();
            std::thread::Builder::new()
                .name(format!("terminal-port-scan-{process_id}"))
                .spawn(move || {
                    // 等待进程启动并开始监听。
                    std::thread::sleep(std::time::Duration::from_secs(3));
                    // 枚举进程树 PID 集合。
                    let tree_pids = crate::process::collect_tree_pids(root_pid);
                    let tree_set: std::collections::HashSet<u32> = tree_pids.iter().copied().collect();
                    // 枚举 OS 监听表，过滤出进程树内的端口。
                    let detected_ports: Vec<u16> = crate::process::port::detect_listening_ports()
                        .unwrap_or_default()
                        .into_iter()
                        .filter(|lp| tree_set.contains(&lp.pid))
                        .map(|lp| lp.port)
                        .collect::<std::collections::HashSet<u16>>()
                        .into_iter()
                        .collect();
                    if detected_ports.is_empty() {
                        log::debug!("terminal-port-scan-{process_id}: no listening ports detected in process tree [{root_pid}]");
                        return;
                    }
                    // 持久化到 DB。
                    let conn = db.lock().unwrap();
                    if let Err(e) = store::set_ports(&conn, process_id, &detected_ports) {
                        log::warn!("terminal-port-scan-{process_id}: failed to persist ports: {e}");
                        return;
                    }
                    // 确权：与 PortAttribution 模式一致，记录端口归属。
                    let port_pids: std::collections::BTreeMap<u16, u32> = {
                        let all_listening = crate::process::port::detect_listening_ports().unwrap_or_default();
                        let mut map = std::collections::BTreeMap::new();
                        for lp in &all_listening {
                            if tree_set.contains(&lp.pid) && detected_ports.contains(&lp.port) {
                                map.entry(lp.port).or_insert(lp.pid);
                            }
                        }
                        map
                    };
                    if !port_pids.is_empty() {
                        let _ = store::set_port_attribution(&conn, process_id, &port_pids);
                    }
                    log::info!(
                        "terminal-port-scan-{process_id}: detected ports {:?} for process tree [{root_pid}]",
                        detected_ports
                    );
                    // 发射 process_started 事件，触发前端刷新以获取更新后的端口信息。
                    emitter.emit(crate::runtime::events::RuntimeEmission::new(
                        crate::runtime::events::EVENT_PROCESS_STARTED,
                        &serde_json::json!({
                            "workspaceId": workspace_id,
                            "processId": process_id,
                            "runtimeName": rn,
                            "portsDetected": detected_ports,
                        }),
                    ));
                })
                .ok(); // 线程 spawn 失败不阻塞主流程。
        }
        Ok(process_id)
    }

    /// 注销终端启动的 Runtime 进程。
    ///
    /// 当 PTY 会话退出时调用，将关联的进程记录更新为终态。
    pub fn unregister_terminal_process(
        &self,
        terminal_session_id: &str,
        exit_code: Option<i32>,
    ) -> AppResult<()> {
        let conn = self.db.lock().unwrap();
        if let Some(row) = store::find_by_terminal_session(&conn, terminal_session_id)? {
            let to = if exit_code.is_some() && exit_code != Some(0) {
                crate::runtime::launch::LifecycleStatus::Failed
            } else {
                crate::runtime::launch::LifecycleStatus::Stopped
            };
            // 直接更新状态，不走 transition_status（避免非法迁移检查）
            let now = chrono::Utc::now().to_rfc3339();
            conn.execute(
                "UPDATE runtime_processes SET status = ?1, exit_code = ?2, stopped_at = ?3, updated_at = ?3 WHERE id = ?4",
                rusqlite::params![to.as_str(), exit_code, now, row.id],
            )?;
            // 发射 process_stopped 事件
            self.emit(crate::runtime::events::EVENT_PROCESS_STOPPED, &serde_json::json!({
                "workspaceId": row.workspace_id,
                "processId": row.id,
                "runtimeName": row.runtime_name,
            }));
        }
        Ok(())
    }

    /// 获取终端启动的进程的 PTY 会话 ID。
    ///
    /// 用于停止终端进程时，返回关联的 session_id 以便命令层关闭 PTY 会话。
    pub fn get_terminal_session_id(&self, process_id: i64) -> AppResult<Option<String>> {
        let conn = self.db.lock().unwrap();
        let row = store::get_process(&conn, process_id)?
            .ok_or_else(|| AppError::NotFound(format!("进程记录 #{process_id} 不存在")))?;
        Ok(row.terminal_session_id)
    }
}
