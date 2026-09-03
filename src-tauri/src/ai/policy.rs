//! 上下文预算策略（设计文档 §8.2）：尾部优先、结构优先、按任务分块。
//!
//! 五类策略只决定**角色 → 优先级层（tier）**的映射；截断/排除的执行是
//! 统一的 [`apply_budget`]：tier 小者优先保留，预算耗尽时可截断的条目
//! 截断（保留头/尾由收集器指定），不可截断或剩余过小的条目排除并标记
//! `BudgetOverflow`——**截断与排除都进 Manifest，不得静默强行发送**。

use serde::{Deserialize, Serialize};

use super::context::{ContextRole, DraftContextItem, TokenEstimator, TruncateKeep};
use super::model::AiTaskKind;
use super::request::{ContextItem, ExclusionReason};

/// 剩余预算低于该值时不再截断（截出几十字符没有信息量），直接排除。
const MIN_TRUNCATE_TOKENS: i64 = 32;

/// 五类预算策略（§8.2）。序列化为 camelCase 字符串供 IPC/前端选择。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BudgetStrategy {
    /// 错误诊断：结构化错误 > 最近错误日志 > 日志尾部 > 环境摘要。
    ErrorDiagnosis,
    /// 日志分析：用户选中范围 > 异常堆栈 > 前后少量上下文。
    LogAnalysis,
    /// Code Review：文件清单和 hunk 结构 > 具体 diff。
    CodeReview,
    /// Commit Message：变更文件、状态、diff 摘要 > 完整 diff。
    CommitMessage,
    /// 多仓库 Summary：每仓库摘要 > 所有文件逐行内容。
    MultiRepoSummary,
}

impl BudgetStrategy {
    pub fn as_str(&self) -> &'static str {
        match self {
            BudgetStrategy::ErrorDiagnosis => "errorDiagnosis",
            BudgetStrategy::LogAnalysis => "logAnalysis",
            BudgetStrategy::CodeReview => "codeReview",
            BudgetStrategy::CommitMessage => "commitMessage",
            BudgetStrategy::MultiRepoSummary => "multiRepoSummary",
        }
    }

    /// 任务默认策略；调用方可按场景覆盖（如 AI-06 日志分析选 LogAnalysis）。
    pub fn for_task(task_kind: AiTaskKind) -> Self {
        match task_kind {
            AiTaskKind::RuntimeDiagnostic => BudgetStrategy::ErrorDiagnosis,
            AiTaskKind::GitReview => BudgetStrategy::CodeReview,
            AiTaskKind::CommitMessage => BudgetStrategy::CommitMessage,
            AiTaskKind::Conflict => BudgetStrategy::CodeReview,
            AiTaskKind::Chat => BudgetStrategy::MultiRepoSummary,
        }
    }

    /// 该策略下角色的优先级层（数值小者优先保留）。结构化事实（错误、
    /// 清单、摘要、用户补充）恒在低层（优先），原始大内容（完整 diff、
    /// 日志尾部）在高层（先被截断/排除）——「结构优先」的落实。
    pub fn tier_of(&self, role: ContextRole) -> u8 {
        use BudgetStrategy as S;
        use ContextRole as R;
        match self {
            S::ErrorDiagnosis => match role {
                R::StructuredError | R::UserNote => 0,
                R::ErrorLog | R::ExceptionStack => 1,
                R::LogTail | R::ConflictState => 2,
                R::EnvironmentSummary | R::RuntimeConfig | R::ProcessInfo => 3,
                _ => 4,
            },
            S::LogAnalysis => match role {
                R::SelectedLogRange | R::UserNote | R::StructuredError => 0,
                R::ExceptionStack | R::ErrorLog => 1,
                R::LogTail => 2,
                R::EnvironmentSummary | R::RuntimeConfig | R::ProcessInfo => 3,
                _ => 4,
            },
            S::CodeReview => match role {
                R::FileList | R::HunkStructure | R::ChangeSummary | R::UserNote | R::ConflictState => 0,
                R::FullDiff | R::ConflictContent => 1,
                R::History | R::Dependency => 2,
                _ => 3,
            },
            S::CommitMessage => match role {
                R::ChangeSummary | R::FileList | R::UserNote => 0,
                R::HunkStructure => 1,
                R::FullDiff | R::History => 2,
                _ => 3,
            },
            S::MultiRepoSummary => match role {
                R::RepoSummary | R::UserNote => 0,
                R::ChangeSummary | R::FileList => 1,
                R::HunkStructure | R::History => 2,
                R::FullDiff => 3,
                _ => 4,
            },
        }
    }
}

