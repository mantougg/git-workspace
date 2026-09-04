use serde::Serialize;
use tauri::State;

use crate::error::AppResult;
use crate::state::AppState;

/// Restart the application after a downloaded update has been installed.
#[tauri::command]
pub fn restart_app(app: tauri::AppHandle) {
    app.restart();
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
}
