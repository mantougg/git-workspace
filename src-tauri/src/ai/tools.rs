//! AI-05/AI-11: typed tool registry. Read-only tools expose workspace facts;
//! write-capable entries only create persisted Action Proposals.
//!
//! Tools are an application capability boundary, not an arbitrary function
//! executor. Definitions are serializable so the same contract can be exposed
//! to the UI and future external adapters. Execution delegates to existing
//! domain services and never spawns a shell or mutates repository/runtime
//! state.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::OnceLock;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::core::{diff, git_status, graph, history};
use crate::db::dao;
use crate::error::{AppError, AppResult};
use crate::java::registry::list_jdks;
use crate::maven::{self, RuntimeScope};
use crate::runtime::{self, RuntimeLogQuery};
use crate::state::AppState;

use super::error::AiError;
use super::proposal::{self, ActionKind, RiskLevel};

pub const TOOL_SCHEMA_VERSION: &str = "1.0";
pub const DEFAULT_TOOL_CALL_LIMIT: u32 = 8;
const DEFAULT_TIMEOUT_MS: u64 = 10_000;
const DEFAULT_RESULT_BYTES: usize = 256 * 1024;

/// Restricted assistant roles from design §9.2.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ToolRole {
    WorkspaceAssistant,
    GitReviewer,
    CommitAssistant,
    ConflictAssistant,
    RuntimeDiagnostician,
    RuntimeConfigAdvisor,
    ActionPlanner,
}

impl ToolRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::WorkspaceAssistant => "workspaceAssistant",
            Self::GitReviewer => "gitReviewer",
            Self::CommitAssistant => "commitAssistant",
            Self::ConflictAssistant => "conflictAssistant",
            Self::RuntimeDiagnostician => "runtimeDiagnostician",
            Self::RuntimeConfigAdvisor => "runtimeConfigAdvisor",
            Self::ActionPlanner => "actionPlanner",
        }
    }
}

/// Scope required by a tool. This is also an explicit guard against an agent
/// silently broadening a request from one repository/workspace to another.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ToolScope {
    Workspace,
    Repository,
    Runtime,
    Jdk,
    Maven,
    Task,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolDefinition {
    pub name: String,
    pub version: String,
    pub input_schema: Value,
    pub allowed_roles: Vec<ToolRole>,
    pub context_scope: ToolScope,
    pub requires_workspace: bool,
    pub may_contain_secrets: bool,
    pub timeout_ms: u64,
    pub max_result_bytes: usize,
    pub read_only: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallRequest {
    pub request_id: String,
    pub tool_name: String,
    pub role: ToolRole,
    pub arguments: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolInvocation {
    pub request_id: String,
    pub tool_name: String,
    pub role: ToolRole,
    pub result: Value,
    pub truncated: bool,
    pub result_bytes: usize,
    pub total_result_bytes: usize,
    pub duration_ms: u64,
    pub parameter_hash: String,
}

/// The execution context contains only shared handles to existing services.
/// It deliberately does not expose Repository handles or command execution.
#[derive(Clone)]
pub struct ToolContext {
    pub db: Arc<Mutex<rusqlite::Connection>>,
    pub runtime: Arc<runtime::RuntimeService>,
    pub status_cache: Arc<moka::sync::Cache<String, crate::models::repository::RepoStatus>>,
    pub task_manager: Arc<crate::task::manager::TaskManager>,
}

impl ToolContext {
    pub fn from_state(state: &AppState) -> Self {
        Self {
            db: Arc::clone(&state.db),
            runtime: Arc::clone(&state.runtime),
            status_cache: Arc::clone(&state.status_cache),
            task_manager: Arc::clone(&state.task_manager),
        }
    }
}

/// Registry and per-user-request call budget.
pub struct ToolRegistry {
    definitions: HashMap<String, ToolDefinition>,
    call_limit: u32,
    calls: Mutex<HashMap<String, u32>>,
}

static GLOBAL_REGISTRY: OnceLock<ToolRegistry> = OnceLock::new();

pub fn registry() -> &'static ToolRegistry {
    GLOBAL_REGISTRY.get_or_init(ToolRegistry::default)
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new(DEFAULT_TOOL_CALL_LIMIT)
    }
}

