//! Runtime Event API（R-12，§64）。
//!
//! 事件名遵循 §64 的 `runtime.<event>` 点分命名，每个事件一个 Tauri event。
//! 高频事件（`process_output` / `build_progress`）不在本层再开节流：
//! `process_output` 的批次直接来自 R-11 日志引擎的聚合 worker
//! （`LogLimits.aggregate_interval` 100ms / `batch_max_lines` 256），
//! `build_progress` 只在生命周期阶段迁移时发出（低频），满足全局约束
//! 「高频事件批量聚合推送」。
//!
//! 事件是「通知」，不是状态传输：payload 只携带 id / 名称 / 时间戳等轻量
//! 字段，UI 收到后经 `runtime_process_status` / `runtime_get_logs` 等查询
//! 命令拉取详情（大 payload 分页走查询命令，不走事件）。
//!
//! 生产链路：R-10 Process Manager / R-11 日志引擎 → [`RuntimeEventSink`]
//! （[`TauriRuntimeBridge`]）→ [`RuntimeEventEmitter`]（[`TauriRuntimeEmitter`]）
//! → 前端。桥接的映射规则集中在纯函数 [`map_internal_event`]，单测覆盖。

use std::sync::{Arc, Mutex};

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::models::task::RuntimeOp;
use crate::runtime::launch::{LifecycleStatus, RuntimeEvent, RuntimeEventSink};
use crate::runtime::logs::LogLine;

// ---------------------------------------------------------------------------
// §64 event names
//
// F-15：Tauri listen 校验事件名只允许字母数字/`-`/`/`/`:`/`_`——不能用 `.`，
// 带点会被前端 listen 拒绝（「invalid args `event` for command `listen`」），
// 订阅链抛错曾阻断 Runtime 视图的数据加载。
// ---------------------------------------------------------------------------

pub const EVENT_PROJECT_DISCOVERED: &str = "runtime_project_discovered";
pub const EVENT_DEPENDENCY_RESOLVED: &str = "runtime_dependency_resolved";
pub const EVENT_BUILD_STARTED: &str = "runtime_build_started";
pub const EVENT_BUILD_PROGRESS: &str = "runtime_build_progress";
pub const EVENT_BUILD_COMPLETED: &str = "runtime_build_completed";
pub const EVENT_PROCESS_STARTED: &str = "runtime_process_started";
pub const EVENT_PROCESS_OUTPUT: &str = "runtime_process_output";
pub const EVENT_PROCESS_STOPPED: &str = "runtime_process_stopped";
pub const EVENT_PROCESS_FAILED: &str = "runtime_process_failed";
pub const EVENT_HEALTH_CHANGED: &str = "runtime_health_changed";
pub const EVENT_FILE_CHANGED: &str = "runtime_file_changed";
pub const EVENT_RESTART_STARTED: &str = "runtime_restart_started";
pub const EVENT_RESTART_COMPLETED: &str = "runtime_restart_completed";

/// Start 流水线的 UI 阶段（§65：Preparing ✓ / Resolving ✓ / Building ▓ /
/// Starting ○）。与 `LifecycleStatus` 的前四个状态一一对应。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RuntimeStage {
    Preparing,
    Resolving,
    Building,
    Starting,
}

/// `health_changed` 的健康取值。R-16 的探针式健康检查之前，由生命周期
/// 迁移推导：进入 Running → Up；停止 / 崩溃 → Down。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HealthStatus {
    Up,
    Down,
}

// ---------------------------------------------------------------------------
// §64 event payloads（全部 camelCase，跨 IPC，有 golden 快照）
// ---------------------------------------------------------------------------

/// `runtime.project_discovered`：依赖解析同步期间新发现的 Maven 项目。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectDiscoveredPayload {
    pub workspace_id: i64,
    /// 相对 workspace 根的 POM 所在目录。
    pub path: String,
    /// `groupId:artifactId:version`。
    pub coordinates: String,
    pub packaging: String,
    /// RFC3339 时间戳。
    pub at: String,
}

