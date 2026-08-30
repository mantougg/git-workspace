//! Context Builder（设计文档 §8.1）：从现有领域服务收集结构化上下文。
//!
//! 硬约束（全局约束 §6 / §13）：
//! - 只调用现有领域服务——Workspace/Repository store、Status Engine、
//!   T-04 Diff、History、T-16 Conflict、R-07 Runtime 配置、R-02/R-03
//!   Closure、R-10/R-16 进程端口、R-11/R-13 日志、R-04/R-05 JDK/Maven；
//!   不直接扫描用户项目、不复制一份领域数据；
//! - 每个条目带 [`ContextRole`]（预算策略 §8.2 据此排优先级）与稳定
//!   `source_id`（Preview 排除项、审计、缓存 hash 的关联键）；
//! - 已在来源侧脱敏的内容（redacted 版 Runtime 配置、写入侧脱敏的日志、
//!   结构化错误的 redacted log_tail）条目标记 `redacted`；本模块不再对
//!   内容做 Secret 处理（统一走 [`super::redact`] 管道）；
//! - 大日志 / 大 diff 分块：日志按行数取尾部、diff 按文件拆分且单文件
//!   行数封顶，预算内的进一步截断由 [`super::policy`] 处理。

use std::path::Path;

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::core::{conflict, diff, git_status, graph};
use crate::error::{AppError, AppResult};
use crate::runtime::service::{RuntimeLogQuery, RuntimeService};
use crate::runtime::{config as runtime_config, logs::LogFilter};

use super::request::{estimate_tokens, ContextItem, ContextKind, ExclusionReason};

/// 单文件 diff 行数上限（与 T-04 IPC 口径一致），防爆量。
const MAX_DIFF_LINES_PER_FILE: usize = 2000;
/// 冲突文件单侧内容上限（字符），T-16 自身 cap 为 500k，上下文场景再收紧。
const MAX_CONFLICT_SIDE_CHARS: usize = 20_000;

// ---------------------------------------------------------------------------
// 角色与草稿条目
// ---------------------------------------------------------------------------

/// 上下文角色：预算策略（§8.2）按角色决定保留优先级，与具体任务解耦。
/// 角色命名与 §8.2 的五类策略措辞一一对应。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ContextRole {
    /// 结构化错误（BuildFailed / ProcessStartFailed 等 AppError 渲染）。
    StructuredError,
    /// 最近错误日志（min_level=Error 的尾部）。
    ErrorLog,
    /// 日志尾部（不区分级别）。
    LogTail,
    /// 用户在日志视图中选中的范围。
    SelectedLogRange,
    /// 异常堆栈片段。
    ExceptionStack,
    /// 环境摘要（JDK / Maven 注册表）。
    EnvironmentSummary,
    /// Runtime 配置（redacted 版）。
    RuntimeConfig,
    /// 进程与端口实况。
    ProcessInfo,
    /// 变更/审查的文件清单。
    FileList,
    /// diff 的 hunk 结构（每文件 hunk 数与增删行统计，不含正文）。
    HunkStructure,
    /// 具体/完整 diff 正文。
    FullDiff,
    /// 变更摘要（分支、ahead/behind、状态计数）。
    ChangeSummary,
    /// 多仓库场景中每个仓库的摘要。
    RepoSummary,
    /// 提交历史。
    History,
    /// 冲突操作状态（merge/cherry-pick/rebase + 冲突文件清单）。
    ConflictState,
    /// 单个冲突文件的内容（ours/theirs/worktree）。
    ConflictContent,
    /// 依赖与 Closure（R-02/R-03）。
    Dependency,
    /// 调用方/用户补充说明。
    UserNote,
}

/// 预算截断时保留内容的哪一侧。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TruncateKeep {
    /// 保留头部（diff、清单类：开头信息量最大）。
    Head,
    /// 保留尾部（日志类：最近的行最重要，§8.2 尾部优先）。
    Tail,
}

