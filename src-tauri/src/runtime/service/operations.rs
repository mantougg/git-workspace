use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::db::dao;
use crate::error::{AppError, AppResult};
use crate::maven;
use crate::models::task::{RuntimeOp, RuntimeTaskOptions, TaskRequest, TaskType};
use crate::runtime::build::pipeline::execute_build;
use crate::runtime::build::{BuildRequest, RingTail};
use crate::runtime::config;
use crate::runtime::events::{
    BuildCompletedPayload, BuildProgressPayload, BuildStartedPayload, DependencyResolvedPayload,
    ProjectDiscoveredPayload, RestartCompletedPayload, RestartStartedPayload, RuntimeStage, EVENT_BUILD_COMPLETED,
    EVENT_BUILD_PROGRESS, EVENT_BUILD_STARTED, EVENT_DEPENDENCY_RESOLVED, EVENT_PROJECT_DISCOVERED,
    EVENT_RESTART_COMPLETED, EVENT_RESTART_STARTED,
};
use crate::runtime::launch::RuntimeProcessInfo;
use crate::runtime::script_approval::{self, ScriptApproval};

use super::*;

impl RuntimeService {
    /// R-21 §49 Stop & Switch：同步优雅停止（带默认宽限），供保护确认后
    /// 在切换分支前调用；进程不存在返回 None。
    pub fn stop_blocking(&self, workspace_id: i64, runtime_name: &str) -> AppResult<Option<RuntimeProcessInfo>> {
        self.processes.stop_runtime(workspace_id, runtime_name, None)
    }

    /// 调整并发上限：立即作用于两个 permit 池并持久化。
    pub fn set_scheduler_config(&self, config: &SchedulerConfig) -> AppResult<()> {
        let config = config.sanitized();
        self.build_scheduler.set_max(config.max_concurrent_builds);
        self.resolve_scheduler.set_max(config.max_concurrent_resolves);
        config.save(&self.scheduler_config_path)?;
        log::info!("R-12: scheduler config updated: {:?}", config);
        Ok(())
    }

    /// `runtime_get_script_approvals`：全部脚本确认记录（UI 管理列表）。
    pub fn script_approval_list(&self) -> Vec<ScriptApproval> {
        self.script_approvals.list()
    }

    /// `runtime_approve_script`：确认一条脚本。后端从配置读脚本内容、
    /// 计算内容哈希并生成预览——哈希必然与流水线校验的一致。
    /// 返回确认记录（`is_new` 语义由调用方按需忽略）。
    pub fn approve_script(
        &self,
        workspace_id: i64,
        runtime_name: &str,
        script_type: &str,
    ) -> AppResult<ScriptApproval> {
        if script_type != "pre" && script_type != "post" {
            return Err(AppError::RuntimeConfig(format!(
                "script_type 必须是 pre / post，收到 '{script_type}'"
            )));
        }
        let config = {
            let conn = self.db.lock().unwrap();
            config::get_config(&conn, workspace_id, runtime_name)?
        };
        let script = match script_type {
            "pre" => config.pre_build_script.as_deref(),
            _ => config.post_build_script.as_deref(),
        }
        .ok_or_else(|| {
            AppError::RuntimeConfig(format!("Runtime '{runtime_name}' 没有配置 {script_type}_build_script"))
        })?;
        let hash = script_approval::script_hash(script);
        let preview = script_approval::script_preview(script);
        self.script_approvals
            .approve(workspace_id, runtime_name, script_type, &hash, &preview)?;
        Ok(ScriptApproval {
            workspace_id,
            runtime_name: runtime_name.to_string(),
            script_type: script_type.to_string(),
            script_hash: hash,
            preview,
            approved_at: chrono::Utc::now().to_rfc3339(),
            last_executed_at: None,
        })
    }

    /// `runtime_reset_script_approvals`：按范围撤销确认（「不再询问」可重置）。
    /// 返回删除条数。
    pub fn reset_script_approvals(&self, workspace_id: Option<i64>, runtime_name: Option<&str>) -> AppResult<usize> {
        self.script_approvals.reset(workspace_id, runtime_name)
    }