impl ToolRegistry {
    pub fn new(call_limit: u32) -> Self {
        let definitions = definitions()
            .into_iter()
            .map(|d| (d.name.clone(), d))
            .collect();
        Self {
            definitions,
            call_limit: call_limit.max(1),
            calls: Mutex::new(HashMap::new()),
        }
    }

    pub fn definitions(&self) -> Vec<ToolDefinition> {
        let mut out: Vec<_> = self.definitions.values().cloned().collect();
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out
    }

    pub fn get(&self, name: &str) -> Option<&ToolDefinition> {
        self.definitions.get(name)
    }

    pub fn reset_request(&self, request_id: &str) {
        self.calls.lock().unwrap().remove(request_id);
    }

    pub async fn invoke(
        &self,
        call: ToolCallRequest,
        context: ToolContext,
    ) -> AppResult<ToolInvocation> {
        let definition = self.validate(&call)?;
        self.reserve_call(&call.request_id)?;

        let request_id = call.request_id.clone();
        let tool_name = call.tool_name.clone();
        let role = call.role;
        let parameter_hash = hash_parameters(&call.arguments);
        let started = Instant::now();
        let timeout_ms = definition.timeout_ms;
        let max_result_bytes = definition.max_result_bytes;
        let task = tokio::task::spawn_blocking(move || {
            execute_sync(&definition, &call.arguments, &context)
        });
        let value =
            match tokio::time::timeout(std::time::Duration::from_millis(timeout_ms), task).await {
                Ok(Ok(result)) => result?,
                Ok(Err(join_error)) => return Err(AppError::Other(join_error.to_string())),
                Err(_) => {
                    return Err(AppError::Ai(AiError::ToolTimeout {
                        tool: tool_name,
                        timeout_ms,
                    }))
                }
            };

        let (result, truncated, result_bytes, total_result_bytes) =
            limit_result(value, max_result_bytes);
        let duration_ms = started.elapsed().as_millis() as u64;
        log::info!(
            "ai tool audit: tool={} role={} request_id={} parameter_hash={} duration_ms={} result_bytes={} truncated={}",
            call_name(&tool_name),
            role.as_str(),
            request_id,
            parameter_hash,
            duration_ms,
            result_bytes,
            truncated
        );
        Ok(ToolInvocation {
            request_id,
            tool_name,
            role,
            result,
            truncated,
            result_bytes,
            total_result_bytes,
            duration_ms,
            parameter_hash,
        })
    }

    fn validate(&self, call: &ToolCallRequest) -> AppResult<ToolDefinition> {
        let Some(definition) = self.definitions.get(&call.tool_name) else {
            return Err(AppError::Ai(AiError::ToolNotFound {
                name: call.tool_name.clone(),
            }));
        };
        if !definition.allowed_roles.contains(&call.role) {
            return Err(AppError::Ai(AiError::ToolNotAllowed {
                tool: call.tool_name.clone(),
                role: call.role.as_str().to_string(),
            }));
        }
        let Some(object) = call.arguments.as_object() else {
            return Err(AppError::Ai(AiError::ToolInputInvalid {
                tool: call.tool_name.clone(),
                message: "arguments must be a JSON object".into(),
            }));
        };
        for required in definition
            .input_schema
            .get("required")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            if !object.contains_key(required) {
                return Err(AppError::Ai(AiError::ToolInputInvalid {
                    tool: call.tool_name.clone(),
                    message: format!("missing required field '{required}'"),
                }));
            }
        }
        Ok(definition.clone())
    }

    fn reserve_call(&self, request_id: &str) -> AppResult<()> {
        let mut calls = self.calls.lock().unwrap();
        let count = calls.entry(request_id.to_string()).or_default();
        if *count >= self.call_limit {
            return Err(AppError::Ai(AiError::ToolCallLimitExceeded {
                max: self.call_limit,
            }));
        }
        *count += 1;
        Ok(())
    }
}

fn call_name(name: &str) -> &str {
    name
}

