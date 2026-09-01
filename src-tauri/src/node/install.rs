//! Explicit dependency installation command construction (N-08).

use std::path::Path;
use std::process::Command;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use crate::error::{AppError, AppResult};
use crate::node::model::{PackageManager, ToolDetection};
use crate::process::streaming::{spawn_streaming, OutputStream};

pub fn build_install_command(executable: &Path, project_dir: &Path) -> Command {
    let is_cmd = executable.extension().is_some_and(|ext| {
        matches!(
            ext.to_string_lossy().to_ascii_lowercase().as_str(),
            "cmd" | "bat"
        )
    });
    let mut command = if cfg!(windows) && is_cmd {
        let mut cmd = Command::new("cmd");
        cmd.args(["/C"]).arg(executable);
        cmd
    } else {
        Command::new(executable)
    };
    command.arg("install").current_dir(project_dir);
    command
}

pub fn execute_install(
    detection: ToolDetection,
    manager: PackageManager,
    project_dir: &Path,
    cancel: Option<&AtomicBool>,
    mut on_line: impl FnMut(OutputStream, &str),
) -> AppResult<String> {
    if manager == PackageManager::Bun {
        return Err(AppError::PackageManagerNotFound(
            "当前版本不支持 bun install；请改选 npm、pnpm 或 yarn".into(),
        ));
    }
    let mut command = build_install_command(&detection.executable, project_dir);
    let mut tail = String::new();
    let exit = spawn_streaming(
        &mut command,
        cancel,
        Some(Duration::from_secs(30 * 60)),
        &mut |stream, line| {
            on_line(stream, line);
            if tail.len() < 16 * 1024 {
                if !tail.is_empty() {
                    tail.push('\n');
                }
                tail.push_str(line);
                tail.truncate(tail.len().min(16 * 1024));
            }
        },
    )?;
    if exit.cancelled {
        return Err(AppError::Task("node_install cancelled by user".into()));
    }
    if exit.timed_out {
        return Err(AppError::Network(format!(
            "{} install 超时（30 分钟）；请检查网络或镜像源后重试",
            manager.name()
        )));
    }
    if exit.exit_code != Some(0) {
        return Err(AppError::Network(format!(
            "{} install 失败（exit code {:?}）。请检查网络、镜像源和 package.json。\n{}",
            manager.name(),
            exit.exit_code,
            tail
        )));
    }
    Ok(tail)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn install_command_is_direct_and_uses_project_dir() {
        let dir = PathBuf::from("/tmp/web");
        let command = build_install_command(Path::new("/usr/bin/pnpm"), &dir);
        assert_eq!(command.get_args().collect::<Vec<_>>(), ["install"]);
        assert_eq!(command.get_current_dir(), Some(dir.as_path()));
    }

    #[test]
    fn bun_install_is_rejected_before_spawn() {
        let error = execute_install(
            ToolDetection {
                executable: PathBuf::from("/missing/bun"),
                version: None,
                raw_output: String::new(),
                probe_ok: false,
                source: crate::node::ToolDetectionSource::Path,
            },
            PackageManager::Bun,
            Path::new("/tmp"),
            None,
            |_, _| {},
        )
        .unwrap_err();
        assert!(matches!(error, AppError::PackageManagerNotFound(_)));
    }

    /// N-08 验收：显式安装真实冒烟。无依赖工程，`install` 在本地完成
    /// （不访问 registry）；探测不到对应包管理器时 skip 并打印原因。
    fn real_install_loopback(manager: PackageManager, manager_name: &str) {
        let detection = match crate::node::detect_package_manager(manager) {
            Ok(detection) => detection,
            Err(err) => {
                eprintln!("N-08: {manager_name} unavailable; skipping real install smoke ({err})");
                return;
            }
        };
        let project = std::env::temp_dir().join(format!("gw_n08_install_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(
            project.join("package.json"),
            r#"{"name":"gw-install-fixture","version":"0.0.0","private":true}"#,
        )
        .unwrap();
        let mut lines = Vec::new();
        let summary = execute_install(detection, manager, &project, None, |_, line| {
            lines.push(line.to_string());
        })
        .unwrap_or_else(|err| panic!("{manager_name} install should succeed: {err}"));
        assert!(!summary.is_empty(), "install should capture output tail");
        assert!(
            project.join("node_modules").is_dir(),
            "{manager_name} install should create node_modules (lines: {lines:?})"
        );
        let _ = std::fs::remove_dir_all(project);
    }

    #[test]
    fn pnpm_install_runs_real_loopback() {
        real_install_loopback(PackageManager::Pnpm, "pnpm");
    }

    #[test]
    fn yarn_install_runs_real_loopback() {
        real_install_loopback(PackageManager::Yarn, "yarn");
    }
}