    /// `runtime_build` / `runtime_start` / `runtime_stop` / `runtime_restart`
    /// 共用的单配置任务组装。
    pub fn operation_task_request(&self, req: &RuntimeOperationRequest, op: RuntimeOp) -> TaskRequest {
        TaskRequest {
            task_type: TaskType::Runtime {
                op,
                workspace_id: req.workspace_id,
                runtime_name: req.runtime_name.clone(),
                options: req.options.clone(),
            },
            repo_path: String::new(),
            repo_name: req.runtime_name.clone(),
        }
    }

    /// `runtime_resolve_dependencies` 的任务组装。
    pub fn resolve_task_request(&self, workspace_id: i64) -> TaskRequest {
        TaskRequest {
            task_type: TaskType::Runtime {
                op: RuntimeOp::ResolveDependencies,
                workspace_id,
                runtime_name: String::new(),
                options: RuntimeTaskOptions::default(),
            },
            repo_path: String::new(),
            repo_name: format!("workspace #{workspace_id} 依赖解析"),
        }
    }

    pub(super) fn exec_build(
        &self,
        workspace_id: i64,
        runtime_name: &str,
        options: &RuntimeTaskOptions,
        cancel: &Arc<AtomicBool>,
    ) -> AppResult<Option<String>> {
        let at = Self::now();
        self.emit(
            EVENT_BUILD_STARTED,
            &BuildStartedPayload {
                workspace_id,
                runtime_name: runtime_name.to_string(),
                op: RuntimeOp::Build,
                at: at.clone(),
            },
        );
        self.emit(
            EVENT_BUILD_PROGRESS,
            &BuildProgressPayload {
                workspace_id,
                runtime_name: runtime_name.to_string(),
                process_id: None,
                stage: RuntimeStage::Building,
                at,
            },
        );

        let result = self.run_build(workspace_id, runtime_name, options, cancel);
        let at = Self::now();
        match result {
            Ok(outcome) => {
                self.emit(
                    EVENT_BUILD_COMPLETED,
                    &BuildCompletedPayload {
                        workspace_id,
                        runtime_name: runtime_name.to_string(),
                        process_id: None,
                        success: true,
                        duration_ms: Some(outcome.build_duration_ms as u64),
                        error: None,
                        at,
                    },
                );
                Ok(Some(format!(
                    "构建完成：{} 个模块，耗时 {}ms（策略 {}）",
                    outcome.modules_built.len(),
                    outcome.build_duration_ms,
                    outcome.strategy.as_str()
                )))
            }
            Err(error) => {
                self.emit(
                    EVENT_BUILD_COMPLETED,
                    &BuildCompletedPayload {
                        workspace_id,
                        runtime_name: runtime_name.to_string(),
                        process_id: None,
                        success: false,
                        duration_ms: None,
                        error: Some(error.to_string()),
                        at,
                    },
                );
                Err(error)
            }
        }
    }

    /// Build-only 任务直接驱动 R-09 流水线（不经 Process Manager，
    /// 无进程行、无日志会话；输出行进 RingTail 仅供错误上下文）。
    /// 构建期间不持有 DB 锁（execute_build 按阶段自行加锁，R-12）。
    fn run_build(
        &self,
        workspace_id: i64,
        runtime_name: &str,
        options: &RuntimeTaskOptions,
        cancel: &Arc<AtomicBool>,
    ) -> AppResult<crate::runtime::build::BuildOutcome> {
        let workspace_root = self.workspace_root(workspace_id)?;
        let request = BuildRequest {
            workspace_id,
            runtime_name: runtime_name.to_string(),
            options: build_options_of(options),
        };
        let mut sink = RingTail::new();
        execute_build(
            &self.db,
            &workspace_root,
            &self.graph_cache,
            &self.closure_cache,
            &self.build_scheduler,
            &*self.maven_runner,
            &request,
            &self.script_approvals,
            &mut sink,
            Some(cancel),
        )
    }

