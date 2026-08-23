//! 子进程流式输出转发（R-09，§28 Build 流程「输出无上限缓冲」）。
//!
//! spawn 后把 stdout/stderr 各交给一个 reader 线程按行读取，经 mpsc 汇总到
//! 主循环逐行转发给 `on_line` 回调；主循环同时轮询退出状态、取消标记与超时，
//! 触发取消/超时时通过 [`crate::process::kill_tree::kill_process_tree`] 杀掉
//! 整棵进程树并 reap。输出本身不做任何上限缓冲（直接转发），尾部上下文由
//! Build 层的 `RingTail` 另行维护。
//!
//! Windows 下镜像 `core/git_ops.rs` 的 `CREATE_NO_WINDOW` 处理，避免弹出
//! 控制台窗口。

use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// 轮询间隔：取消/超时检查的响应粒度。
const POLL_INTERVAL: Duration = Duration::from_millis(50);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputStream {
    Stdout,
    Stderr,
}

/// 流式执行的退出结果。`exit_code` 为 `None` 表示进程被信号终止（无法取码）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StreamingExit {
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub cancelled: bool,
}

/// Spawn `command` 并把输出按行实时转发给 `on_line`，直到进程退出、被取消或超时。
///
/// - `cancel`：外部置位后杀整棵进程树，返回 `cancelled = true`。
/// - `timeout`：从 spawn 起算，超时杀整棵进程树，返回 `timed_out = true`。
///
/// 进程退出后仍会 drain 完 reader 线程残留的行再返回。
pub fn spawn_streaming(
    command: &mut Command,
    cancel: Option<&AtomicBool>,
    timeout: Option<Duration>,
    on_line: &mut dyn FnMut(OutputStream, &str),
) -> std::io::Result<StreamingExit> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW (0x08000000): 不弹出可见控制台（同 git_ops.rs）。
        command.creation_flags(0x0800_0000);
    }
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn()?;

    let stdout = child
        .stdout
        .take()
        .expect("stdout was piped before spawn");
    let stderr = child
        .stderr
        .take()
        .expect("stderr was piped before spawn");
    let (tx, rx) = mpsc::channel::<(OutputStream, String)>();
    let stdout_reader = spawn_reader(stdout, OutputStream::Stdout, tx.clone());
    let stderr_reader = spawn_reader(stderr, OutputStream::Stderr, tx);
    // 主线程不再持有 tx：reader 线程结束后 channel 断开，主循环据此退出。

    let start = Instant::now();
    let mut exit_status = None;
    let mut timed_out = false;
    let mut cancelled = false;

    loop {
        match rx.recv_timeout(POLL_INTERVAL) {
            Ok((stream, line)) => on_line(stream, &line),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                // 两个 reader 都结束了：子进程已关闭输出流，收最终退出码。
                if exit_status.is_none() {
                    exit_status = Some(child.wait()?);
                }
                break;
            }
        }

        if exit_status.is_none() {
            if let Some(status) = child.try_wait()? {
                // 不 break：继续 drain channel 直到 reader 线程结束。
                exit_status = Some(status);
                continue;
            }
            if cancel.is_some_and(|flag| flag.load(Ordering::Relaxed)) {
                cancelled = true;
                crate::process::kill_tree::kill_process_tree(child.id());
                exit_status = Some(child.wait()?);
                continue;
            }
            if let Some(limit) = timeout {
                if start.elapsed() >= limit {
                    timed_out = true;
                    crate::process::kill_tree::kill_process_tree(child.id());
                    exit_status = Some(child.wait()?);
                }
            }
        }
    }

    let _ = stdout_reader.join();
    let _ = stderr_reader.join();
    Ok(StreamingExit {
        exit_code: exit_status.and_then(|status| status.code()),
        timed_out,
        cancelled,
    })
}