fn execute_sync(definition: &ToolDefinition, args: &Value, ctx: &ToolContext) -> AppResult<Value> {
    let workspace_id = args.get("workspaceId").and_then(Value::as_i64);
    if definition.requires_workspace && workspace_id.is_none() {
        return Err(AppError::Ai(AiError::ToolScopeViolation {
            tool: definition.name.clone(),
            message: "workspaceId is required for this tool".into(),
        }));
    }
    let repo_path = if let Some(path) = args.get("repoPath").and_then(Value::as_str) {
        Some(guard_repo_path(ctx, workspace_id, path, &definition.name)?)
    } else {
        None
    };

    match definition.name.as_str() {
        "workspace.list" => Ok(json!(dao::list_workspaces(&ctx.db.lock().unwrap())?)),
        "repository.list" => Ok(json!(dao::list_repositories_by_workspace(
            &ctx.db.lock().unwrap(),
            workspace_id.unwrap()
        )?)),
        "repository.status" => {
            let path = repo_path.ok_or_else(|| input_error(&definition.name, "repoPath"))?;
            let status = git_status::get_repo_status(&path)?;
            ctx.status_cache
                .insert(path.to_string_lossy().to_string(), status.clone());
            Ok(json!(status))
        }
        "repository.diff" => {
            let path = repo_path.ok_or_else(|| input_error(&definition.name, "repoPath"))?;
            let options = args.get("options").cloned().unwrap_or_else(|| json!({}));
            let opt: crate::commands::diff::DiffOptionsParam =
                parse_input(&options, &definition.name)?;
            let config = diff::DiffConfig {
                ignore_whitespace: opt.ignore_whitespace,
                ignore_whitespace_eol: opt.ignore_whitespace_eol,
                ignore_case: opt.ignore_case,
            };
            Ok(json!(diff::get_workdir_diff_with_config(&path, &config)?))
        }
        "repository.history" => {
            let path = repo_path.ok_or_else(|| input_error(&definition.name, "repoPath"))?;
            let max = args.get("maxCount").and_then(Value::as_u64).unwrap_or(100) as usize;
            Ok(json!(graph::get_commit_history(&path, max.min(1000))?))
        }
        "repository.conflicts" => {
            let path = repo_path.ok_or_else(|| input_error(&definition.name, "repoPath"))?;
            Ok(json!(history::conflict_files(&path)?))
        }
        "runtime.listApplications" => Ok(json!(runtime::list_configs(
            &ctx.db.lock().unwrap(),
            workspace_id.unwrap()
        )?)),
        "runtime.getConfig" => {
            let name = required_string(args, "runtimeName", &definition.name)?;
            Ok(json!(runtime::get_config(
                &ctx.db.lock().unwrap(),
                workspace_id.unwrap(),
                name
            )?))
        }
        "runtime.getProcessStatus" => {
            let process_id = required_i64(args, "processId", &definition.name)?;
            let process = ctx.runtime.process_status(process_id)?;
            ensure_process_workspace(&process, workspace_id.unwrap(), &definition.name)?;
            Ok(json!(process))
        }
        "runtime.getClosure" => {
            let project = required_string(args, "project", &definition.name)?;
            let scope = args
                .get("scope")
                .cloned()
                .unwrap_or_else(|| json!(RuntimeScope::Auto));
            let scope: RuntimeScope = parse_input(&scope, &definition.name)?;
            Ok(json!(ctx.runtime.closure_preview(
                workspace_id.unwrap(),
                project,
                &scope
            )?))
        }
        "runtime.getLogs" => {
            let query = log_query(args, &definition.name)?;
            let process = ctx.runtime.process_status(query.process_id)?;
            ensure_process_workspace(&process, query.workspace_id, &definition.name)?;
            Ok(json!(ctx.runtime.get_logs(&query)?))
        }
        "runtime.getErrorContext" => {
            let query = log_query(args, &definition.name)?;
            let process = ctx.runtime.process_status(query.process_id)?;
            ensure_process_workspace(&process, query.workspace_id, &definition.name)?;
            let lines = ctx.runtime.search_logs_tail(&query, 200)?;
            Ok(json!({"process": process, "logs": lines}))
        }
        "jdk.list" => Ok(json!(list_jdks(&ctx.db.lock().unwrap())?)),
        "maven.detect" => {
            let project_dir = args
                .get("projectDir")
                .and_then(Value::as_str)
                .map(std::path::Path::new);
            let configured = args.get("configuredPath").and_then(Value::as_str);
            Ok(json!(maven::detect_maven_candidates(
                project_dir,
                configured
            )))
        }
        "task.getStatus" => {
            let ids = args
                .get("taskIds")
                .and_then(Value::as_array)
                .ok_or_else(|| input_error(&definition.name, "taskIds"))?;
            let ids: Vec<String> = ids
                .iter()
                .filter_map(Value::as_str)
                .map(ToOwned::to_owned)
                .collect();
            Ok(json!(ctx.task_manager.get_status(&ids)))
        }
        "git.createCommitProposal" => {
            let path = repo_path.ok_or_else(|| input_error(&definition.name, "repoPath"))?;
            let message = required_string(args, "message", &definition.name)?;
            let files = args
                .get("files")
                .and_then(Value::as_array)
                .map(|items| items.iter().filter_map(Value::as_str).map(ToOwned::to_owned).collect::<Vec<_>>())
                .unwrap_or_default();
            if files.iter().any(|file| !is_safe_relative_path(file)) {
                return Err(AppError::Ai(AiError::ToolScopeViolation {
                    tool: definition.name.clone(),
                    message: "commit file paths must stay inside the selected repository".into(),
                }));
            }
            let payload = json!({
                "repoPath": path.to_string_lossy(),
                "repoName": path.file_name().and_then(|v| v.to_str()).unwrap_or("repository"),
                "message": message,
                "files": files,
                "amend": args.get("amend").and_then(Value::as_bool).unwrap_or(false),
                "noEdit": args.get("noEdit").and_then(Value::as_bool).unwrap_or(false),
                "indexOnly": args.get("indexOnly").and_then(Value::as_bool).unwrap_or(false),
                "thenPush": args.get("thenPush").and_then(Value::as_bool).unwrap_or(false),
                "allowUnsafe": args.get("allowUnsafe").and_then(Value::as_bool).unwrap_or(false),
            });
            reject_proposal_secrets(&payload)?;
            let (proposal, action_payload) = proposal::new_proposal(
                args.get("requestId").and_then(Value::as_str).map(ToOwned::to_owned),
                ActionKind::GitCreateCommit,
                RiskLevel::Medium,
                json!({"workspaceId": workspace_id, "repoPath": path.to_string_lossy()}),
                vec![path.to_string_lossy().to_string()],
                payload["files"].as_array().cloned().unwrap_or_default().into_iter().filter_map(|v| v.as_str().map(ToOwned::to_owned)).collect(),
                "当前工作区变更将保持不变",
                format!("创建提交：{}", message),
                None,
                Some(format!("git add <files> && git commit -m <message>")),
                true,
                payload,
            );
            proposal::insert(&ctx.db.lock().unwrap(), &proposal, &action_payload)?;
            Ok(json!(proposal))
        }
        "runtime.startProposal" => {
            let runtime_name = required_string(args, "runtimeName", &definition.name)?;
            let options = args.get("options").cloned().unwrap_or_else(|| json!({}));
            let payload = json!({
                "workspaceId": workspace_id.unwrap(),
                "runtimeName": runtime_name,
                "options": options,
            });
            reject_proposal_secrets(&payload)?;
            let (proposal, action_payload) = proposal::new_proposal(
                args.get("requestId").and_then(Value::as_str).map(ToOwned::to_owned),
                ActionKind::RuntimeStart,
                RiskLevel::Medium,
                json!({"workspaceId": workspace_id, "runtimeName": runtime_name}),
                vec![],
                vec![],
                "Runtime 当前未启动",
                format!("启动 Runtime：{}", runtime_name),
                None,
                Some(format!("runtime.start {}", runtime_name)),
                true,
                payload,
            );
            proposal::insert(&ctx.db.lock().unwrap(), &proposal, &action_payload)?;
            Ok(json!(proposal))
        }
        "conflict.applyProposal" => {
            let path = repo_path.ok_or_else(|| input_error(&definition.name, "repoPath"))?;
            let conflict_path = required_string(args, "path", &definition.name)?;
            if !is_safe_relative_path(conflict_path) {
                return Err(AppError::Ai(AiError::ToolScopeViolation {
                    tool: definition.name.clone(),
                    message: "conflict path must stay inside the selected repository".into(),
                }));
            }
            let strategy = required_string(args, "strategy", &definition.name)?;
            if !matches!(strategy, "ours" | "theirs" | "both" | "content") {
                return Err(input_error(&definition.name, "strategy"));
            }
            let content = args.get("content").and_then(Value::as_str);
            let payload = json!({
                "repoPath": path.to_string_lossy(),
                "repoName": path.file_name().and_then(|v| v.to_str()).unwrap_or("repository"),
                "path": conflict_path,
                "strategy": strategy,
                "content": content,
            });
            reject_proposal_secrets(&payload)?;
            let (proposal, action_payload) = proposal::new_proposal(
                args.get("requestId").and_then(Value::as_str).map(ToOwned::to_owned),
                ActionKind::ConflictApply,
                RiskLevel::High,
                json!({"workspaceId": workspace_id, "repoPath": path.to_string_lossy()}),
                vec![path.to_string_lossy().to_string()],
                vec![conflict_path.to_string()],
                "冲突文件保持未解决",
                format!("应用冲突解决：{} ({})", conflict_path, strategy),
                None,
                Some(format!("git conflict resolve {}", conflict_path)),
                true,
                payload,
            );
            proposal::insert(&ctx.db.lock().unwrap(), &proposal, &action_payload)?;
            Ok(json!(proposal))
        }
        "runtime.updateConfigProposal" => {
            let runtime_name = required_string(args, "runtimeName", &definition.name)?;
            let config = args.get("config").cloned().ok_or_else(|| input_error(&definition.name, "config"))?;
            if !config.is_object() {
                return Err(input_error(&definition.name, "config"));
            }
            let payload = json!({"workspaceId": workspace_id.unwrap(), "runtimeName": runtime_name, "config": config});
            reject_proposal_secrets(&payload)?;
            let (proposal, action_payload) = proposal::new_proposal(
                args.get("requestId").and_then(Value::as_str).map(ToOwned::to_owned),
                ActionKind::RuntimeUpdateConfig,
                RiskLevel::High,
                json!({"workspaceId": workspace_id, "runtimeName": runtime_name}),
                vec![],
                vec![],
                "Runtime 配置保持不变",
                format!("更新 Runtime 配置：{}", runtime_name),
                None,
                Some(format!("runtime.update_config {}", runtime_name)),
                true,
                payload,
            );
            proposal::insert(&ctx.db.lock().unwrap(), &proposal, &action_payload)?;
            Ok(json!(proposal))
        }
        _ => Err(AppError::Ai(AiError::ToolNotFound {
            name: definition.name.clone(),
        })),
    }
}

