//! Maven 执行策略 IPC 命令层（R-05，§18 / §19）。
//!
//! 暴露 Maven 检测与执行封装 API 给前端 Settings UI 与 Build Engine：
//! - `detect_maven`：对项目跑优先级链检测 + `mvn -v` 探测，返回 `ResolvedMaven`。
//! - `list_maven_executables` / `get_maven_executable` / `remove_maven_executable`：注册表 CRUD。
//! - `resolve_maven`：对项目解析最终生效 Maven（不强制重新探测，用缓存）。
//! - `preview_maven_command`：预览完整命令行（§75 可追溯）。
//! - `resolve_local_repo`：探测生效本地仓库路径（settings.xml 覆盖）。
//! - `validate_maven_executable`：强制复检单条（fork `mvn -v`）。
//! - `prune_invalid_maven`：惰性校验，把路径已不存在的条目标记失效。
//!
//! 复用 T-03 SQLite 数据层（单写者 / 事务），不另起存储（全局约束 §7）。
//! 检测惰性：`detect` 只在显式调用时 fork；`prune` 只检路径存在性，
//! 禁止每次启动全量重扫（§性能）。

use std::path::{Path, PathBuf};

use rusqlite::Connection;
use tauri::State;

use crate::error::{AppError, AppResult};
use crate::maven::detect_exec::{detect_maven_candidates, probe_version};
use crate::maven::exec_model::{MavenExecutable, MavenExecutionRequest, ResolvedMaven};
use crate::maven::executor::{build_command, preview_command};
use crate::maven::registry::{
    apply_version as apply_maven_version, get_maven_executable, list_maven_executables,
    mark_validity as mark_maven_validity, prune_invalid_paths, remove_maven_executable,
    upsert_maven_executable,
};
use crate::maven::settings::resolve_local_repository;
use crate::state::AppState;

fn lock_db<'r>(state: &'r State<'_, AppState>) -> AppResult<std::sync::MutexGuard<'r, Connection>> {
    state
        .db
        .lock()
        .map_err(|e| AppError::Other(format!("DB lock error: {}", e)))
}

/// 对项目跑优先级链检测 + `mvn -v` 探测，返回 `ResolvedMaven` 并缓存。
///
/// `project_dir` 为项目根（`pom.xml` 所在目录）；`configured_path` 为用户配置
/// 的 Maven 路径（可选）。任一候选探测成功即返回；全部失败返回 `MavenNotFound`
/// 可行动错误（指向 Settings 页）。
#[tauri::command]
pub fn detect_maven(
    state: State<'_, AppState>,
    project_dir: String,
    configured_path: Option<String>,
) -> AppResult<ResolvedMaven> {
    let project = Path::new(&project_dir);
    if !project.is_dir() {
        return Err(AppError::MavenNotFound(format!(
            "项目目录不存在：{project_dir}。请在 Settings 中配置正确的项目路径。"
        )));
    }
    let local_repo = resolve_local_repository(None);
    let resolved = crate::maven::detect_exec::resolve_maven_for_project(
        project,
        configured_path.as_deref(),
        &local_repo,
    );
    let resolved = resolved.ok_or_else(|| {
        AppError::MavenNotFound(format!(
            "未在项目 {project_dir} 找到可用的 Maven（wrapper / 配置 / 系统三者皆缺）。\
             请安装 Maven 或在 Settings 中配置 Maven 可执行路径。"
        ))
    })?;

    // 缓存探测结果到注册表。
    let conn = lock_db(&state)?;
    upsert_maven_executable(&conn, &resolved.executable)?;
    Ok(resolved)
}

/// 列出注册表全部 Maven 可执行体（有效优先、source 优先级升序）。
#[tauri::command]
pub fn list_maven_executables_cmd(state: State<'_, AppState>) -> AppResult<Vec<MavenExecutable>> {
    let conn = lock_db(&state)?;
    list_maven_executables(&conn)
}

