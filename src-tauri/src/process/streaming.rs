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

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
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
    spawn_streaming_ext(command, cancel, timeout, None, on_line)
}

/// [`spawn_streaming`] 的扩展变体（R-10）：spawn 成功后立刻把子进程 pid
/// 写入 `pid_slot`，让外部（Process Manager 的 Stop/Kill）在 `run` 仍阻塞
/// 期间就能拿到 pid 发信号。`pid_slot` 为 `None` 时与原语义完全一致。
pub fn spawn_streaming_ext(
    command: &mut Command,
    cancel: Option<&AtomicBool>,
    timeout: Option<Duration>,
    pid_slot: Option<&std::sync::Mutex<Option<u32>>>,
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
    if let Some(slot) = pid_slot {
        *slot.lock().unwrap() = Some(child.id());
    }

    let stdout = child.stdout.take().expect("stdout was piped before spawn");
    let stderr = child.stderr.take().expect("stderr was piped before spawn");
    let (tx, rx) = mpsc::channel::<(OutputStream, String)>();
    let stdout_reader = spawn_reader(stdout, OutputStream::Stdout, tx.clone());
    let stderr_reader = spawn_reader(stderr, OutputStream::Stderr, tx);
    // 主线程不再持有 tx：reader 线程结束后 channel 断开，主循环据此退出。

    let start = Instant::now();
    let mut exit_status = None;
    let mut timed_out = false;
    let mut cancelled = false;
    // 两侧 reader 均已结束（EOF / 读取错误）。channel 已断开，不能再 recv
    // （会立即返回 Disconnected 造成忙轮询），改为短睡轮询。
    // F-12：绝不能在此处直接 child.wait() 阻塞——子进程在丢失输出 reader
    // 后仍可能存活（如 JVM 输出 GBK 非法字节杀死 reader），阻塞期间
    // cancel 与 timeout 将永远无人轮询。
    let mut readers_done = false;

    loop {
        if readers_done {
            std::thread::sleep(POLL_INTERVAL);
        } else {
            match rx.recv_timeout(POLL_INTERVAL) {
                Ok((stream, line)) => on_line(stream, &line),
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => readers_done = true,
            }
        }

        if exit_status.is_none() {
            if let Some(status) = child.try_wait()? {
                // 不立即退出：继续 drain 残余输出，直到 reader 线程结束。
                exit_status = Some(status);
            } else if cancel.is_some_and(|flag| flag.load(Ordering::Relaxed)) {
                cancelled = true;
                crate::process::kill_tree::kill_process_tree(child.id());
                exit_status = Some(child.wait()?);
            } else if let Some(limit) = timeout {
                if start.elapsed() >= limit {
                    timed_out = true;
                    crate::process::kill_tree::kill_process_tree(child.id());
                    exit_status = Some(child.wait()?);
                }
            }
        }

        if readers_done && exit_status.is_some() {
            break;
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
        let mut buf: Vec<u8> = Vec::new();
        loop {
            buf.clear();
            match reader.read_until(b'\n', &mut buf) {
                // 0 = EOF；>0 含不以换行结尾的最后一段。
                Ok(0) => break,
                Ok(_) => {
                    // F-12：按字节读 + lossy 解码——非法 UTF-8（中文 Windows
                    // 下 JVM 默认 GBK 输出）只把坏字节替换为 U+FFFD，不让
                    // reader 早死。早死会丢光后续日志、管道写满后卡死被监控
                    // 进程，并把主循环推入 Disconnected 分支。
                    let line = String::from_utf8_lossy(&buf);
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
    use std::sync::{Arc, Mutex};

    #[cfg(unix)]
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
        let exit = spawn_streaming(&mut cmd, None, Some(Duration::from_millis(400)), &mut |_, line| {
            lines.push(line.to_string())
        })
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

    /// F-12 次级缺陷回归：非法 UTF-8（中文 Windows 上 JVM 的 GBK 输出）不应
    /// 杀死 reader；lossy 解码后继续读后续行。修复前 `read_line` 遇非法字节
    /// 直接 break，后续输出全丢且触发主循环 Disconnected 分支。
    #[test]
    fn reader_lossy_decodes_invalid_utf8_and_keeps_reading() {
        let data: &[u8] = b"alpha\n\xff\xfegb\nbeta\n";
        let (tx, rx) = mpsc::channel();
        let handle = spawn_reader(std::io::Cursor::new(data), OutputStream::Stdout, tx);
        handle.join().unwrap();
        let lines: Vec<String> = rx.try_iter().map(|(_, line)| line).collect();
        assert_eq!(lines, vec!["alpha", "\u{FFFD}\u{FFFD}gb", "beta"]);
    }

    /// 向 stdout 和 stderr 各写一段非法 UTF-8 后长驻的子进程：两个 reader
    /// 都会死亡、channel 断开，复现 F-12「reader 全断后主循环阻塞在
    /// child.wait()，cancel 失明」的场景（hussar JVM 的 GBK 输出同款）。
    #[cfg(unix)]
    fn stream_killing_command() -> Command {
        // 用八进制转义保证 dash 等 POSIX printf 也能发出原始字节。
        sh_command("printf '\\377\\376\\n'; printf '\\377\\376\\n' >&2; sleep 300")
    }

    /// Windows 等价物：powershell 经 OpenStandardOutput/Error 写原始字节。
    #[cfg(windows)]
    fn stream_killing_command() -> Command {
        let mut cmd = Command::new("powershell");
        cmd.args([
            "-NoProfile",
            "-Command",
            "$b=[byte[]](255,254,10); \
             [Console]::OpenStandardOutput().Write($b,0,3); \
             [Console]::OpenStandardError().Write($b,0,3); \
             Start-Sleep -Seconds 300",
        ]);
        cmd
    }

    /// F-12 回归：reader 全部断开后，cancel 仍须被观察并杀掉进程树。
    /// 看门狗 recv_timeout 保证修复前是「失败」而非「挂死」。
    #[test]
    fn cancel_kills_child_after_readers_disconnect() {
        let mut cmd = stream_killing_command();
        let cancel = Arc::new(AtomicBool::new(false));
        let pid_slot = Arc::new(Mutex::new(None));
        let (done_tx, done_rx) = mpsc::channel();
        {
            let cancel = Arc::clone(&cancel);
            let pid_slot = Arc::clone(&pid_slot);
            std::thread::spawn(move || {
                let exit = spawn_streaming_ext(&mut cmd, Some(&cancel), None, Some(&pid_slot), &mut |_, _| {});
                let _ = done_tx.send(exit);
            });
        }

        // 等 spawn 拿到 pid，再留出两个 reader 吃到非法字节死亡的时间。
        let deadline = Instant::now() + Duration::from_secs(10);
        let pid = loop {
            if let Some(pid) = *pid_slot.lock().unwrap() {
                break pid;
            }
            assert!(Instant::now() < deadline, "spawn 后 10s 内应拿到 pid");
            std::thread::sleep(Duration::from_millis(20));
        };
        std::thread::sleep(Duration::from_millis(800));

        cancel.store(true, Ordering::Relaxed);
        let result = done_rx.recv_timeout(Duration::from_secs(15));
        if result.is_err() {
            // 清理卡住的监督线程与子进程，避免测试进程退出后残留。
            crate::process::kill_tree::kill_process_tree(pid);
        }
        let exit = result
            .expect("reader 断开后 cancel 也必须被观察（F-12）")
            .expect("spawn 不应失败");
        assert!(exit.cancelled);
        std::thread::sleep(Duration::from_millis(200));
        assert!(
            !crate::process::kill_tree::process_alive(pid, None),
            "cancel 后进程树必须真实消失"
        );
    }
}
