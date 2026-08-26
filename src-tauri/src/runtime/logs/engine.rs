//! 日志引擎核心（R-11）：会话生命周期、落盘滚动切分、环形缓冲、批量
//! 聚合推送、流式搜索 / 导出 / 清空。
//!
//! 线程模型：
//! - `on_line` 捕获路径（[`LogSession::log`]）只做脱敏 + 级别解析 +
//!   `mpsc` 发送，不触碰磁盘——日志洪水不会反压进程输出读取；
//! - 每个会话一个 worker 线程按 [`LogLimits::aggregate_interval`] 聚合：
//!   批量写盘（BufWriter + 每批 flush，不逐行 sync）→ 更新有界环形缓冲
//!   → 回调 [`LogAnalyzer`]（§37 预留）→ 分块发出 `RuntimeEvent::Logs`；
//! - [`LogSession::finish`]（幂等）断开发送端并 join worker，进程结束时
//!   日志完整落盘、之后可回查（验收标准）。

use std::collections::{HashMap, VecDeque};
use std::fs::{self, File};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use crate::process::streaming::OutputStream;
use crate::runtime::launch::{RuntimeEvent, RuntimeEventSink};
use crate::runtime::logs::level::parse_level;
use crate::runtime::logs::redact::LogRedactor;
use crate::runtime::logs::{LogAnalyzer, LogEntry, LogLevel, LogLine, LogPhase};

/// 日志目录：`<workspace>/.gitworkspace/logs/<runtime_name>/`。
const LOGS_DIR: &str = "logs";

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
/// 落盘写缓冲：批量 flush 的粒度上限。
const WRITE_BUFFER_BYTES: usize = 64 * 1024;

// ------------------------------------------------------------------
// LogSession：单进程日志会话
// ------------------------------------------------------------------

enum SessionMsg {
    Line(LogLine),
    Clear,
}

/// 有界环形缓冲（模式对齐 R-09 `RingTail`：行数 + 字节双上限）。
#[derive(Default)]
struct Ring {
    lines: VecDeque<LogLine>,
    bytes: usize,
}

impl Ring {
    fn push(&mut self, line: LogLine, limits: &LogLimits) {
        self.bytes += line.line.len() + 1;
        self.lines.push_back(line);
        while self.lines.len() > limits.ring_max_lines || self.bytes > limits.ring_max_bytes {
            if let Some(evicted) = self.lines.pop_front() {
                self.bytes = self.bytes.saturating_sub(evicted.line.len() + 1);
            } else {
                self.bytes = 0;
                break;
            }
        }
    }

    fn clear(&mut self) {
        self.lines.clear();
        self.bytes = 0;
    }
}

/// 单进程日志会话：构建（Build 阶段）与应用运行（Run 阶段）输出统一
/// 经 [`log`][Self::log] 进入。捕获线程与进程生命周期绑定——进程结束
/// 由 manager 调 [`finish`][Self::finish] 收口（幂等）。
pub struct LogSession {
    process_id: i64,
    runtime_name: String,
    dir: PathBuf,
    redactor: LogRedactor,
    seq: AtomicU64,
    tx: Mutex<Option<Sender<SessionMsg>>>,
    ring: Arc<Mutex<Ring>>,
    worker: Mutex<Option<JoinHandle<()>>>,
    finished: AtomicBool,
}

impl LogSession {
    pub fn process_id(&self) -> i64 {
        self.process_id
    }

    pub fn runtime_name(&self) -> &str {
        &self.runtime_name
    }

    /// 本会话日志目录（`<workspace>/.gitworkspace/logs/<runtime>/`）。
    pub fn directory(&self) -> &Path {
        &self.dir
    }

    /// 捕获一行输出：脱敏（落盘前，验收标准「磁盘无明文」）→ 级别解析
    /// → 发送给 worker。会话结束后调用是静默 no-op（幂等收尾的边界）。
    pub fn log(&self, phase: LogPhase, stream: OutputStream, line: &str) {
        let tx = self.tx.lock().unwrap().clone();
        let Some(tx) = tx else { return };
        let masked = self.redactor.mask(line);
        let entry = LogLine {
            seq: self.seq.fetch_add(1, Ordering::Relaxed) + 1,
            at: Utc::now().to_rfc3339(),
            phase,
            stream,
            level: parse_level(&masked),
            line: masked,
        };
        // worker 已退出（finish 竞态）时静默丢弃，不 panic。
        let _ = tx.send(SessionMsg::Line(entry));
    }

