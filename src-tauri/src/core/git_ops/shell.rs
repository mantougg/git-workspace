use std::path::Path;
use std::time::Duration;

use crate::error::{AppError, AppResult};

/// Run a user shell command in `cwd`, killed after `timeout_secs` (T-23
/// pipeline Build/Test steps; default 10 min when unset).
///
/// Output is redirected to temp files instead of pipes: a child writing more
/// than the OS pipe buffer would block forever while we poll `try_wait`
/// without reading. Only the tail (256 KB) of each stream is kept.
pub(super) fn run_shell_command(cwd: &Path, command: &str, timeout_secs: Option<u64>) -> AppResult<String> {
    let timeout = Duration::from_secs(timeout_secs.unwrap_or(600));
    let stamp = format!(
        "gw_shell_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    );
    let out_path = std::env::temp_dir().join(format!("{}.out", stamp));
    let err_path = std::env::temp_dir().join(format!("{}.err", stamp));

    let mut cmd = if cfg!(windows) {
        let mut c = std::process::Command::new("cmd");
        c.args(["/C", command]);
        c
    } else {
        let mut c = std::process::Command::new("sh");
        c.args(["-c", command]);
        c
    };
    cmd.current_dir(cwd)
        .stdout(std::fs::File::create(&out_path)?)
        .stderr(std::fs::File::create(&err_path)?);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW (0x08000000): don't spawn a visible console.
        cmd.creation_flags(0x0800_0000);
    }

    let mut child = cmd
        .spawn()
        .map_err(|e| AppError::Other(format!("命令启动失败: {}", e)))?;
    let start = std::time::Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) => {
                if start.elapsed() > timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = std::fs::remove_file(&out_path);
                    let _ = std::fs::remove_file(&err_path);
                    return Err(AppError::Other(format!(
                        "命令超过 {}s 未结束，已终止",
                        timeout.as_secs()
                    )));
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => {
                let _ = std::fs::remove_file(&out_path);
                let _ = std::fs::remove_file(&err_path);
                return Err(AppError::Other(format!("命令等待失败: {}", e)));
            }
        }
    };

    let stdout = read_tail(&out_path, 256 * 1024);
    let stderr = read_tail(&err_path, 256 * 1024);
    let _ = std::fs::remove_file(&out_path);
    let _ = std::fs::remove_file(&err_path);

    let combined = [stdout.trim(), stderr.trim()].join("\n").trim().to_string();
    if status.success() {
        Ok(combined)
    } else {
        Err(AppError::Other(format!(
            "命令失败（exit {}）: {}",
            status.code().unwrap_or(-1),
            combined
        )))
    }
}

/// Read at most `cap` bytes from the END of a file (bounded memory for
/// potentially huge build logs).
fn read_tail(path: &Path, cap: u64) -> String {
    use std::io::{Read, Seek, SeekFrom};
    let Ok(mut f) = std::fs::File::open(path) else {
        return String::new();
    };
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    let skip = len.saturating_sub(cap);
    let _ = f.seek(SeekFrom::Start(skip));
    let mut buf = Vec::new();
    let _ = f.read_to_end(&mut buf);
    String::from_utf8_lossy(&buf).to_string()
}
