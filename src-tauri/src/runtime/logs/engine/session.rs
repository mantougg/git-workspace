//! 进程日志会话（R-11，B-05 拆分）：`LogSession`、有界环形缓冲
//! `Ring`、捕获线程 → worker 的消息通道。
//!
//! 捕获路径保持轻量：[`LogSession::log`] 只做脱敏 + 级别解析 + `mpsc`
//! 发送，不触碰磁盘——日志洪水不会反压进程输出读取（§4.5）。

use std::collections::VecDeque;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use chrono::Utc;

use crate::process::streaming::OutputStream;
use crate::runtime::logs::level::parse_level;
use crate::runtime::logs::redact::LogRedactor;
use crate::runtime::logs::{LogLine, LogPhase};

use super::LogLimits;

#[derive(Debug)]
pub(super) enum SessionMsg {
    Line(LogLine),
    Clear,
}

/// 有界环形缓冲（模式对齐 R-09 `RingTail`：行数 + 字节双上限）。
#[derive(Default)]
pub(super) struct Ring {
    lines: VecDeque<LogLine>,
    bytes: usize,
}

impl Ring {
    pub(super) fn push(&mut self, line: LogLine, limits: &LogLimits) {
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

    pub(super) fn clear(&mut self) {
        self.lines.clear();
        self.bytes = 0;
    }
}

/// 单进程日志会话：构建（Build 阶段）与应用运行（Run 阶段）输出统一
/// 经 [`log`][Self::log] 进入。捕获线程与进程生命周期绑定——进程结束
/// 由 manager 调 [`finish`][Self::finish] 收口（幂等）。
pub struct LogSession {
    pub(super) process_id: i64,
    pub(super) runtime_name: String,
    pub(super) dir: std::path::PathBuf,
    pub(super) redactor: LogRedactor,
    pub(super) seq: AtomicU64,
    pub(super) tx: Mutex<Option<Sender<SessionMsg>>>,
    pub(super) ring: Arc<Mutex<Ring>>,
    pub(super) worker: Mutex<Option<JoinHandle<()>>>,
    pub(super) finished: AtomicBool,
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
    pub(super) fn request_clear(&self) {
        if let Some(tx) = self.tx.lock().unwrap().as_ref() {
            let _ = tx.send(SessionMsg::Clear);
        }
    }
}
