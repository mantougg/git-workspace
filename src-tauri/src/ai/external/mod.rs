//! AI-12（设计文档 §15）：外部 Agent Adapter。
//!
//! 内部 Tool Registry 是外部 Agent 的**唯一能力来源**；Adapter 是纯映射层，
//! 不含业务逻辑、不出现 Registry 之外的能力。映射关系：
//!
//! ```text
//! Internal Tool Registry (ai::tools)
//!         ├── UI Assistant（ai_execute_tool IPC）
//!         ├── MCP Adapter（mcp.rs：JSON-RPC 映射；server.rs：本地 HTTP 传输）
//!         └── CLI Adapter（cli.rs：`git-workspace ai-tools ...`）
//! ```
//!
//! 与 UI 路径相比，外部路径只多加两条规则：
//!
//! 1. **独立身份与确认标记**：外部调用使用 [`ToolRole::ExternalAgent`] 身份
//!    （不进入任何工具白名单）。只读工具直接放行；执行类（Proposal）工具
//!    必须显式携带确认标记，否则以
//!    [`AiError::ExternalConfirmationRequired`] 拒绝。注册表层随后仍以
//!    内置上限角色 `ActionPlanner` 复检白名单 / Schema / 范围 / Secret /
//!    超时与大小上限——外部权限因此永不超过内置角色（§15）。
//! 2. **UI 确认规则不可绕过**：外部调用 Proposal 工具只会创建
//!    `pending` 状态的 Action Proposal，执行仍走 AI-11 的 UI 确认与任务
//!    队列，Adapter 不提供任何执行入口。
//!
//! 审计：每次外部调用（含被拒绝的调用）向 `ai.log` 写入来源标识、工具名、
//! 参数 hash、结果大小与结果码；不记录参数原文（全局约束 §12）。

pub mod cli;
pub mod mcp;
pub mod server;

use serde::Serialize;
use serde_json::Value;

use crate::error::{AppError, AppResult};

use super::error::AiError;
use super::tools::{self, ToolCallRequest, ToolContext, ToolDefinition, ToolInvocation, ToolRegistry};

/// 外部调用来源（传输适配层），仅用于审计标识。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExternalSource {
    Mcp,
    Cli,
}

impl ExternalSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Mcp => "mcp",
            Self::Cli => "cli",
        }
    }
}

/// 归一化后的外部工具调用。`confirmed` 即 §15 的「确认标记」。
#[derive(Debug, Clone)]
pub struct ExternalCallRequest {
    pub source: ExternalSource,
    pub tool_name: String,
    pub arguments: Value,
    pub confirmed: bool,
}

/// Adapter 侧工具描述。`name` 是适配层名（MCP 命名规则），`registry_name`
/// 是注册表原名；Schema / 只读标记与注册表逐项一致（一致性由测试断言）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalToolDescriptor {
    pub name: String,
    pub registry_name: String,
    pub description: String,
    pub input_schema: Value,
    pub read_only: bool,
}

/// MCP 工具名只允许 `[a-zA-Z0-9_-]`，注册表名含 `.`，做双向映射。
pub fn mcp_tool_name(registry_name: &str) -> String {
    registry_name.replace('.', "_")
}

pub fn registry_tool_name(adapter_name: &str) -> String {
    adapter_name.replace('_', ".")
}

/// Registry → Adapter 的工具清单映射（MCP 与 CLI 共用，保证清单一致）。
pub fn external_tool_manifest(registry: &ToolRegistry) -> Vec<ExternalToolDescriptor> {
    registry
        .definitions()
        .into_iter()
        .map(|definition| ExternalToolDescriptor {
            name: mcp_tool_name(&definition.name),
            registry_name: definition.name.clone(),
            description: describe(&definition),
            input_schema: definition.input_schema.clone(),
            read_only: definition.read_only,
        })
        .collect()
}

fn describe(definition: &ToolDefinition) -> String {
    let scope = serde_json::to_value(definition.context_scope)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default();
    let mut text = if definition.read_only {
        format!("Read-only {scope} tool `{}`", definition.name)
    } else {
        format!(
            "Confirmation-gated proposal tool `{}` (scope {scope}); invoking only creates a \
             pending Action Proposal that must be confirmed in the GitWorkspace UI",
            definition.name
        )
    };
    if definition.may_contain_secrets {
        text.push_str("; results may contain redacted secrets");
    }
    text
}

/// 纯授权步骤：注册表成员判定 + 外部确认标记规则。通过后的调用改以上限
/// 角色 `ActionPlanner` 交给注册表复检（白名单 / Schema / 范围 / Secret /
/// 预算全部由注册表重新执行，§15「重新执行权限、范围与安全校验」）。
pub fn authorize_external_call(registry: &ToolRegistry, request: &ExternalCallRequest) -> AppResult<ToolCallRequest> {
    let Some(definition) = registry.get(&request.tool_name) else {
        return Err(AppError::Ai(AiError::ToolNotFound {
            name: request.tool_name.clone(),
        }));
    };
    if !definition.read_only && !request.confirmed {
        return Err(AppError::Ai(AiError::ExternalConfirmationRequired {
            tool: definition.name.clone(),
        }));
    }
    Ok(ToolCallRequest {
        request_id: uuid::Uuid::new_v4().to_string(),
        tool_name: definition.name.clone(),
        role: super::tools::ToolRole::ActionPlanner,
        arguments: request.arguments.clone(),
    })
}