/// 预算应用结果：最终条目（截断后正文 + 排除标记）与 Manifest。
#[derive(Debug)]
pub struct BudgetOutcome {
    /// 与 `manifest` 同序的最终条目（排除项保留原正文供 UI 展示大小，
    /// 发送时按 `exclusion.is_none()` 过滤）。
    pub items: Vec<DraftContextItem>,
    /// §7.1 Context Manifest（含截断/排除标记；审计与缓存 hash 输入）。
    pub manifest: Vec<ContextItem>,
    /// 参与发送的字符总数（排除项不计）。
    pub total_chars: i64,
    /// 参与发送的估算 token 总数（排除项不计）。
    pub total_estimated_tokens: i64,
    /// 被预算截断的 source_id。
    pub truncated_sources: Vec<String>,
    /// 被预算排除的 source_id（用户/Secret 排除不在此列）。
    pub budget_excluded_sources: Vec<String>,
}

/// 应用预算策略。`budget_tokens <= 0` 表示不限预算（模型未配置上下文
/// 上限时的兜底），全部保留。
pub fn apply_budget(
    drafts: Vec<DraftContextItem>,
    strategy: BudgetStrategy,
    budget_tokens: i64,
    estimator: &TokenEstimator,
) -> BudgetOutcome {
    // 已被前置阶段（用户/Secret）排除的条目不参与预算，直接进 Manifest。
    let (pre_excluded, includable): (Vec<_>, Vec<_>) = drafts.into_iter().partition(|d| d.exclusion.is_some());

    // 按 (tier, 原顺序) 稳定排序：tier 小者优先占用预算。
    let mut ordered: Vec<(usize, DraftContextItem)> = includable.into_iter().enumerate().collect();
    ordered.sort_by_key(|(idx, d)| (strategy.tier_of(d.role), *idx));

    let unlimited = budget_tokens <= 0;
    let mut remaining = budget_tokens;
    let mut truncated_sources = Vec::new();
    let mut budget_excluded_sources = Vec::new();
    let mut included: Vec<DraftContextItem> = Vec::new();

    for (_, mut draft) in ordered {
        let est = estimator.estimate(&draft.content);
        if unlimited || est <= remaining {
            if !unlimited {
                remaining -= est;
            }
            included.push(draft);
            continue;
        }
        // 预算不足：可截断且剩余值得截 → 截断；否则排除。
        let allowed_chars = estimator.chars_for_tokens(remaining);
        if draft.truncate_keep.is_some() && remaining >= MIN_TRUNCATE_TOKENS && allowed_chars > 0 {
            draft.content = truncate_content(&draft.content, allowed_chars, draft.truncate_keep);
            let new_est = estimator.estimate(&draft.content);
            debug_assert!(new_est <= remaining, "截断后必须落在剩余预算内");
            remaining -= new_est;
            truncated_sources.push(draft.source_id.clone());
            included.push(draft);
        } else {
            budget_excluded_sources.push(draft.source_id.clone());
            draft.exclusion = Some(ExclusionReason::BudgetOverflow);
            included.push(draft);
        }
    }

    // Manifest：未排除（含截断）按 tier 序在前，前置排除项保持原顺序在后。
    let mut manifest: Vec<ContextItem> = Vec::new();
    let mut total_chars = 0i64;
    let mut total_estimated_tokens = 0i64;
    for d in &included {
        let mut item = d.manifest_item(estimator);
        item.truncated = truncated_sources.contains(&d.source_id);
        if !item.excluded {
            total_chars += item.char_count;
            total_estimated_tokens += item.estimated_tokens;
        }
        manifest.push(item);
    }
    for d in &pre_excluded {
        manifest.push(d.manifest_item(estimator));
    }
    let mut items = included;
    items.extend(pre_excluded);

    BudgetOutcome {
        items,
        manifest,
        total_chars,
        total_estimated_tokens,
        truncated_sources,
        budget_excluded_sources,
    }
}