/// 收集到的上下文草稿（含正文；尚未做预算/Secret 处理）。
/// 经 redact → policy 管道后转化为发送正文 + Manifest（[`ContextItem`]）。
#[derive(Debug, Clone)]
pub struct DraftContextItem {
    pub kind: ContextKind,
    pub role: ContextRole,
    /// 稳定来源标识（如 `diff:staged:src/main.rs`、`log:app:12:tail`）。
    pub source_id: String,
    pub display_name: String,
    pub content: String,
    /// 来源侧已完成脱敏（redacted 配置 / 落盘脱敏日志 / redacted log_tail）。
    pub redacted: bool,
    /// 预算不足时的截断方向；`None` = 不可截断（要么整发要么排除）。
    pub truncate_keep: Option<TruncateKeep>,
    /// 收集/扫描阶段已确定的排除（用户排除 / Secret 策略）。
    pub exclusion: Option<ExclusionReason>,
}

impl DraftContextItem {
    /// 调用方注入的补充条目（场景特有的结构化错误、UI 选中范围等）。
    pub fn supplementary(
        role: ContextRole,
        kind: ContextKind,
        source_id: impl Into<String>,
        display_name: impl Into<String>,
        content: impl Into<String>,
    ) -> Self {
        Self {
            kind,
            role,
            source_id: source_id.into(),
            display_name: display_name.into(),
            content: content.into(),
            redacted: false,
            truncate_keep: Some(TruncateKeep::Head),
            exclusion: None,
        }
    }

    /// 生成 Manifest 条目（§7.1；只描述来源与计量，不含正文）。
    /// `truncated` 由预算阶段回填，此处恒为 false。
    pub fn manifest_item(&self, estimator: &TokenEstimator) -> ContextItem {
        ContextItem {
            kind: self.kind,
            source_id: self.source_id.clone(),
            display_name: self.display_name.clone(),
            char_count: self.content.chars().count() as i64,
            estimated_tokens: estimator.estimate(&self.content),
            redacted: self.redacted,
            truncated: false,
            excluded: self.exclusion.is_some(),
            exclusion_reason: self.exclusion,
        }
    }
}

// ---------------------------------------------------------------------------
// token 估算（AI-03 实现细节：chars/4 基准 × 校准系数）
// ---------------------------------------------------------------------------

/// token 估算器：以 [`estimate_tokens`]（chars/4）为基准，校准系数
/// `factor` 可按模型配置调整（>1 = 更保守高估；非法值回落 1.0）。
/// 估算值进 `ContextItem.estimated_tokens`，只用于预算门槛与展示，不计费。
#[derive(Debug, Clone, Copy)]
pub struct TokenEstimator {
    factor: f64,
}

impl TokenEstimator {
    pub fn new(factor: Option<f64>) -> Self {
        let factor = factor
            .filter(|f| f.is_finite() && *f > 0.0 && *f <= 10.0)
            .unwrap_or(1.0);
        Self { factor }
    }

    pub fn estimate(&self, text: &str) -> i64 {
        ((estimate_tokens(text) as f64) * self.factor).ceil() as i64
    }

    /// `tokens` 预算对应的字符额度（截断换算用；与 [`Self::estimate`] 互逆）。
    pub fn chars_for_tokens(&self, tokens: i64) -> usize {
        if tokens <= 0 {
            return 0;
        }
        ((tokens as f64) * 4.0 / self.factor).floor().max(0.0) as usize
    }
}

impl Default for TokenEstimator {
    fn default() -> Self {
        Self::new(None)
    }
}

// ---------------------------------------------------------------------------
// 内容 hash（缓存 key / 变更检测；FNV-1a 64，稳定但非加密）
// ---------------------------------------------------------------------------

