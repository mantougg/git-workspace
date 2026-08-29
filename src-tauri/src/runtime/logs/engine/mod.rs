//! Runtime 日志引擎门面（R-11，B-05 拆分，设计文档 §4.5）。
//!
//! 文件布局：
//! - 本文件（`mod.rs`）：公共类型（LogFilter / LogExportOutcome /
//!   LogLimits）与 [`RuntimeLogEngine`] 公共门面（会话表管理）；
//! - `session`：LogSession、有界环形缓冲 Ring、SessionMsg——捕获路径
//!   （`LogSession::log`）只做脱敏 + 级别解析 + `mpsc` 发送，不触碰磁盘；
//! - `worker`：批量聚合、落盘、事件推送、滚动切分（每会话一个线程）；
//! - `query`：search / tail / export / clear，全部流式读取；
//! - `storage`：日志目录、段文件清单、路径安全守卫。
//!
//! 线程模型（不变）：
//! - 每个会话一个 worker 线程按 [`LogLimits::aggregate_interval`] 聚合：
//!   批量写盘（BufWriter + 每批 flush，不逐行 sync）→ 更新有界环形缓冲
//!   → 回调 [`LogAnalyzer`]（§37 预留）→ 分块发出 `RuntimeEvent::Logs`；
//! - [`LogSession::finish`]（幂等）断开发送端并 join worker，进程结束时
//!   日志完整落盘、之后可回查（验收标准）。

use std::collections::HashMap;
use std::fs::File;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::error::AppResult;
use crate::runtime::launch::RuntimeEventSink;
use crate::runtime::logs::redact::LogRedactor;
use crate::runtime::logs::{LogAnalyzer, LogLevel};

mod query;
mod session;
mod storage;
mod worker;

pub use session::LogSession;
use session::Ring;
use storage::{current_segment_path, ensure_logs_dir};
use worker::{worker_main, WorkerCtx};

/// 日志查询 / 过滤条件。`query` 为大小写敏感的子串匹配（对齐 IDEA 默认）。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogFilter {
    /// 子串过滤；`None` / 空串 = 不过滤。
    pub query: Option<String>,
    /// 最低级别（含）；未识别级别的行（`None`，降级原文）不受级别过滤
    /// 影响——stack trace 续行等在 ERROR 过滤下仍可见。
    pub min_level: Option<LogLevel>,
    /// `search` 的返回上限（默认 [`DEFAULT_SEARCH_LIMIT`]）；`export` 忽略
    /// 本字段，始终全量导出匹配行。
    pub limit: Option<usize>,
}

/// 导出结果。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogExportOutcome {
    pub path: String,
    /// 实际写入的行数（与同条件 `search` 的行集一致）。
    pub lines: u64,
}

/// 资源与节流限额（测试可缩小以加速）。
#[derive(Debug, Clone)]
pub struct LogLimits {
    /// 单段文件上限，超过即滚动切分。默认 8 MiB。
    pub segment_max_bytes: u64,
    /// 保留的历史段数（当前段之外）。默认 3 → 每进程约 ≤ 32 MiB。
    pub segments_kept: u32,
    /// 环形内存缓冲行数上限（UI 初始视图 / live tail）。默认 2000。
    pub ring_max_lines: usize,
    /// 环形内存缓冲字节上限。默认 512 KiB。
    pub ring_max_bytes: usize,
    /// 聚合推送间隔（发送端节流）。默认 100ms。
    pub aggregate_interval: Duration,
    /// 单个 `RuntimeEvent::Logs` 事件的最大行数（分块上限）。默认 256。
    pub batch_max_lines: usize,
}

impl Default for LogLimits {
    fn default() -> Self {
        Self {
            segment_max_bytes: 8 * 1024 * 1024,
            segments_kept: 3,
            ring_max_lines: 2000,
            ring_max_bytes: 512 * 1024,
            aggregate_interval: Duration::from_millis(100),
            batch_max_lines: 256,
        }
    }
}

const DEFAULT_SEARCH_LIMIT: usize = 1000;

