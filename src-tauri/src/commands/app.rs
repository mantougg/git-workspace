use serde::Serialize;
use tauri::State;

use crate::error::AppResult;
use crate::state::AppState;

/// Restart the application after a downloaded update has been installed.
#[tauri::command]
pub fn restart_app(app: tauri::AppHandle) {
    app.restart();
}

/// F-53：检查更新的代理兜底。返回规范化后的系统代理 URL
/// （如 `http://127.0.0.1:7897`），未配置/未启用返回 None。
///
/// 顺序：环境变量（`HTTPS_PROXY`/`HTTP_PROXY`/`ALL_PROXY`，大小写均认）→
/// Windows IE 系统代理（注册表 HKCU Internet Settings）。reqwest 默认
/// 已读环境变量，这里显式返回是给 plugin-updater 的 `check({proxy})`
/// 在直连失败时重试用。macOS/Linux 回落到环境变量（reqwest 同源）。
#[tauri::command]
pub fn get_system_proxy() -> Option<String> {
    detect_system_proxy()
}

fn detect_system_proxy() -> Option<String> {
    for key in [
        "HTTPS_PROXY",
        "https_proxy",
        "HTTP_PROXY",
        "http_proxy",
        "ALL_PROXY",
        "all_proxy",
    ] {
        if let Ok(v) = std::env::var(key) {
            if let Some(p) = normalize_proxy_value(&v) {
                return Some(p);
            }
        }
    }
    #[cfg(windows)]
    {
        windows_registry_proxy()
    }
    #[cfg(not(windows))]
    {
        None
    }
}

/// Windows IE 系统代理：`ProxyEnable` 为 1 且 `ProxyServer` 非空时生效。
#[cfg(windows)]
fn windows_registry_proxy() -> Option<String> {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;
    let settings = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Internet Settings")
        .ok()?;
    let enabled: u32 = settings.get_value("ProxyEnable").ok()?;
    if enabled == 0 {
        return None;
    }
    let server: String = settings.get_value("ProxyServer").ok()?;
    normalize_proxy_value(&server)
}

/// 规范化代理值为 `scheme://host:port`：
/// - 已带 scheme（http/https/socks5…）→ 原样返回；
/// - 裸 `host:port`（IE ProxyServer 常见形态）→ 补 `http://`；
/// - IE 分协议形式 `http=h:1;https=h:2;ftp=…` → 取 https，无则 http。
/// 空串 / 无法解析 → None。
fn normalize_proxy_value(raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    if raw.contains('=') {
        // 分协议形式：优先 https 条目。
        let mut http_entry: Option<&str> = None;
        for part in raw.split(';') {
            let part = part.trim();
            if let Some((scheme, addr)) = part.split_once('=') {
                let scheme = scheme.trim().to_ascii_lowercase();
                let addr = addr.trim();
                if addr.is_empty() {
                    continue;
                }
                if scheme == "https" {
                    return normalize_proxy_value(addr);
                }
                if scheme == "http" {
                    http_entry = Some(addr);
                }
            }
        }
        return http_entry.and_then(normalize_proxy_value);
    }
    if raw.contains("://") {
        return Some(raw.to_string());
    }
    // 裸 host:port：至少要含冒号端口才算可解析。
    if raw.rsplit_once(':').map(|(_, p)| p.chars().all(|c| c.is_ascii_digit()) && !p.is_empty()) != Some(true) {
        return None;
    }
    Some(format!("http://{raw}"))
}

/// F-38：清除数据（关于页，二次确认后调用）。只清历史与缓存类表
/// （可重建 / 纯历史），保留全部手动配置表。逐表返回清除行数。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TableClearResult {
    pub table: String,
    pub deleted: u64,
}