/// 按 id 取单条 Maven 可执行体。
#[tauri::command]
pub fn get_maven_executable_cmd(
    state: State<'_, AppState>,
    id: i64,
) -> AppResult<Option<MavenExecutable>> {
    let conn = lock_db(&state)?;
    get_maven_executable(&conn, id)
}

/// 强制复检单条 Maven：重新 fork `mvn -v` 并更新版本字段与有效性。
#[tauri::command]
pub fn validate_maven_executable(
    state: State<'_, AppState>,
    id: i64,
) -> AppResult<MavenExecutable> {
    let existing = {
        let conn = lock_db(&state)?;
        get_maven_executable(&conn, id)?.ok_or_else(|| {
            AppError::MavenNotFound(format!("Maven 可执行体 id={id} 不在注册表中"))
        })?
    };

    let now = chrono::Utc::now().to_rfc3339();
    let path = Path::new(&existing.executable_path);
    if !path.is_file() {
        let conn = lock_db(&state)?;
        mark_maven_validity(&conn, id, false, &now)?;
        return existing_after_update(&conn, id);
    }
    let (info, is_valid) = probe_version(&existing.executable_path);
    let mut probed = existing.clone();
    probed.major_version = info.major_version;
    probed.full_version = info.full_version;
    probed.is_valid = is_valid;
    probed.last_checked = now.clone();
    probed.raw_version = if info.raw.is_empty() {
        None
    } else {
        Some(info.raw)
    };
    let conn = lock_db(&state)?;
    apply_maven_version(&conn, id, &probed, &now)?;
    Ok(probed)
}

/// 惰性校验：把 `executable_path` 已不存在的条目标记 `is_valid=false`。
/// 返回被标记失效的条数（与 R-04 `prune_invalid_jdks` 同策略）。
#[tauri::command]
pub fn prune_invalid_maven(state: State<'_, AppState>) -> AppResult<usize> {
    let mut conn = lock_db(&state)?;
    prune_invalid_paths(&mut conn)
}

/// 按 id 删除单条 Maven 可执行体。
#[tauri::command]
pub fn remove_maven_executable_cmd(state: State<'_, AppState>, id: i64) -> AppResult<()> {
    let conn = lock_db(&state)?;
    remove_maven_executable(&conn, id)
}

/// 探测生效本地仓库路径（settings.xml 覆盖 `~/.m2/repository`）。
#[tauri::command]
pub fn resolve_local_repo(global_settings_path: Option<String>) -> AppResult<PathBuf> {
    Ok(resolve_local_repository(global_settings_path.as_deref().map(Path::new)))
}

/// 预览 Maven 命令行（§75 可追溯）：给定请求结构，返回完整命令字符串。
#[tauri::command]
pub fn preview_maven_command(req: MavenExecutionRequest) -> AppResult<String> {
    Ok(preview_command(&req))
}

/// 仅跑优先级链检测（不 fork `mvn -v`），返回候选列表。供 UI 展示备选来源。
#[tauri::command]
pub fn list_maven_candidates(
    project_dir: Option<String>,
    configured_path: Option<String>,
) -> AppResult<Vec<MavenExecutable>> {
    Ok(detect_maven_candidates(
        project_dir.as_deref().map(Path::new),
        configured_path.as_deref(),
    ))
}

/// 构造完整命令行数组（供 R-09 Build Engine 直接 spawn，确认流在 R-09）。
#[tauri::command]
pub fn build_maven_command(req: MavenExecutionRequest) -> AppResult<Vec<String>> {
    Ok(build_command(&req))
}

fn existing_after_update(conn: &Connection, id: i64) -> AppResult<MavenExecutable> {
    get_maven_executable(conn, id)?.ok_or_else(|| {
        AppError::MavenNotFound(format!("Maven 可执行体 id={id} 不在注册表中"))
    })
}