/// `runtime.dependency_resolved`：一次 workspace 依赖解析同步完成的汇总。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DependencyResolvedPayload {
    pub workspace_id: i64,
    pub projects: usize,
    pub dependencies: usize,
    pub source_mappings: usize,
    pub inserted: usize,
    pub updated: usize,
    pub removed: usize,
    pub elapsed_ms: u64,
    pub at: String,
}

/// `runtime.build_started`：Build / Start（含其构建阶段）开始。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildStartedPayload {
    pub workspace_id: i64,
    pub runtime_name: String,
    pub op: RuntimeOp,
    pub at: String,
}

/// `runtime.build_progress`：Start 流水线阶段迁移（§65 的 UI 阶段显示）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildProgressPayload {
    pub workspace_id: i64,
    pub runtime_name: String,
    /// Build-only 任务没有进程行，为 `None`。
    pub process_id: Option<i64>,
    pub stage: RuntimeStage,
    pub at: String,
}

/// `runtime.build_completed`：构建阶段结束（成功或失败；skip-build 命中
/// 缓存也算成功完成）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildCompletedPayload {
    pub workspace_id: i64,
    pub runtime_name: String,
    pub process_id: Option<i64>,
    pub success: bool,
    /// Build-only 任务由 handler 填实测耗时；桥接推导的 Start 流程为 None。
    pub duration_ms: Option<u64>,
    pub error: Option<String>,
    pub at: String,
}

/// `runtime.process_started`：进程进入 Running。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessStartedPayload {
    pub workspace_id: i64,
    pub process_id: i64,
    pub runtime_name: String,
    pub at: String,
}

/// `runtime.process_output`：一批已脱敏的进程输出行（R-11 聚合批次直转）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessOutputPayload {
    pub process_id: i64,
    pub runtime_name: String,
    pub lines: Vec<LogLine>,
}

/// `runtime.process_stopped`：进程正常终止（主动 Stop 或自身退出码 0）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessStoppedPayload {
    pub workspace_id: i64,
    pub process_id: i64,
    pub runtime_name: String,
    pub exit_code: Option<i32>,
    pub at: String,
}

/// `runtime.process_failed`：进程非预期终止（崩溃 / spawn 后宽限期内退出）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessFailedPayload {
    pub workspace_id: i64,
    pub process_id: i64,
    pub runtime_name: String,
    pub exit_code: Option<i32>,
    pub at: String,
}

/// `runtime.health_changed`。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthChangedPayload {
    pub workspace_id: i64,
    pub process_id: i64,
    pub runtime_name: String,
    pub health: HealthStatus,
    pub at: String,
}

/// `runtime.file_changed`：工作区源文件 / POM 变更（R-17 File Watch 接入
/// 后发射；R-12 先定类型与快照）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileChangedPayload {
    pub workspace_id: i64,
    pub paths: Vec<String>,
    pub at: String,
}

/// `runtime.restart_started`。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RestartStartedPayload {
    pub workspace_id: i64,
    pub runtime_name: String,
    pub at: String,
}

/// `runtime.restart_completed`。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RestartCompletedPayload {
    pub workspace_id: i64,
    pub runtime_name: String,
    pub success: bool,
    pub error: Option<String>,
    pub at: String,
}

// ---------------------------------------------------------------------------
// Emission 抽象（AppHandle 的测试替身接缝）
// ---------------------------------------------------------------------------

/// 一次待发射的 Tauri event：§64 事件名 + 已序列化 payload。
#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeEmission {
    pub name: &'static str,
    pub payload: serde_json::Value,
}

impl RuntimeEmission {
    pub fn new<T: Serialize>(name: &'static str, payload: &T) -> Self {
        Self {
            name,
            payload: serde_json::to_value(payload).unwrap_or(serde_json::Value::Null),
        }
    }
}

/// §64 事件发射端。生产实现是 [`TauriRuntimeEmitter`]；测试用 `VecEmitter`。
pub trait RuntimeEventEmitter: Send + Sync {
    fn emit(&self, emission: RuntimeEmission);
}

