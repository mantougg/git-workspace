//! Secret 管道（设计文档 §10.2）：复用 T-08 `scan_secrets` / `mask_secrets`，
//! **不另起扫描规则**（全局约束 §5 / §13）。
//!
//! 策略：
//! - `Block`（默认）：任何命中都阻断发送，命中类别进 Preview（§18.2）；
//! - `Mask`：命中条目自动脱敏（`mask_secrets`），**脱敏后二次扫描**，
//!   仍命中则继续阻断（防御未来 T-08 规则扩展后「脱敏残留」）；
//! - `Exclude`：条目级机制——用户在 Preview 排除条目后重建请求，
//!   排除在扫描前生效（`ExclusionReason::SecretPolicy` / `User`），
//!   本模块只扫描未排除条目；
//! - `Warn`：命中作为警告展示，需用户明确确认（`warn_confirmed`）才放行。
//!
//! 检测发生在**最终内容生成之后**（调用方先组装好条目正文再进管道）。
//! 报告只携带命中类别与计数，**不含 Secret 原文与位置**（全局约束 §12）。

use serde::{Deserialize, Serialize};

use crate::core::secret::{mask_secrets, scan_secrets, SecretFinding};

use super::context::DraftContextItem;

/// Secret 处理策略选择（§10.2；Exclude 走条目级 exclusions，不在此列）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SecretStrategyKind {
    Block,
    Mask,
    Warn,
}

/// 请求的 Secret 策略（IPC 入参）。默认 Block。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SecretPolicyChoice {
    pub strategy: SecretStrategyKind,
    /// Warn 策略下用户已明确确认「知晓风险仍发送」（§10.2 Warn）。
    #[serde(default)]
    pub warn_confirmed: bool,
}

impl Default for SecretPolicyChoice {
    fn default() -> Self {
        Self {
            strategy: SecretStrategyKind::Block,
            warn_confirmed: false,
        }
    }
}

/// 单条目的命中摘要（类别标签 + 次数；不含原文/位置）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SecretFindingSummary {
    pub source_id: String,
    pub display_name: String,
    /// 命中的 Secret 类别标签（去重排序）。
    pub kinds: Vec<String>,
    pub count: i64,
}

/// 管道结果（进 Preview 契约）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SecretReport {
    /// 全部命中（按条目；Mask 后为脱敏前命中， Warn 为待确认警告）。
    pub findings: Vec<SecretFindingSummary>,
    /// 被自动脱敏的条目 source_id（§10.1「自动脱敏项」）。
    pub masked_sources: Vec<String>,
    /// 是否阻断发送（Block 命中 / Mask 二次扫描仍命中 / Warn 未确认）。
    pub blocked: bool,
    /// 阻断原因涉及的 Secret 类别（去重排序）。
    pub block_kinds: Vec<String>,
    /// Warn 策略存在命中且用户尚未确认。
    pub warn_pending: bool,
}

/// 对最终内容执行 Secret 策略。`items` 中已排除的条目跳过扫描；
/// Mask 会就地改写条目内容并标记 `redacted`。
pub fn apply(items: &mut [DraftContextItem], choice: &SecretPolicyChoice) -> SecretReport {
    apply_with(items, choice, &scan_secrets, &mask_secrets)
}

