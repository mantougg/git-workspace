//! 结构化 AI 错误（设计文档 §17）。
//!
//! 每种错误携带稳定 code、用户可读 message、非敏感 details、recoverable 与
//! suggestedActions，经 `AppError::Ai` 序列化为统一的 `ErrorResponse`
//! （suggestedActions 放在 details JSON 内，不改动 ErrorResponse 线格式）。
//!
//! 硬约束：任何字段都不得包含 API Key 或 Secret 原文（全局约束 §4）。

use serde::Serialize;

/// 结构化 AI 错误（§17）。AI-01 落地配置类 code；AI-02 补齐 Gateway
/// 请求生命周期的 code（`AiRateLimited` / `AiRequestCancelled` / …）。
/// `AiSecretDetected` / Proposal 错误分别由 AI-03 / AI-11 提供。
#[derive(Debug, Clone, thiserror::Error)]
pub enum AiError {
    /// AI 未配置：没有任何可用 Provider/模型，或任务默认链解析不到模型。
    #[error("AI 未配置: {message}")]
    NotConfigured { message: String },

    /// 凭证不可用：OS Credential Store 不可访问，或 Provider 缺少 Key。
    #[error("AI 凭证不可用: {message}")]
    CredentialUnavailable { message: String },

    /// 引用的模型不存在（或所属 Provider 不存在）。
    #[error("AI 模型不存在: {provider_id}/{model_id}")]
    ModelNotFound {
        provider_id: String,
        model_id: String,
    },

    /// 模型不具备任务所需能力（§6.3：请求前报错，不等 Provider 模糊失败）。
    #[error("模型 {model_id} 不支持所需能力 {capability}")]
    ModelCapabilityMismatch {
        provider_id: String,
        model_id: String,
        capability: String,
    },

    /// Provider 不可达或返回非成功状态（网络/TLS/5xx/4xx，归一化，不含响应正文）。
    ///
    /// `transient` 标记是否为临时故障（连接失败/5xx/流式中断）——Gateway
    /// 据此决定是否自动重试（§7.4）；超时与确定性 4xx 为 false。
    #[error("AI Provider 不可用: {message}")]
    ProviderUnavailable { message: String, transient: bool },

    /// Provider 返回 401/403：Key 无效或权限不足。
    #[error("AI 认证失败: {message}")]
    AuthenticationFailed { message: String },

    /// 发送前 Secret 扫描命中（T-08 复用，AI-03 前原型链路保留阻断行为）。
    #[error("检测到敏感信息（{kinds}），已阻止发送给 AI。请先移除或排除相关文件。")]
    SecretDetected { kinds: String },

    /// Provider 返回 429：请求过于频繁（可自动重试，退避由 Gateway 控制）。
    #[error("AI 请求过于频繁（429）: {message}")]
    RateLimited { message: String },

    /// 用户或系统取消了请求（§7.3 任意阶段可入 Cancelled）。
    #[error("AI 请求已取消")]
    RequestCancelled { request_id: String },

    /// 请求内容估算 token 超出预算/模型上下文限制（§6.3 请求前报错）。
    #[error("请求内容超出上下文预算（估算 {estimated_tokens} token > 预算 {budget_tokens}）")]
    ContextTooLarge {
        estimated_tokens: i64,
        budget_tokens: i64,
    },

    /// Provider 响应不是合法协议数据（非法 JSON / 缺字段 / 流式协议违规）。
    #[error("AI 响应无法解析: {message}")]
    ResponseInvalid { message: String },

    /// Preview 未确认时尝试发送（§7.3 硬闸门；正常流程不可达，防御性保留）。
    #[error("请求尚未经用户确认 Preview，禁止发送")]
    PreviewRequired { request_id: String },

    /// Provider 返回策略拒绝（内容策略 / 权限策略等 4xx）。
    #[error("Provider 拒绝了请求: {message}")]
    PolicyRejected { message: String },

    /// 工具名不在注册表中（§9.3：工具是类型化包装，不是任意函数执行器）。
    #[error("AI 工具不存在: {name}")]
    ToolNotFound { name: String },