    pub(super) fn exec_start(
        &self,
        workspace_id: i64,
        runtime_name: &str,
        options: &RuntimeTaskOptions,
        cancel: &Arc<AtomicBool>,
    ) -> AppResult<Option<String>> {
        // §65 阶段事件（build_started → build_progress* → build_completed →
        // process_started → health_changed）由桥接从生命周期迁移推导。
        let _watch = CancelWatch::start(&self.processes, workspace_id, runtime_name, cancel);
        let info = self
            .processes
            .start(workspace_id, runtime_name, start_options_of(options))?;
        Ok(Some(format!(
            "'{}' 已启动（pid {:?}，端口 {:?}）",
            runtime_name, info.pid, info.ports
        )))
    }

    pub(super) fn exec_stop(&self, workspace_id: i64, runtime_name: &str) -> AppResult<Option<String>> {
        match self.processes.stop_runtime(workspace_id, runtime_name, None)? {
            Some(info) => Ok(Some(format!(
                "'{}' 已停止（进程记录 #{}，状态 {}）",
                runtime_name,
                info.process_id,
                info.status.as_str()
            ))),
            None => Ok(Some(format!("'{}' 没有运行中的进程", runtime_name))),
        }
    }

    /// R-17/R-21 的 Rebuild & Restart 入口：Stop → 完整构建 → Start
    /// （与 `restart` 的 skip_build 复用相对；源码变更后必须重建）。
    pub(super) fn exec_rebuild_restart(
        &self,
        workspace_id: i64,
        runtime_name: &str,
        options: &RuntimeTaskOptions,
        cancel: &Arc<AtomicBool>,
    ) -> AppResult<Option<String>> {
        self.emit(
            EVENT_RESTART_STARTED,
            &RestartStartedPayload {
                workspace_id,
                runtime_name: runtime_name.to_string(),
                at: Self::now(),
            },
        );
        let _watch = CancelWatch::start(&self.processes, workspace_id, runtime_name, cancel);
        if self.processes.stop_runtime(workspace_id, runtime_name, None)?.is_some() {
            log::info!("R-17: rebuild-restart stopped previous instance of '{runtime_name}'");
        }
        let mut start_options = start_options_of(options);
        start_options.skip_build = false;
        let result = self.processes.start(workspace_id, runtime_name, start_options);
        let (success, error) = match &result {
            Ok(_) => (true, None),
            Err(e) => (false, Some(e.to_string())),
        };
        self.emit(
            EVENT_RESTART_COMPLETED,
            &RestartCompletedPayload {
                workspace_id,
                runtime_name: runtime_name.to_string(),
                success,
                error,
                at: Self::now(),
            },
        );
        let info = result?;
        Ok(Some(format!("'{}' 已重建并重启（pid {:?}）", runtime_name, info.pid)))
    }

    pub(super) fn exec_restart(
        &self,
        workspace_id: i64,
        runtime_name: &str,
        options: &RuntimeTaskOptions,
        cancel: &Arc<AtomicBool>,
    ) -> AppResult<Option<String>> {
        self.emit(
            EVENT_RESTART_STARTED,
            &RestartStartedPayload {
                workspace_id,
                runtime_name: runtime_name.to_string(),
                at: Self::now(),
            },
        );
        let _watch = CancelWatch::start(&self.processes, workspace_id, runtime_name, cancel);
        let result = self
            .processes
            .restart(workspace_id, runtime_name, start_options_of(options));
        let (success, error) = match &result {
            Ok(_) => (true, None),
            Err(e) => (false, Some(e.to_string())),
        };
        self.emit(
            EVENT_RESTART_COMPLETED,
            &RestartCompletedPayload {
                workspace_id,
                runtime_name: runtime_name.to_string(),
                success,
                error,
                at: Self::now(),
            },
        );
        let info = result?;
        Ok(Some(format!("'{}' 已重启（pid {:?}）", runtime_name, info.pid)))
    }