/// 对最终发送内容（system + 消息正文）计算稳定性 hash。排除项变更后
/// 必须重算（§7.3）。非加密用途，碰撞风险对本场景可接受。
pub fn content_hash(parts: &[&str]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for part in parts {
        for b in part.as_bytes() {
            h ^= u64::from(*b);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        // 分段分隔，避免 ["ab","c"] 与 ["a","bc"] 同 hash。
        h ^= 0xff;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{h:016x}")
}

// ---------------------------------------------------------------------------
// Git 域收集器（T-02 / T-04 / History / T-16）
// ---------------------------------------------------------------------------

/// Workspace 概览（Workspace/Repository store）：名称、路径、仓库清单。
pub fn collect_workspace_summary(
    conn: &Connection,
    workspace_id: i64,
) -> AppResult<DraftContextItem> {
    let ws = crate::db::dao::get_workspace(conn, workspace_id)?;
    let repos = crate::db::dao::list_repositories_by_workspace(conn, workspace_id)?;
    let mut content = format!("Workspace: {} ({})\n仓库数量: {}\n", ws.name, ws.path, repos.len());
    for r in &repos {
        content.push_str(&format!("- {} ({})\n", r.name, r.relative_path));
    }
    Ok(DraftContextItem {
        kind: ContextKind::Repository,
        role: ContextRole::RepoSummary,
        source_id: format!("workspace:{workspace_id}"),
        display_name: format!("Workspace「{}」概览", ws.name),
        content,
        redacted: false,
        truncate_keep: Some(TruncateKeep::Head),
        exclusion: None,
    })
}

/// 仓库状态（Status Engine）：分支/ahead/behind/状态计数 + 变更文件清单。
pub fn collect_repo_status(repo_path: &Path) -> AppResult<Vec<DraftContextItem>> {
    let status = git_status::get_repo_status(repo_path)?;
    let changes = git_status::get_repo_changes(repo_path)?;
    let path_key = repo_path.to_string_lossy().replace('\\', "/");

    let summary = format!(
        "分支: {}\nahead/behind: +{}/-{}\nmodified: {} added: {} deleted: {} untracked: {} staged: {} conflicted: {}\nis_clean: {}",
        if status.is_detached {
            format!("(detached) {}", status.branch)
        } else {
            status.branch.clone()
        },
        status.ahead,
        status.behind,
        status.modified,
        status.added,
        status.deleted,
        status.untracked,
        status.staged,
        status.conflicted,
        status.is_clean,
    );

    let mut file_list = String::new();
    for c in &changes.changes {
        file_list.push_str(&format!("{}\t{}\n", c.status, c.path));
    }
    if file_list.is_empty() {
        file_list.push_str("（无变更文件）\n");
    }

    Ok(vec![
        DraftContextItem {
            kind: ContextKind::Repository,
            role: ContextRole::ChangeSummary,
            source_id: format!("repo:{path_key}:status"),
            display_name: format!("仓库「{}」状态", changes.repo_name),
            content: summary,
            redacted: false,
            truncate_keep: None,
            exclusion: None,
        },
        DraftContextItem {
            kind: ContextKind::Repository,
            role: ContextRole::FileList,
            source_id: format!("repo:{path_key}:changes"),
            display_name: format!("仓库「{}」变更文件清单", changes.repo_name),
            content: file_list,
            redacted: false,
            truncate_keep: Some(TruncateKeep::Head),
            exclusion: None,
        },
    ])
}

/// diff 范围：工作区（T-04 workdir）/ 已暂存 / 未暂存。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DiffScope {
    Workdir,
    Staged,
    Unstaged,
}

impl DiffScope {
    pub fn as_str(&self) -> &'static str {
        match self {
            DiffScope::Workdir => "workdir",
            DiffScope::Staged => "staged",
            DiffScope::Unstaged => "unstaged",
        }
    }

    fn load(&self, repo_path: &Path) -> AppResult<Vec<diff::FileDiff>> {
        let config = diff::DiffConfig::default();
        match self {
            DiffScope::Workdir => diff::get_workdir_diff(repo_path),
            DiffScope::Staged => diff::get_staged_diff_with_config(repo_path, &config),
            DiffScope::Unstaged => diff::get_unstaged_diff_with_config(repo_path, &config),
        }
    }
}

/// diff 摘要（§8.2 Code Review「文件清单和 hunk 结构」/ Commit Message
/// 「diff 摘要」）：每文件状态、hunk 数、增删行统计，不含正文。
pub fn collect_diff_summary(repo_path: &Path, scope: DiffScope) -> AppResult<DraftContextItem> {
    let files = scope.load(repo_path)?;
    let path_key = repo_path.to_string_lossy().replace('\\', "/");
    let mut content = format!("diff 范围: {}，文件数: {}\n", scope.as_str(), files.len());
    for f in &files {
        let (mut adds, mut dels) = (0usize, 0usize);
        for h in &f.hunks {
            for l in &h.lines {
                match l.line_type.as_str() {
                    "add" => adds += 1,
                    "delete" => dels += 1,
                    _ => {}
                }
            }
        }
        content.push_str(&format!(
            "{}\t{}\t{} hunks (+{}/-{})\n",
            f.status, f.new_path, f.hunks.len(), adds, dels
        ));
    }
    Ok(DraftContextItem {
        kind: ContextKind::Diff,
        role: ContextRole::HunkStructure,
        source_id: format!("diff:{}:{path_key}:summary", scope.as_str()),
        display_name: format!("diff 摘要（{}）", scope.as_str()),
        content,
        redacted: false,
        truncate_keep: Some(TruncateKeep::Head),
        exclusion: None,
    })
}

