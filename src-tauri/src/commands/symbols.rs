//! Symbol Index 命令（T-28）。
//!
//! 查询串支持过滤器 token（与批量选择器风格一致）：
//! `@repo:`（路径包含）/ `@group:`（分组名）/ `@status:`（仓库状态，
//! 复用 T-20 facet 引擎）/ `@ext:`（扩展名，逗号多值）/ `@path:`（路径包含），
//! 其余 token 为符号名关键字（AND LIKE）。仓库级过滤（repo/group/status）
//! 在命令层解析为仓库路径集后交给查询层收敛。

use std::path::Path;

use tauri::State;

use crate::commands::batch;
use crate::error::{AppError, AppResult};
use crate::state::AppState;
use crate::symbols::index::{
    self, parse_filters, CallHit, IndexStats, RefHit, RepoScope, SymbolHit,
};

/// 从符号/引用表把仓库级过滤解析成 `RepoScope`。
/// `@repo` / `@group` 走 DB；`@status` 复用 T-20 facet（需 workspace_id）。
fn resolve_scope(
    conn: &rusqlite::Connection,
    state: &State<'_, AppState>,
    filters: &index::RawFilters,
    workspace_id: Option<i64>,
) -> AppResult<RepoScope> {
    if filters.repos.is_empty() && filters.groups.is_empty() && filters.statuses.is_empty() {
        return Ok(RepoScope { repo_paths: None });
    }

    let mut sets: Vec<Vec<String>> = Vec::new();

    for repo_token in &filters.repos {
        let mut stmt = conn.prepare(
            "SELECT path FROM repositories WHERE is_deleted = 0 \
             AND replace(path, char(92), '/') LIKE '%' || ?1",
        )?;
        let paths = stmt
            .query_map([repo_token], |row| row.get::<_, String>(0))?
            .filter_map(|r| r.ok())
            .collect::<Vec<_>>();
        sets.push(paths);
    }

    for group in &filters.groups {
        let mut stmt = conn.prepare(
            "SELECT r.path FROM repositories r \
             JOIN repo_groups g ON r.group_id = g.id \
             WHERE r.is_deleted = 0 AND g.name = ?1",
        )?;
        let paths = stmt
            .query_map([group], |row| row.get::<_, String>(0))?
            .filter_map(|r| r.ok())
            .collect::<Vec<_>>();
        sets.push(paths);
    }

    for status in &filters.statuses {
        let ws_id = workspace_id.ok_or_else(|| {
            AppError::Other(
                "@status: 过滤需要当前工作区上下文（请在工作区内使用该过滤器）".to_string(),
            )
        })?;
        let paths = batch::facet_repo_paths(
            conn,
            &state.status_cache,
            ws_id,
            &format!("@status:{status}"),
        )?;
        sets.push(paths);
    }

    // 各类过滤 AND 语义：集合求交
    let mut iter = sets.into_iter();
    let first = iter.next().unwrap_or_default();
    let mut acc = first;
    for set in iter {
        let set_norm: std::collections::HashSet<String> =
            set.into_iter().map(|p| p.replace('\\', "/")).collect();
        acc.retain(|p| set_norm.contains(&p.replace('\\', "/")));
        if acc.is_empty() {
            break;
        }
    }

    Ok(RepoScope {
        repo_paths: Some(acc),
    })
}

fn index_repo_common(
    repo_path: &str,
    state: &State<'_, AppState>,
    files: Option<Vec<String>>,
) -> AppResult<IndexStats> {
    let mut conn = state
        .db
        .lock()
        .map_err(|e| AppError::Other(format!("DB lock error: {}", e)))?;
    let repo_id = index::repo_id_for_path(&conn, repo_path)?;
    let root = Path::new(repo_path);
    match files {
        Some(files) if !files.is_empty() => index::reindex_files(&mut conn, root, repo_id, &files),
        _ => index::reindex_repo(&mut conn, root, repo_id),
    }
}

/// 构建符号索引（全量增量）；`files` 给定时只重解析这些相对路径文件
/// （单文件变更场景）。
#[tauri::command]
pub fn build_symbol_index(
    repo_path: String,
    files: Option<Vec<String>>,
    state: State<'_, AppState>,
) -> AppResult<IndexStats> {
    index_repo_common(&repo_path, &state, files)
}

/// 符号搜索：过滤器 token + 名称关键字。
#[tauri::command]
pub fn search_symbols(
    query: String,
    workspace_id: Option<i64>,
    state: State<'_, AppState>,
) -> AppResult<Vec<SymbolHit>> {
    let (filters, tokens) = parse_filters(&query);
    let conn = state
        .db
        .lock()
        .map_err(|e| AppError::Other(format!("DB lock error: {}", e)))?;
    let scope = resolve_scope(&conn, &state, &filters, workspace_id)?;
    index::search_symbols(&conn, &scope, &filters.exts, &filters.paths, &tokens)
}

/// 精确名称 → 定义列表（Go To Definition）。
#[tauri::command]
pub fn find_symbol_definitions(
    name: String,
    query: Option<String>,
    workspace_id: Option<i64>,
    state: State<'_, AppState>,
) -> AppResult<Vec<SymbolHit>> {
    let (filters, _tokens) = parse_filters(query.as_deref().unwrap_or(""));
    let conn = state
        .db
        .lock()
        .map_err(|e| AppError::Other(format!("DB lock error: {}", e)))?;
    let scope = resolve_scope(&conn, &state, &filters, workspace_id)?;
    index::find_definitions(&conn, &name, &scope, &filters.exts, &filters.paths)
}

/// 精确名称 → 引用列表（Find References，is_call 标记调用点）。
#[tauri::command]
pub fn find_symbol_references(
    name: String,
    query: Option<String>,
    workspace_id: Option<i64>,
    state: State<'_, AppState>,
) -> AppResult<Vec<RefHit>> {
    let (filters, _tokens) = parse_filters(query.as_deref().unwrap_or(""));
    let conn = state
        .db
        .lock()
        .map_err(|e| AppError::Other(format!("DB lock error: {}", e)))?;
    let scope = resolve_scope(&conn, &state, &filters, workspace_id)?;
    index::find_references(&conn, &name, &scope, &filters.exts, &filters.paths)
}

/// 调用层级：direction = "callers" | "callees"。
#[tauri::command]
pub fn symbol_call_hierarchy(
    name: String,
    direction: String,
    query: Option<String>,
    workspace_id: Option<i64>,
    state: State<'_, AppState>,
) -> AppResult<Vec<CallHit>> {
    let (filters, _tokens) = parse_filters(query.as_deref().unwrap_or(""));
    let conn = state
        .db
        .lock()
        .map_err(|e| AppError::Other(format!("DB lock error: {}", e)))?;
    let scope = resolve_scope(&conn, &state, &filters, workspace_id)?;
    index::call_hierarchy(
        &conn,
        &name,
        &direction,
        &scope,
        &filters.exts,
        &filters.paths,
    )
}