/// 生产实现：桥到 Tauri 的 event 总线。
pub struct TauriRuntimeEmitter {
    app: tauri::AppHandle,
}

impl TauriRuntimeEmitter {
    pub fn new(app: tauri::AppHandle) -> Self {
        Self { app }
    }
}

impl RuntimeEventEmitter for TauriRuntimeEmitter {
    fn emit(&self, emission: RuntimeEmission) {
        use tauri::Emitter;
        if let Err(e) = self.app.emit(emission.name, &emission.payload) {
            log::warn!("R-12: failed to emit {}: {}", emission.name, e);
        }
    }
}

/// 测试实现：收集全部 emission 供断言。
#[cfg(test)]
#[derive(Default)]
pub struct VecEmitter {
    pub emissions: Mutex<Vec<RuntimeEmission>>,
}

#[cfg(test)]
impl VecEmitter {
    pub fn collected(&self) -> Vec<RuntimeEmission> {
        self.emissions.lock().unwrap().clone()
    }

    pub fn names(&self) -> Vec<&'static str> {
        self.emissions
            .lock()
            .unwrap()
            .iter()
            .map(|e| e.name)
            .collect()
    }
}

#[cfg(test)]
impl RuntimeEventEmitter for VecEmitter {
    fn emit(&self, emission: RuntimeEmission) {
        self.emissions.lock().unwrap().push(emission);
    }
}

// ---------------------------------------------------------------------------
// 内部事件（R-10/R-11 `RuntimeEvent`）→ §64 事件的映射
// ---------------------------------------------------------------------------

/// 把一条内部 RuntimeEvent 翻译成零或多条 §64 emission（纯函数，单测覆盖）。
///
/// 映射规则（与 R-10 发射点一一核对过，保证不重复、不漏）：
/// - `Lifecycle → Preparing`：`build_started` + `build_progress(preparing)`
///   （Start/Restart 的构建阶段起点；Build-only 任务由 handler 自己发）
/// - `Lifecycle → Resolving / Building / Starting`：`build_progress(<stage>)`；
///   进入 Starting 额外补 `build_completed(success)` —— 构建阶段到此结束
///   （skip-build 命中缓存时 Preparing 直达 Starting，序列仍然完整）
/// - `Lifecycle → Failed`，from ∈ Preparing/Resolving/Building/Starting：
///   `build_completed(failure)`。spawn 前失败没有进程，不发 `process_failed`；
///   spawn 后早期退出由 `Exited(crashed)` 覆盖
/// - `Lifecycle → Running`：`process_started` + `health_changed(up)`
/// - `Lifecycle → Stopped`（from Running/Stopping）：`process_stopped` +
///   `health_changed(down)`
/// - `Exited(crashed=true)`：`process_failed` + `health_changed(down)`
/// - `Exited(crashed=false)`：不映射（Lifecycle→Stopped 已覆盖）
/// - `Logs`：`process_output`（R-11 已聚合脱敏，原样转发）
/// - `Metrics` / `Ports`：§64 无对应事件，不映射（调用方记 debug 日志）
///
/// `workspace_id`：进程域事件由桥接方按 `process_id` 反查 `runtime_processes`
/// 填入；查不到时传 `None`，payload 记 0 并告警（管理器插行后才发事件，
/// 正常路径必然查到）。
pub fn map_internal_event(
    event: &RuntimeEvent,
    workspace_id: Option<i64>,
    at: &str,
) -> Vec<RuntimeEmission> {
    let ws = workspace_id.unwrap_or(0);
    match event {
        RuntimeEvent::Lifecycle {
            process_id,
            runtime_name,
            from,
            to,
            ..
        } => map_lifecycle(ws, *process_id, runtime_name, *from, *to, at),
        RuntimeEvent::Exited {
            process_id,
            runtime_name,
            exit_code,
            crashed,
        } => {
            if !crashed {
                return Vec::new();
            }
            vec![
                RuntimeEmission::new(
                    EVENT_PROCESS_FAILED,
                    &ProcessFailedPayload {
                        workspace_id: ws,
                        process_id: *process_id,
                        runtime_name: runtime_name.clone(),
                        exit_code: *exit_code,
                        at: at.to_string(),
                    },
                ),
                RuntimeEmission::new(
                    EVENT_HEALTH_CHANGED,
                    &HealthChangedPayload {
                        workspace_id: ws,
                        process_id: *process_id,
                        runtime_name: runtime_name.clone(),
                        health: HealthStatus::Down,
                        at: at.to_string(),
                    },
                ),
            ]
        }
        RuntimeEvent::Logs {
            process_id,
            runtime_name,
            lines,
        } => vec![RuntimeEmission::new(
            EVENT_PROCESS_OUTPUT,
            &ProcessOutputPayload {
                process_id: *process_id,
                runtime_name: runtime_name.clone(),
                lines: lines.clone(),
            },
        )],
        RuntimeEvent::Metrics { .. } | RuntimeEvent::Ports { .. } => Vec::new(),
    }
}