    /// 结束会话：断开发送端，join worker（drain 完残余批次后退出），
    /// 保证落盘完整。幂等。
    pub fn finish(&self) {
        let tx = self.tx.lock().unwrap().take();
        drop(tx);
        if let Some(handle) = self.worker.lock().unwrap().take() {
            let _ = handle.join();
        }
        self.finished.store(true, Ordering::Relaxed);
    }

    pub fn is_finished(&self) -> bool {
        self.finished.load(Ordering::Relaxed)
    }

    /// live tail：环形缓冲最后 `n` 行。
    pub fn tail(&self, n: usize) -> Vec<LogLine> {
        let ring = self.ring.lock().unwrap();
        let skip = ring.lines.len().saturating_sub(n);
        ring.lines.iter().skip(skip).cloned().collect()
    }

    /// 清空（§36）：经 channel 按序到达 worker——清空后到达的行不受影响。
    fn request_clear(&self) {
        if let Some(tx) = self.tx.lock().unwrap().as_ref() {
            let _ = tx.send(SessionMsg::Clear);
        }
    }
}

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

    /// 搜索（§36）：跨滚动段按时间序流式扫描，过滤后返回，最多
    /// `filter.limit`（默认 [`DEFAULT_SEARCH_LIMIT`]）行。
    pub fn search(
        &self,
        workspace_root: &Path,
        runtime_name: &str,
        process_id: i64,
        filter: &LogFilter,
    ) -> AppResult<Vec<LogEntry>> {
        let paths = self.require_segments(workspace_root, runtime_name, process_id)?;
        let limit = filter.limit.unwrap_or(DEFAULT_SEARCH_LIMIT);
        let mut out = Vec::new();
        for_each_file_line(&paths, |line_number, text| {
            let level = parse_level(text);
            if matches_filter(filter, level, text) {
                out.push(LogEntry {
                    line_number,
                    level,
                    text: text.to_string(),
                });
            }
            out.len() < limit
        })?;
        Ok(out)
    }

    /// 实时滚动初始视图 / tail：活跃会话读环形缓冲，否则流式读文件尾部。
    pub fn tail(
        &self,
        workspace_root: &Path,
        runtime_name: &str,
        process_id: i64,
        n: usize,
    ) -> AppResult<Vec<LogEntry>> {
        if let Some(session) = self.session(process_id) {
            return Ok(session
                .tail(n)
                .into_iter()
                .map(|line| LogEntry {
                    line_number: line.seq,
                    level: line.level,
                    text: line.line,
                })
                .collect());
        }
        let paths = self.require_segments(workspace_root, runtime_name, process_id)?;
        tail_paths(&paths, n)
    }

    /// 导出（§36）：与 `search` 共用同一过滤管道（导出内容与显示一致），
    /// 流式写出，不整文件载入内存。全量导出匹配行（忽略 `filter.limit`）。
    pub fn export(
        &self,
        workspace_root: &Path,
        runtime_name: &str,
        process_id: i64,
        filter: &LogFilter,
        dest: &Path,
    ) -> AppResult<LogExportOutcome> {
        let paths = self.require_segments(workspace_root, runtime_name, process_id)?;
        let mut writer = BufWriter::new(File::create(dest)?);
        let mut lines = 0u64;
        for_each_file_line(&paths, |_, text| {
            if matches_filter(filter, parse_level(text), text) {
                if writeln!(writer, "{text}").is_ok() {
                    lines += 1;
                }
            }
            true
        })?;
        writer.flush()?;
        Ok(LogExportOutcome {
            path: dest.display().to_string(),
            lines,
        })
    }

    /// 清空（§36）：活跃会话按序经 worker 截断；已结束会话直接清理段文件。
    pub fn clear(
        &self,
        workspace_root: &Path,
        runtime_name: &str,
        process_id: i64,
    ) -> AppResult<()> {
        if let Some(session) = self.session(process_id) {
            session.request_clear();
            return Ok(());
        }
        let dir = logs_dir(workspace_root, runtime_name)?;
        for path in segment_paths(&dir, process_id, self.limits.segments_kept) {
            if path == current_segment_path(&dir, process_id) {
                let _ = File::create(&path)?;
            } else {
                let _ = fs::remove_file(&path);
            }
        }
        Ok(())
    }

    /// §35：读取应用自身 `application.log`（用户项目只读原则——只读不写）。
    /// 与进程日志同一过滤管道。
    pub fn search_file(&self, path: &Path, filter: &LogFilter) -> AppResult<Vec<LogEntry>> {
        if !path.is_file() {
            return Err(AppError::NotFound(format!(
                "日志文件不存在：{}。请确认应用日志路径配置",
                path.display()
            )));
        }
        let limit = filter.limit.unwrap_or(DEFAULT_SEARCH_LIMIT);
        let mut out = Vec::new();
        for_each_file_line(&[path.to_path_buf()], |line_number, text| {
            let level = parse_level(text);
            if matches_filter(filter, level, text) {
                out.push(LogEntry {
                    line_number,
                    level,
                    text: text.to_string(),
                });
            }
            out.len() < limit
        })?;
        Ok(out)
    }

    /// §35：应用自身 `application.log` 的 tail（只读）。
    pub fn tail_file(&self, path: &Path, n: usize) -> AppResult<Vec<LogEntry>> {
        if !path.is_file() {
            return Err(AppError::NotFound(format!(
                "日志文件不存在：{}。请确认应用日志路径配置",
                path.display()
            )));
        }
        tail_paths(&[path.to_path_buf()], n)
    }

    /// 段文件清单（最旧 → 最新）；一个段都不存在时报可行动错误。
    fn require_segments(
        &self,
        workspace_root: &Path,
        runtime_name: &str,
        process_id: i64,
    ) -> AppResult<Vec<PathBuf>> {
        let dir = logs_dir(workspace_root, runtime_name)?;
        let paths = segment_paths(&dir, process_id, self.limits.segments_kept);
        if paths.is_empty() {
            return Err(AppError::NotFound(format!(
                "进程 #{process_id} 暂无日志文件（目录 {}）。该进程可能是接管的历史进程，\
                 其输出未被本会话捕获",
                dir.display()
            )));
        }
        Ok(paths)
    }
}

