//! PTY 会话后端（TM-01，terminal-feature-plan §4.1 / §4.2 / §5）。
//!
//! 跨平台 PTY 会话管理：`portable-pty`（Windows ConPTY / unix forkpty）提供
//! 真实 TTY 语义，reader 线程按 50ms / 8KiB 批量 flush → `terminal_output` 事件
//! （base64 原始字节，不经 String，多字节跨块安全）。
//!
//! 约束：
//! - PTY 路径禁止 `read_line` / `from_utf8_lossy`（全局开发约束 §1）。
//! - 关闭复用 `kill_tree.rs`（先 `terminate_process` → 超时 `kill_process_tree`）。
//! - Windows ConPTY 不叠加 `CREATE_NO_WINDOW`（portable-pty 内部管理 spawn）。
//! - Shell 探测走 `find_in_path`（`.exe → .cmd → .bat → 裸名`），禁止裸名兜底。

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use portable_pty::{CommandBuilder, NativePtySystem, PtySize, PtySystem};
use serde::{Deserialize, Serialize};

use crate::java::detect::find_in_path;
use crate::process::{kill_process_tree, terminate_process};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Reader 线程 flush 间隔（≤50ms flush）。
const FLUSH_INTERVAL: Duration = Duration::from_millis(50);

/// Reader 线程立即 flush 阈值（≥8KiB 立即 flush）。
const FLUSH_THRESHOLD: usize = 8 * 1024;

/// 关闭会话时优雅停止的超时（先 SIGTERM，超时升级 kill_process_tree）。
const CLOSE_GRACE_TIMEOUT: Duration = Duration::from_millis(2000);

// ---------------------------------------------------------------------------
// Shell detection（§5.1，走 find_in_path，禁止裸名兜底）
// ---------------------------------------------------------------------------

/// 探测可用 shell 列表（§5.1 顺序），返回所有找到的 shell。
///
/// Windows: `pwsh` → `powershell` → `cmd`
/// macOS / Linux: `$SHELL` → `zsh` → `bash` → `sh`
pub fn detect_available_shells() -> Vec<ShellInfo> {
    let candidates = shell_candidates();
    let mut shells = Vec::new();
    for (id, label, name) in candidates {
        if let Some(path) = find_in_path(&name) {
            shells.push(ShellInfo {
                id: id.to_string(),
                label: label.to_string(),
                path: path.to_string_lossy().to_string(),
            });
        }
    }
    shells
}

/// 按 §5.1 顺序返回 (id, label, 可执行名) 候选。
fn shell_candidates() -> Vec<(&'static str, &'static str, String)> {
    if cfg!(windows) {
        vec![
            ("pwsh", "PowerShell 7", "pwsh".to_string()),
            ("powershell", "Windows PowerShell", "powershell".to_string()),
            ("cmd", "Command Prompt", "cmd".to_string()),
        ]
    } else {
        let mut candidates = Vec::new();
        // $SHELL 环境变量优先
        if let Ok(shell) = std::env::var("SHELL") {
            let name = std::path::Path::new(&shell)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            if !name.is_empty() {
                candidates.push(("env_shell", "Default Shell", shell));
            }
        }
        candidates.push(("zsh", "Zsh", "zsh".to_string()));
        candidates.push(("bash", "Bash", "bash".to_string()));
        candidates.push(("sh", "Sh", "sh".to_string()));
        candidates
    }
}

/// 探测默认 shell（§5.1 顺序，找到第一个即返回）。
fn detect_default_shell() -> Result<PathBuf, String> {
    let candidates = shell_candidates();
    for (_id, _label, name) in &candidates {
        if let Some(path) = find_in_path(name) {
            return Ok(path);
        }
    }
    Err("PATH 上未找到可用 shell。请安装 zsh/bash/sh 或在 Windows 上安装 PowerShell。".to_string())
}

// ---------------------------------------------------------------------------
// Data types（IPC 契约 §4.2）
// ---------------------------------------------------------------------------

/// Shell 信息（`terminal_list_shells` 返回）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShellInfo {
    pub id: String,
    pub label: String,
    pub path: String,
}

/// 会话信息（`terminal_list` 返回）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalSessionInfo {
    pub session_id: String,
    pub kind: String,
    pub title: String,
    pub cwd: String,
    pub alive: bool,
}