/// Lifecycle 迁移的映射（`map_internal_event` 的 Lifecycle 分支）。
fn map_lifecycle(
    workspace_id: i64,
    process_id: i64,
    runtime_name: &str,
    from: LifecycleStatus,
    to: LifecycleStatus,
    at: &str,
) -> Vec<RuntimeEmission> {
    use LifecycleStatus::*;

    let progress = |stage: RuntimeStage| {
        RuntimeEmission::new(
            EVENT_BUILD_PROGRESS,
            &BuildProgressPayload {
                workspace_id,
                runtime_name: runtime_name.to_string(),
                process_id: Some(process_id),
                stage,
                at: at.to_string(),
            },
        )
    };
    let build_completed = |success: bool| {
        RuntimeEmission::new(
            EVENT_BUILD_COMPLETED,
            &BuildCompletedPayload {
                workspace_id,
                runtime_name: runtime_name.to_string(),
                process_id: Some(process_id),
                success,
                duration_ms: None,
                error: None,
                at: at.to_string(),
            },
        )
    };

    match to {
        Preparing => vec![
            RuntimeEmission::new(
                EVENT_BUILD_STARTED,
                &BuildStartedPayload {
                    workspace_id,
                    runtime_name: runtime_name.to_string(),
                    op: RuntimeOp::Start,
                    at: at.to_string(),
                },
            ),
            progress(RuntimeStage::Preparing),
        ],
        Resolving => vec![progress(RuntimeStage::Resolving)],
        Building => vec![progress(RuntimeStage::Building)],
        Starting => vec![build_completed(true), progress(RuntimeStage::Starting)],
        Running => vec![
            RuntimeEmission::new(
                EVENT_PROCESS_STARTED,
                &ProcessStartedPayload {
                    workspace_id,
                    process_id,
                    runtime_name: runtime_name.to_string(),
                    at: at.to_string(),
                },
            ),
            RuntimeEmission::new(
                EVENT_HEALTH_CHANGED,
                &HealthChangedPayload {
                    workspace_id,
                    process_id,
                    runtime_name: runtime_name.to_string(),
                    health: HealthStatus::Up,
                    at: at.to_string(),
                },
            ),
        ],
        Failed => match from {
            Preparing | Resolving | Building | Starting => vec![build_completed(false)],
            // Running/Stopping → Failed：崩溃路径由 Exited(crashed) 发
            // process_failed，这里不重复。
            _ => Vec::new(),
        },
        Stopped => match from {
            Running | Stopping => vec![
                RuntimeEmission::new(
                    EVENT_PROCESS_STOPPED,
                    &ProcessStoppedPayload {
                        workspace_id,
                        process_id,
                        runtime_name: runtime_name.to_string(),
                        exit_code: None,
                        at: at.to_string(),
                    },
                ),
                RuntimeEmission::new(
                    EVENT_HEALTH_CHANGED,
                    &HealthChangedPayload {
                        workspace_id,
                        process_id,
                        runtime_name: runtime_name.to_string(),
                        health: HealthStatus::Down,
                        at: at.to_string(),
                    },
                ),
            ],
            _ => Vec::new(),
        },
        // Created 不会被作为迁移目标发出；Stopping 是中间态，无 §64 事件。
        Created | Stopping => Vec::new(),
    }
}