    /// 角色不在该工具的白名单中（§9.2 权限矩阵）。
    #[error("角色 {role} 无权调用工具 {tool}")]
    ToolNotAllowed { tool: String, role: String },

    /// 工具请求超出当前上下文范围（§9.4：不得自行扩大 Workspace/Repository
    /// 范围；如访问当前 Workspace 之外的仓库、缺少 Workspace 上下文）。
    #[error("工具 {tool} 请求超出当前范围: {message}")]
    ToolScopeViolation { tool: String, message: String },

    /// 工具入参不符合其 JSON Schema / 类型契约。
    #[error("工具 {tool} 入参无效: {message}")]
    ToolInputInvalid { tool: String, message: String },

    /// 单次用户请求的工具调用达到上限（§9.4 默认 8 次）——需要用户继续
    /// 确认或缩小范围后才能继续。
    #[error("单次请求的工具调用已达上限 {max} 次，需要确认继续或缩小范围")]
    ToolCallLimitExceeded { max: u32 },

    /// 工具执行超过其声明的超时（§9.3 每个工具声明超时上限）。
    #[error("工具 {tool} 执行超时（>{timeout_ms}ms）")]
    ToolTimeout { tool: String, timeout_ms: u64 },

    /// Proposal 不存在。
    #[error("Action Proposal 不存在: {proposal_id}")]
    ProposalNotFound { proposal_id: String },

    /// Proposal 已过期，必须重新生成。
    #[error("Action Proposal 已过期，请重新生成")]
    ProposalExpired { proposal_id: String },

    /// Proposal 当前状态不允许该转换。
    #[error("Action Proposal 当前状态为 {status}，不能执行该操作")]
    ProposalStateInvalid { proposal_id: String, status: String },

    /// high 风险动作必须显式二次确认。
    #[error("高风险 Action Proposal 需要二次确认")]
    ActionConfirmationRequired { proposal_id: String },
}