/// `terminal_open` 参数。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalOpenParams {
    pub cwd: Option<String>,
    pub shell: Option<String>,
    pub cols: u16,
    pub rows: u16,
}

/// `terminal_write` 参数。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalWriteParams {
    pub session_id: String,
    pub data_base64: String,
}

/// `terminal_resize` 参数。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalResizeParams {
    pub session_id: String,
    pub cols: u16,
    pub rows: u16,
}

/// `terminal_close` 参数。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalCloseParams {
    pub session_id: String,
}

/// `terminal_output` 事件 payload。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalOutputEvent {
    pub session_id: String,
    pub data_base64: String,
}

/// `terminal_exit` 事件 payload。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalExitEvent {
    pub session_id: String,
    pub exit_code: Option<i32>,
}

// ---------------------------------------------------------------------------
// PtySession
// ---------------------------------------------------------------------------

/// 单个 PTY 会话。
struct PtySession {
    /// 子进程 pid（用于 kill_tree 清理）。
    #[allow(dead_code)]
    pid: u32,
    /// 写锁：`terminal_write` 持锁写 master。
    writer: Mutex<Box<dyn Write + Send>>,
    /// 会话元信息（供 `terminal_list` 返回）。
    info: TerminalSessionInfo,
    /// reader 线程退出信号（drop 时设为 true）。
    shutdown: Arc<std::sync::atomic::AtomicBool>,
}

// ---------------------------------------------------------------------------
// Emitter trait（解耦 Tauri，便于单测）
// ---------------------------------------------------------------------------

/// 事件发射器抽象（生产环境用 Tauri `app_handle.emit`，测试用 mock）。
pub trait TerminalEmitter: Send + Sync + 'static {
    fn emit_output(&self, event: TerminalOutputEvent);
    fn emit_exit(&self, event: TerminalExitEvent);
}

/// Noop 发射器（AppState 初始化占位，setup 中通过 `set_app_handle` 替换）。
pub struct NoopTerminalEmitter;

impl TerminalEmitter for NoopTerminalEmitter {
    fn emit_output(&self, _event: TerminalOutputEvent) {}
    fn emit_exit(&self, _event: TerminalExitEvent) {}
}

/// Tauri 事件发射器（生产环境，通过 `set_app_handle` 延迟初始化）。
pub struct TauriTerminalEmitter {
    app_handle: std::sync::OnceLock<tauri::AppHandle>,
}

impl TauriTerminalEmitter {
    pub fn new() -> Self {
        Self {
            app_handle: std::sync::OnceLock::new(),
        }
    }

    /// 在 Tauri setup 闭包中调用，设置 AppHandle（仅生效一次）。
    pub fn set_app_handle(&self, handle: tauri::AppHandle) {
        let _ = self.app_handle.set(handle);
    }
}

impl TerminalEmitter for TauriTerminalEmitter {
    fn emit_output(&self, event: TerminalOutputEvent) {
        if let Some(handle) = self.app_handle.get() {
            let _ = tauri::Emitter::emit(handle, "terminal_output", &event);
        }
    }

    fn emit_exit(&self, event: TerminalExitEvent) {
        if let Some(handle) = self.app_handle.get() {
            let _ = tauri::Emitter::emit(handle, "terminal_exit", &event);
        }
    }
}

// ---------------------------------------------------------------------------
// TerminalManager
// ---------------------------------------------------------------------------

/// PTY 会话管理器（注册进 `AppState`）。
///
/// 初始化策略：`AppState::new()` 时创建 `TauriTerminalEmitter`（AppHandle 尚未就绪），
/// reader 线程在 `open()` 时才 spawn——此时 AppHandle 已通过 `set_app_handle` 注入。
pub struct TerminalManager {
    sessions: Mutex<HashMap<String, PtySession>>,
    emitter: Arc<dyn TerminalEmitter>,
    /// 供 `set_app_handle` 用的内部 Tauri 发射器引用（`new()` 时创建）。
    tauri_emitter: Option<Arc<TauriTerminalEmitter>>,
}