/// 按方向截断到 `max_chars`（字符级，不断 UTF-8），并附加可见截断标记。
fn truncate_content(content: &str, max_chars: usize, keep: Option<TruncateKeep>) -> String {
    const MARKER: &str = "\n... (truncated to fit budget)\n";
    let total = content.chars().count();
    if total <= max_chars {
        return content.to_string();
    }
    let budget = max_chars.saturating_sub(MARKER.chars().count());
    match keep.unwrap_or(TruncateKeep::Head) {
        TruncateKeep::Head => {
            let head: String = content.chars().take(budget).collect();
            format!("{head}{MARKER}")
        }
        TruncateKeep::Tail => {
            let tail: String = content.chars().skip(total - budget).collect();
            format!("{MARKER}{tail}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::context::TruncateKeep;
    use super::super::request::ContextKind;
    use super::*;

    fn draft(role: ContextRole, source: &str, chars: usize) -> DraftContextItem {
        DraftContextItem {
            kind: ContextKind::Log,
            role,
            source_id: source.into(),
            display_name: source.into(),
            content: "x".repeat(chars),
            redacted: false,
            truncate_keep: Some(TruncateKeep::Head),
            exclusion: None,
        }
    }

    fn outcome(strategy: BudgetStrategy, drafts: Vec<DraftContextItem>, budget_tokens: i64) -> BudgetOutcome {
        apply_budget(drafts, strategy, budget_tokens, &TokenEstimator::default())
    }

    fn state(o: &BudgetOutcome, source: &str) -> (bool, bool) {
        let item = o.manifest.iter().find(|i| i.source_id == source).expect("in manifest");
        (item.truncated, item.excluded)
    }

    /// §8.2 错误诊断：结构化错误 > 最近错误日志 > 日志尾部 > 环境摘要。
    /// 预算只够前两层时：日志尾部截断或排除、环境摘要排除。
    #[test]
    fn error_diagnosis_keeps_structured_error_and_error_log_first() {
        let drafts = vec![
            draft(ContextRole::EnvironmentSummary, "env", 400),
            draft(ContextRole::LogTail, "tail", 400),
            draft(ContextRole::ErrorLog, "errlog", 400),
            draft(ContextRole::StructuredError, "struct", 100),
        ];
        // struct(25) + errlog(100) = 125 token；预算 160：tail 可截 35 token，
        // env 无剩余 → 排除。
        let o = outcome(BudgetStrategy::ErrorDiagnosis, drafts, 160);
        assert_eq!(state(&o, "struct"), (false, false));
        assert_eq!(state(&o, "errlog"), (false, false));
        assert_eq!(state(&o, "tail"), (true, false), "日志尾部应被截断保留");
        assert_eq!(state(&o, "env"), (false, true), "环境摘要应被排除");
        assert_eq!(o.budget_excluded_sources, vec!["env"]);
        // Manifest 顺序 = tier 序。
        let order: Vec<&str> = o.manifest.iter().map(|i| i.source_id.as_str()).collect();
        assert_eq!(order, vec!["struct", "errlog", "tail", "env"]);
    }

    /// §8.2 日志分析：用户选中范围 > 异常堆栈 > 前后少量上下文（日志尾部）。
    #[test]
    fn log_analysis_prefers_selected_range_then_stack() {
        let drafts = vec![
            draft(ContextRole::LogTail, "tail", 400),
            draft(ContextRole::ExceptionStack, "stack", 200),
            draft(ContextRole::SelectedLogRange, "selected", 200),
            draft(ContextRole::EnvironmentSummary, "env", 200),
        ];
        // selected(50) + stack(50) = 100；预算 120：tail 截 20 → 不足
        // MIN_TRUNCATE_TOKENS，排除；env 排除。
        let o = outcome(BudgetStrategy::LogAnalysis, drafts, 120);
        assert_eq!(state(&o, "selected"), (false, false));
        assert_eq!(state(&o, "stack"), (false, false));
        assert_eq!(state(&o, "tail"), (false, true), "剩余不足截断阈值应排除");
        assert_eq!(state(&o, "env"), (false, true));
    }

    /// §8.2 Code Review：文件清单和 hunk 结构 > 具体 diff。
    #[test]
    fn code_review_keeps_file_list_and_hunk_structure_over_full_diff() {
        let drafts = vec![
            draft(ContextRole::FullDiff, "diff:a.rs", 800),
            draft(ContextRole::FullDiff, "diff:b.rs", 800),
            draft(ContextRole::HunkStructure, "hunks", 200),
            draft(ContextRole::FileList, "files", 200),
        ];
        // files(50) + hunks(50) = 100；预算 250：diff:a 截断（200→150 token），
        // diff:b 无剩余 → 排除。
        let o = outcome(BudgetStrategy::CodeReview, drafts, 250);
        assert_eq!(state(&o, "files"), (false, false));
        assert_eq!(state(&o, "hunks"), (false, false));
        assert_eq!(state(&o, "diff:a.rs"), (true, false));
        assert_eq!(state(&o, "diff:b.rs"), (false, true));
    }

    /// §8.2 Commit Message：变更文件、状态、diff 摘要 > 完整 diff。
    #[test]
    fn commit_message_keeps_change_summary_over_full_diff() {
        let drafts = vec![
            draft(ContextRole::FullDiff, "diff", 800),
            draft(ContextRole::HunkStructure, "summary", 200),
            draft(ContextRole::ChangeSummary, "status", 100),
        ];
        // status(25) + summary(50) = 75；预算 100：diff 截断到 25 token
        // （不足 MIN_TRUNCATE_TOKENS=32）→ 排除。
        let o = outcome(BudgetStrategy::CommitMessage, drafts, 100);
        assert_eq!(state(&o, "status"), (false, false));
        assert_eq!(state(&o, "summary"), (false, false));
        assert_eq!(state(&o, "diff"), (false, true));
    }

    /// §8.2 多仓库 Summary：每仓库摘要 > 所有文件逐行内容。
    #[test]
    fn multi_repo_summary_keeps_per_repo_summary_over_details() {
        let drafts = vec![
            draft(ContextRole::FullDiff, "repo-a:diff", 400),
            draft(ContextRole::RepoSummary, "repo-a:summary", 200),
            draft(ContextRole::RepoSummary, "repo-b:summary", 200),
            draft(ContextRole::FullDiff, "repo-b:diff", 400),
        ];
        // 两个 summary 各 50 token = 100；预算 150：repo-a:diff 截断
        // （100→50 token），repo-b:diff 排除。
        let o = outcome(BudgetStrategy::MultiRepoSummary, drafts, 150);
        assert_eq!(state(&o, "repo-a:summary"), (false, false));
        assert_eq!(state(&o, "repo-b:summary"), (false, false));
        assert_eq!(state(&o, "repo-a:diff"), (true, false));
        assert_eq!(state(&o, "repo-b:diff"), (false, true));
    }

    /// 不可截断条目预算不足时只能排除，不能截一半。
    #[test]
    fn non_truncatable_item_is_excluded_not_cut() {
        let mut d = draft(ContextRole::StructuredError, "struct", 400);
        d.truncate_keep = None;
        let o = outcome(BudgetStrategy::ErrorDiagnosis, vec![d], 50);
        assert_eq!(state(&o, "struct"), (false, true));
    }

    /// 前置排除（用户/Secret）不占预算，且 Manifest 中保留标记。
    #[test]
    fn pre_excluded_items_skip_budget_but_stay_in_manifest() {
        let mut excluded = draft(ContextRole::FullDiff, "secret-file", 4000);
        excluded.exclusion = Some(ExclusionReason::SecretPolicy);
        let o = outcome(BudgetStrategy::CodeReview, vec![excluded], 10);
        let item = &o.manifest[0];
        assert!(item.excluded);
        assert_eq!(item.exclusion_reason, Some(ExclusionReason::SecretPolicy));
        assert_eq!(o.total_estimated_tokens, 0, "排除项不计入总量");
    }

    /// 日志类截断保留尾部（§8.2 尾部优先）。
    #[test]
    fn log_truncation_keeps_tail() {
        let mut d = draft(ContextRole::LogTail, "tail", 400);
        d.truncate_keep = Some(TruncateKeep::Tail);
        d.content = format!("{}\n{}", "head".repeat(50), "tail".repeat(50));
        let o = outcome(BudgetStrategy::ErrorDiagnosis, vec![d], 40);
        let item = o.items.iter().find(|i| i.source_id == "tail").expect("item");
        assert!(item.content.contains("tailtail"));
        assert!(!item.content.contains("headhead"), "尾部优先应丢弃头部");
    }

    /// budget <= 0 = 不限预算。
    #[test]
    fn zero_budget_means_unlimited() {
        let o = outcome(
            BudgetStrategy::CodeReview,
            vec![draft(ContextRole::FullDiff, "d", 9999)],
            0,
        );
        assert_eq!(state(&o, "d"), (false, false));
    }

    /// 校准系数参与截断换算：系数 2 → 每 token 对应字符数减半。
    #[test]
    fn calibrated_estimator_changes_truncation_size() {
        let d = vec![draft(ContextRole::FullDiff, "d", 800)];
        let o1 = apply_budget(d.clone(), BudgetStrategy::CodeReview, 100, &TokenEstimator::default());
        let o2 = apply_budget(d, BudgetStrategy::CodeReview, 100, &TokenEstimator::new(Some(2.0)));
        let c1 = o1.items[0].content.chars().count();
        let c2 = o2.items[0].content.chars().count();
        assert!(c2 < c1, "系数越大截得越短: {c2} vs {c1}");
    }
}