impl AiError {
    /// 稳定机器可读 code（§17）。
    pub fn code(&self) -> &'static str {
        match self {
            AiError::NotConfigured { .. } => "AiNotConfigured",
            AiError::CredentialUnavailable { .. } => "AiCredentialUnavailable",
            AiError::ModelNotFound { .. } => "AiModelNotFound",
            AiError::ModelCapabilityMismatch { .. } => "AiModelCapabilityMismatch",
            AiError::ProviderUnavailable { .. } => "AiProviderUnavailable",
            AiError::AuthenticationFailed { .. } => "AiAuthenticationFailed",
            AiError::SecretDetected { .. } => "AiSecretDetected",
            AiError::RateLimited { .. } => "AiRateLimited",
            AiError::RequestCancelled { .. } => "AiRequestCancelled",
            AiError::ContextTooLarge { .. } => "AiContextTooLarge",
            AiError::ResponseInvalid { .. } => "AiResponseInvalid",
            AiError::PreviewRequired { .. } => "AiPreviewRequired",
            AiError::PolicyRejected { .. } => "AiPolicyRejected",
            AiError::ToolNotFound { .. } => "AiToolNotFound",
            AiError::ToolNotAllowed { .. } => "AiToolNotAllowed",
            AiError::ToolScopeViolation { .. } => "AiToolScopeViolation",
            AiError::ToolInputInvalid { .. } => "AiToolInputInvalid",
            AiError::ToolCallLimitExceeded { .. } => "AiToolCallLimitExceeded",
            AiError::ToolTimeout { .. } => "AiToolTimeout",
            AiError::ProposalNotFound { .. } => "AiProposalNotFound",
            AiError::ProposalExpired { .. } => "AiProposalExpired",
            AiError::ProposalStateInvalid { .. } => "AiProposalStateInvalid",
            AiError::ActionConfirmationRequired { .. } => "AiActionConfirmationRequired",
        }
    }

    /// 是否可通过用户行动/重试恢复。配置类错误全部可由用户在 AI 设置中
    /// 修复（配置 Provider、录入凭证、另选模型），故均可恢复。
    pub fn recoverable(&self) -> bool {
        true
    }

    /// Gateway 自动重试判定（§7.4）。可重试 = 临时网络错误 / 429 / 5xx /
    /// 流式连接中断（尚未产生输出时）；其余（Key 无效、模型不存在、超时、
    /// 策略拒绝、协议违规等）不自动重试。
    pub fn is_retryable(&self) -> bool {
        match self {
            AiError::ProviderUnavailable { transient, .. } => *transient,
            AiError::RateLimited { .. } => true,
            _ => false,
        }
    }

    /// 建议的下一步行动（§17：配置 / 缩小范围 / 排除文件 / 重新发送等）。
    pub fn suggested_actions(&self) -> Vec<&'static str> {
        match self {
            AiError::NotConfigured { .. } => {
                vec!["打开 AI 设置添加 Provider 与模型", "配置任务默认模型"]
            }
            AiError::CredentialUnavailable { .. } => vec![
                "在 AI 设置-凭证中录入 API Key",
                "凭证存储不可用时选择仅本次会话保存",
            ],
            AiError::ModelNotFound { .. } => {
                vec!["在 AI 设置-模型中检查模型配置", "重新选择任务默认模型"]
            }
            AiError::ModelCapabilityMismatch { .. } => {
                vec!["为该任务选择具备所需能力的模型", "在模型管理中调整能力标记"]
            }
            AiError::ProviderUnavailable { .. } => {
                vec!["检查网络与 baseUrl 配置", "在 AI 设置中测试连接", "稍后重试"]
            }
            AiError::AuthenticationFailed { .. } => {
                vec!["在 AI 设置-凭证中替换 API Key", "确认 Key 的权限与额度"]
            }
            AiError::SecretDetected { .. } => {
                vec!["移除或排除包含敏感信息的文件后重试"]
            }
            AiError::RateLimited { .. } => {
                vec!["稍后重试", "降低请求频率或更换 Provider"]
            }
            AiError::RequestCancelled { .. } => {
                vec!["重新发送请求"]
            }
            AiError::ContextTooLarge { .. } => {
                vec!["缩小上下文范围或排除部分内容", "更换上下文预算更大的模型"]
            }
            AiError::ResponseInvalid { .. } => {
                vec!["重试请求", "在模型管理中确认模型支持所需输出格式"]
            }
            AiError::PreviewRequired { .. } => {
                vec!["在 Preview 中确认发送"]
            }
            AiError::PolicyRejected { .. } => {
                vec!["调整请求内容后重试", "检查 Provider 的内容策略与账户权限"]
            }
            AiError::ToolNotFound { .. } => {
                vec!["从工具注册表中选择可用工具", "检查工具名拼写"]
            }
            AiError::ToolNotAllowed { .. } => {
                vec!["切换到拥有该工具权限的角色", "改用该角色白名单内的工具"]
            }
            AiError::ToolScopeViolation { .. } => {
                vec!["将查询限定在当前 Workspace 范围内", "先通过列表工具确认可用目标"]
            }
            AiError::ToolInputInvalid { .. } => {
                vec!["按工具的 JSON Schema 修正入参后重试"]
            }
            AiError::ToolCallLimitExceeded { .. } => {
                vec!["确认继续以开始新一轮工具调用", "缩小查询范围后重新提问"]
            }
            AiError::ToolTimeout { .. } => {
                vec!["缩小查询范围（减少行数/文件数）后重试", "稍后重试"]
            }
            AiError::ProposalNotFound { .. } => vec!["刷新 Proposal 列表", "重新生成提案"],
            AiError::ProposalExpired { .. } => vec!["重新生成 Action Proposal"],
            AiError::ProposalStateInvalid { .. } => vec!["刷新 Proposal 状态"],
            AiError::ActionConfirmationRequired { .. } => vec!["检查影响范围后进行二次确认"],
        }
    }

    /// 非敏感 details（§17：Provider、模型、请求阶段等），合并
    /// suggestedActions 后作为 `ErrorResponse.details` 的 JSON 字符串。
    pub fn details_json(&self) -> String {
        let mut details = match self {
            AiError::ModelNotFound {
                provider_id,
                model_id,
            } => serde_json::json!({
                "providerId": provider_id,
                "modelId": model_id,
            }),
            AiError::ModelCapabilityMismatch {
                provider_id,
                model_id,
                capability,
            } => serde_json::json!({
                "providerId": provider_id,
                "modelId": model_id,
                "capability": capability,
            }),
            AiError::SecretDetected { kinds } => serde_json::json!({
                "secretKinds": kinds,
            }),
            AiError::ProviderUnavailable { transient, .. } => serde_json::json!({
                "transient": transient,
            }),
            AiError::RequestCancelled { request_id } => serde_json::json!({
                "requestId": request_id,
            }),
            AiError::PreviewRequired { request_id } => serde_json::json!({
                "requestId": request_id,
            }),
            AiError::ContextTooLarge {
                estimated_tokens,
                budget_tokens,
            } => serde_json::json!({
                "estimatedTokens": estimated_tokens,
                "budgetTokens": budget_tokens,
            }),
            AiError::ToolNotFound { name } => serde_json::json!({
                "tool": name,
            }),
            AiError::ToolNotAllowed { tool, role } => serde_json::json!({
                "tool": tool,
                "role": role,
            }),
            AiError::ToolScopeViolation { tool, .. } => serde_json::json!({
                "tool": tool,
            }),
            AiError::ToolInputInvalid { tool, .. } => serde_json::json!({
                "tool": tool,
            }),
            AiError::ToolCallLimitExceeded { max } => serde_json::json!({
                "maxCalls": max,
            }),
            AiError::ToolTimeout { tool, timeout_ms } => serde_json::json!({
                "tool": tool,
                "timeoutMs": timeout_ms,
            }),
            AiError::ProposalNotFound { proposal_id }
            | AiError::ProposalExpired { proposal_id }
            | AiError::ProposalStateInvalid { proposal_id, .. }
            | AiError::ActionConfirmationRequired { proposal_id } => serde_json::json!({
                "proposalId": proposal_id,
            }),
            _ => serde_json::json!({}),
        };
        details["suggestedActions"] = serde_json::json!(self.suggested_actions());
        details.to_string()
    }
}

