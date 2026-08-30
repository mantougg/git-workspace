//! AI Assistant 智能层（设计文档 `docs/ai-assistant-design.md`）。
//!
//! - [`provider`]：Provider 配置模型、DAO 与连接测试（§6.1）；
//! - [`model`]：模型能力目录、任务级默认模型解析（§6.2 / §6.3）；
//! - [`credentials`]：OS Credential Store 存取 + 会话级临时凭证（§6.4）；
//! - [`request`]：类型化请求模型与结构化结果（§7.1 / §8.4）；
//! - [`context`]：Context Builder——只调现有领域服务收集结构化上下文（§8.1）；
//! - [`policy`]：五类上下文预算策略（§8.2：截断/排除全进 Manifest）；
//! - [`redact`]：Secret 管道（§10.2：Block/Mask/Exclude/Warn，复用 T-08）；
//! - [`prompt`]：Prompt 分层组装（§8.3：用户内容不进系统约束）；
//! - [`preview`]：发送前 Preview 契约与构建（§10.1，零网络）；
//! - [`lifecycle`]：请求生命周期状态机（§7.3）；
//! - [`adapters`]：三种协议的 Provider Adapter（§7.2）；
//! - [`gateway`]：统一 Gateway——唯一允许访问 AI 网络的地方（§7.3）；
//! - [`events`]：流式事件契约（`ai-request://progress`）；
//! - [`error`]：结构化 AI 错误（§17），经 `AppError::Ai` 序列化到前端。
//!
//! 全局约束：AI 层只编排，不重复实现 Git/Runtime 领域逻辑；Key 不进日志、
//! 错误、URL、进程命令行（`docs/tasks-ai/00-全局开发约束.md` §4）。

pub mod adapters;
pub mod context;
pub mod credentials;
pub mod error;
pub mod events;
pub mod gateway;
pub mod lifecycle;
pub mod model;
pub mod policy;
pub mod preview;
pub mod prompt;
pub mod provider;
pub mod redact;
pub mod request;
pub mod transport;

#[cfg(test)]
mod gateway_tests;

pub use credentials::CredentialManager;
pub use error::AiError;
pub use events::AiRequestEvent;
pub use gateway::{AiGateway, AiRequestSnapshot, GatewayConfig};
pub use lifecycle::RequestPhase;
pub use model::{
    ensure_task_capability, list_models, list_task_defaults, resolve_model, AiModel,
    AiModelDefaults, AiTaskDefault, AiTaskKind, ModelCapability, ModelResolutionSource,
    ResolvedModel, SaveAiModelRequest,
};
pub use provider::{
    delete_provider, get_provider, list_providers, save_provider, test_connection, AiProvider,
    AiProviderTestResult, ApiType, NetworkPolicy, SaveAiProviderRequest,
};
pub use request::{
    parse_result, AiMessage, AiRequest, AiResult, AiTokenUsage, ContextItem, ContextKind,
    ExclusionReason, MessageRole, ResponseFormat, ToolPolicy,
};