/// 逐文件完整 diff（§8.2 Code Review「具体 diff」）：每文件一个条目，
/// 单文件行数封顶（[`MAX_DIFF_LINES_PER_FILE`]），超出的行标记截断。
pub fn collect_diff_files(repo_path: &Path, scope: DiffScope) -> AppResult<Vec<DraftContextItem>> {
    let files = scope.load(repo_path)?;
    let path_key = repo_path.to_string_lossy().replace('\\', "/");
    Ok(files
        .iter()
        .map(|f| DraftContextItem {
            kind: ContextKind::Diff,
            role: ContextRole::FullDiff,
            source_id: format!("diff:{}:{path_key}:{}", scope.as_str(), f.new_path),
            display_name: format!("diff: {}", f.new_path),
            content: render_file_diff(f),
            redacted: false,
            truncate_keep: Some(TruncateKeep::Head),
            exclusion: None,
        })
        .collect())
}

/// 渲染单文件 diff 为 unified 风格文本（行数封顶，超出标记截断）。
pub fn render_file_diff(file: &diff::FileDiff) -> String {
    let mut out = format!("--- {} ({})\n", file.new_path, file.status);
    let mut lines = 0usize;
    'outer: for hunk in &file.hunks {
        out.push_str(&format!(
            "@@ -{},{} +{},{} @@\n",
            hunk.old_start, hunk.old_lines, hunk.new_start, hunk.new_lines
        ));
        for line in &hunk.lines {
            let prefix = match line.line_type.as_str() {
                "add" => "+",
                "delete" => "-",
                _ => " ",
            };
            out.push_str(prefix);
            out.push_str(&line.content);
            out.push('\n');
            lines += 1;
            if lines >= MAX_DIFF_LINES_PER_FILE {
                out.push_str("... (file diff truncated)\n");
                break 'outer;
            }
        }
    }
    out
}

/// 提交历史（Graph/History 能力）：最近 `max_count` 条。
pub fn collect_history(repo_path: &Path, max_count: usize) -> AppResult<DraftContextItem> {
    let commits = graph::get_commit_history(repo_path, max_count)?;
    let path_key = repo_path.to_string_lossy().replace('\\', "/");
    let mut content = String::new();
    for c in &commits {
        let first_line = c.message.lines().next().unwrap_or("");
        content.push_str(&format!("{} {} ({})\n", c.short_oid, first_line, c.author));
    }
    if content.is_empty() {
        content.push_str("（无提交历史）\n");
    }
    Ok(DraftContextItem {
        kind: ContextKind::Repository,
        role: ContextRole::History,
        source_id: format!("repo:{path_key}:history"),
        display_name: format!("最近 {} 条提交", commits.len()),
        content,
        redacted: false,
        truncate_keep: Some(TruncateKeep::Head),
        exclusion: None,
    })
}

