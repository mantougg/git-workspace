use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::core::logger::{logs_dir, LOG_FILES};
use crate::error::{AppError, AppResult};

/// Metadata about a single log file.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogFileInfo {
    pub name: String,
    pub path: String,
    pub size_bytes: u64,
}

/// List the five per-module log files with their current sizes.
#[tauri::command]
pub fn list_log_files() -> AppResult<Vec<LogFileInfo>> {
    let dir = logs_dir();
    let mut files = Vec::with_capacity(LOG_FILES.len());
    for name in LOG_FILES {
        let path = dir.join(name);
        let size_bytes = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        files.push(LogFileInfo {
            name: name.to_string(),
            path: path.to_string_lossy().to_string(),
            size_bytes,
        });
    }
    Ok(files)
}

/// Open the log directory in the system file manager.
#[tauri::command]
pub fn open_logs() -> AppResult<()> {
    let dir = logs_dir();
    if !dir.exists() {
        fs::create_dir_all(&dir)?;
    }
    open_in_file_manager(&dir)
}

/// Export (copy) the log files into `target_dir` (or a default timestamped
/// directory under the logs dir), returning the export directory path.
#[tauri::command]
pub fn export_logs(target_dir: Option<String>) -> AppResult<String> {
    let dir = logs_dir();
    let dest = match target_dir {
        Some(t) => PathBuf::from(t),
        None => dir
            .join("export")
            .join(format!("logs-{}", chrono::Utc::now().format("%Y%m%d-%H%M%S"))),
    };
    fs::create_dir_all(&dest)?;
    for name in LOG_FILES {
        let src = dir.join(name);
        if src.exists() {
            fs::copy(&src, dest.join(name))?;
        }
    }
    Ok(dest.to_string_lossy().to_string())
}

/// Clear (truncate) all five log files. The logger opens files in append mode,
/// so subsequent writes resume from the new (zero) length.
#[tauri::command]
pub fn clear_logs() -> AppResult<()> {
    let dir = logs_dir();
    for name in LOG_FILES {
        let path = dir.join(name);
        if path.exists() {
            fs::write(&path, b"")?;
        }
    }
    Ok(())
}

/// Open a path in the OS file manager.
fn open_in_file_manager(path: &Path) -> AppResult<()> {
    #[cfg(target_os = "windows")]
    let status = std::process::Command::new("explorer").arg(path).status();
    #[cfg(target_os = "macos")]
    let status = std::process::Command::new("open").arg(path).status();
    #[cfg(target_os = "linux")]
    let status = std::process::Command::new("xdg-open").arg(path).status();
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    let status: std::io::Result<std::process::ExitStatus> = Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "open_in_file_manager is not supported on this platform",
    ));

    match status {
        Ok(s) if s.success() => Ok(()),
        Ok(s) => Err(AppError::Other(format!("File manager exited with status {}", s))),
        Err(e) => Err(AppError::Other(format!("Failed to open file manager: {}", e))),
    }
}
