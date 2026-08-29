//! Runtime 环境变量合并（R-07，B-06 拆分）：System / Global / Workspace /
//! Runtime / Application 多层环境合并，以及 workspace 级环境文件读写。
//!
//! 合并顺序（优先级从低到高）：System < Global < Workspace < Runtime <
//! Application。Global 层存于 app data，Workspace 层存于
//! `.gitworkspace/environment.json`。

use std::collections::BTreeMap;
use std::path::Path;

use rusqlite::Connection;

use crate::error::{AppError, AppResult};

use super::repository::{get_summary, workspace_root};
use super::storage::{
    read_config_file, read_environment_file, workspace_environment_path, write_json_atomic,
    WorkspaceEnvironmentDocument,
};
use super::validation::{
    preserve_masked_environment, redact_environment, reject_symlink, validate_environment,
};

/// Environment layers are merged from low to high priority.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EnvironmentLayers {
    pub system: BTreeMap<String, String>,
    pub global: BTreeMap<String, String>,
    pub workspace: BTreeMap<String, String>,
    pub runtime: BTreeMap<String, String>,
    pub application: BTreeMap<String, String>,
}

pub fn merge_environment(layers: &EnvironmentLayers) -> BTreeMap<String, String> {
    let mut merged = BTreeMap::new();
    for source in [
        &layers.system,
        &layers.global,
        &layers.workspace,
        &layers.runtime,
        &layers.application,
    ] {
        merged.extend(
            source
                .iter()
                .map(|(key, value)| (key.clone(), value.clone())),
        );
    }
    merged
}

/// Resolve System < Global < Workspace < Runtime < Application precedence.
pub fn resolve_environment(
    conn: &Connection,
    workspace_id: i64,
    name: &str,
) -> AppResult<BTreeMap<String, String>> {
    let summary = get_summary(conn, workspace_id, name)?
        .ok_or_else(|| AppError::NotFound(format!("Runtime 配置 '{}' 不存在", name)))?;
    let config = read_config_file(Path::new(&summary.config_path), name)?;
    let root = workspace_root(conn, workspace_id)?;
    let workspace = read_environment_file(&workspace_environment_path(&root))?;
    let global =
        read_environment_file(&crate::get_app_data_dir().join("runtime-global-environment.json"))?;
    Ok(merge_environment(&EnvironmentLayers {
        system: std::env::vars().collect(),
        global,
        workspace,
        runtime: config.runtime_environment,
        application: config.environment,
    }))
}

pub fn get_workspace_environment(
    conn: &Connection,
    workspace_id: i64,
) -> AppResult<BTreeMap<String, String>> {
    let root = workspace_root(conn, workspace_id)?;
    Ok(redact_environment(read_environment_file(
        &workspace_environment_path(&root),
    )?))
}

pub fn set_workspace_environment(
    conn: &Connection,
    workspace_id: i64,
    environment: BTreeMap<String, String>,
) -> AppResult<()> {
    validate_environment(&environment)?;
    let root = workspace_root(conn, workspace_id)?;
    let path = workspace_environment_path(&root);
    reject_symlink(&root.join(".gitworkspace"))?;
    let existing = read_environment_file(&path)?;
    let environment = preserve_masked_environment(environment, &existing);
    write_json_atomic(&path, &WorkspaceEnvironmentDocument { environment })
}