/// Action payloads may contain user-provided file content or environment
/// values.  The default Block policy applies before proposal persistence so
/// secrets cannot be stranded in `ai_proposals` or the task history.
fn reject_proposal_secrets(payload: &Value) -> AppResult<()> {
    let findings = crate::core::secret::scan_secrets(&payload.to_string());
    if findings.is_empty() {
        return Ok(());
    }
    let mut kinds = findings
        .iter()
        .map(|finding| finding.kind.label())
        .collect::<Vec<_>>();
    kinds.sort_unstable();
    kinds.dedup();
    Err(AppError::Ai(AiError::SecretDetected {
        kinds: kinds.join("、"),
    }))
}

fn is_safe_relative_path(path: &str) -> bool {
    let candidate = std::path::Path::new(path);
    candidate.is_relative()
        && candidate
            .components()
            .all(|component| !matches!(component, std::path::Component::ParentDir))
}

fn required_string<'a>(args: &'a Value, key: &str, tool: &str) -> AppResult<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .filter(|v| !v.trim().is_empty())
        .ok_or_else(|| input_error(tool, key))
}

fn required_i64(args: &Value, key: &str, tool: &str) -> AppResult<i64> {
    args.get(key)
        .and_then(Value::as_i64)
        .ok_or_else(|| input_error(tool, key))
}