/// 可清除的历史/缓存表（顺序按 FK 依赖：先子表后父表）。
/// 保留配置与用户数据：workspaces / repositories / repo_groups / tasks* /
/// change_sets* / workspace_stashes* / runtime_projects / jdks /
/// maven_executables / node_* / ai_providers / ai_models / ai_settings /
/// ai_task_defaults / plugin_actions / scheduled_tasks。
const CLEARABLE_TABLES: &[&str] = &[
    // Runtime 依赖索引（引用 maven_projects，先清；可由「解析依赖」重建）
    "runtime_dependencies",
    // Maven 索引缓存（重新扫描/解析可重建）
    "maven_source_mappings",
    "maven_artifacts",
    "maven_dependencies",
    "maven_modules",
    "maven_projects",
    // AI 历史与缓存
    "ai_result_cache",
    "ai_requests",
    "ai_messages",
    "ai_sessions",
    "ai_proposals",
    "ai_reviews",
    "ai_tasks",
    // 符号索引缓存
    "symbol_references",
    "symbol_refs",
    "symbol_index_files",
    "symbols",
    // 仓库索引缓存（重新扫描仓库可重建）
    "file_status",
    "repo_status",
    "commit_files",
    "commit_parents",
    "commits",
    "branches",
    "remote_branches",
    "tags",
    "stashes",
    "worktrees",
    // 运行历史
    "operation_log_items",
    "operation_logs",
    "task_history",
    "runtime_processes",
];

/// 清除历史与缓存表。单事务执行，任一表失败整体回滚。
/// 表名均为内部常量，无注入风险。
#[tauri::command]
pub fn clear_cached_data(state: State<'_, AppState>) -> AppResult<Vec<TableClearResult>> {
    let mut conn = state
        .db
        .lock()
        .map_err(|_| rusqlite::Error::InvalidQuery)?;
    let tx = conn.transaction()?;
    let mut results = Vec::with_capacity(CLEARABLE_TABLES.len());
    for table in CLEARABLE_TABLES {
        let deleted = tx.execute(&format!("DELETE FROM {table}"), [])?;
        results.push(TableClearResult {
            table: table.to_string(),
            deleted: deleted as u64,
        });
    }
    tx.commit()?;
    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 清除范围纪律：配置/用户数据表绝不能进 CLEARABLE_TABLES。
    #[test]
    fn clearable_tables_exclude_config_and_user_data() {
        const PROTECTED: &[&str] = &[
            "workspaces",
            "repositories",
            "repo_groups",
            "tasks",
            "task_items",
            "task_dependencies",
            "change_sets",
            "change_set_repositories",
            "workspace_stashes",
            "workspace_stash_items",
            "runtime_projects",
            "jdks",
            "maven_executables",
            "node_projects",
            "node_executables",
            "ai_providers",
            "ai_models",
            "ai_settings",
            "ai_task_defaults",
            "plugin_actions",
            "scheduled_tasks",
        ];
        for table in PROTECTED {
            assert!(
                !CLEARABLE_TABLES.contains(table),
                "{table} 是配置/用户数据表，禁止进入清除范围"
            );
        }
    }

    /// F-53 代理值规范化：裸 host:port 补 scheme；分协议形式取 https；
    /// 已带 scheme 原样；空串/无端口 → None。
    #[test]
    fn normalize_proxy_value_cases() {
        assert_eq!(
            normalize_proxy_value("127.0.0.1:7897").as_deref(),
            Some("http://127.0.0.1:7897")
        );
        assert_eq!(
            normalize_proxy_value("http=127.0.0.1:7890;https=127.0.0.1:7897").as_deref(),
            Some("http://127.0.0.1:7897")
        );
        assert_eq!(
            normalize_proxy_value("http=10.0.0.1:8080;ftp=10.0.0.1:2121").as_deref(),
            Some("http://10.0.0.1:8080")
        );
        assert_eq!(
            normalize_proxy_value("socks5://127.0.0.1:1080").as_deref(),
            Some("socks5://127.0.0.1:1080")
        );
        assert_eq!(normalize_proxy_value(""), None);
        assert_eq!(normalize_proxy_value("   "), None);
        assert_eq!(normalize_proxy_value("localhost"), None, "无端口不可解析");
    }
}
