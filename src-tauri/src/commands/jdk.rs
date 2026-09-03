//! JDK Manager IPC 命令层（R-04，§31 / §32）。
//!
//! 暴露 JDK 注册表 API 给前端 Settings UI：
//! - `discover_jdks`：触发多来源发现 + `java -version` 探测，批量 upsert。
//! - `list_jdks` / `get_jdk` / `remove_jdk`：注册表 CRUD。
//! - `add_jdk_manual`：用户手选 JDK 根目录，校验通过才入库（`JdkNotFound`）。
//! - `validate_jdk`：强制复检单条（fork `java -version`）。
//! - `prune_invalid_jdks`：惰性校验，把 home 已不存在的条目标记失效。
//!
//! 复用 T-03 SQLite 数据层（单写者 / 事务），不另起存储（全局约束 §7）。
//! 检测惰性：`discover` 只在显式调用时 fork；`prune` 只检路径存在性，
//! 禁止每次启动全量重扫（§性能）。

use std::path::Path;

use rusqlite::Connection;
use tauri::State;

use crate::error::{AppError, AppResult};
use crate::java::detect::{java_exec_for_home, javac_exec_for_home};
use crate::java::discover_jdks as run_discover;
use crate::java::model::{JdkDiscoverySource, JdkInstallation};
use crate::java::registry::{
    apply_version, get_jdk as get_jdk_row, list_jdks as list_jdk_rows, mark_validity, prune_invalid_homes,
    remove_jdk as remove_jdk_row, upsert_jdk, upsert_jdks_batch,
};
use crate::java::version::{parse_java_version, JdkVersionInfo};
use crate::state::AppState;

/// 锁 DB 句柄的样板，错误统一包成 `Other`（与既有命令一致）。
fn lock_db<'r>(state: &'r State<'_, AppState>) -> AppResult<std::sync::MutexGuard<'r, Connection>> {
    state
        .db
        .lock()
        .map_err(|e| AppError::Other(format!("DB lock error: {}", e)))
}

/// 触发本机 JDK 多来源发现并批量 upsert 到注册表。
///
/// 返回发现并入库的 JDK 数量。此命令会 fork `java -version`（只读探测，
/// 非 shell 脚本；全局约束 §3 的自动执行禁令不适用）。重复调用幂等
/// （按 `home_path` upsert）。
#[tauri::command]
pub fn discover_jdks(state: State<'_, AppState>) -> AppResult<usize> {
    let discovered = run_discover();
    let count = discovered.len();
    if discovered.is_empty() {
        return Ok(0);
    }
    let mut conn = lock_db(&state)?;
    upsert_jdks_batch(&mut conn, &discovered)?;
    Ok(count)
}

/// 列出注册表全部 JDK（有效优先、major 降序、路径升序）。
#[tauri::command]
pub fn list_jdks(state: State<'_, AppState>) -> AppResult<Vec<JdkInstallation>> {
    let conn = lock_db(&state)?;
    list_jdk_rows(&conn)
}

/// 按 id 取单条 JDK。
#[tauri::command]
pub fn get_jdk(state: State<'_, AppState>, id: i64) -> AppResult<Option<JdkInstallation>> {
    let conn = lock_db(&state)?;
    get_jdk_row(&conn, id)
}

/// 用户手动添加 JDK：校验路径含 `bin/java`（或 `java.exe`）才入库。
///
/// 无效路径返回 `JdkNotFound` 可行动错误（验收标准：手动添加无效路径给出
/// 可行动提示）。校验通过但 `java -version` 探测失败也入库（`is_valid=false`），
/// 便于用户排查后重检，而非静默丢弃。
#[tauri::command]
pub fn add_jdk_manual(state: State<'_, AppState>, home_path: String) -> AppResult<JdkInstallation> {
    let home = Path::new(&home_path);
    if !home.is_dir() {
        return Err(AppError::JdkNotFound(format!(
            "路径不存在或不是目录：{home_path}。请选择 JDK 根目录（含 bin/java 的目录）。"
        )));
    }
    let java_exec = java_exec_for_home(home).ok_or_else(|| {
        AppError::JdkNotFound(format!(
            "未在 {home_path}/bin 下找到 java 可执行文件。请确认这是 JDK 根目录而非 bin 子目录。"
        ))
    })?;
    let javac_exec = javac_exec_for_home(home);
    // fork `java -version` 探测版本（手动添加时立即探测，给用户即时反馈）。
    let (info, is_valid) = probe_version(&java_exec);
    let canon = std::fs::canonicalize(home).unwrap_or_else(|_| home.to_path_buf());
    let mut jdk = JdkInstallation::new(canon.to_string_lossy().to_string(), JdkDiscoverySource::Manual);
    jdk.major_version = info.major_version;
    jdk.full_version = info.full_version;
    jdk.vendor = info.vendor;
    jdk.architecture = info.architecture;
    jdk.bitness = info.bitness;
    jdk.java_exec = Some(java_exec.to_string_lossy().to_string());
    jdk.javac_exec = javac_exec.map(|p| p.to_string_lossy().to_string());
    jdk.is_valid = is_valid;
    jdk.last_checked = chrono::Utc::now().to_rfc3339();
    jdk.raw_version = if info.raw.is_empty() { None } else { Some(info.raw) };

    let conn = lock_db(&state)?;
    upsert_jdk(&conn, &jdk)?;
    // 重新读取拿到 id / 时间戳。
    let inserted = list_jdk_rows(&conn)?
        .into_iter()
        .find(|j| j.home_path == jdk.home_path)
        .unwrap_or(jdk);
    Ok(inserted)
}

