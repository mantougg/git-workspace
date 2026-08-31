//! Node.js project discovery IPC (N-02).

use tauri::{command, State};

use crate::error::{AppError, AppResult};
use crate::node::{
    discover_package_jsons, global_package_cache, sync_node_projects, NodeExecutable,
    NodeExecutableKind, NodeExecutableRequest, NodeProjectNode, PackageManager,
};
use crate::runtime::config::workspace_root;
use crate::state::AppState;
use crate::models::task::{TaskRequest, TaskType};

/// Discover and index workspace `package.json` files, then return the hot-path
/// SQLite list. The workspace path and scan depth are read from the DB so the
/// command cannot escape the configured workspace boundary.
#[command]
pub fn node_list_projects(
    workspace_id: i64,
    state: State<'_, AppState>,
) -> AppResult<Vec<NodeProjectNode>> {
    let (root, scan_depth) = {
        let conn = state
            .db
            .lock()
            .map_err(|error| AppError::Other(format!("DB lock error: {error}")))?;
        let root = workspace_root(&conn, workspace_id)?;
        let depth: i64 = conn.query_row(
            "SELECT scan_depth FROM workspaces WHERE id = ?1",
            [workspace_id],
            |row| row.get(0),
        )?;
        (root, depth.max(1) as usize)
    };

    let discovery = discover_package_jsons(&root, scan_depth, Some(global_package_cache()), None);
    let mut conn = state
        .db
        .lock()
        .map_err(|error| AppError::Other(format!("DB lock error: {error}")))?;
    sync_node_projects(&mut conn, workspace_id, &discovery)
}

/// List user-registered Node.js and package-manager executables.
#[command]
pub fn node_list_executables(state: State<'_, AppState>) -> AppResult<Vec<NodeExecutable>> {
    let conn = state
        .db
        .lock()
        .map_err(|error| AppError::Other(format!("DB lock error: {error}")))?;
    crate::node::registry::list_node_executables(&conn)
}

/// Register a concrete executable path after a version probe. Registration is
/// explicit and never changes the user's PATH or project files.
#[command]
pub fn node_add_executable(
    request: NodeExecutableRequest,
    state: State<'_, AppState>,
) -> AppResult<NodeExecutable> {
    let path = std::path::Path::new(request.executable_path.trim());
    if !path.is_file() {
        return Err(AppError::Other(format!(
            "Node 可执行文件不存在或不是文件：{}。请选择 node、npm、pnpm 或 yarn 的实际可执行路径。",
            request.executable_path
        )));
    }
    match request.kind {
        NodeExecutableKind::Node if request.package_manager.is_some() => {
            return Err(AppError::Other(
                "Node 注册条目不能携带 packageManager".into(),
            ));
        }
        NodeExecutableKind::PackageManager if request.package_manager.is_none() => {
            return Err(AppError::Other(
                "包管理器注册条目必须指定 packageManager".into(),
            ));
        }
        _ => {}
    }
    if request.package_manager == Some(PackageManager::Bun) {
        return Err(AppError::PackageManagerNotFound(
            "当前版本不支持注册 bun 执行链；请注册 npm、pnpm 或 yarn".into(),
        ));
    }
    let canonical = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let mut entry = NodeExecutable::new(
        request.kind,
        request.package_manager,
        canonical.to_string_lossy().to_string(),
    );
    let detection = crate::node::detect::probe_tool(&canonical);
    entry.version = detection.version;
    entry.raw_output = detection.raw_output;
    entry.is_valid = detection.probe_ok;
    entry.last_checked = chrono::Utc::now().to_rfc3339();
    if !entry.is_valid {
        return Err(AppError::Other(format!(
            "{} 版本探测失败，请确认路径指向可执行文件而非目录或 shell 脚本。",
            canonical.display()
        )));
    }
    let conn = state
        .db
        .lock()
        .map_err(|error| AppError::Other(format!("DB lock error: {error}")))?;
    let id = crate::node::registry::upsert_node_executable(&conn, &entry)?;
    entry.id = Some(id);
    Ok(entry)
}

/// Re-probe one registered executable and update its cached version/validity.
#[command]
pub fn node_validate_executable(
    id: i64,
    state: State<'_, AppState>,
) -> AppResult<NodeExecutable> {
    let existing = {
        let conn = state
            .db
            .lock()
            .map_err(|error| AppError::Other(format!("DB lock error: {error}")))?;
        crate::node::registry::get_node_executable(&conn, id)?.ok_or_else(|| {
            AppError::NotFound(format!("Node executable id={id} 不在注册表中"))
        })?
    };
    let mut updated = existing.clone();
    let path = std::path::Path::new(&existing.executable_path);
    if path.is_file() {
        let detection = crate::node::detect::probe_tool(path);
        updated.version = detection.version;
        updated.raw_output = detection.raw_output;
        updated.is_valid = detection.probe_ok;
    } else {
        updated.is_valid = false;
        updated.raw_output = "executable path no longer exists".into();
    }
    updated.last_checked = chrono::Utc::now().to_rfc3339();
    let conn = state
        .db
        .lock()
        .map_err(|error| AppError::Other(format!("DB lock error: {error}")))?;
    crate::node::registry::apply_node_probe(&conn, id, &updated, &updated.last_checked)?;
    Ok(updated)
}

#[command]
pub fn node_remove_executable(id: i64, state: State<'_, AppState>) -> AppResult<()> {
    let conn = state
        .db
        .lock()
        .map_err(|error| AppError::Other(format!("DB lock error: {error}")))?;
    crate::node::registry::remove_node_executable(&conn, id)
}

#[command]
pub fn node_prune_executables(state: State<'_, AppState>) -> AppResult<usize> {
    let mut conn = state
        .db
        .lock()
        .map_err(|error| AppError::Other(format!("DB lock error: {error}")))?;
    crate::node::registry::prune_invalid_paths(&mut conn)
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeInstallRequest {
    pub project_dir: String,
    pub package_manager: PackageManager,
    #[serde(default)]
    pub confirmed: bool,
}

/// Submit an explicit dependency installation. A first call without
/// confirmation only returns a structured confirmation error.
#[command]
pub fn node_install(
    request: NodeInstallRequest,
    state: State<'_, AppState>,
) -> AppResult<String> {
    let project = std::path::Path::new(request.project_dir.trim());
    if !project.is_dir() || !project.join("package.json").is_file() {
        return Err(AppError::ProjectNotFound(format!(
            "Node 项目目录无效：{}（需要包含 package.json）",
            request.project_dir
        )));
    }
    if request.package_manager == PackageManager::Bun {
        return Err(AppError::PackageManagerNotFound(
            "当前版本不支持 bun install；请改选 npm、pnpm 或 yarn".into(),
        ));
    }
    let project = std::fs::canonicalize(project).unwrap_or_else(|_| project.to_path_buf());
    let preview = format!(
        "{} install (cwd {})",
        request.package_manager.name(),
        project.display()
    );
    if !request.confirmed {
        return Err(AppError::NodeInstallConfirmationRequired {
            project_dir: project.to_string_lossy().into_owned(),
            package_manager: request.package_manager.name().into(),
            command_preview: preview,
        });
    }
    let task = TaskRequest {
        task_type: TaskType::NodeInstall {
            project_dir: project.to_string_lossy().into_owned(),
            package_manager: request.package_manager,
        },
        repo_path: project.to_string_lossy().into_owned(),
        repo_name: project
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Node install".into()),
    };
    state
        .task_manager
        .submit(&[task])?
        .into_iter()
        .next()
        .ok_or_else(|| AppError::Task("node_install 任务提交失败：未返回任务 id".into()))
}
