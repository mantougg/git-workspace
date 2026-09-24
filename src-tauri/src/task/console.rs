//! Git Console 镜像设施（TM-04 / PAF-08 / PAF-25 / GF-07）。
//!
//! 任务队列（`worker.rs`）与单仓网络操作命令（`commands/git_ops.rs` 的
//! `sync_fetch/sync_pull/sync_push/smart_pull`、`commands/branch.rs` 的
//! `push_branch`）共用本模块：
//!
//! - [`ConsoleStreamer`]：git 流式输出 → Git Console 的 100ms 聚合桥，
//!   同时累积完整输出与 stderr 尾部（收尾事件 / 可读错误还原复用）；
//! - `git_op_output`（逐行镜像）与 GF-07 新增的 `git_op_started` /
//!   `git_op_finished`（单仓操作生命周期，前端据此展示取消入口）。

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter};

use crate::error::AppError;
use crate::models::task::TaskType;
use crate::process::OutputStream;

/// PAF-08/PAF-25：git_op_output 实时镜像的 100ms 聚合窗口（T-06 批量思路，
/// 防 clone/fetch 进度行事件风暴）。
pub(crate) const CONSOLE_FLUSH_INTERVAL: Duration = Duration::from_millis(100);
/// 失败时合成可读错误所保留的 stderr 尾部行数。
pub(crate) const STDERR_TAIL_LINES: usize = 8;

/// Network 操作在 Git Console 的命令标题行（与收尾 `git_command_result` 共用）。
pub(crate) fn network_console_command(task_type: &TaskType) -> Option<String> {
    match task_type {
        TaskType::Fetch => Some("git fetch <remote>".to_string()),
        TaskType::Pull => Some("git pull --ff-only".to_string()),
        TaskType::Push => Some("git push".to_string()),
        TaskType::Clone { url, .. } => Some(format!("git clone {}", url)),
        _ => None,
    }
}

/// TM-04：Git Console 镜像事件（git_op_output）。
pub(crate) fn emit_git_op_output(
    app: &AppHandle,
    repo_path: &str,
    repo_name: &str,
    command: &str,
    stream: &str,
    line: &str,
) {
    let _ = app.emit(
        "git_op_output",
        serde_json::json!({
            "repoPath": repo_path,
            "repoName": repo_name,
            "command": command,
            "stream": stream,
            "line": line,
        }),
    );
}

/// GF-07：单仓网络操作开始事件。前端 Git Console 据此弹出面板并登记取消入口
/// （`op_id` 即 `cancel_git_op` 的取消凭据）。
pub(crate) fn emit_git_op_started(app: &AppHandle, op_id: &str, repo_path: &str, repo_name: &str, command: &str) {
    let _ = app.emit(
        "git_op_started",
        serde_json::json!({
            "opId": op_id,
            "repoPath": repo_path,
            "repoName": repo_name,
            "command": command,
        }),
    );
}

/// GF-07：单仓网络操作结束事件（成功 / 失败 / 取消都经此收口）。
pub(crate) fn emit_git_op_finished(app: &AppHandle, op_id: &str, success: bool, error: Option<&str>) {
    let _ = app.emit(
        "git_op_finished",
        serde_json::json!({
            "opId": op_id,
            "success": success,
            "error": error,
        }),
    );
}

/// PAF-25：把 git 流式输出实时桥接到 Git Console。
///
/// - 100ms 窗口聚合进度行后再 emit（防事件风暴）；
/// - 累积完整输出（收尾的 `git_command_result` / DAG / batch 汇总复用）；
/// - 保留 stderr 尾部：`run_git_streaming` 对非零退出只给通用错误，凭尾部
///   还原可读原因（认证失败等）。
pub(crate) struct ConsoleStreamer {
    app: AppHandle,
    repo_path: String,
    repo_name: String,
    command: String,
    batch: Vec<(OutputStream, String)>,
    window_start: Instant,
    full_output: String,
    stderr_tail: VecDeque<String>,
}

impl ConsoleStreamer {
    pub(crate) fn new(app: AppHandle, repo_path: String, repo_name: String, command: String) -> Self {
        ConsoleStreamer {
            app,
            repo_path,
            repo_name,
            command,
            batch: Vec::new(),
            window_start: Instant::now(),
            full_output: String::new(),
            stderr_tail: VecDeque::new(),
        }
    }

    /// 命令标题行（`$ git fetch <remote>` 样式，先于输出发出）。
    pub(crate) fn emit_meta_header(&self) {
        emit_git_op_output(
            &self.app,
            &self.repo_path,
            &self.repo_name,
            &self.command,
            "meta",
            &format!("$ {}", self.command),
        );
    }

    pub(crate) fn on_line(&mut self, stream: OutputStream, line: &str) {
        self.full_output.push_str(line);
        self.full_output.push('\n');
        if matches!(stream, OutputStream::Stderr) {
            if self.stderr_tail.len() == STDERR_TAIL_LINES {
                self.stderr_tail.pop_front();
            }
            self.stderr_tail.push_back(line.to_string());
        }
        self.batch.push((stream, line.to_string()));
        if self.window_start.elapsed() >= CONSOLE_FLUSH_INTERVAL {
            self.flush();
        }
    }

    pub(crate) fn flush(&mut self) {
        if self.batch.is_empty() {
            return;
        }
        for (stream, line) in std::mem::take(&mut self.batch) {
            let stream_str = match stream {
                OutputStream::Stdout => "stdout",
                OutputStream::Stderr => "stderr",
            };
            emit_git_op_output(
                &self.app,
                &self.repo_path,
                &self.repo_name,
                &self.command,
                stream_str,
                &line,
            );
        }
        self.window_start = Instant::now();
    }

    /// 非成功结局在 Console 补一行可读结论（超时/取消/失败）。
    pub(crate) fn emit_outcome_line(&self, reason: &str) {
        emit_git_op_output(
            &self.app,
            &self.repo_path,
            &self.repo_name,
            &self.command,
            "meta",
            &format!("✘ {reason}"),
        );
    }

    /// 失败时合成可读错误：优先 stderr 尾部，退化到原始错误。
    pub(crate) fn readable_error(&self, err: AppError) -> AppError {
        let msg = err.to_string();
        if msg.contains("exited with code") && !self.stderr_tail.is_empty() {
            let tail = self
                .stderr_tail
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>()
                .join("\n");
            AppError::Git(git2::Error::from_str(tail.trim()))
        } else {
            err
        }
    }

    /// 累积的完整输出（trim 后供命令返回值 / 事件使用）。
    pub(crate) fn full_output(&self) -> &str {
        &self.full_output
    }
}

/// GF-07：流式网络操作的统一收尾（worker 队列路径与单仓命令路径共用语义）：
/// flush 残留批次 → 失败时把超时/取消/一般失败翻成人话，Console 补结论行 →
/// 用 stderr 尾部还原可读错误。成功返回 trim 后的完整输出。
pub(crate) fn finish_streaming(
    result: crate::error::AppResult<crate::process::StreamingExit>,
    streamer: &ConsoleStreamer,
    timeout: Duration,
) -> crate::error::AppResult<String> {
    match result {
        Ok(_) => Ok(streamer.full_output().trim().to_string()),
        Err(e) => {
            let msg = e.to_string();
            let reason = if msg.contains("timed out") {
                format!("超时（≥{}s），git 进程已终止", timeout.as_secs())
            } else if msg.contains("cancelled") {
                "已取消".to_string()
            } else {
                msg
            };
            streamer.emit_outcome_line(&reason);
            Err(streamer.readable_error(e))
        }
    }
}