/// 外部调用统一入口（MCP / CLI 共用）：授权 → 注册表执行 → 审计。
/// 被拒绝的调用同样审计（来源 + 错误码），不含参数原文。
pub async fn run_external_call(
    registry: &ToolRegistry,
    context: ToolContext,
    request: ExternalCallRequest,
) -> AppResult<ToolInvocation> {
    let started = std::time::Instant::now();
    let parameter_hash = tools::hash_parameters(&request.arguments);
    let result = match authorize_external_call(registry, &request) {
        Ok(call) => registry.invoke(call, context).await,
        Err(error) => Err(error),
    };
    let (outcome, error_code, result_bytes) = match &result {
        Ok(invocation) => ("ok", "-", invocation.result_bytes),
        Err(AppError::Ai(error)) => ("rejected", error.code(), 0),
        Err(_) => ("rejected", "InternalError", 0),
    };
    log::info!(
        "{}",
        audit_line(
            &request,
            outcome,
            error_code,
            &parameter_hash,
            result_bytes,
            started.elapsed().as_millis() as u64,
        )
    );
    result
}

fn audit_line(
    request: &ExternalCallRequest,
    outcome: &str,
    error_code: &str,
    parameter_hash: &str,
    result_bytes: usize,
    duration_ms: u64,
) -> String {
    format!(
        "ai external audit: source={} tool={} confirmed={} outcome={} error_code={} \
         parameter_hash={} result_bytes={} duration_ms={}",
        request.source.as_str(),
        request.tool_name,
        request.confirmed,
        outcome,
        error_code,
        parameter_hash,
        result_bytes,
        duration_ms
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn registry() -> ToolRegistry {
        ToolRegistry::default()
    }

    fn request(tool: &str, confirmed: bool) -> ExternalCallRequest {
        ExternalCallRequest {
            source: ExternalSource::Mcp,
            tool_name: tool.into(),
            arguments: json!({"workspaceId": 1}),
            confirmed,
        }
    }

    /// 验收：MCP/CLI 两个 Adapter 的工具清单与 Registry 一致（共用同一
    /// manifest 函数，名称双向映射可逆、Schema 逐项相等）。
    #[test]
    fn manifest_matches_registry_one_to_one() {
        let registry = registry();
        let definitions = registry.definitions();
        let manifest = external_tool_manifest(&registry);
        assert_eq!(manifest.len(), definitions.len());
        for (descriptor, definition) in manifest.iter().zip(definitions.iter()) {
            assert_eq!(descriptor.registry_name, definition.name);
            assert_eq!(descriptor.name, mcp_tool_name(&definition.name));
            assert_eq!(registry_tool_name(&descriptor.name), definition.name);
            assert_eq!(descriptor.input_schema, definition.input_schema);
            assert_eq!(descriptor.read_only, definition.read_only);
        }
    }

    /// 验收：外部 Agent 权限不超过内置角色——凡外部可触达的工具（全部只读
    /// 工具 + 带确认标记的 Proposal 工具）都必须白名单内置上限角色。
    #[test]
    fn external_capability_never_exceeds_builtin_ceiling() {
        let registry = registry();
        for definition in registry.definitions() {
            assert!(
                definition
                    .allowed_roles
                    .contains(&super::super::tools::ToolRole::ActionPlanner),
                "{} must whitelist the built-in ceiling role",
                definition.name
            );
            assert!(!definition
                .allowed_roles
                .contains(&super::super::tools::ToolRole::ExternalAgent));
        }
    }

    /// 验收：越权调用被拒——工具不在注册表（能力之外）。
    #[test]
    fn authorize_rejects_tool_outside_registry() {
        let error = authorize_external_call(&registry(), &request("shell.exec", false)).unwrap_err();
        assert!(matches!(error, AppError::Ai(AiError::ToolNotFound { .. })));
    }

    #[test]
    fn authorize_allows_read_only_tool_without_marker() {
        let call = authorize_external_call(&registry(), &request("workspace.list", false))
            .expect("read-only tools need no marker");
        assert_eq!(call.tool_name, "workspace.list");
        assert_eq!(call.role, super::super::tools::ToolRole::ActionPlanner);
    }

    /// 验收：写操作缺确认标记被拒。
    #[test]
    fn authorize_rejects_proposal_without_confirmation_marker() {
        let error = authorize_external_call(&registry(), &request("runtime.startProposal", false)).unwrap_err();
        match error {
            AppError::Ai(AiError::ExternalConfirmationRequired { tool }) => {
                assert_eq!(tool, "runtime.startProposal")
            }
            other => panic!("expected ExternalConfirmationRequired, got {other:?}"),
        }
        // 全部四个 Proposal 工具一视同仁。
        for tool in [
            "git.createCommitProposal",
            "runtime.startProposal",
            "conflict.applyProposal",
            "runtime.updateConfigProposal",
        ] {
            assert!(matches!(
                authorize_external_call(&registry(), &request(tool, false)),
                Err(AppError::Ai(AiError::ExternalConfirmationRequired { .. }))
            ));
            assert!(authorize_external_call(&registry(), &request(tool, true)).is_ok());
        }
    }

    /// 验收：审计含来源标识与参数 hash，无敏感原文。
    #[test]
    fn audit_line_has_source_and_hash_but_no_raw_arguments() {
        let mut req = request("runtime.getLogs", false);
        req.source = ExternalSource::Cli;
        req.arguments = json!({"runtimeName": "prod-api-secret-value"});
        let line = audit_line(&req, "ok", "-", &tools::hash_parameters(&req.arguments), 42, 7);
        assert!(line.contains("source=cli"));
        assert!(line.contains("tool=runtime.getLogs"));
        assert!(line.contains("result_bytes=42"));
        assert!(!line.contains("prod-api-secret-value"));
    }
}