/// 强制复检单条 JDK：重新 fork `java -version` 并更新版本字段与有效性。
///
/// 返回更新后的 JDK 条目。路径已不存在则标记 `is_valid=false` 并返回。
#[tauri::command]
pub fn validate_jdk(state: State<'_, AppState>, id: i64) -> AppResult<JdkInstallation> {
    let existing = {
        let conn = lock_db(&state)?;
        get_jdk_row(&conn, id)?.ok_or_else(|| AppError::JdkNotFound(format!("JDK id={id} 不在注册表中")))?
    };

    let home = Path::new(&existing.home_path);
    let now = chrono::Utc::now().to_rfc3339();
    if !home.is_dir() {
        // 路径已消失：只标记失效，不删除（便于用户排查）。
        let conn = lock_db(&state)?;
        mark_validity(&conn, id, false, &now)?;
        return existing_after_update(&conn, id);
    }
    let java_exec = match java_exec_for_home(home) {
        Some(p) => p,
        None => {
            let conn = lock_db(&state)?;
            mark_validity(&conn, id, false, &now)?;
            return existing_after_update(&conn, id);
        }
    };
    let (info, is_valid) = probe_version(&java_exec);
    let mut probed = existing.clone();
    probed.java_exec = Some(java_exec.to_string_lossy().to_string());
    probed.javac_exec = javac_exec_for_home(home).map(|p| p.to_string_lossy().to_string());
    probed.major_version = info.major_version;
    probed.full_version = info.full_version;
    probed.vendor = info.vendor;
    probed.architecture = info.architecture;
    probed.bitness = info.bitness;
    probed.is_valid = is_valid;
    probed.last_checked = now.clone();
    probed.raw_version = if info.raw.is_empty() { None } else { Some(info.raw) };
    let conn = lock_db(&state)?;
    apply_version(&conn, id, &probed, &now)?;
    Ok(probed)
}

/// 惰性校验：把 `home_path` 已不存在的条目标记 `is_valid=false`。
///
/// 返回被标记失效的条数。不 fork 进程、不删除条目（全局约束性能原则）。
#[tauri::command]
pub fn prune_invalid_jdks(state: State<'_, AppState>) -> AppResult<usize> {
    let mut conn = lock_db(&state)?;
    prune_invalid_homes(&mut conn)
}

/// 按 id 删除单条 JDK（用户在 Settings UI 移除）。
#[tauri::command]
pub fn remove_jdk(state: State<'_, AppState>, id: i64) -> AppResult<()> {
    let conn = lock_db(&state)?;
    remove_jdk_row(&conn, id)
}

/// Fork `java -version` 并解析。返回 `(info, is_valid)`。
/// 复用 detect 内的同名逻辑，但此处独立实现以避免循环依赖并保持命令层自洽。
fn probe_version(java_exec: &Path) -> (JdkVersionInfo, bool) {
    let output = std::process::Command::new(java_exec).arg("-version").output();
    let combined = match output {
        Ok(o) => {
            let mut s = String::new();
            s.push_str(&String::from_utf8_lossy(&o.stderr));
            s.push('\n');
            s.push_str(&String::from_utf8_lossy(&o.stdout));
            s
        }
        Err(err) => {
            log::warn!("JDK probe {:?} failed: {}", java_exec, err);
            let mut info = JdkVersionInfo::default();
            info.raw = format!("probe error: {err}");
            return (info, false);
        }
    };
    let info = parse_java_version(&combined);
    let valid = info.major_version.is_some();
    (info, valid)
}

fn existing_after_update(conn: &Connection, id: i64) -> AppResult<JdkInstallation> {
    get_jdk_row(conn, id)?.ok_or_else(|| AppError::JdkNotFound(format!("JDK id={id} 不在注册表中")))
}