// ------------------------------------------------------------------
// RuntimeLogEngine
// ------------------------------------------------------------------

/// Runtime 日志引擎：管理全部进程日志会话，并提供落盘文件的流式查询。
/// 引擎本身不持有事件 sink——每个会话在开启时绑定调用方给的 sink
/// （与 manager 的 `RuntimeProcessDeps.events` 同源，R-12 桥接 Tauri）。
pub struct RuntimeLogEngine {
    sessions: Mutex<HashMap<i64, Arc<LogSession>>>,
    analyzers: Mutex<Vec<Arc<dyn LogAnalyzer>>>,
    limits: LogLimits,
}

impl RuntimeLogEngine {
    pub fn new() -> Self {
        Self::with_limits(LogLimits::default())
    }

    pub fn with_limits(limits: LogLimits) -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
            analyzers: Mutex::new(Vec::new()),
            limits,
        }
    }

    /// §37 预留：注册智能增强分析器（此后开启的会话生效）。
    pub fn register_analyzer(&self, analyzer: Arc<dyn LogAnalyzer>) {
        self.analyzers.lock().unwrap().push(analyzer);
    }

    /// 开启一个进程日志会话：创建 `.gitworkspace/logs/<runtime>/` 与当前
    /// 段文件（IO 错误同步上抛，fail-fast），spawn 聚合 worker。
    ///
    /// `secrets` 为本次运行的敏感环境值（[`sensitive_env_values`]
    /// [crate::runtime::logs::redact::sensitive_env_values]），仅内存持有。
    pub fn open_session(
        &self,
        workspace_root: &Path,
        runtime_name: &str,
        process_id: i64,
        secrets: Vec<String>,
        events: Arc<dyn RuntimeEventSink>,
    ) -> AppResult<Arc<LogSession>> {
        let dir = ensure_logs_dir(workspace_root, runtime_name)?;
        // 同 process_id 的旧会话是异常残留（DB id 唯一，纯防御）。
        self.finish_session(process_id);
        let file = File::create(current_segment_path(&dir, process_id))?;
        let ring = Arc::new(Mutex::new(Ring::default()));
        let (tx, rx) = mpsc::channel();
        let ctx = WorkerCtx {
            process_id,
            runtime_name: runtime_name.to_string(),
            dir: dir.clone(),
            limits: self.limits.clone(),
            events,
            analyzers: self.analyzers.lock().unwrap().clone(),
            ring: Arc::clone(&ring),
        };
        let worker = std::thread::Builder::new()
            .name(format!("gw-runtime-log-{process_id}"))
            .spawn(move || worker_main(ctx, rx, file))?;
        let session = Arc::new(LogSession {
            process_id,
            runtime_name: runtime_name.to_string(),
            dir,
            redactor: LogRedactor::new(secrets),
            seq: AtomicU64::new(0),
            tx: Mutex::new(Some(tx)),
            ring,
            worker: Mutex::new(Some(worker)),
            finished: AtomicBool::new(false),
        });
        self.sessions
            .lock()
            .unwrap()
            .insert(process_id, Arc::clone(&session));
        Ok(session)
    }

    /// 活跃会话查询（manager 装配用）。
    pub fn session(&self, process_id: i64) -> Option<Arc<LogSession>> {
        self.sessions.lock().unwrap().get(&process_id).cloned()
    }

    /// 结束并摘除会话（幂等；无会话时 no-op）。进程终态路径统一调这里。
    pub fn finish_session(&self, process_id: i64) {
        if let Some(session) = self.sessions.lock().unwrap().remove(&process_id) {
            session.finish();
        }
    }
}

impl Default for RuntimeLogEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for RuntimeLogEngine {
    fn drop(&mut self) {
        let sessions: Vec<Arc<LogSession>> = self
            .sessions
            .lock()
            .unwrap()
            .drain()
            .map(|(_, s)| s)
            .collect();
        for session in sessions {
            session.finish();
        }
    }
}

// ------------------------------------------------------------------
// tests
// ------------------------------------------------------------------

#[cfg(test)]
mod tests;
