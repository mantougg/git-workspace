//! AI 请求模型与结构化结果（设计文档 §7.1 / §8.4）。
//!
//! `AiRequest` 是领域无关但类型化的请求：业务语义经 `taskKind` +
//! `systemInstruction` 表达；上下文来源必须以 `contextManifest` 列出
//! （来源、范围、字符数、估算 token、是否脱敏/排除），而不是只发拼接字符串。
//! 消息内容（含上下文正文）由调用方组装进 `messages`——AI-03 的
//! Context Builder 负责；Gateway 只做预算/能力/Secret 前置校验。

use serde::{Deserialize, Serialize};

use super::model::AiTaskKind;

/// 上下文条目类别（§7.1）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ContextKind {
    Diff,
    Log,
    Error,
    Repository,
    Runtime,
    Dependency,
    File,
}

impl ContextKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            ContextKind::Diff => "diff",
            ContextKind::Log => "log",
            ContextKind::Error => "error",
            ContextKind::Repository => "repository",
            ContextKind::Runtime => "runtime",
            ContextKind::Dependency => "dependency",
            ContextKind::File => "file",
        }
    }
}

/// 条目被排除的原因（§8.2 / §10.2）。`None` = 未排除。
/// 预算超限与用户排除都必须在 Manifest 与 UI 可见，不得静默发送。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ExclusionReason {
    /// 用户在 Preview 中手动排除。
    User,
    /// 上下文预算超限被策略排除（§8.2）。
    BudgetOverflow,
    /// Secret 策略排除（§10.2 Exclude）。
    SecretPolicy,
}

impl ExclusionReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            ExclusionReason::User => "user",
            ExclusionReason::BudgetOverflow => "budgetOverflow",
            ExclusionReason::SecretPolicy => "secretPolicy",
        }
    }
}

/// 上下文清单条目（§7.1）。只描述来源与计量，不含正文。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextItem {
    pub kind: ContextKind,
    /// 来源标识（如文件路径、运行时名、diff 范围）。
    pub source_id: String,
    /// UI 展示名。
    pub display_name: String,
    pub char_count: i64,
    pub estimated_tokens: i64,
    /// 是否经过脱敏（T-08 Mask）。
    pub redacted: bool,
    /// 是否为适配预算被截断（§8.2：截断必须可见）。
    #[serde(default)]
    pub truncated: bool,
    /// 是否被排除（排除项不参与估算与发送）。
    pub excluded: bool,
    /// 排除原因；未排除为 None（契约稳定：始终序列化，可空）。
    #[serde(default)]
    pub exclusion_reason: Option<ExclusionReason>,
}

/// 消息角色。System 消息由 Gateway 合并进 `ProviderRequest.system`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MessageRole {
    System,
    User,
    Assistant,
}

impl MessageRole {
    pub fn as_str(&self) -> &'static str {
        match self {
            MessageRole::System => "system",
            MessageRole::User => "user",
            MessageRole::Assistant => "assistant",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "system" => Some(MessageRole::System),
            "user" => Some(MessageRole::User),
            "assistant" => Some(MessageRole::Assistant),
            _ => None,
        }
    }
}

/// 一条对话消息。内容作为不可信数据传递（§8.3），Adapter 不做拼接改写。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiMessage {
    pub role: MessageRole,
    pub content: String,
}

/// 期望的响应形态（§7.1 `responseFormat`）。
/// `Json` 要求模型具备 `structuredOutput` 能力（§6.3 前置校验）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ResponseFormat {
    Text,
    Json,
}

/// 工具策略（§9）：第一期只读。`ReadOnlyWhitelist` 表示允许只读工具
/// 白名单（AI-05 Tool Registry 落地后生效），Gateway/Adapter 不执行写操作。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ToolPolicy {
    Disabled,
    ReadOnlyWhitelist,
}