impl Default for RuntimeLogEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for RuntimeLogEngine {
    fn drop(&mut self) {
        let sessions: Vec<Arc<LogSession>> =
            self.sessions.lock().unwrap().drain().map(|(_, s)| s).collect();
        for session in sessions {
            session.finish();
        }
    }
}

// ------------------------------------------------------------------
// worker：聚合 / 落盘 / 推送
// ------------------------------------------------------------------

struct WorkerCtx {
    process_id: i64,
    runtime_name: String,
    dir: PathBuf,
    limits: LogLimits,
    events: Arc<dyn RuntimeEventSink>,
    analyzers: Vec<Arc<dyn LogAnalyzer>>,
    ring: Arc<Mutex<Ring>>,
}

fn worker_main(ctx: WorkerCtx, rx: Receiver<SessionMsg>, file: File) {
    let mut writer = BufWriter::with_capacity(WRITE_BUFFER_BYTES, file);
    let mut current_bytes: u64 = 0;
    let mut pending: Vec<LogLine> = Vec::new();
    let mut disconnected = false;

    // 聚合节流：每个周期先等首条消息，再用完 interval 的剩余时间收集突发，
    // 保证事件速率有界（≤ 1/interval）且稀疏行也在一个 interval 内推送。
    loop {
        match rx.recv_timeout(ctx.limits.aggregate_interval) {
            Ok(SessionMsg::Line(line)) => pending.push(line),
            Ok(SessionMsg::Clear) => do_clear(&ctx, &mut writer, &mut current_bytes, &mut pending),
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
        let deadline = std::time::Instant::now() + ctx.limits.aggregate_interval;
        loop {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() {
                break;
            }
            match rx.recv_timeout(remaining) {
                Ok(SessionMsg::Line(line)) => pending.push(line),
                Ok(SessionMsg::Clear) => {
                    do_clear(&ctx, &mut writer, &mut current_bytes, &mut pending)
                }
                Err(mpsc::RecvTimeoutError::Timeout) => break,
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    disconnected = true;
                    break;
                }
            }
        }
        flush_batch(&ctx, &mut writer, &mut current_bytes, &mut pending);
        if disconnected {
            break;
        }
    }
    let _ = writer.flush();
}

/// 清空：丢弃待写批次，删除历史段，截断当前段，清环形缓冲。
/// 按 channel 顺序处理——Clear 之后到达的行不受影响。
fn do_clear(
    ctx: &WorkerCtx,
    writer: &mut BufWriter<File>,
    current_bytes: &mut u64,
    pending: &mut Vec<LogLine>,
) {
    pending.clear();
    for i in 1..=ctx.limits.segments_kept {
        let _ = fs::remove_file(ctx.dir.join(format!("{}.{i}.log", ctx.process_id)));
    }
    match reopen_segment(&ctx.dir, ctx.process_id) {
        Ok(fresh) => {
            *writer = fresh;
            *current_bytes = 0;
        }
        Err(error) => log::error!("R-11: log clear failed for #{}: {error}", ctx.process_id),
    }
    ctx.ring.lock().unwrap().clear();
}

