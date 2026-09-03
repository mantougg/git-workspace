//! Runtime 配置仓储（R-07，B-06 拆分）：SQLite 元数据索引与配置生命周期
//! （创建 / 列表 / 读取 / 更新 / 删除）。
//!
//! 持久化边界（不变）：JSON 文件先写（原子写），成功后才更新 SQLite 行；
//! 元数据更新失败时回滚文件（删除新文件或恢复旧文件字节）。

use std::fs;
use std::path::{Path, PathBuf};

use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};

use crate::error::{AppError, AppResult};

use super::model::{
    CreateRuntimeConfigRequest, RuntimeApplicationConfig, RuntimeConfigSummary, RuntimeKind, UpdateRuntimeConfigRequest,
};
use super::storage::{
    config_path, ensure_runtime_dir, normalized_for_storage, normalized_loaded_config, read_config_file,
    write_bytes_atomic, write_config_file,
};
use super::validation::{preserve_masked_values, redact_config, reject_symlink, validate_runtime_name};

/// Create a Runtime config. The JSON document is written before the index row.
pub fn create_config(conn: &Connection, request: &CreateRuntimeConfigRequest) -> AppResult<RuntimeApplicationConfig> {
    request.config.validate()?;
    let root = workspace_root(conn, request.workspace_id)?;
    validate_for_workspace(conn, request.workspace_id, &request.config)?;
    ensure_runtime_dir(&root)?;
    let path = config_path(&root, &request.config.name)?;
    if path.exists() {
        return Err(AppError::Conflict(format!("Runtime 配置已存在：{}", path.display())));
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
        "SELECT id, workspace_id, name, project, kind, main_class, jdk, profile,
                build_engine, config_path, created_at, updated_at
         FROM runtime_projects WHERE workspace_id = ?1 ORDER BY name COLLATE NOCASE",
    )?;
    let rows = stmt.query_map([workspace_id], |row| {
        Ok(RuntimeConfigSummary {
            id: row.get(0)?,
            workspace_id: row.get(1)?,
            name: row.get(2)?,
            project: row.get(3)?,
            kind: parse_runtime_kind(row.get::<_, String>(4)?),
            main_class: row.get(5)?,
            jdk: row.get(6)?,
            profile: row.get(7)?,
            build_engine: row.get(8)?,
            config_path: row.get(9)?,
            created_at: row.get(10)?,
            updated_at: row.get(11)?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(AppError::from)
}

pub fn get_config(conn: &Connection, workspace_id: i64, name: &str) -> AppResult<RuntimeApplicationConfig> {
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
    validate_for_workspace(conn, workspace_id, &config)?;
    sync_metadata_if_changed(conn, &summary, &config)?;
    Ok(config)
}

pub fn update_config(conn: &Connection, request: &UpdateRuntimeConfigRequest) -> AppResult<RuntimeApplicationConfig> {
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
    validate_for_workspace(conn, request.workspace_id, &config)?;
    ensure_runtime_dir(&root)?;
    let new_path = config_path(&root, &config.name)?;
    if config.name != request.name && runtime_name_exists(conn, request.workspace_id, &config.name)? {
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
         SET name = ?1, project = ?2, kind = ?3, root_project_id = ?4, main_class = ?5,
             jdk = ?6, profile = ?7, build_engine = ?8, config_path = ?9,
             updated_at = ?10
         WHERE workspace_id = ?11 AND name = ?12",
        params![
            config.name,
            config.project,
            runtime_kind_name(config.kind),
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

pub(crate) fn workspace_root(conn: &Connection, workspace_id: i64) -> AppResult<PathBuf> {
    let path: Option<String> = conn
        .query_row("SELECT path FROM workspaces WHERE id = ?1", [workspace_id], |row| {
            row.get(0)
        })
        .optional()?;
    let path = path.ok_or_else(|| AppError::ProjectNotFound(format!("workspace id={} 不存在", workspace_id)))?;
    let root = PathBuf::from(path);
    if !root.is_dir() {
        return Err(AppError::ProjectNotFound(format!(
            "Workspace 目录不存在：{}",
            root.display()
        )));
    }
    Ok(root)
}

fn runtime_name_exists(conn: &Connection, workspace_id: i64, name: &str) -> AppResult<bool> {
    let exists: i64 = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM runtime_projects WHERE workspace_id = ?1 AND name = ?2)",
        params![workspace_id, name],
        |row| row.get(0),
    )?;
    Ok(exists != 0)
}

fn resolve_root_project_id(conn: &Connection, workspace_id: i64, project: &str) -> AppResult<Option<i64>> {
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
            workspace_id, name, project, kind, root_project_id, main_class, jdk, profile,
            build_engine, config_path, created_at, updated_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?11)",
        params![
            workspace_id,
            config.name,
            config.project,
            runtime_kind_name(config.kind),
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

pub(super) fn get_summary(conn: &Connection, workspace_id: i64, name: &str) -> AppResult<Option<RuntimeConfigSummary>> {
    conn.query_row(
        "SELECT id, workspace_id, name, project, kind, main_class, jdk, profile,
                build_engine, config_path, created_at, updated_at
         FROM runtime_projects WHERE workspace_id = ?1 AND name = ?2",
        params![workspace_id, name],
        |row| {
            Ok(RuntimeConfigSummary {
                id: row.get(0)?,
                workspace_id: row.get(1)?,
                name: row.get(2)?,
                project: row.get(3)?,
                kind: parse_runtime_kind(row.get::<_, String>(4)?),
                main_class: row.get(5)?,
                jdk: row.get(6)?,
                profile: row.get(7)?,
                build_engine: row.get(8)?,
                config_path: row.get(9)?,
                created_at: row.get(10)?,
                updated_at: row.get(11)?,
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
        && summary.kind == config.kind
    {
        return Ok(());
    }
    conn.execute(
        "UPDATE runtime_projects
         SET project = ?1, kind = ?2, main_class = ?3, jdk = ?4, profile = ?5,
             build_engine = ?6, updated_at = ?7
         WHERE id = ?8",
        params![
            config.project,
            runtime_kind_name(config.kind),
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

fn runtime_kind_name(kind: RuntimeKind) -> &'static str {
    match kind {
        RuntimeKind::SpringBoot => "springBoot",
        RuntimeKind::Node => "node",
    }
}

fn parse_runtime_kind(value: String) -> RuntimeKind {
    if value.eq_ignore_ascii_case("node") {
        RuntimeKind::Node
    } else {
        RuntimeKind::SpringBoot
    }
}

/// Validate Node script references against the indexed manifest, with a disk
/// fallback when the discovery index is stale or not yet populated.
fn validate_for_workspace(conn: &Connection, workspace_id: i64, config: &RuntimeApplicationConfig) -> AppResult<()> {
    if config.kind != RuntimeKind::Node {
        return Ok(());
    }
    let script = config.node_script.as_deref().unwrap_or_default().trim();
    let project_key = normalize_path(Path::new(&config.project));
    let indexed = {
        let mut stmt = conn.prepare("SELECT path, scripts_json FROM node_projects WHERE workspace_id = ?1")?;
        let rows = stmt.query_map([workspace_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        let mut value = None;
        for row in rows {
            let (path, json) = row?;
            if normalize_path(Path::new(&path)) == project_key {
                value = Some(json);
                break;
            }
        }
        value.and_then(|json| serde_json::from_str::<serde_json::Value>(&json).ok())
    };
    let scripts = indexed.or_else(|| read_scripts_from_disk(conn, workspace_id, &config.project));
    let available = scripts
        .as_ref()
        .and_then(serde_json::Value::as_object)
        .map(|object| {
            let mut names = object.keys().cloned().collect::<Vec<_>>();
            names.sort();
            names
        })
        .unwrap_or_default();
    if scripts
        .as_ref()
        .and_then(serde_json::Value::as_object)
        .map(|object| !object.contains_key(script))
        .unwrap_or(true)
    {
        return Err(AppError::ScriptNotFound {
            project: config.project.clone(),
            script: Some(script.to_string()),
            available,
        });
    }
    Ok(())
}

fn read_scripts_from_disk(conn: &Connection, workspace_id: i64, project: &str) -> Option<serde_json::Value> {
    let root = workspace_root(conn, workspace_id).ok()?;
    let mut path = PathBuf::from(project);
    if !path.is_absolute() {
        path = root.join(path);
    }
    let package = if path.is_file() {
        path
    } else {
        path.join("package.json")
    };
    let value: serde_json::Value = serde_json::from_slice(&fs::read(package).ok()?).ok()?;
    value.get("scripts").cloned()
}

fn normalize_path(path: &Path) -> String {
    let canonical = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let value = canonical.to_string_lossy();
    let value = value
        .strip_prefix(r"\\?\UNC\")
        .map(|rest| format!(r"\\{rest}"))
        .or_else(|| value.strip_prefix(r"\\?\").map(str::to_string))
        .unwrap_or_else(|| value.to_string());
    let value = value.replace('\\', "/");
    if cfg!(windows) {
        value.to_ascii_lowercase()
    } else {
        value
    }
}
