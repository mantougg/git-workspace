//! Runtime 配置校验与敏感字段处理（R-07，B-06 拆分）：名称、路径、
//! 符号链接守卫、环境变量 key 校验；脱敏与占位符保留（§4.8——完整
//! 秘密值不得重新暴露到 IPC）。

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use crate::error::{AppError, AppResult};

use super::model::{RuntimeApplicationConfig, MASKED_VALUE};

pub(super) fn validate_runtime_name(name: &str) -> AppResult<()> {
    let trimmed = name.trim();
    if trimmed.is_empty()
        || trimmed == "."
        || trimmed == ".."
        || trimmed != name
        || name.len() > 128
        || name.chars().any(|c| {
            c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')
        })
    {
        return Err(AppError::RuntimeConfig(format!(
            "Runtime 名称 '{}' 不能用作配置文件名；请移除首尾空格、路径分隔符或 Windows 保留字符",
            name
        )));
    }
    Ok(())
}

pub(super) fn validate_environment(environment: &BTreeMap<String, String>) -> AppResult<()> {
    for key in environment.keys() {
        if key.is_empty() || key.contains('=') || key.chars().any(|c| c == '\0' || c.is_control()) {
            return Err(AppError::RuntimeConfig(format!(
                "环境变量 key '{}' 无效：不能包含控制字符或 '='",
                key
            )));
        }
    }
    Ok(())
}

pub(super) fn reject_symlink(path: &Path) -> AppResult<()> {
    if let Ok(metadata) = fs::symlink_metadata(path) {
        if metadata.file_type().is_symlink() {
            return Err(AppError::Permission(format!(
                "拒绝通过符号链接写入 Runtime 配置目录：{}",
                path.display()
            )));
        }
    }
    Ok(())
}

pub(super) fn preserve_masked_values(
    mut incoming: RuntimeApplicationConfig,
    existing: &RuntimeApplicationConfig,
) -> RuntimeApplicationConfig {
    incoming.environment = preserve_masked_environment(incoming.environment, &existing.environment);
    incoming.runtime_environment =
        preserve_masked_environment(incoming.runtime_environment, &existing.runtime_environment);
    incoming
}

pub(super) fn redact_config(mut config: RuntimeApplicationConfig) -> RuntimeApplicationConfig {
    config.environment = redact_environment(config.environment);
    config.runtime_environment = redact_environment(config.runtime_environment);
    config
}

pub(super) fn redact_environment(
    environment: BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    environment
        .into_iter()
        .map(|(key, value)| {
            if is_sensitive_key(&key) {
                (key, MASKED_VALUE.to_string())
            } else {
                (key, value)
            }
        })
        .collect()
}

pub(super) fn preserve_masked_environment(
    mut incoming: BTreeMap<String, String>,
    existing: &BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    for (key, value) in &mut incoming {
        if value == MASKED_VALUE {
            if let Some(previous) = existing.get(key) {
                *value = previous.clone();
            }
        }
    }
    incoming
}

fn is_sensitive_key(key: &str) -> bool {
    crate::core::secret::is_sensitive_environment_key(key)
}
