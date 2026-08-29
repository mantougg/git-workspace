//! 结构化 AI 错误（设计文档 §17）。
//!
//! 每种错误携带稳定 code、用户可读 message、非敏感 details、recoverable 与
//! suggestedActions，经 `AppError::Ai` 序列化为统一的 `ErrorResponse`
//! （suggestedActions 放在 details JSON 内，不改动 ErrorResponse 线格式）。
//!
//! 硬约束：任何字段都不得包含 API Key 或 Secret 原文（全局约束 §4）。

use serde::Serialize;

/// 结构化 AI 错误（§17）。AI-01 落地前四个配置类 code，另含原型 `ai_review`
/// 迁移所需的 Provider/认证/Secret code；其余 code 随后续任务补充。
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

    /// Provider 不可达或返回非成功状态（网络/TLS/5xx，归一化，不含响应正文）。
    #[error("AI Provider 不可用: {message}")]
    ProviderUnavailable { message: String },

    /// Provider 返回 401/403：Key 无效或权限不足。
    #[error("AI 认证失败: {message}")]
    AuthenticationFailed { message: String },

    /// 发送前 Secret 扫描命中（T-08 复用，AI-03 前原型链路保留阻断行为）。
    #[error("检测到敏感信息（{kinds}），已阻止发送给 AI。请先移除或排除相关文件。")]
    SecretDetected { kinds: String },
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
        }
    }

    /// 是否可通过用户行动/重试恢复。配置类错误全部可由用户在 AI 设置中
    /// 修复（配置 Provider、录入凭证、另选模型），故均可恢复。
    pub fn recoverable(&self) -> bool {
        true
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
    }
}