/// 类型化 AI 请求（§7.1）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiRequest {
    /// 请求 ID（UUID），生命周期与事件推送的关联键。
    pub request_id: String,
    /// 会话 ID（AI-04 落地后由会话层填充）。
    pub session_id: Option<String>,
    pub task_kind: AiTaskKind,
    /// 显式指定 Provider/模型；为空时走任务默认模型解析链（§6.3）。
    pub provider_id: Option<String>,
    pub model_id: Option<String>,
    /// 业务系统约束（角色 prompt 的受信部分，由后端组装，§8.3）。
    pub system_instruction: String,
    pub messages: Vec<AiMessage>,
    pub context_manifest: Vec<ContextItem>,
    pub response_format: ResponseFormat,
    pub tool_policy: ToolPolicy,
    /// 请求总 token 预算（prompt + completion 估算上限；0 = 不限制）。
    pub token_budget: i64,
    pub temperature: Option<f64>,
    pub stream: bool,
    /// §10.2 Warn 策略：用户在 Preview 中明确确认「知晓低置信度 Secret
    /// 提示仍发送」后置 true。默认 false = 任何命中都阻断（§18.2）。
    #[serde(default)]
    pub secret_warn_confirmed: bool,
    /// 是否允许复用结果缓存（§11.3）。默认 true；「重新生成」场景置 false
    /// 以强制重新调用模型。
    #[serde(default = "default_use_cache")]
    pub use_cache: bool,
}

fn default_use_cache() -> bool {
    true
}

/// token 用量（协议归一化后；部分 Provider 不回传时为 None）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AiTokenUsage {
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
}

/// 结构化结果类别（§8.4）。非法 JSON 降级为纯文本 `Answer`（§18.1）。
///
/// `DiagnosticReport` / `ReviewReport` / `ConflictProposal` / `ActionProposal`
/// 的 payload 形状由承载场景的后续任务（AI-06 / AI-08 / AI-09 / AI-11）定型，
/// 这里以 JSON 对象透传；`Answer` / `GeneratedText` 携带纯文本。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum AiResult {
    /// 普通解释（也是结构化解析失败时的降级兜底）。
    Answer { text: String },
    /// 原因、证据、排查路径、建议。
    DiagnosticReport { payload: serde_json::Value },
    /// summary、issues（severity/file/description）。
    ReviewReport { payload: serde_json::Value },
    /// commit message、PR description 等生成文本。
    GeneratedText { text: String },
    /// 冲突解决建议（proposed content + diff）。
    ConflictProposal { payload: serde_json::Value },
    /// 未来的结构化待确认动作。第一期 Gateway 不产生（§9.4 只读）。
    ActionProposal { payload: serde_json::Value },
}

/// token 粗估（§7.1 `estimatedTokens`）：~4 字符/token。中文实际约 1.5~2
/// 字符/token，本估算偏保守低估，仅用于预算门槛与展示，不用于计费。
pub fn estimate_tokens(text: &str) -> i64 {
    (text.chars().count() as i64 + 3) / 4
}

/// taskKind → 期望的结构化结果类别（§8.4）；chat/commitMessage 的常规输出
/// 走 Answer/GeneratedText 文本路径。
fn structured_variant_for(task_kind: AiTaskKind) -> fn(serde_json::Value) -> AiResult {
    match task_kind {
        AiTaskKind::RuntimeDiagnostic => |v| AiResult::DiagnosticReport { payload: v },
        AiTaskKind::GitReview => |v| AiResult::ReviewReport { payload: v },
        AiTaskKind::Conflict => |v| AiResult::ConflictProposal { payload: v },
        AiTaskKind::CommitMessage => |v| AiResult::GeneratedText {
            text: serde_json_to_plain_text(&v),
        },
        AiTaskKind::Chat => |v| AiResult::Answer {
            text: serde_json_to_plain_text(&v),
        },
    }
}

/// JSON 值的纯文本降级表示（pretty JSON；用于 commitMessage/chat 意外收到
/// JSON 对象时的可读输出）。
fn serde_json_to_plain_text(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        other => serde_json::to_string_pretty(other).unwrap_or_default(),
    }
}

/// 去掉模型输出常见的 markdown 代码围栏（```json ... ```）。
fn strip_code_fences(text: &str) -> &str {
    let t = text.trim();
    let Some(rest) = t.strip_prefix("```") else {
        return t;
    };
    let rest = rest.trim_start_matches(|c: char| c.is_ascii_alphanumeric());
    let rest = rest.trim_start_matches(['\r', '\n']);
    let rest = rest.trim_end();
    let Some(inner) = rest.strip_suffix("```") else {
        return t;
    };
    inner.trim()
}