impl Serialize for AiError {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        crate::error::AppError::Ai(self.clone()).serialize(serializer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_variant_has_stable_code_and_actions() {
        let cases: Vec<(AiError, &str)> = vec![
            (
                AiError::NotConfigured {
                    message: "no provider".into(),
                },
                "AiNotConfigured",
            ),
            (
                AiError::CredentialUnavailable {
                    message: "keyring locked".into(),
                },
                "AiCredentialUnavailable",
            ),
            (
                AiError::ModelNotFound {
                    provider_id: "p".into(),
                    model_id: "m".into(),
                },
                "AiModelNotFound",
            ),
            (
                AiError::ModelCapabilityMismatch {
                    provider_id: "p".into(),
                    model_id: "m".into(),
                    capability: "structuredOutput".into(),
                },
                "AiModelCapabilityMismatch",
            ),
            (
                AiError::ProviderUnavailable {
                    message: "connect timeout".into(),
                    transient: true,
                },
                "AiProviderUnavailable",
            ),
            (
                AiError::AuthenticationFailed {
                    message: "401".into(),
                },
                "AiAuthenticationFailed",
            ),
            (
                AiError::SecretDetected {
                    kinds: "AWS Key".into(),
                },
                "AiSecretDetected",
            ),
            (
                AiError::RateLimited {
                    message: "429".into(),
                },
                "AiRateLimited",
            ),
            (
                AiError::RequestCancelled {
                    request_id: "r1".into(),
                },
                "AiRequestCancelled",
            ),
            (
                AiError::ContextTooLarge {
                    estimated_tokens: 100,
                    budget_tokens: 50,
                },
                "AiContextTooLarge",
            ),
            (
                AiError::ResponseInvalid {
                    message: "not json".into(),
                },
                "AiResponseInvalid",
            ),
            (
                AiError::PreviewRequired {
                    request_id: "r1".into(),
                },
                "AiPreviewRequired",
            ),
            (
                AiError::PolicyRejected {
                    message: "policy".into(),
                },
                "AiPolicyRejected",
            ),
            (
                AiError::ToolNotFound {
                    name: "git.nope".into(),
                },
                "AiToolNotFound",
            ),
            (
                AiError::ToolNotAllowed {
                    tool: "runtime.getLogs".into(),
                    role: "gitReviewer".into(),
                },
                "AiToolNotAllowed",
            ),
            (
                AiError::ToolScopeViolation {
                    tool: "repository.status".into(),
                    message: "repo outside workspace".into(),
                },
                "AiToolScopeViolation",
            ),
            (
                AiError::ToolInputInvalid {
                    tool: "repository.diff".into(),
                    message: "missing repoPath".into(),
                },
                "AiToolInputInvalid",
            ),
            (AiError::ToolCallLimitExceeded { max: 8 }, "AiToolCallLimitExceeded"),
            (
                AiError::ToolTimeout {
                    tool: "runtime.getLogs".into(),
                    timeout_ms: 5000,
                },
                "AiToolTimeout",
            ),
        ];
        for (err, code) in cases {
            assert_eq!(err.code(), code);
            assert!(err.recoverable(), "{} must be user-recoverable", code);
            assert!(
                !err.suggested_actions().is_empty(),
                "{} must carry suggested actions",
                code
            );
            let details: serde_json::Value =
                serde_json::from_str(&err.details_json()).expect("details must be JSON");
            assert!(details["suggestedActions"].is_array());
        }
    }

    /// 重试分类（§7.4）：临时网络/429 可重试；Key 无效、超时、策略拒绝等
    /// 直接失败。
    #[test]
    fn retryable_classification_matches_design() {
        assert!(
            AiError::ProviderUnavailable {
                message: "connect reset".into(),
                transient: true
            }
            .is_retryable()
        );
        assert!(
            AiError::RateLimited {
                message: "429".into()
            }
            .is_retryable()
        );
        assert!(
            !AiError::ProviderUnavailable {
                message: "请求超时".into(),
                transient: false
            }
            .is_retryable(),
            "超时不自动重试，避免长等待翻倍"
        );
        assert!(
            !AiError::AuthenticationFailed {
                message: "401".into()
            }
            .is_retryable()
        );
        assert!(
            !AiError::ModelNotFound {
                provider_id: "p".into(),
                model_id: "m".into()
            }
            .is_retryable()
        );
        assert!(
            !AiError::PolicyRejected {
                message: "policy".into()
            }
            .is_retryable()
        );
    }

    /// 结构化变体的 details 必须带上下文字段（不含敏感内容）。
    #[test]
    fn structured_variants_carry_context() {
        let mismatch = AiError::ModelCapabilityMismatch {
            provider_id: "p1".into(),
            model_id: "m1".into(),
            capability: "toolCalling".into(),
        };
        let details: serde_json::Value = serde_json::from_str(&mismatch.details_json()).unwrap();
        assert_eq!(details["providerId"], "p1");
        assert_eq!(details["modelId"], "m1");
        assert_eq!(details["capability"], "toolCalling");

        let not_found = AiError::ModelNotFound {
            provider_id: "p1".into(),
            model_id: "gone".into(),
        };
        let details: serde_json::Value = serde_json::from_str(&not_found.details_json()).unwrap();
        assert_eq!(details["modelId"], "gone");

        let too_large = AiError::ContextTooLarge {
            estimated_tokens: 120,
            budget_tokens: 100,
        };
        let details: serde_json::Value = serde_json::from_str(&too_large.details_json()).unwrap();
        assert_eq!(details["estimatedTokens"], 120);
        assert_eq!(details["budgetTokens"], 100);
    }
}