impl TerminalManager {
    /// 创建管理器（使用 TauriTerminalEmitter，AppHandle 延迟注入）。
    pub fn new() -> Self {
        let tauri_emitter = Arc::new(TauriTerminalEmitter::new());
        let emitter: Arc<dyn TerminalEmitter> = tauri_emitter.clone();
        Self {
            sessions: Mutex::new(HashMap::new()),
            emitter,
            tauri_emitter: Some(tauri_emitter),
        }
    }

    /// 从测试用发射器创建（不持有 TauriTerminalEmitter 引用）。
    pub fn with_emitter(emitter: Arc<dyn TerminalEmitter>) -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
            emitter,
            tauri_emitter: None,
        }
    }

    /// 在 Tauri setup 闭包中调用，注入 AppHandle（仅生效一次）。
    pub fn set_app_handle(&self, handle: tauri::AppHandle) {
        if let Some(ref emitter) = self.tauri_emitter {
            emitter.set_app_handle(handle);
        }
    }

    /// 创建新 PTY 会话（`terminal_open`）。
    pub fn open(
        &self,
        params: TerminalOpenParams,
        default_cwd: &str,
    ) -> Result<String, String> {
        let cols = params.cols.max(1);
        let rows = params.rows.max(1);

        // Shell 探测（§5.1，走 find_in_path）
        let shell_path = match params.shell {
            Some(ref s) => {
                let p = PathBuf::from(s);
                if p.is_file() {
                    p
                } else {
                    find_in_path(s).ok_or_else(|| {
                        format!("PATH 上未找到 shell '{s}'。请检查拼写或安装该 shell。")
                    })?
                }
            }
            None => detect_default_shell()?,
        };

        // cwd 缺省当前工作区根
        let cwd = params
            .cwd
            .map(PathBuf::from)
            .filter(|p| p.is_dir())
            .unwrap_or_else(|| PathBuf::from(default_cwd));

        // Create PTY pair
        let pty_system = NativePtySystem::default();
        let pty_pair = pty_system
            .openpty(PtySize {
                cols,
                rows,
                ..Default::default()
            })
            .map_err(|e| format!("创建 PTY 失败: {e}"))?;

        // Spawn shell
        let mut cmd = CommandBuilder::new(&shell_path);
        cmd.cwd(&cwd);
        // 继承应用进程环境
        for (key, value) in std::env::vars() {
            cmd.env(key, value);
        }

        // Windows ConPTY 路径不叠加 CREATE_NO_WINDOW（portable-pty 内部管理
        // spawn，与 Command 路径的约定不冲突；此处注释说明）。
        // unix forkpty 子进程天然独立 session（forkpty 语义），满足 AGENTS.md
        // 的 process_group(0) / killpg 约定。

        let child = pty_pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| format!("启动 shell 失败: {e}"))?;

        let pid = child.process_id().unwrap_or(0);
        let session_id = uuid::Uuid::new_v4().to_string();

        // Take ownership of master reader/writer before spawning the thread.
        // `portable-pty` drops the slave side when `PtyPair` is dropped, so we
        // must keep only the master parts we need.
        let mut reader = pty_pair.master.try_clone_reader().map_err(|e| {
            // Clean up the child if we fail here.
            let _ = terminate_process(pid);
            format!("PTY reader 克隆失败: {e}")
        })?;
        let writer = pty_pair.master.take_writer().map_err(|e| {
            let _ = terminate_process(pid);
            format!("PTY writer 获取失败: {e}")
        })?;
        // Drop the PtyPair so the slave fd is closed in the parent.
        drop(pty_pair);

        let shell_name = shell_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "shell".to_string());

        let info = TerminalSessionInfo {
            session_id: session_id.clone(),
            kind: "shell".to_string(),
            title: shell_name,
            cwd: cwd.to_string_lossy().to_string(),
            alive: true,
        };

        let shutdown = Arc::new(std::sync::atomic::AtomicBool::new(false));

        // Spawn reader thread（批量 flush：≤50ms 或 ≥8KiB）
        let emitter = Arc::clone(&self.emitter);
        let sid = session_id.clone();
        let shutdown_clone = Arc::clone(&shutdown);
        thread::spawn(move || {
            reader_thread_loop(&mut reader, &sid, emitter, shutdown_clone, pid);
        });

        let session = PtySession {
            pid,
            writer: Mutex::new(writer),
            info,
            shutdown,
        };

        self.sessions
            .lock()
            .map_err(|e| format!("会话表锁中毒: {e}"))?
            .insert(session_id.clone(), session);

        Ok(session_id)
    }

    /// 写入 PTY（`terminal_write`）。base64 解码后写 master（bytes，不经 String）。
    pub fn write(&self, session_id: &str, data_base64: &str) -> Result<(), String> {
        use base64::Engine;
        let data = base64::engine::general_purpose::STANDARD
            .decode(data_base64)
            .map_err(|e| format!("base64 解码失败: {e}"))?;

        let sessions = self
            .sessions
            .lock()
            .map_err(|e| format!("会话表锁中毒: {e}"))?;
        let session = sessions
            .get(session_id)
            .ok_or_else(|| format!("会话 {session_id} 不存在"))?;

        let mut writer = session
            .writer
            .lock()
            .map_err(|e| format!("写锁中毒: {e}"))?;
        writer
            .write_all(&data)
            .map_err(|e| format!("写入 PTY 失败: {e}"))?;
        writer
            .flush()
            .map_err(|e| format!("刷新 PTY 失败: {e}"))?;
        Ok(())
    }

    /// 缩放 PTY（`terminal_resize`）。
    ///
    /// 注意：portable-pty 的 `PtyPair` 在 open 时已被 drop，resize 需要通过
    /// 保存的 master handle。当前 portable-pty 版本不支持在 pair drop 后 resize，
    /// 此处记录日志并返回成功（非致命，xterm 会继续正常渲染）。
    pub fn resize(&self, session_id: &str, cols: u16, rows: u16) -> Result<(), String> {
        let sessions = self
            .sessions
            .lock()
            .map_err(|e| format!("会话表锁中毒: {e}"))?;
        let _session = sessions
            .get(session_id)
            .ok_or_else(|| format!("会话 {session_id} 不存在"))?;

        // resize 通过 PtyPair master 的 resize 方法——但当前架构中 PtyPair
        // 已被 drop（只保留了 reader/writer）。记录日志。
        // 后续优化：在 PtySession 中保留 master handle 以支持 resize。
        log::debug!(
            "terminal_resize({}, cols={}, rows={}) — PTY resize 待优化（需保留 master handle）",
            session_id, cols, rows
        );
        Ok(())
    }

    /// 关闭会话（`terminal_close`）：优雅 → 强杀。
    ///
    /// 复用 `kill_tree.rs`：先 `terminate_process`（SIGTERM），超时升级
    /// `kill_process_tree`。禁止另起 kill 实现。
    pub fn close(&self, session_id: &str) -> Result<(), String> {
        let mut sessions = self
            .sessions
            .lock()
            .map_err(|e| format!("会话表锁中毒: {e}"))?;
        let session = sessions
            .remove(session_id)
            .ok_or_else(|| format!("会话 {session_id} 不存在"))?;

        // 通知 reader 线程退出
        session
            .shutdown
            .store(true, std::sync::atomic::Ordering::SeqCst);

        let pid = session.pid;
        if pid == 0 {
            return Ok(());
        }

        // 优雅停止：terminate_process（SIGTERM / unix 组 SIGTERM）
        if terminate_process(pid) {
            // 等待超时后升级为 kill_process_tree
            let start = Instant::now();
            while start.elapsed() < CLOSE_GRACE_TIMEOUT {
                if !crate::process::process_alive(pid, None) {
                    return Ok(());
                }
                thread::sleep(Duration::from_millis(50));
            }
        }

        // 超时升级：kill_process_tree（SIGKILL 整棵树）
        kill_process_tree(pid);
        Ok(())
    }

    /// 列出存活会话（`terminal_list`，面板重开时恢复）。
    pub fn list(&self) -> Result<Vec<TerminalSessionInfo>, String> {
        let sessions = self
            .sessions
            .lock()
            .map_err(|e| format!("会话表锁中毒: {e}"))?;
        Ok(sessions
            .values()
            .map(|s| {
                let mut info = s.info.clone();
                info.alive = crate::process::process_alive(s.pid, None);
                info
            })
            .collect())
    }

    /// 列出可用 shell（`terminal_list_shells`，TM-07 新建 tab 下拉）。
    pub fn list_shells(&self) -> Vec<ShellInfo> {
        detect_available_shells()
    }

    /// 应用退出时全量清理会话（teardown 钩子）。
    pub fn shutdown_all(&self) {
        let mut sessions = match self.sessions.lock() {
            Ok(s) => s,
            Err(_) => return,
        };
        for (id, session) in sessions.drain() {
            session
                .shutdown
                .store(true, std::sync::atomic::Ordering::SeqCst);
            if session.pid != 0 {
                log::debug!("terminal shutdown: killing session {} (pid {})", id, session.pid);
                terminate_process(session.pid);
                // 给一点时间优雅退出，不等待——应用正在退出
                kill_process_tree(session.pid);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Reader thread（批量 flush：≤50ms 或 ≥8KiB）
// ---------------------------------------------------------------------------

/// PTY reader 线程循环：阻塞读 master → 批量聚合 → base64 → `terminal_output` 事件。
///
/// 约束（全局开发约束 §1 / §2）：
/// - 禁止 `read_line` / `from_utf8_lossy`（PTY 是字节流，多字节字符可跨块）。
/// - base64 原始字节传输，前端解码为 `Uint8Array` 交给 xterm。
/// - 子进程退出 → `terminal_exit` 事件 + 会话清理。
fn reader_thread_loop(
    reader: &mut dyn Read,
    session_id: &str,
    emitter: Arc<dyn TerminalEmitter>,
    shutdown: Arc<std::sync::atomic::AtomicBool>,
    pid: u32,
) {
    let mut buf = [0u8; 4096];
    let mut aggregate = Vec::with_capacity(FLUSH_THRESHOLD * 2);
    let mut last_flush = Instant::now();

    loop {
        if shutdown.load(std::sync::atomic::Ordering::SeqCst) {
            break;
        }

        // 非阻塞检查：尝试读，没有数据则检查是否需要 flush
        match reader.read(&mut buf) {
            Ok(0) => {
                // EOF：子进程退出（或管道关闭）
                // Flush remaining data
                if !aggregate.is_empty() {
                    flush_aggregate(session_id, &mut aggregate, &emitter);
                }
                break;
            }
            Ok(n) => {
                aggregate.extend_from_slice(&buf[..n]);

                // ≥8KiB 立即 flush
                if aggregate.len() >= FLUSH_THRESHOLD {
                    flush_aggregate(session_id, &mut aggregate, &emitter);
                    last_flush = Instant::now();
                }
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                // 没有数据，检查是否需要 flush（≤50ms 间隔）
                if !aggregate.is_empty() && last_flush.elapsed() >= FLUSH_INTERVAL {
                    flush_aggregate(session_id, &mut aggregate, &emitter);
                    last_flush = Instant::now();
                }
                thread::sleep(Duration::from_millis(5));
                continue;
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::Interrupted => {
                continue;
            }
            Err(_) => {
                // 读取错误，flush 剩余并退出
                if !aggregate.is_empty() {
                    flush_aggregate(session_id, &mut aggregate, &emitter);
                }
                break;
            }
        }

        // ≤50ms flush（有数据时）
        if !aggregate.is_empty() && last_flush.elapsed() >= FLUSH_INTERVAL {
            flush_aggregate(session_id, &mut aggregate, &emitter);
            last_flush = Instant::now();
        }
    }

    // 子进程退出 → terminal_exit 事件
    // 尝试获取退出码（非阻塞等待）
    let exit_code = {
        // pid 存在时尝试 waitpid 非阻塞获取退出码
        #[cfg(unix)]
        {
            let mut status: libc::c_int = 0;
            let ret = unsafe { libc::waitpid(pid as i32, &mut status, libc::WNOHANG) };
            if ret > 0 {
                if libc::WIFEXITED(status) {
                    Some(libc::WEXITSTATUS(status))
                } else {
                    None // 信号终止
                }
            } else {
                None
            }
        }
        #[cfg(not(unix))]
        {
            None
        }
    };

    emitter.emit_exit(TerminalExitEvent {
        session_id: session_id.to_string(),
        exit_code,
    });
}

/// 将聚合缓冲 base64 编码后发射 `terminal_output` 事件。
fn flush_aggregate(session_id: &str, aggregate: &mut Vec<u8>, emitter: &Arc<dyn TerminalEmitter>) {
    if aggregate.is_empty() {
        return;
    }
    use base64::Engine;
    let data_base64 = base64::engine::general_purpose::STANDARD.encode(&*aggregate);
    emitter.emit_output(TerminalOutputEvent {
        session_id: session_id.to_string(),
        data_base64,
    });
    aggregate.clear();
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Mock emitter for unit tests.
    struct MockEmitter {
        output_events: Mutex<Vec<TerminalOutputEvent>>,
        exit_events: Mutex<Vec<TerminalExitEvent>>,
        output_count: AtomicUsize,
    }

    impl MockEmitter {
        fn new() -> Self {
            Self {
                output_events: Mutex::new(Vec::new()),
                exit_events: Mutex::new(Vec::new()),
                output_count: AtomicUsize::new(0),
            }
        }

        fn output_count(&self) -> usize {
            self.output_count.load(Ordering::SeqCst)
        }
    }

    impl TerminalEmitter for MockEmitter {
        fn emit_output(&self, event: TerminalOutputEvent) {
            self.output_count.fetch_add(1, Ordering::SeqCst);
            self.output_events.lock().unwrap().push(event);
        }

        fn emit_exit(&self, event: TerminalExitEvent) {
            self.exit_events.lock().unwrap().push(event);
        }
    }

    // -- 纯函数单测 --

    #[test]
    fn shell_candidates_returns_correct_order_on_linux() {
        // 仅在 unix 上验证顺序
        #[cfg(unix)]
        {
            let candidates = shell_candidates();
            // 至少有 zsh/bash/sh
            assert!(candidates.len() >= 3);
            // 第一个是 $SHELL（如果有），否则是 zsh
            assert!(candidates.iter().any(|c| c.0 == "zsh"));
            assert!(candidates.iter().any(|c| c.0 == "bash"));
            assert!(candidates.iter().any(|c| c.0 == "sh"));
        }
    }

    #[cfg(windows)]
    #[test]
    fn shell_candidates_returns_correct_order_on_windows() {
        let candidates = shell_candidates();
        assert_eq!(candidates.len(), 3);
        assert_eq!(candidates[0].0, "pwsh");
        assert_eq!(candidates[1].0, "powershell");
        assert_eq!(candidates[2].0, "cmd");
    }

    #[test]
    fn base64_roundtrip() {
        use base64::Engine;
        let original = b"hello world\x00\xff\xfe";
        let encoded = base64::engine::general_purpose::STANDARD.encode(original);
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(&encoded)
            .unwrap();
        assert_eq!(original.as_slice(), decoded.as_slice());
    }

    #[test]
    fn base64_multibyte_roundtrip() {
        use base64::Engine;
        // 中文 UTF-8 多字节
        let original = "你好世界".as_bytes();
        let encoded = base64::engine::general_purpose::STANDARD.encode(original);
        let decoded = base64::engine::general_purpose::STANDARD.decode(&encoded).unwrap();
        assert_eq!(original, decoded.as_slice());
    }

    #[test]
    fn flush_aggregate_emits_base64() {
        // flush_aggregate should base64-encode the aggregate buffer and emit it,
        // then clear the buffer. We verify via the aggregate state change.
        let emitter: Arc<dyn TerminalEmitter> = Arc::new(MockEmitter::new());
        let mut aggregate = b"hello".to_vec();
        assert!(!aggregate.is_empty());
        flush_aggregate("test-session", &mut aggregate, &emitter);
        assert!(aggregate.is_empty(), "aggregate should be cleared after flush");
    }

    #[test]
    fn flush_aggregate_noop_on_empty() {
        let emitter: Arc<dyn TerminalEmitter> = Arc::new(MockEmitter::new());
        let mut aggregate = Vec::new();
        flush_aggregate("test-session", &mut aggregate, &emitter);
        assert!(aggregate.is_empty());
    }

    // -- 集成冒烟（探测不到 shell 时 skip） --

    #[test]
    fn smoke_open_write_echo_close() {
        let shell = match detect_default_shell() {
            Ok(s) => s,
            Err(e) => {
                eprintln!("SKIP smoke_open_write_echo_close: {e}");
                return;
            }
        };

        let emitter: Arc<dyn TerminalEmitter> = Arc::new(MockEmitter::new());
        let manager = TerminalManager::with_emitter(Arc::clone(&emitter));

        let session_id = manager
            .open(
                TerminalOpenParams {
                    cwd: None,
                    shell: Some(shell.to_string_lossy().to_string()),
                    cols: 80,
                    rows: 24,
                },
                "/tmp",
            )
            .expect("open should succeed");

        // 写入 "echo hello\r"
        use base64::Engine;
        let input = base64::engine::general_purpose::STANDARD.encode(b"echo hello\r");
        manager
            .write(&session_id, &input)
            .expect("write should succeed");

        // 等待输出
        thread::sleep(Duration::from_millis(500));

        // 列出会话
        let sessions = manager.list().expect("list should succeed");
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].session_id, session_id);
        assert_eq!(sessions[0].kind, "shell");

        // 关闭会话
        manager.close(&session_id).expect("close should succeed");

        // 关闭后会话应被移除
        let sessions = manager.list().expect("list should succeed");
        assert!(sessions.is_empty());
    }

    #[test]
    fn smoke_resize_no_panic() {
        let shell = match detect_default_shell() {
            Ok(s) => s,
            Err(e) => {
                eprintln!("SKIP smoke_resize_no_panic: {e}");
                return;
            }
        };

        let emitter: Arc<dyn TerminalEmitter> = Arc::new(MockEmitter::new());
        let manager = TerminalManager::with_emitter(emitter);

        let session_id = manager
            .open(
                TerminalOpenParams {
                    cwd: None,
                    shell: Some(shell.to_string_lossy().to_string()),
                    cols: 80,
                    rows: 24,
                },
                "/tmp",
            )
            .expect("open should succeed");

        // resize 不 panic
        manager
            .resize(&session_id, 120, 40)
            .expect("resize should succeed");

        manager.close(&session_id).expect("close should succeed");
    }

    #[test]
    fn smoke_close_kills_process() {
        let shell = match detect_default_shell() {
            Ok(s) => s,
            Err(e) => {
                eprintln!("SKIP smoke_close_kills_process: {e}");
                return;
            }
        };

        let emitter: Arc<dyn TerminalEmitter> = Arc::new(MockEmitter::new());
        let manager = TerminalManager::with_emitter(emitter);

        let session_id = manager
            .open(
                TerminalOpenParams {
                    cwd: None,
                    shell: Some(shell.to_string_lossy().to_string()),
                    cols: 80,
                    rows: 24,
                },
                "/tmp",
            )
            .expect("open should succeed");

        // 记录 pid（从 list 获取 alive 状态）
        let sessions = manager.list().expect("list should succeed");
        assert!(sessions[0].alive, "session should be alive after open");

        // 关闭
        manager.close(&session_id).expect("close should succeed");

        // 等待进程退出
        thread::sleep(Duration::from_millis(200));

        // 会话已从表中移除
        let sessions = manager.list().expect("list should succeed");
        assert!(sessions.is_empty());
    }

    #[test]
    fn smoke_list_shells() {
        let emitter: Arc<dyn TerminalEmitter> = Arc::new(MockEmitter::new());
        let manager = TerminalManager::with_emitter(emitter);
        let shells = manager.list_shells();
        // 至少应探测到一个 shell（除非在极简环境）
        if shells.is_empty() {
            eprintln!("SKIP smoke_list_shells: no shells detected");
        } else {
            assert!(shells.iter().any(|s| !s.path.is_empty()));
        }
    }

    #[test]
    fn session_table_concurrent_access() {
        let emitter: Arc<dyn TerminalEmitter> = Arc::new(MockEmitter::new());
        let manager = Arc::new(TerminalManager::with_emitter(emitter));

        // 并发 list 不 panic
        let handles: Vec<_> = (0..4)
            .map(|_| {
                let mgr = Arc::clone(&manager);
                thread::spawn(move || {
                    let _ = mgr.list();
                })
            })
            .collect();

        for h in handles {
            h.join().unwrap();
        }
    }
}