/// 把模型文本解析为结果（§8.4 / §18.1）。
///
/// - `response_format == Json`：解析 JSON 对象并按 taskKind 映射结果类别；
///   非法 JSON / 非 object 降级为纯文本 Answer（commitMessage 降级为
///   GeneratedText）。
/// - 其余按文本任务处理：commitMessage → GeneratedText，其余 → Answer。
pub fn parse_result(
    task_kind: AiTaskKind,
    response_format: ResponseFormat,
    text: &str,
) -> AiResult {
    let text = strip_code_fences(text);
    if response_format == ResponseFormat::Json {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(text) {
            if v.is_object() {
                return structured_variant_for(task_kind)(v);
            }
        }
    }
    match task_kind {
        AiTaskKind::CommitMessage => AiResult::GeneratedText {
            text: text.to_string(),
        },
        _ => AiResult::Answer {
            text: text.to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estimate_tokens_is_chars_over_four() {
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(estimate_tokens("abcd"), 1);
        assert_eq!(estimate_tokens("abcde"), 2, "向上取整");
        assert_eq!(estimate_tokens("abcdefgh"), 2);
        assert_eq!(estimate_tokens("四个汉字"), 1);
    }

    #[test]
    fn parse_result_maps_json_by_task_kind() {
        let json = r#"{"summary":"ok","issues":[]}"#;
        assert!(matches!(
            parse_result(AiTaskKind::GitReview, ResponseFormat::Json, json),
            AiResult::ReviewReport { .. }
        ));
        assert!(matches!(
            parse_result(AiTaskKind::RuntimeDiagnostic, ResponseFormat::Json, json),
            AiResult::DiagnosticReport { .. }
        ));
        assert!(matches!(
            parse_result(AiTaskKind::Conflict, ResponseFormat::Json, json),
            AiResult::ConflictProposal { .. }
        ));
    }

    #[test]
    fn parse_result_degrades_invalid_json_to_answer() {
        let bad = "这不是 JSON";
        assert_eq!(
            parse_result(AiTaskKind::GitReview, ResponseFormat::Json, bad),
            AiResult::Answer {
                text: bad.to_string()
            }
        );
        // JSON 数组不是 object，同样降级
        assert!(matches!(
            parse_result(AiTaskKind::GitReview, ResponseFormat::Json, "[1,2]"),
            AiResult::Answer { .. }
        ));
    }

    #[test]
    fn parse_result_strips_code_fences() {
        let fenced = "```json\n{\"a\":1}\n```";
        assert!(matches!(
            parse_result(AiTaskKind::GitReview, ResponseFormat::Json, fenced),
            AiResult::ReviewReport { .. }
        ));
    }

    #[test]
    fn parse_result_text_tasks() {
        assert_eq!(
            parse_result(AiTaskKind::CommitMessage, ResponseFormat::Text, "feat: x"),
            AiResult::GeneratedText {
                text: "feat: x".to_string()
            }
        );
        assert_eq!(
            parse_result(AiTaskKind::Chat, ResponseFormat::Text, "hello"),
            AiResult::Answer {
                text: "hello".to_string()
            }
        );
    }

    #[test]
    fn result_serde_tags_are_camel_case() {
        assert_eq!(
            serde_json::to_value(AiResult::Answer {
                text: "x".into()
            })
            .unwrap(),
            serde_json::json!({"type": "answer", "text": "x"})
        );
        assert_eq!(
            serde_json::to_value(AiResult::DiagnosticReport {
                payload: serde_json::json!({"a": 1})
            })
            .unwrap(),
            serde_json::json!({"type": "diagnosticReport", "payload": {"a": 1}})
        );
    }

    #[test]
    fn context_kind_serde_names_match_design() {
        assert_eq!(
            serde_json::to_value(ContextKind::Runtime).unwrap(),
            "runtime"
        );
        assert_eq!(serde_json::to_value(ContextKind::Diff).unwrap(), "diff");
    }
}