// ---------------------------------------------------------------------------
// 生产桥接：RuntimeEventSink → §64 Tauri events
// ---------------------------------------------------------------------------

/// R-10/R-11 内部事件到 §64 Event API 的桥。装配进
/// `RuntimeProcessDeps.events`（替换 R-10 默认的 `LoggingEventSink`）。
pub struct TauriRuntimeBridge {
    emitter: Arc<dyn RuntimeEventEmitter>,
    /// 进程域事件反查 workspace_id 用（`runtime_processes` 行）。
    db: Arc<Mutex<Connection>>,
}

impl TauriRuntimeBridge {
    pub fn new(emitter: Arc<dyn RuntimeEventEmitter>, db: Arc<Mutex<Connection>>) -> Self {
        Self { emitter, db }
    }

    /// 按 process_id 反查 workspace_id（失败告警并返回 None）。
    fn workspace_of(&self, process_id: i64) -> Option<i64> {
        let conn = self.db.lock().ok()?;
        match crate::runtime::launch::store::get_process(&conn, process_id) {
            Ok(Some(row)) => Some(row.workspace_id),
            Ok(None) => {
                log::warn!("R-12: process #{process_id} not found for event workspace lookup");
                None
            }
            Err(e) => {
                log::warn!("R-12: workspace lookup for process #{process_id} failed: {e}");
                None
            }
        }
    }
}

