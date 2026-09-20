//! 终端 Tauri commands（TM-01，terminal-feature-plan §4.2 IPC 契约）。
//!
//! 5 个 command + 2 个事件（`terminal_output` / `terminal_exit`），
//! 名称/payload 与方案 §4.2 完全一致（camelCase payload，事件名无 `.`）。

use tauri::State;

use crate::process::pty::{
    detect_default_shell, shell_kind, ShellInfo, ShellKind, TerminalCloseParams,
    TerminalOpenParams, TerminalResizeParams, TerminalSessionInfo, TerminalWriteParams,
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

/// TM-06：在终端中启动 runtime（降级模式）。
///
/// 打开一个可交互 Shell tab 并写入启动命令执行。
/// 此模式无健康检查/端口检测/日志落盘，UI 需明示降级。
///
/// 支持 env 注入（按目标 shell 语法适配）和脱敏闸门。
#[tauri::command]
pub async fn runtime_start_in_terminal(
    state: State<'_, AppState>,
    command: String,
    cwd: Option<String>,
    env: Option<std::collections::HashMap<String, String>>,
) -> Result<String, String> {
    // TM-06：脱敏闸门 — 检查 env 是否包含敏感项
    if let Some(ref env_map) = env {
        for (key, value) in env_map {
            if is_sensitive_env(key, value) {
                return Err(format!(
                    "环境变量 {} 包含敏感信息，无法在终端中显示。请使用常规启动模式。",
                    key
                ));
            }
        }
    }

    // 1. 先解析目标 shell，按 shell 语法适配命令行（F-44：PowerShell 行首
    //    引号路径需 `&` 调用运算符，否则 ParserError；env 注入语法随之分流）。
    //    解析来源与 TerminalManager::open 的默认探测是同一个函数，随后把
    //    同一路径显式传给 open，保证适配目标与实际 shell 始终一致。
    let shell_path = detect_default_shell()?;
    let full_command = assemble_command_for_shell(&command, env.as_ref(), shell_kind(&shell_path));

    // 2. 打开 PTY 会话
    let default_cwd = cwd.unwrap_or_else(|| {
        std::env::current_dir()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string()
    });

    let session_id = state.terminal.open(
        TerminalOpenParams {
            cwd: Some(default_cwd.clone()),
            shell: Some(shell_path.to_string_lossy().to_string()),
            cols: 80,
            rows: 24,
        },
        &default_cwd,
    )?;

    // 3. 写入启动命令 + 回车
    use base64::Engine;
    let cmd_bytes = format!("{}\r", full_command);
    let cmd_base64 = base64::engine::general_purpose::STANDARD.encode(cmd_bytes.as_bytes());
    state.terminal.write(&session_id, &cmd_base64)?;

    Ok(session_id)
}

/// 组装写入 PTY 的完整命令行（按目标 shell 语法适配，F-44）。
///
/// - PowerShell：行首为引号（含空格路径被 `launcher::plan_shell_command`
///   加双引号）时补 `&` 调用运算符——否则 PowerShell 把行首字符串当表达式，
///   后续参数触发 ParserError（"表达式或语句中存在意外的标记"）。env 注入
///   `$env:K='V'; …`（`;` 连接兼容 Windows PowerShell 5.1——`&&` 仅 pwsh 7+；
///   `set` 在 PowerShell 是 Set-Variable 别名，不注入进程环境）。
/// - cmd：`set K=V && …`；cmd 的引号首词原生作为命令名，无需 `&`。
/// - posix sh：`K=V …` 前缀；引号首词原生作为命令词，无需处理。
fn assemble_command_for_shell(
    command: &str,
    env: Option<&std::collections::HashMap<String, String>>,
    kind: ShellKind,
) -> String {
    let command = match kind {
        ShellKind::PowerShell if command.starts_with('"') || command.starts_with('\'') => {
            format!("& {command}")
        }
        _ => command.to_string(),
    };
    let Some(env_map) = env else {
        return command;
    };
    if env_map.is_empty() {
        return command;
    }
    match kind {
        ShellKind::PowerShell => {
            // 单引号字符串（PowerShell 不展开变量），内部 `'` 转义为 `''`
            let sets: Vec<String> = env_map
                .iter()
                .map(|(k, v)| format!("$env:{} = '{}'", k, v.replace('\'', "''")))
                .collect();
            format!("{}; {}", sets.join("; "), command)
        }
        ShellKind::Cmd => {
            let sets: Vec<String> = env_map
                .iter()
                .map(|(k, v)| format!("set {}={}", k, v))
                .collect();
            format!("{} && {}", sets.join(" && "), command)
        }
        ShellKind::Posix => {
            let prefix: Vec<String> = env_map
                .iter()
                .map(|(k, v)| format!("{}={}", k, v))
                .collect();
            format!("{} {}", prefix.join(" "), command)
        }
    }
}

/// 检查环境变量是否包含敏感信息。
///
/// 常见敏感模式：
/// - KEY 包含 SECRET/TOKEN/PASSWORD/API_KEY/CREDENTIAL
/// - VALUE 长度 > 20 且看起来像 base64/hex
fn is_sensitive_env(key: &str, value: &str) -> bool {
    let key_upper = key.to_uppercase();
    let sensitive_keywords = [
        "SECRET", "TOKEN", "PASSWORD", "API_KEY", "CREDENTIAL",
        "PRIVATE", "AUTH", "SIGNING",
    ];
    for keyword in &sensitive_keywords {
        if key_upper.contains(keyword) {
            return true;
        }
    }
    // 检查值是否看起来像密钥（长字符串，base64/hex 格式）
    if value.len() > 32 {
        let is_hex = value.chars().all(|c| c.is_ascii_hexdigit());
        let is_base64 = value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '/' || c == '=');
        if is_hex || is_base64 {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env_of(pairs: &[(&str, &str)]) -> std::collections::HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    /// F-44 原始案例：jdk-1.8 在 Program Files（含空格）下，plan_shell_command
    /// 产出引号路径命令，写入 PowerShell 必须带 `&` 调用运算符。
    #[test]
    fn powershell_quoted_executable_gets_call_operator() {
        let cmd = r#""C:\Program Files\Java\jdk-1.8\bin\java.exe" -XX:TieredStopAtLevel=1 -Dmanagement.endpoints.jmx.exposure.include=* -cp pathing.jar com.jxdinfo.hussar.example.HussarApplication"#;
        let line = assemble_command_for_shell(cmd, None, ShellKind::PowerShell);
        assert_eq!(line, format!("& {cmd}"));
    }

    /// 非引号首词在 PowerShell 下原生可执行，不应加 `&`。
    #[test]
    fn powershell_unquoted_executable_unchanged() {
        let cmd = r"C:\tools\mvn.cmd spring-boot:run";
        assert_eq!(
            assemble_command_for_shell(cmd, None, ShellKind::PowerShell),
            cmd
        );
    }

    /// cmd 与 posix sh 的引号首词原生作为命令名/命令词，永不加 `&`。
    #[test]
    fn cmd_and_posix_never_get_call_operator() {
        let cmd = r#""C:\Program Files\Java\jdk-1.8\bin\java.exe" -jar app.jar"#;
        assert_eq!(assemble_command_for_shell(cmd, None, ShellKind::Cmd), cmd);
        assert_eq!(assemble_command_for_shell(cmd, None, ShellKind::Posix), cmd);
    }

    #[test]
    fn powershell_env_uses_env_provider_and_semicolon_join() {
        let env = env_of(&[("SERVER_PORT", "8080"), ("PROFILE", "dev")]);
        let line =
            assemble_command_for_shell("java -jar app.jar", Some(&env), ShellKind::PowerShell);
        // HashMap 顺序不定：两个赋值都出现、以 `; ` 收尾接命令即可
        assert!(line.contains("$env:SERVER_PORT = '8080'"));
        assert!(line.contains("$env:PROFILE = 'dev'"));
        assert!(line.ends_with("; java -jar app.jar"));
    }

    /// PowerShell 单引号字符串内的 `'` 必须转义为 `''`。
    #[test]
    fn powershell_env_value_single_quote_escaped() {
        let env = env_of(&[("A", "x'y")]);
        assert_eq!(
            assemble_command_for_shell("cmdline", Some(&env), ShellKind::PowerShell),
            "$env:A = 'x''y'; cmdline"
        );
    }

    /// PowerShell env + 引号路径：`&` 保留在命令上，赋值在前。
    #[test]
    fn powershell_env_with_quoted_executable() {
        let env = env_of(&[("A", "1")]);
        let cmd = r#""C:\Program Files\Java\jdk-1.8\bin\java.exe" -jar app.jar"#;
        assert_eq!(
            assemble_command_for_shell(cmd, Some(&env), ShellKind::PowerShell),
            format!("$env:A = '1'; & {cmd}")
        );
    }

    #[test]
    fn cmd_env_uses_set_and_double_ampersand() {
        let env = env_of(&[("A", "1")]);
        assert_eq!(
            assemble_command_for_shell("java -jar app.jar", Some(&env), ShellKind::Cmd),
            "set A=1 && java -jar app.jar"
        );
    }

    #[test]
    fn posix_env_uses_prefix_assignments() {
        let env = env_of(&[("A", "1"), ("B", "2")]);
        let line = assemble_command_for_shell("./run.sh", Some(&env), ShellKind::Posix);
        assert!(line.starts_with("A=1 ") || line.starts_with("B=2 "));
        assert!(line.ends_with(" ./run.sh"));
        assert!(line.contains("A=1") && line.contains("B=2"));
    }

    #[test]
    fn none_or_empty_env_returns_command_itself() {
        let cmd = r#""C:\a b\java.exe" -jar app.jar"#;
        for kind in [ShellKind::PowerShell, ShellKind::Cmd, ShellKind::Posix] {
            let expected = if kind == ShellKind::PowerShell {
                format!("& {cmd}")
            } else {
                cmd.to_string()
            };
            assert_eq!(assemble_command_for_shell(cmd, None, kind), expected);
            assert_eq!(
                assemble_command_for_shell(cmd, Some(&std::collections::HashMap::new()), kind),
                expected
            );
        }
    }
}
