//! Runtime configuration model and persistence (R-07).
//!
//! SQLite stores the fast metadata index while the complete user-owned
//! configuration lives under the workspace runtime directory. File writes
//! happen first and are atomic; only after the file is durable do we update
//! the SQLite row.

use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::{AppError, AppResult};

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
}

fn default_schema_version() -> u32 {
    CURRENT_SCHEMA_VERSION
}

fn default_build_engine() -> Option<String> {
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

/// Create a Runtime config. The JSON document is written before the index row.
pub fn create_config(
    conn: &Connection,
    request: &CreateRuntimeConfigRequest,
) -> AppResult<RuntimeApplicationConfig> {
    request.config.validate()?;
    let root = workspace_root(conn, request.workspace_id)?;
    ensure_runtime_dir(&root)?;
    let path = config_path(&root, &request.config.name)?;
    if path.exists() {
        return Err(AppError::Conflict(format!(
            "Runtime 配置已存在：{}",
            path.display()
        )));
    }
    if runtime_name_exists(conn, request.workspace_id, &request.config.name)? {
        return Err(AppError::Conflict(format!(
            "工作区中已存在名为 '{}' 的 Runtime",
            request.config.name
        )));
    }

    let config = normalized_for_storage(request.config.clone());
    write_config_file(&path, &config)?;
    let now = Utc::now().to_rfc3339();
    if let Err(error) = insert_metadata(conn, request.workspace_id, &config, &path, &now) {
        let _ = fs::remove_file(&path);
        return Err(error);
    }
    Ok(redact_config(config))
}

/// List only SQLite metadata; no JSON file is opened on this hot path.
pub fn list_configs(conn: &Connection, workspace_id: i64) -> AppResult<Vec<RuntimeConfigSummary>> {
    // Validate the workspace id without opening any Runtime JSON files.
    let _ = workspace_root(conn, workspace_id)?;
    let mut stmt = conn.prepare(
        "SELECT id, workspace_id, name, project, main_class, jdk, profile,
                build_engine, config_path, created_at, updated_at
         FROM runtime_projects WHERE workspace_id = ?1 ORDER BY name COLLATE NOCASE",
    )?;
    let rows = stmt.query_map([workspace_id], |row| {
        Ok(RuntimeConfigSummary {
            id: row.get(0)?,
            workspace_id: row.get(1)?,
            name: row.get(2)?,
            project: row.get(3)?,
            main_class: row.get(4)?,
            jdk: row.get(5)?,
            profile: row.get(6)?,
            build_engine: row.get(7)?,
            config_path: row.get(8)?,
            created_at: row.get(9)?,
            updated_at: row.get(10)?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(AppError::from)
}

pub fn get_config(
    conn: &Connection,
    workspace_id: i64,
    name: &str,
) -> AppResult<RuntimeApplicationConfig> {
    let config = load_config_unredacted(conn, workspace_id, name)?;
    Ok(redact_config(config))
}

/// Internal engine load path (R-09 Build Engine): same read/validate/sync logic
/// as `get_config` but **without** redaction — the build pipeline needs the real
/// environment values to spawn Maven. The result must never cross IPC.
pub(crate) fn load_config_unredacted(
    conn: &Connection,
    workspace_id: i64,
    name: &str,
) -> AppResult<RuntimeApplicationConfig> {
    validate_runtime_name(name)?;
    let summary = get_summary(conn, workspace_id, name)?
        .ok_or_else(|| AppError::NotFound(format!("Runtime 配置 '{}' 不存在", name)))?;
    let mut config = read_config_file(Path::new(&summary.config_path), name)?;
    config = normalized_loaded_config(config, name);
    config.validate()?;
    sync_metadata_if_changed(conn, &summary, &config)?;
    Ok(config)
}

pub fn update_config(
    conn: &Connection,
    request: &UpdateRuntimeConfigRequest,
) -> AppResult<RuntimeApplicationConfig> {
    validate_runtime_name(&request.name)?;
    let current = get_summary(conn, request.workspace_id, &request.name)?
        .ok_or_else(|| AppError::NotFound(format!("Runtime 配置 '{}' 不存在", request.name)))?;
    let old_path = PathBuf::from(&current.config_path);
    reject_symlink(&old_path)?;
    let old_bytes = fs::read(&old_path).map_err(|error| {
        AppError::RuntimeConfig(format!(
            "无法读取 Runtime 配置文件 {}：{}。更新已取消",
            old_path.display(),
            error
        ))
    })?;
    let mut config = request.config.clone();
    if config.name.trim().is_empty() {
        config.name = request.name.clone();
    }
    config.validate()?;
    let root = workspace_root(conn, request.workspace_id)?;
    ensure_runtime_dir(&root)?;
    let new_path = config_path(&root, &config.name)?;
    if config.name != request.name && runtime_name_exists(conn, request.workspace_id, &config.name)?
    {
        return Err(AppError::Conflict(format!(
            "工作区中已存在名为 '{}' 的 Runtime",
            config.name
        )));
    }
    if old_path != new_path && new_path.exists() {
        return Err(AppError::Conflict(format!(
            "目标 Runtime 配置文件已存在：{}",
            new_path.display()
        )));
    }

    // Preserve redacted secrets when a UI sends the unchanged placeholder.
    let existing = read_config_file(&old_path, &request.name)?;
    config = preserve_masked_values(config, &existing);
    let stored = normalized_for_storage(config.clone());
    write_config_file(&new_path, &stored)?;

    let now = Utc::now().to_rfc3339();
    let root_project_id = resolve_root_project_id(conn, request.workspace_id, &config.project)?;
    let update_result = conn.execute(
        "UPDATE runtime_projects
         SET name = ?1, project = ?2, root_project_id = ?3, main_class = ?4,
             jdk = ?5, profile = ?6, build_engine = ?7, config_path = ?8,
             updated_at = ?9
         WHERE workspace_id = ?10 AND name = ?11",
        params![
            config.name,
            config.project,
            root_project_id,
            config.main_class,
            config.jdk,
            config.profile,
            config.build_engine,
            new_path.to_string_lossy().to_string(),
            now,
            request.workspace_id,
            request.name,
        ],
    );
    if let Err(error) = update_result {
        if old_path == new_path {
            let _ = write_bytes_atomic(&old_path, &old_bytes);
        } else {
            let _ = fs::remove_file(&new_path);
        }
        return Err(error.into());
    }
    if old_path != new_path {
        let _ = fs::remove_file(old_path);
    }
    Ok(redact_config(stored))
}

pub fn delete_config(conn: &Connection, workspace_id: i64, name: &str) -> AppResult<()> {
    validate_runtime_name(name)?;
    let summary = get_summary(conn, workspace_id, name)?
        .ok_or_else(|| AppError::NotFound(format!("Runtime 配置 '{}' 不存在", name)))?;
    let path = PathBuf::from(&summary.config_path);
    reject_symlink(&path)?;
    let backup = if path.is_file() {
        let bytes = fs::read(&path)?;
        fs::remove_file(&path)?;
        Some(bytes)
    } else {
        None
    };
    let result = conn.execute(
        "DELETE FROM runtime_projects WHERE workspace_id = ?1 AND name = ?2",
        params![workspace_id, name],
    );
    if let Err(error) = result {
        if let Some(bytes) = backup {
            let _ = write_bytes_atomic(&path, &bytes);
        }
        return Err(error.into());
    }
    Ok(())
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

fn default_runtime_dir(root: &Path) -> PathBuf {
    root.join(".gitworkspace").join("runtimes")
}

fn ensure_runtime_dir(root: &Path) -> AppResult<PathBuf> {
    let gitworkspace = root.join(".gitworkspace");
    reject_symlink(&gitworkspace)?;
    fs::create_dir_all(&gitworkspace)?;
    let runtimes = gitworkspace.join("runtimes");
    reject_symlink(&runtimes)?;
    fs::create_dir_all(&runtimes)?;
    Ok(runtimes)
}

fn reject_symlink(path: &Path) -> AppResult<()> {
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

fn workspace_environment_path(root: &Path) -> PathBuf {
    root.join(".gitworkspace").join("environment.json")
}

pub(crate) fn workspace_root(conn: &Connection, workspace_id: i64) -> AppResult<PathBuf> {
    let path: Option<String> = conn
        .query_row(
            "SELECT path FROM workspaces WHERE id = ?1",
            [workspace_id],
            |row| row.get(0),
        )
        .optional()?;
    let path = path.ok_or_else(|| {
        AppError::ProjectNotFound(format!("workspace id={} 不存在", workspace_id))
    })?;
    let root = PathBuf::from(path);
    if !root.is_dir() {
        return Err(AppError::ProjectNotFound(format!(
            "Workspace 目录不存在：{}",
            root.display()
        )));
    }
    Ok(root)
}

fn config_path(root: &Path, name: &str) -> AppResult<PathBuf> {
    validate_runtime_name(name)?;
    Ok(default_runtime_dir(root).join(format!("{name}.json")))
}

fn validate_runtime_name(name: &str) -> AppResult<()> {
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

fn validate_environment(environment: &BTreeMap<String, String>) -> AppResult<()> {
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

fn runtime_name_exists(conn: &Connection, workspace_id: i64, name: &str) -> AppResult<bool> {
    let exists: i64 = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM runtime_projects WHERE workspace_id = ?1 AND name = ?2)",
        params![workspace_id, name],
        |row| row.get(0),
    )?;
    Ok(exists != 0)
}

fn resolve_root_project_id(
    conn: &Connection,
    workspace_id: i64,
    project: &str,
) -> AppResult<Option<i64>> {
    conn.query_row(
        "SELECT id FROM maven_projects
         WHERE workspace_id = ?1 AND (path = ?2 OR artifact_id = ?2)
         ORDER BY CASE WHEN path = ?2 THEN 0 ELSE 1 END, id LIMIT 1",
        params![workspace_id, project],
        |row| row.get(0),
    )
    .optional()
    .map_err(AppError::from)
}

fn insert_metadata(
    conn: &Connection,
    workspace_id: i64,
    config: &RuntimeApplicationConfig,
    path: &Path,
    now: &str,
) -> AppResult<i64> {
    let root_project_id = resolve_root_project_id(conn, workspace_id, &config.project)?;
    conn.execute(
        "INSERT INTO runtime_projects (
            workspace_id, name, project, root_project_id, main_class, jdk, profile,
            build_engine, config_path, created_at, updated_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10)",
        params![
            workspace_id,
            config.name,
            config.project,
            root_project_id,
            config.main_class,
            config.jdk,
            config.profile,
            config.build_engine,
            path.to_string_lossy().to_string(),
            now,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

fn get_summary(
    conn: &Connection,
    workspace_id: i64,
    name: &str,
) -> AppResult<Option<RuntimeConfigSummary>> {
    conn.query_row(
        "SELECT id, workspace_id, name, project, main_class, jdk, profile,
                build_engine, config_path, created_at, updated_at
         FROM runtime_projects WHERE workspace_id = ?1 AND name = ?2",
        params![workspace_id, name],
        |row| {
            Ok(RuntimeConfigSummary {
                id: row.get(0)?,
                workspace_id: row.get(1)?,
                name: row.get(2)?,
                project: row.get(3)?,
                main_class: row.get(4)?,
                jdk: row.get(5)?,
                profile: row.get(6)?,
                build_engine: row.get(7)?,
                config_path: row.get(8)?,
                created_at: row.get(9)?,
                updated_at: row.get(10)?,
            })
        },
    )
    .optional()
    .map_err(AppError::from)
}

fn sync_metadata_if_changed(
    conn: &Connection,
    summary: &RuntimeConfigSummary,
    config: &RuntimeApplicationConfig,
) -> AppResult<()> {
    if summary.project == config.project
        && summary.main_class == config.main_class
        && summary.jdk == config.jdk
        && summary.profile == config.profile
        && summary.build_engine == config.build_engine
    {
        return Ok(());
    }
    conn.execute(
        "UPDATE runtime_projects
         SET project = ?1, main_class = ?2, jdk = ?3, profile = ?4,
             build_engine = ?5, updated_at = ?6
         WHERE id = ?7",
        params![
            config.project,
            config.main_class,
            config.jdk,
            config.profile,
            config.build_engine,
            Utc::now().to_rfc3339(),
            summary.id,
        ],
    )?;
    Ok(())
}

fn normalized_for_storage(mut config: RuntimeApplicationConfig) -> RuntimeApplicationConfig {
    config.schema_version = CURRENT_SCHEMA_VERSION;
    if config.build_engine.is_none() {
        config.build_engine = default_build_engine();
    }
    config
}

fn normalized_loaded_config(
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

fn preserve_masked_values(
    mut incoming: RuntimeApplicationConfig,
    existing: &RuntimeApplicationConfig,
) -> RuntimeApplicationConfig {
    incoming.environment = preserve_masked_environment(incoming.environment, &existing.environment);
    incoming.runtime_environment =
        preserve_masked_environment(incoming.runtime_environment, &existing.runtime_environment);
    incoming
}

fn redact_config(mut config: RuntimeApplicationConfig) -> RuntimeApplicationConfig {
    config.environment = redact_environment(config.environment);
    config.runtime_environment = redact_environment(config.runtime_environment);
    config
}

fn redact_environment(environment: BTreeMap<String, String>) -> BTreeMap<String, String> {
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

fn preserve_masked_environment(
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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorkspaceEnvironmentDocument {
    #[serde(default)]
    environment: BTreeMap<String, String>,
}

fn read_environment_file(path: &Path) -> AppResult<BTreeMap<String, String>> {
    reject_symlink(path)?;
    if !path.exists() {
        return Ok(BTreeMap::new());
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

fn read_config_file(path: &Path, expected_name: &str) -> AppResult<RuntimeApplicationConfig> {
    let content = read_file(path)?;
    let config = serde_json::from_str::<RuntimeApplicationConfig>(&content)
        .map_err(|error| invalid_json_error(path, error))?;
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

fn read_file(path: &Path) -> AppResult<String> {
    reject_symlink(path)?;
    let mut file = File::open(path).map_err(|error| {
        AppError::RuntimeConfig(format!(
            "无法读取 Runtime 配置文件 {}：{}。请确认文件存在且可读",
            path.display(),
            error
        ))
    })?;
    let mut content = String::new();
    file.read_to_string(&mut content).map_err(|error| {
        AppError::RuntimeConfig(format!(
            "无法读取 Runtime 配置文件 {}：{}",
            path.display(),
            error
        ))
    })?;
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

fn write_config_file(path: &Path, config: &RuntimeApplicationConfig) -> AppResult<()> {
    write_json_atomic(path, config)
}

pub(crate) fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> AppResult<()> {
    let mut bytes = serde_json::to_vec_pretty(value)?;
    bytes.push(b'\n');
    write_bytes_atomic(path, &bytes)
}

fn write_bytes_atomic(path: &Path, bytes: &[u8]) -> AppResult<()> {
    reject_symlink(path)?;
    let parent = path.parent().ok_or_else(|| {
        AppError::RuntimeConfig(format!("配置路径没有父目录：{}", path.display()))
    })?;
    fs::create_dir_all(parent)?;
    let file_name = path.file_name().unwrap_or_default().to_string_lossy();
    let temp_path = parent.join(format!(".{file_name}.tmp-{}", Uuid::new_v4()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp_path)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    fn open_db() -> (Connection, PathBuf) {
        let mut conn = Connection::open_in_memory().unwrap();
        db::init_db(&mut conn).unwrap();
        let root = std::env::temp_dir().join(format!("gw_runtime_config_{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        conn.execute(
            "INSERT INTO workspaces (name, path, created_at, updated_at) VALUES ('w', ?1, 't', 't')",
            [root.to_string_lossy().to_string()],
        )
        .unwrap();
        (conn, root)
    }

    fn sample(name: &str) -> RuntimeApplicationConfig {
        RuntimeApplicationConfig {
            name: name.into(),
            project: "repo-boot".into(),
            main_class: Some("com.example.Application".into()),
            jdk: Some("21".into()),
            profile: Some("dev".into()),
            vm_options: vec!["-Xmx1g".into()],
            program_arguments: vec!["--server.port=8080".into()],
            environment: BTreeMap::from([
                ("SERVER_PORT".into(), "8080".into()),
                ("DB_PASSWORD".into(), "secret".into()),
            ]),
            runtime_environment: BTreeMap::from([("RUNTIME_FLAG".into(), "on".into())]),
            ..Default::default()
        }
    }

    #[test]
    fn crud_round_trip_uses_json_for_full_config_and_sqlite_for_list() {
        let (conn, root) = open_db();
        let request = CreateRuntimeConfigRequest {
            workspace_id: 1,
            config: sample("boot"),
        };
        let created = create_config(&conn, &request).unwrap();
        assert_eq!(created.environment["DB_PASSWORD"], MASKED_VALUE);
        let path = root.join(".gitworkspace/runtimes/boot.json");
        assert!(path.is_file());

        let summaries = list_configs(&conn, 1).unwrap();
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].project, "repo-boot");

        let loaded = get_config(&conn, 1, "boot").unwrap();
        assert_eq!(loaded.environment["SERVER_PORT"], "8080");
        assert_eq!(loaded.environment["DB_PASSWORD"], MASKED_VALUE);

        delete_config(&conn, 1, "boot").unwrap();
        assert!(!path.exists());
        assert!(list_configs(&conn, 1).unwrap().is_empty());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn update_preserves_masked_secret_and_supports_rename() {
        let (conn, root) = open_db();
        create_config(
            &conn,
            &CreateRuntimeConfigRequest {
                workspace_id: 1,
                config: sample("old"),
            },
        )
        .unwrap();
        let mut update = sample("new");
        update
            .environment
            .insert("DB_PASSWORD".into(), MASKED_VALUE.into());
        update
            .environment
            .insert("SERVER_PORT".into(), "9090".into());
        update_config(
            &conn,
            &UpdateRuntimeConfigRequest {
                workspace_id: 1,
                name: "old".into(),
                config: update,
            },
        )
        .unwrap();
        let raw = read_file(&root.join(".gitworkspace/runtimes/new.json")).unwrap();
        assert!(raw.contains("secret"));
        assert!(!root.join(".gitworkspace/runtimes/old.json").exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn environment_layers_merge_application_over_runtime_workspace_global_system() {
        let layers = EnvironmentLayers {
            system: BTreeMap::from([("A".into(), "system".into())]),
            global: BTreeMap::from([("A".into(), "global".into()), ("G".into(), "1".into())]),
            workspace: BTreeMap::from([("A".into(), "workspace".into())]),
            runtime: BTreeMap::from([("A".into(), "runtime".into())]),
            application: BTreeMap::from([("A".into(), "application".into())]),
        };
        let merged = merge_environment(&layers);
        assert_eq!(merged["A"], "application");
        assert_eq!(merged["G"], "1");
    }

    #[test]
    fn malformed_json_reports_path_line_and_column() {
        let (conn, root) = open_db();
        create_config(
            &conn,
            &CreateRuntimeConfigRequest {
                workspace_id: 1,
                config: sample("broken"),
            },
        )
        .unwrap();
        let path = root.join(".gitworkspace/runtimes/broken.json");
        fs::write(&path, "{\n  \"name\": \"broken\",\n  \"project\": ").unwrap();
        let error = get_config(&conn, 1, "broken").unwrap_err();
        let message = error.to_string();
        assert!(message.contains("broken.json"));
        assert!(message.contains("第"));
        assert!(message.contains("列"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn old_json_defaults_new_fields_and_profiles_support_both_injection_forms() {
        let old: RuntimeApplicationConfig =
            serde_json::from_str(r#"{"name":"boot","project":"repo-boot","profile":"dev"}"#)
                .unwrap();
        assert_eq!(old.schema_version, CURRENT_SCHEMA_VERSION);
        assert!(old.vm_options.is_empty());
        assert_eq!(
            old.with_default_profile_injection().vm_options,
            vec!["-Dspring.profiles.active=dev"]
        );

        let mut args = old.clone();
        args.program_arguments = vec!["--spring.profiles.active=test".into()];
        assert_eq!(args.injected_profile().as_deref(), Some("test"));
        args.program_arguments.clear();
        args.vm_options = vec!["-Dspring.profiles.active=prod".into()];
        assert_eq!(args.injected_profile().as_deref(), Some("prod"));
    }
}
