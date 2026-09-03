//! 日志存储（R-11，B-05 拆分）：日志目录、段文件清单、路径安全守卫。
//!
//! 目录布局：`<workspace>/.gitworkspace/logs/<runtime_name>/`，段文件
//! `<process_id>.log`（当前段）与 `<process_id>.N.log`（滚动历史段，
//! N 越大越旧）。

use std::fs;
use std::path::{Path, PathBuf};

use crate::error::{AppError, AppResult};

/// 日志目录：`<workspace>/.gitworkspace/logs/<runtime_name>/`。
const LOGS_DIR: &str = "logs";

pub(super) fn current_segment_path(dir: &Path, process_id: i64) -> PathBuf {
    dir.join(format!("{process_id}.log"))
}

/// 段文件清单（最旧 → 最新），只含实际存在的段。
pub(super) fn segment_paths(dir: &Path, process_id: i64, keep: u32) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    for i in (1..=keep).rev() {
        let path = dir.join(format!("{process_id}.{i}.log"));
        if path.is_file() {
            paths.push(path);
        }
    }
    let current = current_segment_path(dir, process_id);
    if current.is_file() {
        paths.push(current);
    }
    paths
}

pub(super) fn logs_dir(workspace_root: &Path, runtime_name: &str) -> AppResult<PathBuf> {
    validate_runtime_name(runtime_name)?;
    Ok(workspace_root.join(".gitworkspace").join(LOGS_DIR).join(runtime_name))
}

/// 创建日志目录（写路径）；先校验名再落任何目录，沿用 R-07 配置的
/// 符号链接拒绝守卫。
pub(super) fn ensure_logs_dir(workspace_root: &Path, runtime_name: &str) -> AppResult<PathBuf> {
    validate_runtime_name(runtime_name)?;
    let gitworkspace = workspace_root.join(".gitworkspace");
    // R-14 §78 只读护栏：日志目录必须在 workspace/.gitworkspace 下。
    crate::runtime::guard::assert_workspace_write_path(&gitworkspace, workspace_root, "日志落盘")?;
    reject_symlink(&gitworkspace)?;
    let logs = gitworkspace.join(LOGS_DIR);
    reject_symlink(&logs)?;
    fs::create_dir_all(&logs)?;
    let dir = logs.join(runtime_name);
    reject_symlink(&dir)?;
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn validate_runtime_name(name: &str) -> AppResult<()> {
    if name.is_empty() || name == "." || name == ".." || name.contains(['/', '\\']) {
        return Err(AppError::RuntimeConfig(format!(
            "Runtime 名称 '{name}' 不能用作日志目录名（禁止空名、路径分隔符与 . / ..）"
        )));
    }
    Ok(())
}

fn reject_symlink(path: &Path) -> AppResult<()> {
    if let Ok(metadata) = fs::symlink_metadata(path) {
        if metadata.file_type().is_symlink() {
            return Err(AppError::Permission(format!(
                "拒绝通过符号链接写入日志目录：{}",
                path.display()
            )));
        }
    }
    Ok(())
}