fn spawn_reader<R: std::io::Read + Send + 'static>(
    reader: R,
    stream: OutputStream,
    tx: mpsc::Sender<(OutputStream, String)>,
) -> JoinHandle<()> {
    std::thread::spawn(move || {
        let mut reader = BufReader::new(reader);
        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line) {
                // 0 = EOF；>0 含不以换行结尾的最后一段。
                Ok(0) => break,
                Ok(_) => {
                    let trimmed = line.trim_end_matches(['\r', '\n']);
                    if tx.send((stream, trimmed.to_string())).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn sh_command(script: &str) -> Command {
        let mut cmd = Command::new("sh");
        cmd.args(["-c", script]);
        cmd
    }

    #[cfg(unix)]
    #[test]
    fn forwards_stdout_and_stderr_lines() {
        let mut cmd = sh_command("echo out-1; echo err-1 1>&2; echo out-2");
        let mut lines: Vec<(OutputStream, String)> = Vec::new();
        let exit = spawn_streaming(&mut cmd, None, None, &mut |stream, line| {
            lines.push((stream, line.to_string()));
        })
        .unwrap();

        assert_eq!(exit.exit_code, Some(0));
        assert!(!exit.timed_out && !exit.cancelled);
        let stdout: Vec<_> = lines
            .iter()
            .filter(|(s, _)| *s == OutputStream::Stdout)
            .map(|(_, l)| l.as_str())
            .collect();
        let stderr: Vec<_> = lines
            .iter()
            .filter(|(s, _)| *s == OutputStream::Stderr)
            .map(|(_, l)| l.as_str())
            .collect();
        assert_eq!(stdout, ["out-1", "out-2"]);
        assert_eq!(stderr, ["err-1"]);
    }

    #[cfg(unix)]
    #[test]
    fn forwards_trailing_line_without_newline() {
        let mut cmd = sh_command("printf 'no-newline'");
        let mut lines = Vec::new();
        let exit = spawn_streaming(&mut cmd, None, None, &mut |_, line| {
            lines.push(line.to_string());
        })
        .unwrap();
        assert_eq!(exit.exit_code, Some(0));
        assert_eq!(lines, ["no-newline"]);
    }

    #[cfg(unix)]
    #[test]
    fn cancel_kills_process_tree() {
        let mut cmd = sh_command("sleep 300 & echo started; wait");
        let cancel = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&cancel);
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            flag.store(true, Ordering::Relaxed);
        });
        let start = Instant::now();
        let exit = spawn_streaming(&mut cmd, Some(&cancel), None, &mut |_, _| {}).unwrap();

        assert!(exit.cancelled);
        assert!(!exit.timed_out);
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "cancel must kill the tree instead of waiting for sleep 300"
        );

        // 兜底确认没有 sleep 300 残留。
        std::thread::sleep(Duration::from_millis(200));
        let mut system = sysinfo::System::new_with_specifics(
            sysinfo::RefreshKind::new().with_processes(sysinfo::ProcessRefreshKind::new()),
        );
        system.refresh_processes();
        assert!(
            !system
                .processes()
                .values()
                .any(|p| p.name() == "sleep" && p.cmd().iter().any(|a| a == "300")),
            "sleep 300 must be gone after cancel"
        );
    }

    #[cfg(unix)]
    #[test]
    fn timeout_kills_process_and_marks_timed_out() {
        let mut cmd = sh_command("echo before; sleep 300");
        let mut lines = Vec::new();
        let start = Instant::now();
        let exit = spawn_streaming(
            &mut cmd,
            None,
            Some(Duration::from_millis(400)),
            &mut |_, line| lines.push(line.to_string()),
        )
        .unwrap();

        assert!(exit.timed_out);
        assert!(!exit.cancelled);
        assert_eq!(lines, ["before"]);
        assert!(start.elapsed() < Duration::from_secs(10));
    }

    #[cfg(unix)]
    #[test]
    fn non_zero_exit_code_is_reported() {
        let mut cmd = sh_command("exit 3");
        let exit = spawn_streaming(&mut cmd, None, None, &mut |_, _| {}).unwrap();
        assert_eq!(exit.exit_code, Some(3));
        assert!(!exit.timed_out && !exit.cancelled);
    }
}
