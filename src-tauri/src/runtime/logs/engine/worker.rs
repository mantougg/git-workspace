//! 日志 worker（R-11，B-05 拆分）：批量聚合、落盘、事件推送、滚动切分。
//!
//! 每个会话一个 worker 线程按 `aggregate_interval` 聚合：批量写盘
//! （BufWriter + 每批 flush，不逐行 sync）→ 更新有界环形缓冲 → 回调
//! [`LogAnalyzer`](crate::runtime::logs::LogAnalyzer)（§37 预留）→
//! 分块发出 `RuntimeEvent::Logs`。文件 IO 与分析只发生在线程侧，捕获
//! 线程不触碰磁盘（§4.5）。

use std::fs;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};

use crate::error::AppResult;
use crate::runtime::launch::{RuntimeEvent, RuntimeEventSink};
use crate::runtime::logs::{LogAnalyzer, LogLine};

use super::session::{Ring, SessionMsg};
use super::storage::current_segment_path;
use super::LogLimits;

/// 落盘写缓冲：批量 flush 的粒度上限。
const WRITE_BUFFER_BYTES: usize = 64 * 1024;

pub(super) struct WorkerCtx {
    pub(super) process_id: i64,
    pub(super) runtime_name: String,
    pub(super) dir: PathBuf,
    pub(super) limits: LogLimits,
    pub(super) events: Arc<dyn RuntimeEventSink>,
    pub(super) analyzers: Vec<Arc<dyn LogAnalyzer>>,
    pub(super) ring: Arc<Mutex<Ring>>,
}

pub(super) fn worker_main(ctx: WorkerCtx, rx: Receiver<SessionMsg>, file: std::fs::File) {
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
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => break,
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
                Err(RecvTimeoutError::Timeout) => break,
                Err(RecvTimeoutError::Disconnected) => {
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
    writer: &mut BufWriter<std::fs::File>,
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
    writer: &mut BufWriter<std::fs::File>,
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
    writer: &mut BufWriter<std::fs::File>,
    current_bytes: &mut u64,
) -> AppResult<()> {
    let keep = ctx.limits.segments_kept;
    if keep > 0 {
        let _ = fs::remove_file(ctx.dir.join(format!("{}.{keep}.log", ctx.process_id)));
        for i in (1..keep).rev() {
            let from = ctx.dir.join(format!("{}.{i}.log", ctx.process_id));
            if from.exists() {
                fs::rename(
                    &from,
                    ctx.dir.join(format!("{}.{}.log", ctx.process_id, i + 1)),
                )?;
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

fn reopen_segment(dir: &Path, process_id: i64) -> std::io::Result<BufWriter<std::fs::File>> {
    std::fs::File::create(current_segment_path(dir, process_id))
        .map(|file| BufWriter::with_capacity(WRITE_BUFFER_BYTES, file))
}