    /// §63 `resolve_dependencies`：发现 + 索引同步，全程本地（全局约束 §10
    /// 网络边界；远程解析发生在 Build，不在此）。
    pub(super) fn exec_resolve(&self, workspace_id: i64, cancel: &Arc<AtomicBool>) -> AppResult<Option<String>> {
        // §66：Dependency Resolve 并发限流（默认 4）；排队可取消。
        let _permit = self
            .resolve_scheduler
            .acquire_cancelable(cancel)
            .ok_or_else(|| AppError::Task("依赖解析已取消（排队等待解析位时）".into()))?;

        let (root, scan_depth) = {
            let conn = self.db.lock().unwrap();
            let ws = dao::get_workspace(&conn, workspace_id)?;
            (PathBuf::from(ws.path), ws.scan_depth.max(1) as usize)
        };

        // 同步前的已知项目集合（增量发现 diff 用；首次同步为空）。
        let known_paths: BTreeSet<String> = {
            let conn = self.db.lock().unwrap();
            maven::query_dependency_graph(&conn, workspace_id)
                .map(|g| {
                    g.projects
                        .iter()
                        .map(|p| p.path.to_string_lossy().to_string())
                        .collect()
                })
                .unwrap_or_default()
        };

        if cancel.load(Ordering::Relaxed) {
            return Err(AppError::Task("依赖解析已取消".into()));
        }
        let discovery = maven::discover_poms(&root, scan_depth, Some(&self.pom_cache), Some(cancel));
        if cancel.load(Ordering::Relaxed) {
            return Err(AppError::Task("依赖解析已取消".into()));
        }

        let local_repository = maven::settings::resolve_local_repository(None);
        let stats = {
            let mut conn = self.db.lock().unwrap();
            maven::sync_workspace_index(&mut conn, workspace_id, &discovery, &local_repository)?
        };
        // 索引已变：图 / 闭包缓存失效（下次读取重建）。
        self.graph_cache.invalidate_all();
        self.closure_cache.invalidate_all();

        let graph = {
            let conn = self.db.lock().unwrap();
            maven::query_dependency_graph(&conn, workspace_id)?
        };

        // project_discovered：增量发现的项目逐个发（有上限，见常量注释）。
        // 路径比较对 Windows 分隔符不敏感（DB 索引为正斜杠，discovery 为
        // 原生路径，R-14 修复）。
        let new_projects: Vec<_> = discovery
            .projects
            .iter()
            .filter(|p| !known_paths.contains(&p.path.to_string_lossy().replace('\\', "/")))
            .collect();
        if !known_paths.is_empty() && new_projects.len() <= MAX_PROJECT_DISCOVERED_EVENTS {
            for project in new_projects {
                self.emit(
                    EVENT_PROJECT_DISCOVERED,
                    &ProjectDiscoveredPayload {
                        workspace_id,
                        path: display_path(&root, &project.path),
                        coordinates: format!("{}:{}:{}", project.group_id, project.artifact_id, project.version),
                        packaging: project.packaging.clone(),
                        at: Self::now(),
                    },
                );
            }
        }

        self.emit(
            EVENT_DEPENDENCY_RESOLVED,
            &DependencyResolvedPayload {
                workspace_id,
                projects: graph.projects.len(),
                dependencies: graph.dependencies.len(),
                source_mappings: graph.source_mappings.len(),
                inserted: stats.inserted,
                updated: stats.updated,
                removed: stats.removed,
                elapsed_ms: discovery.elapsed_ms as u64,
                at: Self::now(),
            },
        );

        Ok(Some(format!(
            "依赖解析完成：{} 个项目 / {} 条依赖边 / {} 条源码映射（新增 {}、更新 {}、移除 {}，{}ms）",
            graph.projects.len(),
            graph.dependencies.len(),
            graph.source_mappings.len(),
            stats.inserted,
            stats.updated,
            stats.removed,
            discovery.elapsed_ms
        )))
    }
}

/// `project_discovered` 单次同步的爆发上限：增量同步按项目逐个发射；
/// 首次全量发现（大量新项目）只发 `dependency_resolved` 汇总，UI 据此
/// 重拉 `runtime_list_projects`，避免事件洪泛（R-12 高频聚合约束）。
const MAX_PROJECT_DISCOVERED_EVENTS: usize = 50;

/// 相对 workspace 根展示 POM 所在目录（事件 payload 用）。
fn display_path(root: &std::path::Path, pom_path: &std::path::Path) -> String {
    let relative = pom_path
        .strip_prefix(root)
        .unwrap_or(pom_path)
        .to_string_lossy()
        .to_string();
    relative.strip_suffix("/pom.xml").unwrap_or(&relative).to_string()
}
