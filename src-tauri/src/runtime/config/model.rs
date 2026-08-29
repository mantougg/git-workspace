//! Runtime 配置模型（R-07，B-06 拆分）：完整配置文档、请求/摘要 DTO、
//! schema 版本常量与脱敏占位符。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use crate::maven::RuntimeScope;

use super::validation::validate_environment;
use super::validation::validate_runtime_name;

pub const CURRENT_SCHEMA_VERSION: u32 = 1;
pub const MASKED_VALUE: &str = "••••••••";

/// Complete user-editable Runtime configuration document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeApplicationConfig {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub project: String,
    #[serde(default)]
    pub main_class: Option<String>,
    #[serde(default)]
    pub jdk: Option<String>,
    #[serde(default)]
    pub profile: Option<String>,
    #[serde(default)]
    pub vm_options: Vec<String>,
    #[serde(default)]
    pub program_arguments: Vec<String>,
    /// Application-level environment variables (highest user-configurable layer).
    #[serde(default)]
    pub environment: BTreeMap<String, String>,
    /// Runtime-level environment variables.
    #[serde(default)]
    pub runtime_environment: BTreeMap<String, String>,
    #[serde(default = "default_build_engine")]
    pub build_engine: Option<String>,
    /// Runtime Scope（R-03 §15）：Auto / Manual / Hybrid。缺省 Auto；
    /// R-09 构建流水线按此限定 Runtime Closure 范围。
    #[serde(default)]
    pub scope: RuntimeScope,
    /// R-14 §75 Command Safety：构建前执行的用户脚本。**默认禁止自动执行**；
    /// 首次执行必须用户确认（确认状态持久化于 app data，内容变更后需重新确认）。
    #[serde(default)]
    pub pre_build_script: Option<String>,
    /// R-14 §75 Command Safety：构建成功后执行的用户脚本（同上确认规则）。
    #[serde(default)]
    pub post_build_script: Option<String>,
    /// R-16 §41 健康检查配置；`None` = 不探针（保持 R-12 生命周期推导的
    /// up/down 语义）。配置持久化在本 JSON 内（向后兼容：缺字段有默认值）。
    #[serde(default)]
    pub health_check: Option<crate::runtime::health::HealthCheckConfig>,
    /// R-17 §42 自动重启开关（File Watch 检测到源码变更 → 增量重建 →
    /// 自动重启）。默认关（`None`/`false` 均视为关）；watch 引擎只监听
    /// 开启了该开关且进程活跃的应用。
    #[serde(default)]
    pub auto_restart: Option<bool>,
}

fn default_schema_version() -> u32 {
    CURRENT_SCHEMA_VERSION
}

pub(super) fn default_build_engine() -> Option<String> {
    Some("maven".to_string())
}

impl Default for RuntimeApplicationConfig {
    fn default() -> Self {
        Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            name: String::new(),
            project: String::new(),
            main_class: None,
            jdk: None,
            profile: None,
            vm_options: Vec::new(),
            program_arguments: Vec::new(),
            environment: BTreeMap::new(),
            runtime_environment: BTreeMap::new(),
            build_engine: default_build_engine(),
            scope: RuntimeScope::Auto,
            pre_build_script: None,
            post_build_script: None,
            health_check: None,
            auto_restart: None,
        }
    }
}

impl RuntimeApplicationConfig {
    pub fn validate(&self) -> AppResult<()> {
        validate_runtime_name(&self.name)?;
        if self.project.trim().is_empty() {
            return Err(AppError::RuntimeConfig(
                "Runtime 配置的 project 不能为空，请选择一个 Maven 项目".into(),
            ));
        }
        if self.schema_version > CURRENT_SCHEMA_VERSION {
            return Err(AppError::RuntimeConfig(format!(
                "配置 {} 使用了不受支持的 schemaVersion={}（当前支持到 {}），请升级 GitWorkspace",
                self.name, self.schema_version, CURRENT_SCHEMA_VERSION
            )));
        }
        validate_environment(&self.environment)?;
        validate_environment(&self.runtime_environment)?;
        if let Some(health) = &self.health_check {
            health.validate()?;
        }
        Ok(())
    }

    /// Resolve a profile supplied via either supported Spring Boot form.
    pub fn injected_profile(&self) -> Option<String> {
        self.vm_options
            .iter()
            .find_map(|arg| arg.strip_prefix("-Dspring.profiles.active="))
            .or_else(|| {
                self.program_arguments
                    .iter()
                    .find_map(|arg| arg.strip_prefix("--spring.profiles.active="))
            })
            .map(ToOwned::to_owned)
            .or_else(|| self.profile.clone())
    }

    /// Add the VM-option form only when no explicit profile injection exists.
    pub fn with_default_profile_injection(&self) -> Self {
        let mut next = self.clone();
        if let Some(profile) = self.profile.as_deref() {
            let has_injection = self
                .vm_options
                .iter()
                .any(|arg| arg.starts_with("-Dspring.profiles.active="))
                || self
                    .program_arguments
                    .iter()
                    .any(|arg| arg.starts_with("--spring.profiles.active="));
            if !has_injection {
                next.vm_options
                    .push(format!("-Dspring.profiles.active={profile}"));
            }
        }
        next
    }
}

/// SQLite-only metadata returned by the fast list operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeConfigSummary {
    pub id: i64,
    pub workspace_id: i64,
    pub name: String,
    pub project: String,
    pub main_class: Option<String>,
    pub jdk: Option<String>,
    pub profile: Option<String>,
    pub build_engine: Option<String>,
    pub config_path: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateRuntimeConfigRequest {
    pub workspace_id: i64,
    pub config: RuntimeApplicationConfig,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateRuntimeConfigRequest {
    pub workspace_id: i64,
    pub name: String,
    pub config: RuntimeApplicationConfig,
}
