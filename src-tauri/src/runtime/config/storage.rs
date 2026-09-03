//! Runtime 配置文件存储（R-07，B-06 拆分）：JSON 读写、原子写
//! （临时文件 + rename + 备份回滚）、schema 默认值归一化。
//!
//! 配置文件位于 `.gitworkspace/runtimes/<name>.json`；写路径一律经
//! `reject_symlink` 守卫。原子写失败不产生半写文件。

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use serde::Serialize;
use uuid::Uuid;

use crate::error::{AppError, AppResult};

use super::model::default_build_engine;
use super::model::{RuntimeApplicationConfig, CURRENT_SCHEMA_VERSION};
use super::validation::{reject_symlink, validate_environment};

fn default_runtime_dir(root: &Path) -> PathBuf {
    root.join(".gitworkspace").join("runtimes")
}

pub(super) fn ensure_runtime_dir(root: &Path) -> AppResult<PathBuf> {
    let gitworkspace = root.join(".gitworkspace");
    reject_symlink(&gitworkspace)?;
    // R-14 §78 只读护栏：配置目录必须在 workspace/.gitworkspace 下。
    crate::runtime::guard::assert_workspace_write_path(&gitworkspace, root, "Runtime 配置目录")?;
    fs::create_dir_all(&gitworkspace)?;
    let runtimes = gitworkspace.join("runtimes");
    reject_symlink(&runtimes)?;
    fs::create_dir_all(&runtimes)?;
    Ok(runtimes)
}

pub(super) fn workspace_environment_path(root: &Path) -> PathBuf {
    root.join(".gitworkspace").join("environment.json")
}

pub(super) fn config_path(root: &Path, name: &str) -> AppResult<PathBuf> {
    super::validation::validate_runtime_name(name)?;
    Ok(default_runtime_dir(root).join(format!("{name}.json")))
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct WorkspaceEnvironmentDocument {
    #[serde(default)]
    pub(super) environment: std::collections::BTreeMap<String, String>,
}

pub(super) fn read_environment_file(path: &Path) -> AppResult<std::collections::BTreeMap<String, String>> {
    reject_symlink(path)?;
    if !path.exists() {
        return Ok(std::collections::BTreeMap::new());
    }
    let content = read_file(path)?;
    match serde_json::from_str::<WorkspaceEnvironmentDocument>(&content) {
        Ok(document) => {
            validate_environment(&document.environment)?;
            Ok(document.environment)
        }
        Err(error) => Err(invalid_json_error(path, error)),
    }
}

pub(super) fn read_config_file(path: &Path, expected_name: &str) -> AppResult<RuntimeApplicationConfig> {
    let content = read_file(path)?;
    let config =
        serde_json::from_str::<RuntimeApplicationConfig>(&content).map_err(|error| invalid_json_error(path, error))?;
    if !config.name.trim().is_empty() && config.name != expected_name {
        return Err(AppError::RuntimeConfig(format!(
            "配置文件 {} 的 name='{}' 与索引名称 '{}' 不一致；请通过 Runtime 配置更新它",
            path.display(),
            config.name,
            expected_name
        )));
    }
    Ok(config)
}

pub(super) fn read_file(path: &Path) -> AppResult<String> {
    reject_symlink(path)?;
    let mut file = File::open(path).map_err(|error| {
        AppError::RuntimeConfig(format!(
            "无法读取 Runtime 配置文件 {}：{}。请确认文件存在且可读",
            path.display(),
            error
        ))
    })?;
    let mut content = String::new();
    file.read_to_string(&mut content)
        .map_err(|error| AppError::RuntimeConfig(format!("无法读取 Runtime 配置文件 {}：{}", path.display(), error)))?;
    Ok(content)
}

fn invalid_json_error(path: &Path, error: serde_json::Error) -> AppError {
    AppError::RuntimeConfig(format!(
        "配置文件 {} 的 JSON 无效（第 {} 行，第 {} 列）：{}。请修复 JSON 后重试",
        path.display(),
        error.line(),
        error.column(),
        error
    ))
}

pub(super) fn write_config_file(path: &Path, config: &RuntimeApplicationConfig) -> AppResult<()> {
    write_json_atomic(path, config)
}

pub(crate) fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> AppResult<()> {
    let mut bytes = serde_json::to_vec_pretty(value)?;
    bytes.push(b'\n');
    write_bytes_atomic(path, &bytes)
}

pub(super) fn write_bytes_atomic(path: &Path, bytes: &[u8]) -> AppResult<()> {
    reject_symlink(path)?;
    let parent = path
        .parent()
        .ok_or_else(|| AppError::RuntimeConfig(format!("配置路径没有父目录：{}", path.display())))?;
    fs::create_dir_all(parent)?;
    let file_name = path.file_name().unwrap_or_default().to_string_lossy();
    let temp_path = parent.join(format!(".{file_name}.tmp-{}", Uuid::new_v4()));
    let mut file = OpenOptions::new().write(true).create_new(true).open(&temp_path)?;
    let result = (|| -> AppResult<()> {
        file.write_all(bytes)?;
        file.sync_all()?;
        let backup_path = parent.join(format!(".{file_name}.bak-{}", Uuid::new_v4()));
        let had_existing = path.exists();
        if had_existing {
            fs::rename(path, &backup_path)?;
        }
        match fs::rename(&temp_path, path) {
            Ok(()) => {
                if had_existing {
                    let _ = fs::remove_file(backup_path);
                }
                Ok(())
            }
            Err(error) => {
                if had_existing {
                    let _ = fs::rename(&backup_path, path);
                }
                Err(error.into())
            }
        }
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }
    result
}

pub(super) fn normalized_for_storage(mut config: RuntimeApplicationConfig) -> RuntimeApplicationConfig {
    config.schema_version = CURRENT_SCHEMA_VERSION;
    if config.build_engine.is_none() {
        config.build_engine = default_build_engine();
    }
    config
}

pub(super) fn normalized_loaded_config(
    mut config: RuntimeApplicationConfig,
    file_name: &str,
) -> RuntimeApplicationConfig {
    if config.name.trim().is_empty() {
        config.name = file_name.to_string();
    }
    if config.build_engine.is_none() {
        config.build_engine = default_build_engine();
    }
    config
}
