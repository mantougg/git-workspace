//! 终端 Tauri commands（TM-01，terminal-feature-plan §4.2 IPC 契约）。
//!
//! 5 个 command + 2 个事件（`terminal_output` / `terminal_exit`），
//! 名称/payload 与方案 §4.2 完全一致（camelCase payload，事件名无 `.`）。

use tauri::State;

use crate::process::pty::{
    ShellInfo, TerminalCloseParams, TerminalOpenParams, TerminalResizeParams, TerminalSessionInfo,
    TerminalWriteParams,
};
use crate::state::AppState;

/// 打开新 PTY 会话（shell 探测按 §5.1 顺序，cwd 缺省当前工作区根）。
#[tauri::command]
pub async fn terminal_open(
    state: State<'_, AppState>,
    params: TerminalOpenParams,
) -> Result<String, String> {
    // cwd 缺省当前工作区根（从 db 第一个 workspace 取，fallback 到进程 cwd）
    let default_cwd = get_default_cwd(&state);
    state.terminal.open(params, &default_cwd)
}

/// 获取默认工作目录：从数据库第一个 workspace 取根路径，fallback 到进程 cwd。
fn get_default_cwd(state: &AppState) -> String {
    // 尝试从数据库取第一个 workspace 的根路径
    if let Ok(conn) = state.db.lock() {
        if let Ok(mut stmt) = conn.prepare("SELECT root_path FROM workspaces LIMIT 1") {
            if let Ok(mut rows) = stmt.query([]) {
                if let Ok(Some(row)) = rows.next() {
                    if let Ok(path) = row.get::<_, String>(0) {
                        return path;
                    }
                }
            }
        }
    }
    // fallback: 进程当前目录
    std::env::current_dir()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string()
}

/// 向 PTY 写入（base64 解码后写 master，bytes，不经 String）。
#[tauri::command]
pub async fn terminal_write(
    state: State<'_, AppState>,
    params: TerminalWriteParams,
) -> Result<(), String> {
    state.terminal.write(&params.session_id, &params.data_base64)
}

/// 缩放 PTY（xterm fit 时同步）。
#[tauri::command]
pub async fn terminal_resize(
    state: State<'_, AppState>,
    params: TerminalResizeParams,
) -> Result<(), String> {
    state
        .terminal
        .resize(&params.session_id, params.cols, params.rows)
}

/// 关闭会话（优雅 → 强杀，复用 kill_tree.rs）。
#[tauri::command]
pub async fn terminal_close(
    state: State<'_, AppState>,
    params: TerminalCloseParams,
) -> Result<(), String> {
    state.terminal.close(&params.session_id)
}

/// 列出存活会话（面板重开时恢复）。
#[tauri::command]
pub async fn terminal_list(
    state: State<'_, AppState>,
) -> Result<Vec<TerminalSessionInfo>, String> {
    state.terminal.list()
}

/// 列出可用 shell（TM-07 新建 tab 下拉，按 §5.1 顺序）。
#[tauri::command]
pub async fn terminal_list_shells(state: State<'_, AppState>) -> Result<Vec<ShellInfo>, String> {
    Ok(state.terminal.list_shells())
}