/// 批量落盘（每批一次 flush）→ 环形缓冲 → §37 分析器 → 分块发事件。
fn flush_batch(
    ctx: &WorkerCtx,
    writer: &mut BufWriter<File>,
    current_bytes: &mut u64,
    pending: &mut Vec<LogLine>,
) {
    if pending.is_empty() {
        return;
    }
    let batch = std::mem::take(pending);
    for line in &batch {
        if *current_bytes >= ctx.limits.segment_max_bytes {
            if let Err(error) = rotate_segments(ctx, writer, current_bytes) {
                log::error!("R-11: log rotation failed for #{}: {error}", ctx.process_id);
            }
        }
        if let Err(error) = writeln!(writer, "{}", line.line) {
            log::error!("R-11: log write failed for #{}: {error}", ctx.process_id);
        }
        *current_bytes += line.line.len() as u64 + 1;
        ctx.ring.lock().unwrap().push(line.clone(), &ctx.limits);
        for analyzer in &ctx.analyzers {
            analyzer.analyze(line);
        }
    }
    // 批量 flush，不逐行 sync（任务文档「架构/性能注意点」）。
    if let Err(error) = writer.flush() {
        log::error!("R-11: log flush failed for #{}: {error}", ctx.process_id);
    }
    for chunk in batch.chunks(ctx.limits.batch_max_lines.max(1)) {
        ctx.events.emit(RuntimeEvent::Logs {
            process_id: ctx.process_id,
            runtime_name: ctx.runtime_name.clone(),
            lines: chunk.to_vec(),
        });
    }
}

/// 滚动切分：删最旧段，逐段移位（`N-1→N … 1→2，当前段→1`），重开当前段。
fn rotate_segments(
    ctx: &WorkerCtx,
    writer: &mut BufWriter<File>,
    current_bytes: &mut u64,
) -> AppResult<()> {
    let keep = ctx.limits.segments_kept;
    if keep > 0 {
        let _ = fs::remove_file(ctx.dir.join(format!("{}.{keep}.log", ctx.process_id)));
        for i in (1..keep).rev() {
            let from = ctx.dir.join(format!("{}.{i}.log", ctx.process_id));
            if from.exists() {
                fs::rename(&from, ctx.dir.join(format!("{}.{}.log", ctx.process_id, i + 1)))?;
            }
        }
        let current = current_segment_path(&ctx.dir, ctx.process_id);
        if current.exists() {
            fs::rename(&current, ctx.dir.join(format!("{}.1.log", ctx.process_id)))?;
        }
    } else {
        // 不保留历史段：直接截断当前段。
        let _ = writer.flush();
    }
    *writer = reopen_segment(&ctx.dir, ctx.process_id)?;
    *current_bytes = 0;
    Ok(())
}

fn reopen_segment(dir: &Path, process_id: i64) -> std::io::Result<BufWriter<File>> {
    File::create(current_segment_path(dir, process_id))
        .map(|file| BufWriter::with_capacity(WRITE_BUFFER_BYTES, file))
}

// ------------------------------------------------------------------
// 文件查询管道（search / export / tail 共用）
// ------------------------------------------------------------------

/// 跨段按时间序（最旧段 → 当前段）流式逐行回调；`f` 返回 `false` 提前停。
/// 全局行号跨段连续（1 起）。
fn for_each_file_line(paths: &[PathBuf], mut f: impl FnMut(u64, &str) -> bool) -> AppResult<()> {
    let mut line_number = 0u64;
    for path in paths {
        let reader = BufReader::new(File::open(path)?);
        for line in reader.lines() {
            let line = line?;
            line_number += 1;
            if !f(line_number, &line) {
                return Ok(());
            }
        }
    }
    Ok(())
}

/// 过滤判定：级别过滤（未识别级别不受影响）+ 子串过滤。
fn matches_filter(filter: &LogFilter, level: Option<LogLevel>, text: &str) -> bool {
    if let Some(min) = filter.min_level {
        if let Some(level) = level {
            if level < min {
                return false;
            }
        }
    }
    if let Some(query) = &filter.query {
        if !query.is_empty() && !text.contains(query.as_str()) {
            return false;
        }
    }
    true
}