/// 冲突状态（T-16）：操作类型 + 冲突文件清单 + 逐文件冲突内容。
pub fn collect_conflicts(repo_path: &Path) -> AppResult<Vec<DraftContextItem>> {
    let state = conflict::operation_state(repo_path)?;
    let path_key = repo_path.to_string_lossy().replace('\\', "/");

    let mut ops: Vec<&str> = Vec::new();
    if state.merge {
        ops.push("merge");
    }
    if state.cherry_pick {
        ops.push("cherry-pick");
    }
    if state.revert {
        ops.push("revert");
    }
    if state.rebase.is_some() {
        ops.push("rebase");
    }
    let mut summary = format!(
        "进行中的操作: {}\n冲突文件数: {}\n",
        if ops.is_empty() { "无".to_string() } else { ops.join(", ") },
        state.conflicts.len()
    );
    for c in &state.conflicts {
        summary.push_str(&format!("{}\t{}\n", c.conflict_type, c.path));
    }

    let mut items = vec![DraftContextItem {
        kind: ContextKind::Repository,
        role: ContextRole::ConflictState,
        source_id: format!("repo:{path_key}:conflicts"),
        display_name: "冲突状态与文件清单".to_string(),
        content: summary,
        redacted: false,
        truncate_keep: None,
        exclusion: None,
    }];

    for c in &state.conflicts {
        if let Ok(content) = conflict::conflict_content(repo_path, &c.path) {
            items.push(DraftContextItem {
                kind: ContextKind::File,
                role: ContextRole::ConflictContent,
                source_id: format!("conflict:{path_key}:{}", c.path),
                display_name: format!("冲突文件: {}", c.path),
                content: render_conflict_content(&content),
                redacted: false,
                truncate_keep: Some(TruncateKeep::Head),
                exclusion: None,
            });
        }
    }
    Ok(items)
}

/// 渲染冲突内容（ours / theirs / worktree 三段，单侧字符封顶）。
fn render_conflict_content(content: &conflict::ConflictContent) -> String {
    let mut out = String::new();
    let mut push_side = |label: &str, side: &Option<String>| {
        if let Some(text) = side {
            out.push_str(&format!("<<<<<<< {label}\n"));
            let truncated: String = text.chars().take(MAX_CONFLICT_SIDE_CHARS).collect();
            out.push_str(&truncated);
            if truncated.chars().count() < text.chars().count() {
                out.push_str("\n... (side truncated)");
            }
            out.push_str(&format!("\n>>>>>>> {label}\n"));
        }
    };
    push_side("OURS", &content.ours);
    push_side("THEIRS", &content.theirs);
    push_side("WORKTREE", &content.worktree);
    out
}

// ---------------------------------------------------------------------------
// Runtime 域收集器（R-07 / R-10 / R-16 / R-11 / R-13 / R-02 / R-03 / R-04 / R-05）
// ---------------------------------------------------------------------------

/// Runtime 配置（R-07）：**只取 redacted 版**（敏感 env 值已掩码），
/// 未脱敏版是 `pub(crate)` 且不得跨越边界。
pub fn collect_runtime_config(
    conn: &Connection,
    workspace_id: i64,
    name: &str,
) -> AppResult<DraftContextItem> {
    let cfg = runtime_config::get_config(conn, workspace_id, name)?;
    let mut content = format!(
        "Runtime: {}\n项目: {}\nmain_class: {}\njdk: {}\nprofile: {}\nbuild_engine: {}\n",
        cfg.name,
        cfg.project,
        cfg.main_class.as_deref().unwrap_or("(未设置)"),
        cfg.jdk.as_deref().unwrap_or("(未设置)"),
        cfg.profile.as_deref().unwrap_or("(未设置)"),
        cfg.build_engine.as_deref().unwrap_or("(未设置)"),
    );
    if !cfg.vm_options.is_empty() {
        content.push_str(&format!("vm_options: {}\n", cfg.vm_options.join(" ")));
    }
    if !cfg.program_arguments.is_empty() {
        content.push_str(&format!(
            "program_arguments: {}\n",
            cfg.program_arguments.join(" ")
        ));
    }
    if !cfg.environment.is_empty() {
        content.push_str("environment:\n");
        for (k, v) in &cfg.environment {
            content.push_str(&format!("  {k}={v}\n"));
        }
    }
    Ok(DraftContextItem {
        kind: ContextKind::Runtime,
        role: ContextRole::RuntimeConfig,
        source_id: format!("runtime:{workspace_id}:{name}:config"),
        display_name: format!("Runtime「{name}」配置"),
        content,
        redacted: true,
        truncate_keep: Some(TruncateKeep::Head),
        exclusion: None,
    })
}