fn input_error(tool: &str, key: &str) -> AppError {
    AppError::Ai(AiError::ToolInputInvalid {
        tool: tool.into(),
        message: format!("missing or invalid '{key}'"),
    })
}

fn log_query(args: &Value, tool: &str) -> AppResult<RuntimeLogQuery> {
    Ok(RuntimeLogQuery {
        workspace_id: required_i64(args, "workspaceId", tool)?,
        runtime_name: required_string(args, "runtimeName", tool)?.to_string(),
        process_id: required_i64(args, "processId", tool)?,
        filter: args
            .get("filter")
            .cloned()
            .map(|value| parse_input(&value, tool))
            .transpose()?
            .unwrap_or_default(),
    })
}

fn parse_input<T: serde::de::DeserializeOwned>(value: &Value, tool: &str) -> AppResult<T> {
    serde_json::from_value(value.clone()).map_err(|error| {
        AppError::Ai(AiError::ToolInputInvalid {
            tool: tool.into(),
            message: error.to_string(),
        })
    })
}

fn ensure_process_workspace(
    process: &Option<crate::runtime::RuntimeProcessInfo>,
    workspace_id: i64,
    tool: &str,
) -> AppResult<()> {
    match process {
        Some(process) if process.workspace_id == workspace_id => Ok(()),
        Some(_) => Err(AppError::Ai(AiError::ToolScopeViolation {
            tool: tool.into(),
            message: "process does not belong to the current workspace".into(),
        })),
        None => Ok(()),
    }
}