/// 流式 tail：全程只保留最后 `n` 行的滑动窗口，内存有界。
fn tail_paths(paths: &[PathBuf], n: usize) -> AppResult<Vec<LogEntry>> {
    if n == 0 {
        return Ok(Vec::new());
    }
    let mut window: VecDeque<(u64, String)> = VecDeque::with_capacity(n.min(1024));
    for_each_file_line(paths, |line_number, text| {
        if window.len() >= n {
            window.pop_front();
        }
        window.push_back((line_number, text.to_string()));
        true
    })?;
    Ok(window
        .into_iter()
        .map(|(line_number, text)| {
            let level = parse_level(&text);
            LogEntry {
                line_number,
                level,
                text,
            }
        })
        .collect())
}

// ------------------------------------------------------------------
// 路径与目录守卫
// ------------------------------------------------------------------

fn current_segment_path(dir: &Path, process_id: i64) -> PathBuf {
    dir.join(format!("{process_id}.log"))
}

/// 段文件清单（最旧 → 最新），只含实际存在的段。
fn segment_paths(dir: &Path, process_id: i64, keep: u32) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    for i in (1..=keep).rev() {
        let path = dir.join(format!("{process_id}.{i}.log"));
        if path.is_file() {
            paths.push(path);
        }
    }
    let current = current_segment_path(dir, process_id);
    if current.is_file() {
        paths.push(current);
    }
    paths
}

fn logs_dir(workspace_root: &Path, runtime_name: &str) -> AppResult<PathBuf> {
    validate_runtime_name(runtime_name)?;
    Ok(workspace_root
        .join(".gitworkspace")
        .join(LOGS_DIR)
        .join(runtime_name))
}

