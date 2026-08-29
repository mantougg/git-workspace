//! AI Assistant 智能层（设计文档 `docs/ai-assistant-design.md`）。
//!
//! AI-01：Provider / Model / Credential 与 AI Settings 的配置层——
//! - [`provider`]：Provider 配置模型、DAO 与连接测试（§6.1）；
//! - [`model`]：模型能力目录、任务级默认模型解析（§6.2 / §6.3）；
//! - [`credentials`]：OS Credential Store 存取 + 会话级临时凭证（§6.4）；
//! - [`error`]：结构化 AI 错误（§17），经 `AppError::Ai` 序列化到前端。
//!
//! 全局约束：AI 层只编排，不重复实现 Git/Runtime 领域逻辑；Key 不进日志、
//! 错误、URL、进程命令行（`docs/tasks-ai/00-全局开发约束.md` §4）。

pub mod credentials;
pub mod error;
pub mod model;
pub mod provider;

pub use credentials::CredentialManager;
pub use error::AiError;
pub use model::{
    ensure_task_capability, list_models, list_task_defaults, resolve_model, AiModel,
    AiModelDefaults, AiTaskDefault, AiTaskKind, ModelCapability, ModelResolutionSource,
    ResolvedModel, SaveAiModelRequest,
};
pub use provider::{
    delete_provider, get_provider, list_providers, save_provider, test_connection, AiProvider,
    AiProviderTestResult, NetworkPolicy, ProviderKind, SaveAiProviderRequest,
};