/// 进程与端口实况（R-10/R-16）：状态、pid、端口、退出码、运行时长。
pub fn collect_runtime_processes(
    service: &RuntimeService,
    workspace_id: i64,
) -> AppResult<DraftContextItem> {
    let processes = service.list_processes(workspace_id)?;
    let mut content = format!("进程数: {}\n", processes.len());
    for p in &processes {
        content.push_str(&format!(
            "- {} [{}] pid={} ports=[{}] exit_code={} uptime={}s\n",
            p.runtime_name,
            p.status.as_str(),
            p.pid.map(|v| v.to_string()).unwrap_or_else(|| "-".into()),
            p.ports
                .iter()
                .map(|v| v.to_string())
                .collect::<Vec<_>>()
                .join(","),
            p.exit_code
                .map(|v| v.to_string())
                .unwrap_or_else(|| "-".into()),
            p.uptime_seconds.unwrap_or(0),
        ));
    }
    Ok(DraftContextItem {
        kind: ContextKind::Runtime,
        role: ContextRole::ProcessInfo,
        source_id: format!("runtime:{workspace_id}:processes"),
        display_name: "Runtime 进程与端口实况".to_string(),
        content,
        redacted: false,
        truncate_keep: Some(TruncateKeep::Head),
        exclusion: None,
    })
}

/// 日志尾部（R-11/R-13）：最近 `n` 行（写入侧已脱敏，标记 redacted）。
pub fn collect_runtime_log_tail(
    service: &RuntimeService,
    workspace_id: i64,
    runtime_name: &str,
    process_id: i64,
    n: usize,
) -> AppResult<DraftContextItem> {
    let query = RuntimeLogQuery {
        workspace_id,
        runtime_name: runtime_name.to_string(),
        process_id,
        filter: LogFilter::default(),
    };
    let entries = service.tail_logs(&query, n)?;
    Ok(log_item(
        entries,
        ContextRole::LogTail,
        format!("log:{workspace_id}:{runtime_name}:{process_id}:tail"),
        format!("日志尾部（最近 {n} 行）"),
    ))
}

/// 最近错误日志（R-11/R-13，§8.2 错误诊断「最近错误日志」）：
/// min_level=Error 的尾部 `n` 行。
pub fn collect_runtime_error_logs(
    service: &RuntimeService,
    workspace_id: i64,
    runtime_name: &str,
    process_id: i64,
    n: usize,
) -> AppResult<DraftContextItem> {
    let query = RuntimeLogQuery {
        workspace_id,
        runtime_name: runtime_name.to_string(),
        process_id,
        filter: LogFilter {
            min_level: Some(crate::runtime::logs::LogLevel::Error),
            ..LogFilter::default()
        },
    };
    let entries = service.search_logs_tail(&query, n)?;
    Ok(log_item(
        entries,
        ContextRole::ErrorLog,
        format!("log:{workspace_id}:{runtime_name}:{process_id}:errors"),
        format!("最近错误日志（{n} 行内）"),
    ))
}

fn log_item(
    entries: Vec<crate::runtime::logs::LogEntry>,
    role: ContextRole,
    source_id: String,
    display_name: String,
) -> DraftContextItem {
    let mut content = String::new();
    for e in &entries {
        content.push_str(&e.text);
        content.push('\n');
    }
    if content.is_empty() {
        content.push_str("（无匹配日志）\n");
    }
    DraftContextItem {
        kind: ContextKind::Log,
        role,
        source_id,
        display_name,
        content,
        redacted: true,
        truncate_keep: Some(TruncateKeep::Tail),
        exclusion: None,
    }
}

/// 环境摘要（R-04/R-05）：JDK 与 Maven 注册表。
pub fn collect_environment_summary(conn: &Connection) -> AppResult<DraftContextItem> {
    let jdks = crate::java::registry::list_jdks(conn)?;
    let mavens = crate::maven::registry::list_maven_executables(conn)?;
    let mut content = format!("JDK（{} 个）:\n", jdks.len());
    for j in &jdks {
        content.push_str(&format!(
            "- {} {} ({})\n",
            j.full_version.as_deref().unwrap_or("unknown"),
            j.architecture.as_deref().unwrap_or("?"),
            j.home_path
        ));
    }
    content.push_str(&format!("Maven（{} 个）:\n", mavens.len()));
    for m in &mavens {
        content.push_str(&format!(
            "- {} ({})\n",
            m.full_version.as_deref().unwrap_or("unknown"),
            m.executable_path
        ));
    }
    Ok(DraftContextItem {
        kind: ContextKind::Runtime,
        role: ContextRole::EnvironmentSummary,
        source_id: "env:jdk-maven".to_string(),
        display_name: "JDK / Maven 环境摘要".to_string(),
        content,
        redacted: false,
        truncate_keep: Some(TruncateKeep::Head),
        exclusion: None,
    })
}

