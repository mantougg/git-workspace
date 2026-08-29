//! 日志文件查询（R-11，B-05 拆分）：search / tail / export / clear。
//!
//! 全部走流式读取（跨段按时间序逐行），不把整个日志文件加载到内存；
//! search 与 export 共用同一过滤管道，导出内容与显示一致。应用自身
//! `application.log` 走只读的 `search_file` / `tail_file`。

use std::collections::VecDeque;
use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};

use crate::error::{AppError, AppResult};
use crate::runtime::logs::level::parse_level;
use crate::runtime::logs::{LogEntry, LogLevel};

use super::storage::{current_segment_path, logs_dir, segment_paths};
use super::{LogExportOutcome, LogFilter, RuntimeLogEngine, DEFAULT_SEARCH_LIMIT};

impl RuntimeLogEngine {
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
            if matches_filter(filter, parse_level(text), text) && writeln!(writer, "{text}").is_ok()
            {
                lines += 1;
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
                let _ = std::fs::remove_file(&path);
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