fn guard_repo_path(
    ctx: &ToolContext,
    workspace_id: Option<i64>,
    path: &str,
    tool: &str,
) -> AppResult<std::path::PathBuf> {
    let workspace_id = workspace_id.ok_or_else(|| input_error(tool, "workspaceId"))?;
    let root = dao::get_workspace(&ctx.db.lock().unwrap(), workspace_id)?.path;
    let candidate = std::path::Path::new(path);
    let candidate = if candidate.is_absolute() {
        candidate.to_path_buf()
    } else {
        std::path::Path::new(&root).join(candidate)
    };
    let root_norm = root.replace('\\', "/").trim_end_matches('/').to_string();
    let candidate_norm = candidate.to_string_lossy().replace('\\', "/");
    if candidate_norm != root_norm && !candidate_norm.starts_with(&(root_norm.clone() + "/")) {
        return Err(AppError::Ai(AiError::ToolScopeViolation {
            tool: tool.into(),
            message: "repository is outside the current workspace".into(),
        }));
    }
    Ok(candidate)
}

fn hash_parameters(value: &Value) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    value.to_string().hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

fn limit_result(value: Value, max_bytes: usize) -> (Value, bool, usize, usize) {
    let encoded = serde_json::to_vec(&value).unwrap_or_default();
    let total = encoded.len();
    if total <= max_bytes {
        return (value, false, total, total);
    }
    let mut preview_budget = max_bytes.saturating_sub(96);
    let (result, bytes) = loop {
        let preview =
            String::from_utf8_lossy(&encoded[..encoded.len().min(preview_budget)]).to_string();
        let result = json!({"truncated": true, "totalBytes": total, "preview": preview});
        let bytes = serde_json::to_vec(&result).map(|v| v.len()).unwrap_or(0);
        if bytes <= max_bytes || preview_budget == 0 {
            break (result, bytes);
        }
        preview_budget = preview_budget.saturating_mul(3) / 4;
    };
    (result, true, bytes, total)
}

fn schema(properties: Value, required: &[&str]) -> Value {
    json!({"type":"object", "properties":properties, "required":required, "additionalProperties":true})
}

fn roles(roles: &[ToolRole]) -> Vec<ToolRole> {
    let mut out = Vec::new();
    for role in roles.iter().copied().chain([ToolRole::ActionPlanner]) {
        if !out.contains(&role) {
            out.push(role);
        }
    }
    out
}

fn definition(
    name: &str,
    scope: ToolScope,
    required: bool,
    secret: bool,
    properties: Value,
    fields: &[&str],
    allowed: &[ToolRole],
) -> ToolDefinition {
    ToolDefinition {
        name: name.into(),
        version: TOOL_SCHEMA_VERSION.into(),
        input_schema: schema(properties, fields),
        allowed_roles: roles(allowed),
        context_scope: scope,
        requires_workspace: required,
        may_contain_secrets: secret,
        timeout_ms: DEFAULT_TIMEOUT_MS,
        max_result_bytes: DEFAULT_RESULT_BYTES,
        read_only: true,
    }
}

fn proposal_definition(
    name: &str,
    scope: ToolScope,
    properties: Value,
    fields: &[&str],
) -> ToolDefinition {
    let mut definition = definition(name, scope, true, false, properties, fields, &[ToolRole::ActionPlanner]);
    definition.read_only = false;
    definition
}