/// 创建日志目录（写路径）；先校验名再落任何目录，沿用 R-07 配置的
/// 符号链接拒绝守卫。
fn ensure_logs_dir(workspace_root: &Path, runtime_name: &str) -> AppResult<PathBuf> {
    validate_runtime_name(runtime_name)?;
    let gitworkspace = workspace_root.join(".gitworkspace");
    // R-14 §78 只读护栏：日志目录必须在 workspace/.gitworkspace 下。
    crate::runtime::guard::assert_workspace_write_path(&gitworkspace, workspace_root, "日志落盘")?;
    reject_symlink(&gitworkspace)?;
    let logs = gitworkspace.join(LOGS_DIR);
    reject_symlink(&logs)?;
    fs::create_dir_all(&logs)?;
    let dir = logs.join(runtime_name);
    reject_symlink(&dir)?;
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn validate_runtime_name(name: &str) -> AppResult<()> {
    if name.is_empty() || name == "." || name == ".." || name.contains(['/', '\\']) {
        return Err(AppError::RuntimeConfig(format!(
            "Runtime 名称 '{name}' 不能用作日志目录名（禁止空名、路径分隔符与 . / ..）"
        )));
    }
    Ok(())
}

fn reject_symlink(path: &Path) -> AppResult<()> {
    if let Ok(metadata) = fs::symlink_metadata(path) {
        if metadata.file_type().is_symlink() {
            return Err(AppError::Permission(format!(
                "拒绝通过符号链接写入日志目录：{}",
                path.display()
            )));
        }
    }
    Ok(())
}

// ------------------------------------------------------------------
// tests
// ------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Instant;

    use super::*;
    use crate::runtime::config::MASKED_VALUE;
    use crate::runtime::launch::VecEventSink;

    fn temp_root(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "gw_r11_{tag}_{}",
            Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    fn test_limits() -> LogLimits {
        LogLimits {
            segment_max_bytes: 1024 * 1024,
            segments_kept: 2,
            ring_max_lines: 100,
            ring_max_bytes: 16 * 1024,
            aggregate_interval: Duration::from_millis(20),
            batch_max_lines: 16,
        }
    }

    fn open(
        engine: &RuntimeLogEngine,
        root: &Path,
        process_id: i64,
        secrets: Vec<String>,
        events: &Arc<VecEventSink>,
    ) -> Arc<LogSession> {
        engine
            .open_session(root, "app", process_id, secrets, events.clone())
            .unwrap()
    }

    fn log_dir(root: &Path) -> PathBuf {
        root.join(".gitworkspace").join("logs").join("app")
    }

    /// 全部段文件内容（最旧 → 最新拼接）。
    fn persisted(root: &Path, process_id: i64, keep: u32) -> String {
        segment_paths(&log_dir(root), process_id, keep)
            .iter()
            .map(|path| std::fs::read_to_string(path).unwrap())
            .collect::<Vec<_>>()
            .join("")
    }

    fn collected_lines(events: &VecEventSink, process_id: i64) -> Vec<LogLine> {
        events
            .collected()
            .into_iter()
            .flat_map(|event| match event {
                RuntimeEvent::Logs {
                    process_id: id,
                    lines,
                    ..
                } if id == process_id => lines,
                _ => Vec::new(),
            })
            .collect()
    }

    /// 验收标准「磁盘日志文件无未脱敏 secret」+ 脱敏规则覆盖。
    #[test]
    fn capture_masks_secrets_before_persisting() {
        let root = temp_root("mask");
        let events = Arc::new(VecEventSink::default());
        let engine = RuntimeLogEngine::with_limits(test_limits());
        let session = open(&engine, &root, 1, vec!["s3cret-value".into()], &events);
        session.log(LogPhase::Run, OutputStream::Stdout, "password=123456");
        session.log(
            LogPhase::Run,
            OutputStream::Stdout,
            "connecting with s3cret-value",
        );
        session.log(
            LogPhase::Run,
            OutputStream::Stdout,
            "2026-08-23 12:00:00.123  INFO 1 --- [main] c.e.App : ok",
        );
        engine.finish_session(1);

        let on_disk = persisted(&root, 1, test_limits().segments_kept);
        assert!(!on_disk.contains("123456"), "key=value 明文不得落盘");
        assert!(!on_disk.contains("s3cret-value"), "环境变量值明文不得落盘");
        assert!(on_disk.contains(MASKED_VALUE), "环境值应替换为掩码");
        assert!(on_disk.contains("c.e.App : ok"), "普通行原样保留");

        // 事件批次中的行同样已脱敏，且级别被解析。
        let lines = collected_lines(&events, 1);
        assert_eq!(lines.len(), 3);
        assert!(lines.iter().all(|l| !l.line.contains("123456")));
        assert_eq!(lines[2].level, Some(LogLevel::Info));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 验收标准「高频输出下 UI 保持响应（事件聚合生效）」：批次有界、
    /// 行不丢、序不乱、环形缓冲有界。
    #[test]
    fn flood_is_aggregated_and_ring_stays_bounded() {
        let root = temp_root("flood");
        let events = Arc::new(VecEventSink::default());
        let engine = RuntimeLogEngine::with_limits(test_limits());
        let session = open(&engine, &root, 2, vec![], &events);
        for i in 0..5000 {
            session.log(
                LogPhase::Run,
                OutputStream::Stdout,
                &format!("flood line {i}"),
            );
        }
        engine.finish_session(2);

        let lines = collected_lines(&events, 2);
        assert_eq!(lines.len(), 5000, "聚合不得丢行");
        let batch_count = events
            .collected()
            .iter()
            .filter(|e| matches!(e, RuntimeEvent::Logs { process_id: 2, .. }))
            .count();
        assert!(
            batch_count <= 5000 / 16 + 8,
            "每事件 ≤16 行 + 少量周期边界批次: {batch_count}"
        );
        let seqs: Vec<u64> = lines.iter().map(|l| l.seq).collect();
        assert_eq!(seqs, (1..=5000).collect::<Vec<_>>(), "序号连续有序");

        // 环形缓冲有界（ring_max_lines = 100）。
        assert_eq!(session.tail(1000).len(), 100);
        // 落盘完整。
        let on_disk = persisted(&root, 2, test_limits().segments_kept);
        assert_eq!(on_disk.lines().count(), 5000);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 验收标准「实时可见」：稀疏行在聚合间隔内推送，不等进程结束。
    #[test]
    fn trickle_line_is_emitted_promptly() {
        let root = temp_root("trickle");
        let events = Arc::new(VecEventSink::default());
        let engine = RuntimeLogEngine::with_limits(test_limits());
        let session = open(&engine, &root, 3, vec![], &events);
        session.log(LogPhase::Run, OutputStream::Stdout, "lonely line");

        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if !collected_lines(&events, 3).is_empty() {
                break;
            }
            assert!(Instant::now() < deadline, "稀疏行应在聚合间隔内推送");
            std::thread::sleep(Duration::from_millis(10));
        }
        engine.finish_session(3);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 「日志文件滚动切分与容量上限」：超限时移位、删除最旧段。
    #[test]
    fn rotation_splits_and_caps_segments() {
        let root = temp_root("rotate");
        let events = Arc::new(VecEventSink::default());
        let mut limits = test_limits();
        limits.segment_max_bytes = 64; // 每段约 3 行
        limits.segments_kept = 2;
        let engine = RuntimeLogEngine::with_limits(limits);
        let session = open(&engine, &root, 4, vec![], &events);
        for i in 0..20 {
            session.log(
                LogPhase::Run,
                OutputStream::Stdout,
                &format!("line-{i:02}-abcdefghij"),
            );
        }
        engine.finish_session(4);

        let dir = log_dir(&root);
        assert!(dir.join("4.log").is_file());
        assert!(dir.join("4.1.log").is_file());
        assert!(dir.join("4.2.log").is_file());
        assert!(!dir.join("4.3.log").exists(), "超出保留数的最旧段被删除");
        let total: usize = segment_paths(&dir, 4, 2)
            .iter()
            .map(|p| std::fs::read_to_string(p).unwrap().lines().count())
            .sum();
        assert!(total < 20, "滚动切分后容量有上限（最旧行被淘汰）");
        let current = std::fs::read_to_string(dir.join("4.log")).unwrap();
        assert!(current.contains("line-19"), "最新行在当前段");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 验收标准「级别过滤/搜索可用」。
    #[test]
    fn search_filters_by_query_and_min_level() {
        let root = temp_root("search");
        let events = Arc::new(VecEventSink::default());
        let engine = RuntimeLogEngine::with_limits(test_limits());
        let session = open(&engine, &root, 5, vec![], &events);
        for line in [
            "[INFO] booting",
            "[WARN] disk low",
            "[ERROR] boom happened",
            "    at com.example.Trace",
        ] {
            session.log(LogPhase::Run, OutputStream::Stdout, line);
        }
        engine.finish_session(5);

        let all = engine.search(&root, "app", 5, &LogFilter::default()).unwrap();
        assert_eq!(all.len(), 4);
        assert_eq!(all[0].level, Some(LogLevel::Info));
        assert_eq!(all[1].level, Some(LogLevel::Warn));
        assert_eq!(all[2].level, Some(LogLevel::Error));
        assert_eq!(all[3].level, None, "未识别行降级为原文");
        assert_eq!(all[0].line_number, 1);

        let filtered = engine
            .search(
                &root,
                "app",
                5,
                &LogFilter {
                    min_level: Some(LogLevel::Warn),
                    ..Default::default()
                },
            )
            .unwrap();
        let texts: Vec<&str> = filtered.iter().map(|e| e.text.as_str()).collect();
        assert_eq!(
            texts,
            ["[WARN] disk low", "[ERROR] boom happened", "    at com.example.Trace"],
            "级别过滤不淘汰，无级别行（stack trace 续行）保持可见"
        );

        let queried = engine
            .search(
                &root,
                "app",
                5,
                &LogFilter {
                    query: Some("boom".into()),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(queried.len(), 1);

        let limited = engine
            .search(
                &root,
                "app",
                5,
                &LogFilter {
                    limit: Some(2),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(limited.len(), 2);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 验收标准「导出内容与显示一致」。
    #[test]
    fn export_writes_exactly_what_search_displays() {
        let root = temp_root("export");
        let events = Arc::new(VecEventSink::default());
        let engine = RuntimeLogEngine::with_limits(test_limits());
        let session = open(&engine, &root, 6, vec![], &events);
        for line in ["[INFO] booting", "[WARN] disk low", "[ERROR] boom happened"] {
            session.log(LogPhase::Run, OutputStream::Stdout, line);
        }
        engine.finish_session(6);

        let filter = LogFilter {
            min_level: Some(LogLevel::Warn),
            ..Default::default()
        };
        let shown = engine.search(&root, "app", 6, &filter).unwrap();
        let dest = root.join("export.txt");
        let outcome = engine.export(&root, "app", 6, &filter, &dest).unwrap();
        let exported: Vec<String> = std::fs::read_to_string(&dest)
            .unwrap()
            .lines()
            .map(str::to_string)
            .collect();
        let shown_texts: Vec<String> = shown.into_iter().map(|e| e.text).collect();
        assert_eq!(outcome.lines as usize, shown_texts.len());
        assert_eq!(exported, shown_texts, "导出内容必须与显示一致");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 验收标准「进程结束后日志完整保留、可回查」+ 清空。
    #[test]
    fn logs_remain_queryable_after_process_ends() {
        let root = temp_root("replay");
        let events = Arc::new(VecEventSink::default());
        let engine = RuntimeLogEngine::with_limits(test_limits());
        let session = open(&engine, &root, 7, vec![], &events);
        for i in 0..5 {
            session.log(LogPhase::Run, OutputStream::Stdout, &format!("line {i}"));
        }
        engine.finish_session(7);
        assert!(session.is_finished());

        let tail = engine.tail(&root, "app", 7, 2).unwrap();
        let texts: Vec<&str> = tail.iter().map(|e| e.text.as_str()).collect();
        assert_eq!(texts, ["line 3", "line 4"]);
        assert_eq!(engine.search(&root, "app", 7, &LogFilter::default()).unwrap().len(), 5);

        engine.clear(&root, "app", 7).unwrap();
        assert!(
            engine
                .search(&root, "app", 7, &LogFilter::default())
                .unwrap()
                .is_empty(),
            "清空后搜索应为空"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// §36 清空：活跃会话按序经 worker 截断，清空后到达的行保留。
    #[test]
    fn clear_on_live_session_truncates_via_worker() {
        let root = temp_root("clear");
        let events = Arc::new(VecEventSink::default());
        let engine = RuntimeLogEngine::with_limits(test_limits());
        let session = open(&engine, &root, 8, vec![], &events);
        session.log(LogPhase::Run, OutputStream::Stdout, "before clear");
        engine.clear(&root, "app", 8).unwrap();
        session.log(LogPhase::Run, OutputStream::Stdout, "after clear");
        engine.finish_session(8);

        let on_disk = persisted(&root, 8, test_limits().segments_kept);
        assert!(!on_disk.contains("before clear"));
        assert!(on_disk.contains("after clear"));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// §35：读取应用自身 application.log——只读，不修改用户文件。
    #[test]
    fn application_log_is_read_readonly() {
        let root = temp_root("applog");
        let engine = RuntimeLogEngine::with_limits(test_limits());
        let app_log = root.join("application.log");
        std::fs::write(
            &app_log,
            "2026-08-23 12:00:00.123  INFO 1 --- [main] c.e.App : up\n\
             2026-08-23 12:00:01.123 ERROR 1 --- [main] c.e.App : down\n",
        )
        .unwrap();
        let before = std::fs::read_to_string(&app_log).unwrap();

        let entries = engine.search_file(&app_log, &LogFilter::default()).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[1].level, Some(LogLevel::Error));
        let tail = engine.tail_file(&app_log, 1).unwrap();
        assert_eq!(tail.len(), 1);
        assert!(tail[0].text.contains("down"));
        assert_eq!(std::fs::read_to_string(&app_log).unwrap(), before, "只读");

        let missing = root.join("nope.log");
        assert!(matches!(
            engine.search_file(&missing, &LogFilter::default()),
            Err(AppError::NotFound(_))
        ));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// §37 预留接口位：注册的分析器收到已脱敏的行。
    #[test]
    fn registered_analyzer_receives_masked_lines() {
        struct Spy {
            seen: Mutex<Vec<String>>,
        }
        impl LogAnalyzer for Spy {
            fn analyze(&self, line: &LogLine) {
                self.seen.lock().unwrap().push(line.line.clone());
            }
        }

        let root = temp_root("spy");
        let events = Arc::new(VecEventSink::default());
        let engine = RuntimeLogEngine::with_limits(test_limits());
        let spy = Arc::new(Spy {
            seen: Mutex::new(Vec::new()),
        });
        engine.register_analyzer(spy.clone());
        let session = open(&engine, &root, 9, vec![], &events);
        session.log(LogPhase::Run, OutputStream::Stdout, "password=abc123");
        engine.finish_session(9);

        let seen = spy.seen.lock().unwrap();
        assert_eq!(seen.len(), 1);
        assert!(!seen[0].contains("abc123"), "分析器不得见到明文");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 查询无任何日志文件的进程 → 可行动 NotFound。
    #[test]
    fn query_without_log_files_is_actionable_not_found() {
        let root = temp_root("none");
        let engine = RuntimeLogEngine::with_limits(test_limits());
        let error = engine
            .search(&root, "app", 404, &LogFilter::default())
            .unwrap_err();
        assert!(matches!(error, AppError::NotFound(_)));
        assert!(error.to_string().contains("暂无日志文件"));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 非法 runtime 名不得逃逸日志目录（路径守卫）。
    #[test]
    fn runtime_name_path_escape_is_rejected() {
        let root = temp_root("guard");
        let events = Arc::new(VecEventSink::default());
        let engine = RuntimeLogEngine::with_limits(test_limits());
        let result = engine.open_session(&root, "../escape", 1, vec![], events);
        assert!(
            matches!(result, Err(AppError::RuntimeConfig(_))),
            "路径逃逸必须被拒绝"
        );
        assert!(!root.join(".gitworkspace/logs").exists());
        let _ = std::fs::remove_dir_all(&root);
    }
}
