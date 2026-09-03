use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::AppResult;
use crate::maven::{DependencyEdge, MavenModuleLink, MavenProjectNode, RuntimeClosure, SourceMapping};
use crate::models::task::RuntimeTaskOptions;
use crate::runtime::build::scheduler::{DEFAULT_MAX_CONCURRENT_BUILDS, DEFAULT_MAX_CONCURRENT_RESOLVES};
use crate::runtime::config;
use crate::runtime::logs::LogFilter;

// Scheduler 配置（§66 可配置）
// ---------------------------------------------------------------------------

/// Runtime Task Scheduler 并发上限。落 `<app_data_dir>/runtime-scheduler.json`
/// （机器级配置，先例：`health-weights.json`），serde default 容错缺字段。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SchedulerConfig {
    #[serde(default = "default_max_builds")]
    pub max_concurrent_builds: usize,
    #[serde(default = "default_max_resolves")]
    pub max_concurrent_resolves: usize,
}

fn default_max_builds() -> usize {
    DEFAULT_MAX_CONCURRENT_BUILDS
}

fn default_max_resolves() -> usize {
    DEFAULT_MAX_CONCURRENT_RESOLVES
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            max_concurrent_builds: DEFAULT_MAX_CONCURRENT_BUILDS,
            max_concurrent_resolves: DEFAULT_MAX_CONCURRENT_RESOLVES,
        }
    }
}

impl SchedulerConfig {
    /// 从 JSON 文件加载；文件不存在 / 解析失败回退默认（记日志，不致命）。
    pub fn load(path: &std::path::Path) -> Self {
        match std::fs::read_to_string(path) {
            Ok(text) => match serde_json::from_str::<SchedulerConfig>(&text) {
                Ok(config) => {
                    let config = config.sanitized();
                    log::info!("R-12: scheduler config loaded from {:?}: {:?}", path, config);
                    config
                }
                Err(e) => {
                    log::warn!("R-12: invalid scheduler config {:?}: {}; using defaults", path, e);
                    Self::default()
                }
            },
            Err(_) => Self::default(),
        }
    }

    /// 持久化（原子写，复用 R-07 的写盘助手）。
    pub fn save(&self, path: &std::path::Path) -> AppResult<()> {
        config::write_json_atomic(path, &self.sanitized())
    }

    /// 上限至少为 1（0 会死锁 permit 池）。
    pub(super) fn sanitized(mut self) -> Self {
        self.max_concurrent_builds = self.max_concurrent_builds.max(1);
        self.max_concurrent_resolves = self.max_concurrent_resolves.max(1);
        self
    }
}

/// 生产配置路径（`<app_data_dir>/runtime-scheduler.json`）。
pub fn scheduler_config_path() -> PathBuf {
    crate::get_app_data_dir().join("runtime-scheduler.json")
}

// ---------------------------------------------------------------------------
// IPC 请求 / 视图类型（§63；golden 快照覆盖）
// ---------------------------------------------------------------------------

/// `runtime_build` / `runtime_start` / `runtime_restart` 的请求。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeOperationRequest {
    pub workspace_id: i64,
    pub runtime_name: String,
    #[serde(default)]
    pub options: RuntimeTaskOptions,
}

/// `runtime_get_logs` 的请求。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeLogQuery {
    pub workspace_id: i64,
    pub runtime_name: String,
    pub process_id: i64,
    #[serde(default)]
    pub filter: LogFilter,
}

/// `runtime_inspect_project` 的返回：DB 索引视角的项目详情（模块关系、
/// 依赖边、源码映射）。POM 级细节（profiles/plugins）不入索引，需要时由
/// UI 走 `preview_maven_command` 等 R-05 命令查看。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectInspection {
    pub project: MavenProjectNode,
    /// 该项目声明的子模块链接。
    pub modules: Vec<MavenModuleLink>,
    /// 该项目作为子模块时的父项目 id。
    pub parent_project_id: Option<i64>,
    /// 该项目的依赖边（出边）。
    pub dependencies: Vec<DependencyEdge>,
    /// 指向该项目的源码映射。
    pub source_mappings: Vec<SourceMapping>,
}

/// `runtime_get_dependency_graph` 的返回。大 payload 约束（R-12 任务文档）
/// 的落实：`max_edges` 截断依赖边（`truncated` 标记 + `total_dependencies`
/// 给出真实总数），或按 `project_id` 只取单项目出边下钻。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DependencyGraphView {
    pub workspace_id: i64,
    pub fingerprint: String,
    pub projects: Vec<MavenProjectNode>,
    pub modules: Vec<MavenModuleLink>,
    pub dependencies: Vec<DependencyEdge>,
    pub source_mappings: Vec<SourceMapping>,
    pub total_dependencies: usize,
    pub truncated: bool,
}

/// `runtime_get_closure`（R-13）的返回：给定 Scope 下的 Runtime Closure
/// 预览（R-03 §14/§15），`cache_hit` 标记是否命中 graph fingerprint 缓存。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClosurePreview {
    pub closure: RuntimeClosure,
    pub cache_hit: bool,
}
