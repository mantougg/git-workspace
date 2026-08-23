//! Runtime 日志引擎（R-11，§35 日志系统、§36 日志功能、§37 智能增强预留、
//! §77 Runtime Log Secret Mask）。
//!
//! 统一接管构建与应用进程的 stdout / stderr：实时捕获 → **落盘前脱敏**
//! （[`redact::LogRedactor`]，规则与 T-08 共用）→ 落盘
//! `<workspace>/.gitworkspace/logs/<runtime>/<process_id>.log`（滚动切分 +
//! 容量上限）→ 环形内存缓冲 + 批量聚合经 `RuntimeEvent::Logs` 推送
//! （IPC/前端由 R-12 / R-13 接入，与 R-09 / R-10 同边界）。
//!
//! 特有设计点（任务文档「架构/性能注意点」）：
//! - **背压**：捕获线程（`on_line`）只做脱敏 + 发送，文件写盘与事件聚
//!   合在独立的 worker 线程按 `aggregate_interval` 批量进行；UI 侧永远
//!   只见到有界批次，日志洪水不打爆渲染。
//! - **级别识别**：[`level::parse_level`] 覆盖 Logback / Log4j2 默认
//!   pattern 与 Maven `[INFO]` 风格；识别不出降级为原文（`level=None`），
//!   级别过滤不误杀无级别行（stack trace 续行保持可见）。
//! - **大文件**：搜索 / 导出 / tail 全部流式读取，不整文件载入内存；
//!   导出与搜索共用同一过滤管道，导出内容与显示一致。
//! - **§37 预留**：[`LogAnalyzer`] 是 Exception Detection / Stack Trace
//!   Folding / Error Highlight 的挂接点，本任务只留接口位不实现。
//! - 应用自身 `application.log` 走只读的 `search_file` / `tail_file`
//!   （用户项目只读原则，全局约束 §2）。

pub mod engine;
pub mod level;
pub mod redact;

pub use engine::{LogExportOutcome, LogFilter, LogLimits, LogSession, RuntimeLogEngine};
pub use level::parse_level;
pub use redact::{sensitive_env_values, LogRedactor};

use serde::{Deserialize, Serialize};

use crate::process::streaming::OutputStream;

/// 日志级别（§36 过滤集合 + TRACE；序数语义：越大越严重）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

impl LogLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            LogLevel::Trace => "trace",
            LogLevel::Debug => "debug",
            LogLevel::Info => "info",
            LogLevel::Warn => "warn",
            LogLevel::Error => "error",
        }
    }
}

/// 日志来源阶段：构建（R-09 输出）或应用运行（R-10 进程输出）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogPhase {
    Build,
    Run,
}

/// 实时日志行（聚合批次 `RuntimeEvent::Logs` 的元素）。`line` 已脱敏。
/// camelCase 序列化，R-12 起跨 IPC。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogLine {
    /// 会话内单调序号（1 起）。
    pub seq: u64,
    /// 接收时间（RFC3339）。
    pub at: String,
    pub phase: LogPhase,
    pub stream: OutputStream,
    /// 解析出的级别；`None` = 未识别，降级为原文。
    pub level: Option<LogLevel>,
    pub line: String,
}

/// 文件查询（search / tail）的返回行：来自落盘文本，级别实时重解析。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogEntry {
    /// 跨滚动段的全局行号（1 起，最旧段开始计）。
    pub line_number: u64,
    pub level: Option<LogLevel>,
    pub text: String,
}

/// §37 智能增强预留挂接点：Exception Detection / Stack Trace Folding /
/// Error Highlight 由后续任务实现并注册到 [`RuntimeLogEngine`]。
///
/// 在 worker 线程按行回调（已脱敏、已解析级别）；实现必须廉价且不得
/// panic，panic 会中断日志写盘循环。
pub trait LogAnalyzer: Send + Sync {
    fn analyze(&self, line: &LogLine);
}