fn secret_proposal_definition(
    name: &str,
    scope: ToolScope,
    properties: Value,
    fields: &[&str],
) -> ToolDefinition {
    let mut definition = proposal_definition(name, scope, properties, fields);
    definition.may_contain_secrets = true;
    definition
}

pub fn definitions() -> Vec<ToolDefinition> {
    use ToolRole::*;
    vec![
        definition(
            "workspace.list",
            ToolScope::Workspace,
            false,
            false,
            json!({}),
            &[],
            &[WorkspaceAssistant],
        ),
        definition(
            "repository.list",
            ToolScope::Workspace,
            true,
            false,
            json!({"workspaceId":{"type":"integer"}}),
            &["workspaceId"],
            &[
                WorkspaceAssistant,
                GitReviewer,
                CommitAssistant,
                ConflictAssistant,
            ],
        ),
        definition(
            "repository.status",
            ToolScope::Repository,
            true,
            false,
            json!({"workspaceId":{"type":"integer"},"repoPath":{"type":"string"}}),
            &["workspaceId", "repoPath"],
            &[
                WorkspaceAssistant,
                GitReviewer,
                CommitAssistant,
                ConflictAssistant,
            ],
        ),
        definition(
            "repository.diff",
            ToolScope::Repository,
            true,
            false,
            json!({"workspaceId":{"type":"integer"},"repoPath":{"type":"string"},"options":{"type":"object"}}),
            &["workspaceId", "repoPath"],
            &[GitReviewer, CommitAssistant, ConflictAssistant],
        ),
        definition(
            "repository.history",
            ToolScope::Repository,
            true,
            false,
            json!({"workspaceId":{"type":"integer"},"repoPath":{"type":"string"},"maxCount":{"type":"integer"}}),
            &["workspaceId", "repoPath"],
            &[WorkspaceAssistant, GitReviewer, CommitAssistant],
        ),
        definition(
            "repository.conflicts",
            ToolScope::Repository,
            true,
            false,
            json!({"workspaceId":{"type":"integer"},"repoPath":{"type":"string"}}),
            &["workspaceId", "repoPath"],
            &[ConflictAssistant],
        ),
        definition(
            "runtime.listApplications",
            ToolScope::Runtime,
            true,
            false,
            json!({"workspaceId":{"type":"integer"}}),
            &["workspaceId"],
            &[RuntimeDiagnostician, RuntimeConfigAdvisor],
        ),
        definition(
            "runtime.getConfig",
            ToolScope::Runtime,
            true,
            false,
            json!({"workspaceId":{"type":"integer"},"runtimeName":{"type":"string"}}),
            &["workspaceId", "runtimeName"],
            &[RuntimeDiagnostician, RuntimeConfigAdvisor],
        ),
        definition(
            "runtime.getProcessStatus",
            ToolScope::Runtime,
            true,
            false,
            json!({"workspaceId":{"type":"integer"},"processId":{"type":"integer"}}),
            &["workspaceId", "processId"],
            &[RuntimeDiagnostician],
        ),
        definition(
            "runtime.getClosure",
            ToolScope::Runtime,
            true,
            false,
            json!({"workspaceId":{"type":"integer"},"project":{"type":"string"},"scope":{}}),
            &["workspaceId", "project"],
            &[RuntimeDiagnostician, RuntimeConfigAdvisor],
        ),
        definition(
            "runtime.getLogs",
            ToolScope::Runtime,
            true,
            true,
            json!({"workspaceId":{"type":"integer"},"runtimeName":{"type":"string"},"processId":{"type":"integer"},"filter":{"type":"object"}}),
            &["workspaceId", "runtimeName", "processId"],
            &[RuntimeDiagnostician],
        ),
        definition(
            "runtime.getErrorContext",
            ToolScope::Runtime,
            true,
            true,
            json!({"workspaceId":{"type":"integer"},"runtimeName":{"type":"string"},"processId":{"type":"integer"}}),
            &["workspaceId", "runtimeName", "processId"],
            &[RuntimeDiagnostician],
        ),
        definition(
            "jdk.list",
            ToolScope::Jdk,
            false,
            false,
            json!({}),
            &[],
            &[RuntimeDiagnostician, RuntimeConfigAdvisor],
        ),
        definition(
            "maven.detect",
            ToolScope::Maven,
            false,
            false,
            json!({"projectDir":{"type":"string"},"configuredPath":{"type":"string"}}),
            &[],
            &[RuntimeDiagnostician, RuntimeConfigAdvisor],
        ),
        definition(
            "task.getStatus",
            ToolScope::Task,
            false,
            false,
            json!({"taskIds":{"type":"array","items":{"type":"string"}}}),
            &["taskIds"],
            &[RuntimeDiagnostician, RuntimeConfigAdvisor],
        ),
        secret_proposal_definition(
            "git.createCommitProposal",
            ToolScope::Repository,
            json!({"workspaceId":{"type":"integer"},"repoPath":{"type":"string"},"message":{"type":"string"},"files":{"type":"array"},"amend":{"type":"boolean"},"noEdit":{"type":"boolean"},"indexOnly":{"type":"boolean"},"thenPush":{"type":"boolean"},"allowUnsafe":{"type":"boolean"},"requestId":{"type":"string"}}),
            &["workspaceId", "repoPath", "message"],
        ),
        secret_proposal_definition(
            "runtime.startProposal",
            ToolScope::Runtime,
            json!({"workspaceId":{"type":"integer"},"runtimeName":{"type":"string"},"options":{"type":"object"},"requestId":{"type":"string"}}),
            &["workspaceId", "runtimeName"],
        ),
        secret_proposal_definition(
            "conflict.applyProposal",
            ToolScope::Repository,
            json!({"workspaceId":{"type":"integer"},"repoPath":{"type":"string"},"path":{"type":"string"},"strategy":{"type":"string"},"content":{"type":"string"},"requestId":{"type":"string"}}),
            &["workspaceId", "repoPath", "path", "strategy"],
        ),
        secret_proposal_definition(
            "runtime.updateConfigProposal",
            ToolScope::Runtime,
            json!({"workspaceId":{"type":"integer"},"runtimeName":{"type":"string"},"config":{"type":"object"},"requestId":{"type":"string"}}),
            &["workspaceId", "runtimeName", "config"],
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_contains_all_first_phase_tools() {
        assert_eq!(definitions().len(), 19);
        assert!(definitions()
            .iter()
            .all(|d| d.version == TOOL_SCHEMA_VERSION));
        assert_eq!(definitions().iter().filter(|d| !d.read_only).count(), 4);
    }

    #[test]
    fn action_planner_can_read_every_tool_but_git_reviewer_cannot_read_runtime_logs() {
        let registry = ToolRegistry::default();
        assert!(registry
            .definitions()
            .iter()
            .all(|d| d.allowed_roles.contains(&ToolRole::ActionPlanner)));
        assert!(!registry
            .get("runtime.getLogs")
            .unwrap()
            .allowed_roles
            .contains(&ToolRole::GitReviewer));
    }

    #[test]
    fn result_limit_marks_truncation() {
        let (value, truncated, bytes, total) = limit_result(json!({"body":"x".repeat(1000)}), 128);
        assert!(truncated);
        assert!(value["truncated"].as_bool().unwrap());
        assert!(bytes <= 256);
        assert!(total > bytes);
    }

    #[test]
    fn parameter_hash_is_stable_without_logging_values() {
        assert_eq!(
            hash_parameters(&json!({"a":1})),
            hash_parameters(&json!({"a":1}))
        );
        assert_ne!(
            hash_parameters(&json!({"a":1})),
            hash_parameters(&json!({"a":2}))
        );
    }

    #[test]
    fn request_budget_stops_at_eight_calls() {
        let registry = ToolRegistry::default();
        for _ in 0..8 {
            registry.reserve_call("req-1").unwrap();
        }
        let error = registry.reserve_call("req-1").unwrap_err();
        assert!(matches!(
            error,
            AppError::Ai(AiError::ToolCallLimitExceeded { max: 8 })
        ));
        registry.reset_request("req-1");
        registry.reserve_call("req-1").unwrap();
    }

    #[test]
    fn schema_snapshot_is_stable() {
        let actual = serde_json::to_string_pretty(&definitions()).unwrap() + "\n";
        let path =
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("golden/ai_tools.json");
        if std::env::var("GW_UPDATE_GOLDEN").is_ok() {
            std::fs::write(path, actual).unwrap();
            return;
        }
        let expected = std::fs::read_to_string(path).expect("missing golden/ai_tools.json");
        assert_eq!(expected, actual);
    }
}