impl RuntimeEventSink for TauriRuntimeBridge {
    fn emit(&self, event: RuntimeEvent) {
        // Metrics / Ports 无 §64 对应事件：保留 R-10 默认 sink 的日志行为。
        if matches!(event, RuntimeEvent::Metrics { .. } | RuntimeEvent::Ports { .. }) {
            log::debug!("R-12: internal event {event:?}");
            return;
        }
        let workspace_id = match &event {
            RuntimeEvent::Lifecycle { process_id, .. } => self.workspace_of(*process_id),
            RuntimeEvent::Exited { process_id, .. } => self.workspace_of(*process_id),
            _ => None,
        };
        let at = chrono::Utc::now().to_rfc3339();
        for emission in map_internal_event(&event, workspace_id, &at) {
            self.emitter.emit(emission);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::streaming::OutputStream;
    use crate::runtime::logs::{LogLevel, LogPhase};

    const AT: &str = "2026-08-25T00:00:00Z";

    fn lifecycle(from: LifecycleStatus, to: LifecycleStatus) -> RuntimeEvent {
        RuntimeEvent::Lifecycle {
            process_id: 7,
            runtime_name: "app".into(),
            from,
            to,
            at: AT.into(),
        }
    }

    /// §65 验收：成功 Start 的阶段事件序列 Preparing→Resolving→Building→
    /// Starting→Running，且事件流为 build_started → build_progress* →
    /// build_completed → process_started → health_changed(up)。
    #[test]
    fn successful_start_emits_full_stage_sequence() {
        use LifecycleStatus::*;
        let transitions = [
            (Created, Preparing),
            (Preparing, Resolving),
            (Resolving, Building),
            (Building, Starting),
            (Starting, Running),
        ];
        let mut names = Vec::new();
        for (from, to) in transitions {
            for e in map_internal_event(&lifecycle(from, to), Some(1), AT) {
                names.push(e.name);
            }
        }
        assert_eq!(
            names,
            vec![
                EVENT_BUILD_STARTED,
                EVENT_BUILD_PROGRESS, // preparing
                EVENT_BUILD_PROGRESS, // resolving
                EVENT_BUILD_PROGRESS, // building
                EVENT_BUILD_COMPLETED,
                EVENT_BUILD_PROGRESS, // starting
                EVENT_PROCESS_STARTED,
                EVENT_HEALTH_CHANGED,
            ]
        );
    }

    /// skip-build（Restart 路径）：Preparing 直达 Starting，序列仍完整。
    #[test]
    fn skip_build_start_keeps_consistent_sequence() {
        use LifecycleStatus::*;
        let mut names = Vec::new();
        for (from, to) in [(Created, Preparing), (Preparing, Starting), (Starting, Running)] {
            names.extend(
                map_internal_event(&lifecycle(from, to), Some(1), AT)
                    .iter()
                    .map(|e| e.name),
            );
        }
        assert_eq!(
            names,
            vec![
                EVENT_BUILD_STARTED,
                EVENT_BUILD_PROGRESS,
                EVENT_BUILD_COMPLETED,
                EVENT_BUILD_PROGRESS,
                EVENT_PROCESS_STARTED,
                EVENT_HEALTH_CHANGED,
            ]
        );
    }

    /// 构建失败（spawn 前）：build_completed(failure)，不发 process_failed。
    #[test]
    fn build_failure_emits_failed_completion_without_process_failed() {
        use LifecycleStatus::*;
        let emissions = map_internal_event(&lifecycle(Building, Failed), Some(1), AT);
        assert_eq!(emissions.len(), 1);
        assert_eq!(emissions[0].name, EVENT_BUILD_COMPLETED);
        assert_eq!(emissions[0].payload["success"], serde_json::json!(false));
    }

    /// 崩溃：Exited(crashed) → process_failed + health_changed(down)；
    /// 伴随的 Lifecycle Running→Failed 不再重复发 process_failed。
    #[test]
    fn crash_emits_process_failed_exactly_once() {
        use LifecycleStatus::*;
        let mut names: Vec<&'static str> = map_internal_event(
            &RuntimeEvent::Exited {
                process_id: 7,
                runtime_name: "app".into(),
                exit_code: Some(1),
                crashed: true,
            },
            Some(1),
            AT,
        )
        .iter()
        .map(|e| e.name)
        .collect();
        names.extend(
            map_internal_event(&lifecycle(Running, Failed), Some(1), AT)
                .iter()
                .map(|e| e.name),
        );
        assert_eq!(
            names.iter().filter(|n| **n == EVENT_PROCESS_FAILED).count(),
            1
        );
        assert!(names.contains(&EVENT_HEALTH_CHANGED));
    }

    /// 优雅停止：Lifecycle→Stopped 发 process_stopped；Exited(不 crashed) 不重复。
    #[test]
    fn graceful_stop_emits_process_stopped_once() {
        use LifecycleStatus::*;
        let mut names: Vec<&'static str> =
            map_internal_event(&lifecycle(Stopping, Stopped), Some(1), AT)
                .iter()
                .map(|e| e.name)
                .collect();
        names.extend(
            map_internal_event(
                &RuntimeEvent::Exited {
                    process_id: 7,
                    runtime_name: "app".into(),
                    exit_code: Some(0),
                    crashed: false,
                },
                Some(1),
                AT,
            )
            .iter()
            .map(|e| e.name),
        );
        assert_eq!(
            names.iter().filter(|n| **n == EVENT_PROCESS_STOPPED).count(),
            1
        );
    }

    /// R-11 日志批次原样转为 process_output；Metrics/Ports 不映射。
    #[test]
    fn logs_batch_maps_to_process_output() {
        let line = LogLine {
            seq: 1,
            at: AT.into(),
            phase: LogPhase::Run,
            stream: OutputStream::Stdout,
            level: Some(LogLevel::Info),
            line: "hello".into(),
        };
        let emissions = map_internal_event(
            &RuntimeEvent::Logs {
                process_id: 7,
                runtime_name: "app".into(),
                lines: vec![line],
            },
            None,
            AT,
        );
        assert_eq!(emissions.len(), 1);
        assert_eq!(emissions[0].name, EVENT_PROCESS_OUTPUT);
        assert_eq!(emissions[0].payload["lines"][0]["line"], "hello");

        assert!(map_internal_event(
            &RuntimeEvent::Ports {
                process_id: 7,
                ports: vec![8080]
            },
            None,
            AT
        )
        .is_empty());
    }
}