/// 扫描/脱敏函数可注入的核心（测试用桩覆盖「二次扫描仍命中」分支；
/// 生产路径固定为 T-08 实现，见 [`apply`]）。
fn apply_with(
    items: &mut [DraftContextItem],
    choice: &SecretPolicyChoice,
    scan: &dyn Fn(&str) -> Vec<SecretFinding>,
    mask: &dyn Fn(&str) -> String,
) -> SecretReport {
    let mut report = SecretReport::default();
    let mut block_kinds: Vec<String> = Vec::new();

    for item in items.iter_mut().filter(|i| i.exclusion.is_none()) {
        let findings = scan(&item.content);
        if findings.is_empty() {
            continue;
        }
        let mut kinds: Vec<String> = findings.iter().map(|f| f.kind.label().to_string()).collect();
        kinds.sort();
        kinds.dedup();
        report.findings.push(SecretFindingSummary {
            source_id: item.source_id.clone(),
            display_name: item.display_name.clone(),
            kinds: kinds.clone(),
            count: findings.len() as i64,
        });

        match choice.strategy {
            SecretStrategyKind::Block => {
                report.blocked = true;
                block_kinds.extend(kinds);
            }
            SecretStrategyKind::Mask => {
                let masked = mask(&item.content);
                // §10.2：脱敏后的内容仍需再次检查。
                let rescan = scan(&masked);
                if rescan.is_empty() {
                    item.content = masked;
                    item.redacted = true;
                    report.masked_sources.push(item.source_id.clone());
                } else {
                    // 二次扫描仍命中 → 继续阻断（不脱敏，按 Block 处理）。
                    let mut rescan_kinds: Vec<String> = rescan.iter().map(|f| f.kind.label().to_string()).collect();
                    rescan_kinds.sort();
                    rescan_kinds.dedup();
                    report.blocked = true;
                    block_kinds.extend(rescan_kinds);
                }
            }
            SecretStrategyKind::Warn => {
                if choice.warn_confirmed {
                    // 用户已明确确认：放行（命中仍在 findings 中展示）。
                } else {
                    report.blocked = true;
                    report.warn_pending = true;
                    block_kinds.extend(kinds);
                }
            }
        }
    }

    block_kinds.sort();
    block_kinds.dedup();
    report.block_kinds = block_kinds;
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::context::ContextRole;
    use crate::ai::request::{ContextKind, ExclusionReason};
    use crate::core::secret::SecretKind;

    fn item(source: &str, content: &str) -> DraftContextItem {
        DraftContextItem::supplementary(ContextRole::FullDiff, ContextKind::Diff, source, source, content)
    }

    const AWS: &str = "const key = \"AKIAIOSFODNN7EXAMPLE\";";
    const JWT: &str =
        "token=eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.dozjgNryP4J3jVmNHl0w5N_XgL0n3I9PlFUP0THsR8U";
    const KEY: &str = "-----BEGIN RSA PRIVATE KEY-----\nMII...";
    const PASSWORD: &str = "password=supersecret123";
    const GITHUB: &str = "ghp_abcdefghijklmnopqrstuvwxyz0123456789";

    /// §18.2 集成：AWS Key / JWT / 私钥 / 密码 / Token 默认阻断。
    #[test]
    fn block_strategy_blocks_all_high_risk_kinds_by_default() {
        for (name, content) in [
            ("aws", AWS),
            ("jwt", JWT),
            ("private-key", KEY),
            ("password", PASSWORD),
            ("github-token", GITHUB),
        ] {
            let mut items = vec![item(name, content)];
            let report = apply(&mut items, &SecretPolicyChoice::default());
            assert!(report.blocked, "{name} 默认必须阻断");
            assert_eq!(report.findings.len(), 1);
            assert!(!report.block_kinds.is_empty());
        }
    }

    /// 干净内容不阻断、无命中。
    #[test]
    fn clean_content_passes() {
        let mut items = vec![item("clean", "fn main() { println!(\"hi\"); }")];
        let report = apply(&mut items, &SecretPolicyChoice::default());
        assert!(!report.blocked && report.findings.is_empty());
    }

    /// Mask：命中条目被脱敏并标记 redacted，二次扫描通过后放行。
    #[test]
    fn mask_strategy_redacts_and_unblocks() {
        let mut items = vec![item("aws", AWS), item("clean", "ok")];
        let choice = SecretPolicyChoice {
            strategy: SecretStrategyKind::Mask,
            warn_confirmed: false,
        };
        let report = apply(&mut items, &choice);
        assert!(!report.blocked);
        assert_eq!(report.masked_sources, vec!["aws"]);
        assert!(items[0].redacted);
        assert!(!items[0].content.contains("AKIAIOSFODNN7EXAMPLE"));
    }

    /// Mask 二次扫描仍命中 → 继续阻断（桩：脱敏后仍留密码模式）。
    #[test]
    fn mask_rescan_hit_keeps_blocking() {
        let leaky_mask = |_: &str| "password=***".to_string();
        let scan = |text: &str| scan_secrets(text);
        let mut items = vec![item("pwd", PASSWORD)];
        let choice = SecretPolicyChoice {
            strategy: SecretStrategyKind::Mask,
            warn_confirmed: false,
        };
        let report = apply_with(&mut items, &choice, &scan, &leaky_mask);
        assert!(report.blocked, "二次扫描命中必须继续阻断");
        assert!(report.masked_sources.is_empty(), "阻断条目不得标记为已脱敏");
        assert!(!items[0].redacted, "阻断条目不得改写内容");
    }

    /// Warn：未确认阻断（warn_pending），确认后放行且命中仍展示。
    #[test]
    fn warn_strategy_requires_explicit_confirmation() {
        let mut items = vec![item("aws", AWS)];
        let unconfirmed = SecretPolicyChoice {
            strategy: SecretStrategyKind::Warn,
            warn_confirmed: false,
        };
        let report = apply(&mut items, &unconfirmed);
        assert!(report.blocked && report.warn_pending);

        let mut items = vec![item("aws", AWS)];
        let confirmed = SecretPolicyChoice {
            strategy: SecretStrategyKind::Warn,
            warn_confirmed: true,
        };
        let report = apply(&mut items, &confirmed);
        assert!(!report.blocked && !report.warn_pending);
        assert_eq!(report.findings.len(), 1, "命中必须保留展示");
    }

    /// Exclude：排除的条目不参与扫描（排除在扫描前生效）。
    #[test]
    fn excluded_items_are_not_scanned() {
        let mut excluded = item("secret.env", PASSWORD);
        excluded.exclusion = Some(ExclusionReason::SecretPolicy);
        let mut items = vec![excluded, item("clean", "ok")];
        let report = apply(&mut items, &SecretPolicyChoice::default());
        assert!(!report.blocked && report.findings.is_empty());
    }
}