/// 项目依赖（R-02/R-03 Closure）：项目 GAV、模块与出边依赖清单。
pub fn collect_project_dependencies(
    service: &RuntimeService,
    workspace_id: i64,
    project: &str,
) -> AppResult<DraftContextItem> {
    let inspection = service.inspect_project(workspace_id, project)?;
    let mut content = format!(
        "项目: {} ({})\n模块数: {}\n依赖数: {}\n",
        inspection.project.coordinates.gav(),
        inspection.project.path.display(),
        inspection.modules.len(),
        inspection.dependencies.len(),
    );
    for edge in &inspection.dependencies {
        let d = &edge.dependency;
        content.push_str(&format!(
            "- {}:{}:{} ({})\n",
            d.group_id,
            d.artifact_id,
            d.version.as_deref().unwrap_or("(managed)"),
            format!("{:?}", d.scope).to_lowercase(),
        ));
    }
    Ok(DraftContextItem {
        kind: ContextKind::Dependency,
        role: ContextRole::Dependency,
        source_id: format!("deps:{workspace_id}:{}", inspection.project.coordinates.gav()),
        display_name: format!("项目「{}」依赖", inspection.project.coordinates.artifact_id),
        content,
        redacted: false,
        truncate_keep: Some(TruncateKeep::Head),
        exclusion: None,
    })
}

/// 结构化错误（R-14 结构化 AppError，§8.2 错误诊断最高优先级）。
/// BuildFailed 等携带的 log_tail 在写入侧已脱敏（RingTail），标记 redacted。
pub fn collect_structured_error(error: &AppError) -> DraftContextItem {
    DraftContextItem {
        kind: ContextKind::Error,
        role: ContextRole::StructuredError,
        source_id: format!("error:{}", error.code()),
        display_name: format!("结构化错误（{}）", error.code()),
        content: format!("code: {}\nmessage: {}", error.code(), error),
        redacted: true,
        truncate_keep: None,
        exclusion: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_estimator_default_is_chars_over_four() {
        let est = TokenEstimator::default();
        assert_eq!(est.estimate(""), 0);
        assert_eq!(est.estimate("abcd"), 1);
        assert_eq!(est.estimate("abcde"), 2);
    }

    #[test]
    fn token_estimator_applies_calibration_factor() {
        let conservative = TokenEstimator::new(Some(2.0));
        assert_eq!(conservative.estimate("abcd"), 2, "系数 2 → 估算翻倍");
        // 非法系数回落 1.0
        assert_eq!(TokenEstimator::new(Some(0.0)).estimate("abcd"), 1);
        assert_eq!(TokenEstimator::new(Some(f64::NAN)).estimate("abcd"), 1);
        assert_eq!(TokenEstimator::new(Some(99.0)).estimate("abcd"), 1);
    }

    #[test]
    fn content_hash_is_stable_and_order_sensitive() {
        let h1 = content_hash(&["ab", "c"]);
        let h2 = content_hash(&["ab", "c"]);
        let h3 = content_hash(&["a", "bc"]);
        assert_eq!(h1, h2);
        assert_ne!(h1, h3, "分段不同必须产生不同 hash");
        assert_eq!(h1.len(), 16);
    }

    #[test]
    fn manifest_item_reflects_draft_state() {
        let mut draft = DraftContextItem::supplementary(
            ContextRole::UserNote,
            ContextKind::File,
            "note:1",
            "备注",
            "abcd",
        );
        let item = draft.manifest_item(&TokenEstimator::default());
        assert_eq!(item.char_count, 4);
        assert_eq!(item.estimated_tokens, 1);
        assert!(!item.excluded && !item.truncated && !item.redacted);

        draft.redacted = true;
        draft.exclusion = Some(ExclusionReason::User);
        let item = draft.manifest_item(&TokenEstimator::default());
        assert!(item.redacted && item.excluded);
        assert_eq!(item.exclusion_reason, Some(ExclusionReason::User));
    }
}
