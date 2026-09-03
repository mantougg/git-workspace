//! 符号索引落库与查询（T-28）。
//!
//! - 增量：`symbol_index_files` 记录每文件内容 SHA-256，内容未变的文件
//!   直接跳过——「单文件变更只重解析该文件」。
//! - 查询：定义（symbols）/ 引用（symbol_refs）/ 调用层级（引用 + 最近
//!   包含函数相关子查询取最深容器），均命中 name / repo+file 索引。
//! - 过滤：`@ext:` / `@path:` 走 file_path；`@repo:` / `@group:` /
//!   `@status:` 由命令层解析成仓库路径集后以 `repo_id IN (...)` 收敛
//!   （分块参数，规避 SQLite 变量上限）。

use std::collections::HashSet;
use std::fs;
use std::path::Path;

use rusqlite::{params, Connection};
use serde::Serialize;
use sha2::{Digest, Sha256};
use walkdir::WalkDir;

use crate::error::{AppError, AppResult};

use super::extract;
use super::lang::detect_language;

/// 与 code_index（commands/ai.rs）一致的跳过目录。
pub const SKIP_DIRS: &[&str] = &[
    "node_modules",
    "target",
    "dist",
    "build",
    ".git",
    "__pycache__",
    ".next",
    ".nuxt",
    "vendor",
    ".venv",
];

/// 单文件解析上限（字节）——超大文件不进符号索引。
pub const MAX_FILE_BYTES: u64 = 200_000;

/// 单次查询结果上限。
const HIT_LIMIT: i64 = 200;
const REF_LIMIT: i64 = 500;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexStats {
    pub files_scanned: usize,
    pub files_reindexed: usize,
    pub files_skipped: usize,
    pub symbols: usize,
    pub refs: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SymbolHit {
    pub repo_path: String,
    pub file_path: String,
    pub name: String,
    pub kind: String,
    pub line: i64,
    pub end_line: Option<i64>,
    pub container: Option<String>,
    pub signature: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RefHit {
    pub repo_path: String,
    pub file_path: String,
    pub name: String,
    pub line: i64,
    pub is_call: bool,
}

/// 调用层级条目。callers：file/line = 调用点所在函数位置；
/// callees：file/line = 调用者位置、name = 被调用者名。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CallHit {
    pub name: String,
    pub repo_path: String,
    pub file_path: String,
    pub line: i64,
    pub kind: String,
    pub call_count: i64,
}

// ---------------------------------------------------------------------------
// 过滤解析（纯函数）
// ---------------------------------------------------------------------------

#[derive(Default, Debug, PartialEq, Eq)]
pub struct RawFilters {
    pub repos: Vec<String>,
    pub groups: Vec<String>,
    pub statuses: Vec<String>,
    pub exts: Vec<String>,
    pub paths: Vec<String>,
}

/// 解析查询串：`@key:value` token 收敛为过滤，其余 token 为名称关键字。
pub fn parse_filters(raw: &str) -> (RawFilters, Vec<String>) {
    let mut filters = RawFilters::default();
    let mut tokens = Vec::new();
    for token in raw.split_whitespace() {
        if let Some((key, value)) = token.split_once(':') {
            let key = key.trim_start_matches('@').to_ascii_lowercase();
            let value = value.trim().to_string();
            if value.is_empty() {
                continue;
            }
            match key.as_str() {
                "repo" => filters.repos.push(value),
                "group" => filters.groups.push(value),
                "status" => filters.statuses.push(value),
                "ext" => {
                    // @ext:rs,ts 支持逗号多值
                    for ext in value.split(',').map(str::trim).filter(|s| !s.is_empty()) {
                        filters.exts.push(ext.trim_start_matches('.').to_string());
                    }
                }
                "path" => filters.paths.push(value),
                _ => tokens.push(token.to_string()),
            }
        } else {
            tokens.push(token.to_string());
        }
    }
    (filters, tokens)
}

// ---------------------------------------------------------------------------
// 仓库定位
// ---------------------------------------------------------------------------

/// 按归一化路径定位仓库 id（精确 → 后缀匹配；缺行报可行动错误）。
pub fn repo_id_for_path(conn: &Connection, repo_path: &str) -> AppResult<i64> {
    let key = repo_path.replace('\\', "/");
    if let Ok(id) = conn.query_row(
        "SELECT id FROM repositories WHERE is_deleted = 0 \
         AND replace(path, char(92), '/') = ?1 LIMIT 1",
        params![key],
        |row| row.get::<_, i64>(0),
    ) {
        return Ok(id);
    }
    // 后缀匹配（用户传了仓库子目录等场景）
    if let Ok(id) = conn.query_row(
        "SELECT id FROM repositories WHERE is_deleted = 0 \
         AND replace(path, char(92), '/') LIKE '%' || ?1 LIMIT 1",
        params![key],
        |row| row.get::<_, i64>(0),
    ) {
        return Ok(id);
    }
    Err(AppError::NotFound(format!(
        "未在仓库列表中找到 {repo_path}，请先扫描工作区后再构建符号索引"
    )))
}

// ---------------------------------------------------------------------------
// 索引构建
// ---------------------------------------------------------------------------

/// 全量走查 + 按 hash 增量重建一个仓库（未变更文件直接跳过）。
/// 磁盘上已删除的文件从索引移除。
pub fn reindex_repo(conn: &mut Connection, repo_root: &Path, repo_id: i64) -> AppResult<IndexStats> {
    let mut stats = IndexStats {
        files_scanned: 0,
        files_reindexed: 0,
        files_skipped: 0,
        symbols: 0,
        refs: 0,
    };
    let mut seen: HashSet<String> = HashSet::new();

    let mut walker = WalkDir::new(repo_root).into_iter();
    while let Some(entry) = walker.next() {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        if entry.file_type().is_dir() {
            let name = entry.file_name().to_string_lossy();
            // 跳过依赖/构建产物目录与隐藏目录（.git / .idea / .vscode 等）
            if SKIP_DIRS.contains(&name.as_ref()) || name.starts_with('.') {
                walker.skip_current_dir();
            }
            continue;
        }
        let abs = entry.path();
        let rel = match abs.strip_prefix(repo_root) {
            Ok(r) => r.to_string_lossy().replace('\\', "/"),
            Err(_) => continue,
        };
        let ext = match abs.extension().and_then(|e| e.to_str()) {
            Some(e) => e.to_string(),
            None => continue,
        };
        if detect_language(&ext).is_none() {
            continue;
        }
        stats.files_scanned += 1;
        seen.insert(rel.clone());
        if process_file(conn, repo_id, repo_root, &rel, abs)? {
            stats.files_reindexed += 1;
        } else {
            stats.files_skipped += 1;
        }
    }

    // 移除磁盘上已不存在的文件索引
    let stored: Vec<String> = {
        let mut stmt = conn.prepare("SELECT file_path FROM symbol_index_files WHERE repo_id = ?1")?;
        let rows = stmt.query_map(params![repo_id], |row| row.get::<_, String>(0))?;
        rows.filter_map(|r| r.ok()).collect()
    };
    for path in stored {
        if !seen.contains(&path) {
            remove_file(conn, repo_id, &path)?;
        }
    }

    stats.symbols = count(conn, "SELECT COUNT(*) FROM symbols WHERE repo_id = ?1", repo_id)?;
    stats.refs = count(conn, "SELECT COUNT(*) FROM symbol_refs WHERE repo_id = ?1", repo_id)?;
    Ok(stats)
}

/// 增量入口：仅处理显式给定的相对路径文件（watcher / 单文件重解析）。
pub fn reindex_files(conn: &mut Connection, repo_root: &Path, repo_id: i64, files: &[String]) -> AppResult<IndexStats> {
    let mut stats = IndexStats {
        files_scanned: files.len(),
        files_reindexed: 0,
        files_skipped: 0,
        symbols: 0,
        refs: 0,
    };
    for rel in files {
        let rel_norm = rel.replace('\\', "/");
        let abs = repo_root.join(rel);
        if !abs.is_file() {
            // 文件已删除 → 清索引
            remove_file(conn, repo_id, &rel_norm)?;
            continue;
        }
        if process_file(conn, repo_id, repo_root, &rel_norm, &abs)? {
            stats.files_reindexed += 1;
        } else {
            stats.files_skipped += 1;
        }
    }
    stats.symbols = count(conn, "SELECT COUNT(*) FROM symbols WHERE repo_id = ?1", repo_id)?;
    stats.refs = count(conn, "SELECT COUNT(*) FROM symbol_refs WHERE repo_id = ?1", repo_id)?;
    Ok(stats)
}

/// 处理单文件：hash 未变返回 false（跳过）；变更则事务内替换。
fn process_file(conn: &mut Connection, repo_id: i64, _repo_root: &Path, rel: &str, abs: &Path) -> AppResult<bool> {
    let lang = match abs.extension().and_then(|e| e.to_str()).and_then(detect_language) {
        Some(l) => l,
        None => return Ok(false),
    };
    let meta = match fs::metadata(abs) {
        Ok(m) if m.is_file() => m,
        _ => return Ok(false),
    };
    if meta.len() > MAX_FILE_BYTES {
        return Ok(false);
    }
    let bytes = match fs::read(abs) {
        Ok(b) => b,
        Err(_) => return Ok(false),
    };
    let content = match String::from_utf8(bytes) {
        Ok(s) => s,
        Err(_) => return Ok(false), // 非文本 / 非 UTF-8 不进索引
    };
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    let hash = format!("{:x}", hasher.finalize());

    let existing: Option<String> = conn
        .query_row(
            "SELECT content_hash FROM symbol_index_files WHERE repo_id = ?1 AND file_path = ?2",
            params![repo_id, rel],
            |row| row.get(0),
        )
        .ok();
    if existing.as_deref() == Some(hash.as_str()) {
        return Ok(false);
    }

    let extraction = extract::extract(lang, &content);

    let tx = conn.transaction()?;
    tx.execute(
        "DELETE FROM symbols WHERE repo_id = ?1 AND file_path = ?2",
        params![repo_id, rel],
    )?;
    tx.execute(
        "DELETE FROM symbol_refs WHERE repo_id = ?1 AND file_path = ?2",
        params![repo_id, rel],
    )?;
    if let Some(ext) = extraction {
        let now = chrono::Utc::now().to_rfc3339();
        let mut sym_stmt = tx.prepare(
            "INSERT INTO symbols (repo_id, file_path, name, kind, line, updated_at, end_line, container, signature) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        )?;
        for s in &ext.symbols {
            sym_stmt.execute(params![
                repo_id,
                rel,
                s.name,
                s.kind,
                s.line as i64,
                now,
                s.end_line as i64,
                s.container,
                s.signature
            ])?;
        }
        let mut ref_stmt = tx.prepare(
            "INSERT INTO symbol_refs (repo_id, name, file_path, line, is_call, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        )?;
        for r in &ext.refs {
            ref_stmt.execute(params![repo_id, r.name, rel, r.line as i64, r.is_call as i64, now])?;
        }
    }
    tx.execute(
        "INSERT INTO symbol_index_files (repo_id, file_path, content_hash, updated_at) \
         VALUES (?1, ?2, ?3, ?4) \
         ON CONFLICT(repo_id, file_path) DO UPDATE SET content_hash = ?3, updated_at = ?4",
        params![repo_id, rel, hash, chrono::Utc::now().to_rfc3339()],
    )?;
    tx.commit()?;
    Ok(true)
}

/// 移除单文件的索引（文件删除场景）。
pub fn remove_file(conn: &mut Connection, repo_id: i64, rel: &str) -> AppResult<()> {
    let tx = conn.transaction()?;
    tx.execute(
        "DELETE FROM symbols WHERE repo_id = ?1 AND file_path = ?2",
        params![repo_id, rel],
    )?;
    tx.execute(
        "DELETE FROM symbol_refs WHERE repo_id = ?1 AND file_path = ?2",
        params![repo_id, rel],
    )?;
    tx.execute(
        "DELETE FROM symbol_index_files WHERE repo_id = ?1 AND file_path = ?2",
        params![repo_id, rel],
    )?;
    tx.commit()?;
    Ok(())
}

fn count(conn: &Connection, sql: &str, repo_id: i64) -> AppResult<usize> {
    let n: i64 = conn.query_row(sql, params![repo_id], |row| row.get(0))?;
    Ok(n as usize)
}

// ---------------------------------------------------------------------------
// 查询
// ---------------------------------------------------------------------------

/// 仓库级过滤解析后的作用域：None = 全部仓库。
#[derive(Default)]
pub struct RepoScope {
    pub repo_paths: Option<Vec<String>>,
}

/// 组装 `<col> IN (...)` 仓库收敛条件。
/// 归一化在 SQL 内做（`replace(path, char(92), '/')`，R-02 path_key 语义）；
/// 分块规避 SQLite 变量上限；空集 → 恒假（1=0）。
fn repo_scope_sql(scope: &RepoScope, col: &str) -> (String, Vec<String>) {
    match &scope.repo_paths {
        None => (String::new(), Vec::new()),
        Some(paths) if paths.is_empty() => ("AND 1 = 0".to_string(), Vec::new()),
        Some(paths) => {
            let mut chunks: Vec<String> = Vec::new();
            for chunk in paths.chunks(400) {
                let placeholders = chunk.iter().map(|_| "?").collect::<Vec<_>>().join(",");
                chunks.push(format!("{col} IN ({placeholders})"));
            }
            (format!("AND ({})", chunks.join(" OR ")), paths.to_vec())
        }
    }
}

/// 组装 file_path 过滤（@ext / @path，归一化后 LIKE）。
fn file_filter_sql(exts: &[String], paths: &[String], col: &str) -> (String, Vec<String>) {
    let mut conds: Vec<String> = Vec::new();
    let mut binds: Vec<String> = Vec::new();
    for ext in exts {
        conds.push(format!("replace({col}, char(92), '/') LIKE ?"));
        binds.push(format!("%.{ext}"));
    }
    for p in paths {
        conds.push(format!("replace({col}, char(92), '/') LIKE ?"));
        binds.push(format!("%{p}%"));
    }
    if conds.is_empty() {
        (String::new(), binds)
    } else {
        (format!("AND ({})", conds.join(" OR ")), binds)
    }
}

/// 名称条件：tokens 全部 LIKE 命中（AND）。
fn name_filter_sql(tokens: &[String], col: &str) -> (String, Vec<String>) {
    let mut sql = String::new();
    let mut binds = Vec::new();
    for t in tokens {
        sql.push_str(&format!("AND {col} LIKE ?"));
        binds.push(format!("%{t}%"));
    }
    (sql, binds)
}

/// 符号搜索（name LIKE tokens）。
pub fn search_symbols(
    conn: &Connection,
    scope: &RepoScope,
    exts: &[String],
    paths: &[String],
    tokens: &[String],
) -> AppResult<Vec<SymbolHit>> {
    let (scope_sql, scope_binds) = repo_scope_sql(scope, "s.repo_id");
    let (file_sql, file_binds) = file_filter_sql(exts, paths, "s.file_path");
    let (name_sql, name_binds) = name_filter_sql(tokens, "s.name");
    let sql = format!(
        "SELECT rp.path, s.file_path, s.name, s.kind, s.line, s.end_line, s.container, s.signature \
         FROM symbols s JOIN repositories rp ON rp.id = s.repo_id \
         WHERE 1 = 1 {scope_sql} {file_sql} {name_sql} \
         ORDER BY s.name, s.file_path, s.line LIMIT {HIT_LIMIT}"
    );
    let mut stmt = conn.prepare(&sql)?;
    let bind_all: Vec<&String> = scope_binds
        .iter()
        .chain(file_binds.iter())
        .chain(name_binds.iter())
        .collect();
    let rows = stmt.query_map(rusqlite::params_from_iter(bind_all), map_symbol_hit)?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

/// 精确名称 → 定义列表（Go To Definition 数据面）。
pub fn find_definitions(
    conn: &Connection,
    name: &str,
    scope: &RepoScope,
    exts: &[String],
    paths: &[String],
) -> AppResult<Vec<SymbolHit>> {
    let (scope_sql, scope_binds) = repo_scope_sql(scope, "s.repo_id");
    let (file_sql, file_binds) = file_filter_sql(exts, paths, "s.file_path");
    let sql = format!(
        "SELECT rp.path, s.file_path, s.name, s.kind, s.line, s.end_line, s.container, s.signature \
         FROM symbols s JOIN repositories rp ON rp.id = s.repo_id \
         WHERE s.name = ?1 {scope_sql} {file_sql} \
         ORDER BY s.file_path, s.line LIMIT {HIT_LIMIT}"
    );
    let mut stmt = conn.prepare(&sql)?;
    let bind_all: Vec<&String> = scope_binds.iter().chain(file_binds.iter()).collect();
    let rows = stmt.query_map(
        rusqlite::params_from_iter(std::iter::once(&name.to_string()).chain(bind_all)),
        map_symbol_hit,
    )?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

/// 按名查引用（symbol_refs，含调用点标记）。
pub fn find_references(
    conn: &Connection,
    name: &str,
    scope: &RepoScope,
    exts: &[String],
    paths: &[String],
) -> AppResult<Vec<RefHit>> {
    let (scope_sql, scope_binds) = repo_scope_sql(scope, "r.repo_id");
    let (file_sql, file_binds) = file_filter_sql(exts, paths, "r.file_path");
    let sql = format!(
        "SELECT rp.path, r.file_path, r.name, r.line, r.is_call \
         FROM symbol_refs r JOIN repositories rp ON rp.id = r.repo_id \
         WHERE r.name = ?1 {scope_sql} {file_sql} \
         ORDER BY r.file_path, r.line LIMIT {REF_LIMIT}"
    );
    let mut stmt = conn.prepare(&sql)?;
    let bind_all: Vec<&String> = scope_binds.iter().chain(file_binds.iter()).collect();
    let rows = stmt.query_map(
        rusqlite::params_from_iter(std::iter::once(&name.to_string()).chain(bind_all)),
        |row| {
            Ok(RefHit {
                repo_path: row.get(0)?,
                file_path: row.get(1)?,
                name: row.get(2)?,
                line: row.get(3)?,
                is_call: row.get::<_, i64>(4)? != 0,
            })
        },
    )?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

/// 调用层级：direction = "callers"（谁调用了 name）或 "callees"（name 调了谁）。
/// 容器函数取「最深包含」（相关子查询 MAX(line)），避免外层函数误报；
/// callers 的 file/line = 调用点所在函数位置，callees 的 name = 被调用者名。
pub fn call_hierarchy(
    conn: &Connection,
    name: &str,
    direction: &str,
    scope: &RepoScope,
    exts: &[String],
    paths: &[String],
) -> AppResult<Vec<CallHit>> {
    let (scope_sql, scope_binds) = repo_scope_sql(scope, "r.repo_id");
    let (file_sql, file_binds) = file_filter_sql(exts, paths, "r.file_path");
    let (name_expr, name_where) = match direction {
        "callees" => ("r.name", "s.name = ?1 AND r.name != ?1"),
        _ => ("s.name", "r.name = ?1 AND s.name != ?1"),
    };
    let sql = format!(
        "SELECT {name_expr} AS hit_name, rp.path, s.file_path, s.line, s.kind, COUNT(*) AS call_count \
         FROM symbol_refs r \
         JOIN symbols s ON s.repo_id = r.repo_id AND s.file_path = r.file_path \
           AND s.kind IN ('function','method') \
           AND s.line <= r.line AND s.end_line >= r.line \
           AND s.line = ( \
             SELECT MAX(s2.line) FROM symbols s2 \
             WHERE s2.repo_id = r.repo_id AND s2.file_path = r.file_path \
               AND s2.kind IN ('function','method') \
               AND s2.line <= r.line AND s2.end_line >= r.line) \
         JOIN repositories rp ON rp.id = s.repo_id \
         WHERE r.is_call = 1 AND {name_where} {scope_sql} {file_sql} \
         GROUP BY {name_expr}, rp.path, s.file_path, s.line, s.kind \
         ORDER BY call_count DESC LIMIT 100"
    );
    let mut stmt = conn.prepare(&sql)?;
    // 绑定顺序：?1 name；scope binds；file binds
    let bind_all: Vec<&String> = scope_binds.iter().chain(file_binds.iter()).collect();
    let rows = stmt.query_map(
        rusqlite::params_from_iter(std::iter::once(&name.to_string()).chain(bind_all)),
        |row| {
            Ok(CallHit {
                name: row.get(0)?,
                repo_path: row.get(1)?,
                file_path: row.get(2)?,
                line: row.get(3)?,
                kind: row.get(4)?,
                call_count: row.get(5)?,
            })
        },
    )?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

fn map_symbol_hit(row: &rusqlite::Row) -> rusqlite::Result<SymbolHit> {
    Ok(SymbolHit {
        repo_path: row.get(0)?,
        file_path: row.get(1)?,
        name: row.get(2)?,
        kind: row.get(3)?,
        line: row.get(4)?,
        end_line: row.get(5)?,
        container: row.get(6)?,
        signature: row.get(7)?,
    })
}

// ---------------------------------------------------------------------------
// tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// 内存库（全量迁移）+ 磁盘 fixture 仓库，返回 (conn, repo_root)。
    fn fixture(name: &str, files: &[(&str, &str)]) -> (Connection, PathBuf) {
        let mut conn = Connection::open_in_memory().unwrap();
        crate::db::migrate(&mut conn).unwrap();
        conn.execute(
            "INSERT INTO workspaces (name, path, created_at, updated_at) \
             VALUES ('ws', '/tmp/gw-symbols-ws', '', '')",
            [],
        )
        .unwrap();
        let root = std::env::temp_dir().join(format!("gw-symbols-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        for (rel, content) in files {
            let path = root.join(rel);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, content).unwrap();
        }
        conn.execute(
            "INSERT INTO repositories (workspace_id, path, name, relative_path, created_at, updated_at) \
             VALUES (1, ?1, 'repo', '', '', '')",
            params![root.to_string_lossy().to_string()],
        )
        .unwrap();
        (conn, root)
    }

    #[test]
    fn parse_filters_splits_tokens() {
        let (f, tokens) = parse_filters("handler @ext:rs,ts @path:src/api @status:dirty @repo:core");
        assert_eq!(f.exts, vec!["rs", "ts"]);
        assert_eq!(f.paths, vec!["src/api"]);
        assert_eq!(f.statuses, vec!["dirty"]);
        assert_eq!(f.repos, vec!["core"]);
        assert_eq!(tokens, vec!["handler"]);
    }

    #[test]
    fn incremental_reindex_only_reparses_changed_files() {
        let (mut conn, root) = fixture(
            "incr",
            &[
                ("src/lib.rs", "fn alpha() {}\nfn beta() {}\n"),
                ("src/other.py", "def gamma():\n    pass\n"),
            ],
        );
        let repo_id = repo_id_for_path(&conn, root.to_str().unwrap()).unwrap();
        let stats = reindex_repo(&mut conn, &root, repo_id).unwrap();
        assert_eq!(stats.files_reindexed, 2);
        assert_eq!(stats.symbols, 3);

        // 无变更 → 全部跳过
        let stats = reindex_repo(&mut conn, &root, repo_id).unwrap();
        assert_eq!(stats.files_reindexed, 0);
        assert_eq!(stats.files_skipped, 2);

        // 只改一个文件 → 全量走查也只重解析该文件
        std::fs::write(root.join("src/lib.rs"), "fn alpha() {}\nfn beta2() {}\n").unwrap();
        let stats = reindex_repo(&mut conn, &root, repo_id).unwrap();
        assert_eq!(stats.files_reindexed, 1);
        assert_eq!(stats.files_skipped, 1);
        let defs = find_definitions(&conn, "beta2", &RepoScope::default(), &[], &[]).unwrap();
        assert_eq!(defs.len(), 1);
        assert!(find_definitions(&conn, "beta", &RepoScope::default(), &[], &[])
            .unwrap()
            .is_empty());

        // 磁盘删除文件 → 索引同步清理
        std::fs::remove_file(root.join("src/other.py")).unwrap();
        let stats = reindex_repo(&mut conn, &root, repo_id).unwrap();
        assert_eq!(stats.files_reindexed, 0);
        let py_defs = find_definitions(&conn, "gamma", &RepoScope::default(), &[], &[]).unwrap();
        assert!(py_defs.is_empty());
    }

    #[test]
    fn definitions_references_and_hierarchy() {
        let (mut conn, root) = fixture(
            "hier",
            &[(
                "src/lib.rs",
                r#"
pub fn worker(input: u32) -> u32 { input + 1 }
pub fn outer() -> u32 {
    fn inner() -> u32 {
        worker(1)
            + worker(2)
    }
    inner()
}
pub fn boss() -> u32 { worker(3) }
"#,
            )],
        );
        let repo_id = repo_id_for_path(&conn, root.to_str().unwrap()).unwrap();
        reindex_repo(&mut conn, &root, repo_id).unwrap();

        // 定义
        let defs = find_definitions(&conn, "worker", &RepoScope::default(), &[], &[]).unwrap();
        assert_eq!(defs.len(), 1);
        assert_eq!(defs[0].kind, "function");
        assert_eq!(defs[0].line, 2);

        // 引用（is_call）
        let refs = find_references(&conn, "worker", &RepoScope::default(), &[], &[]).unwrap();
        assert_eq!(refs.len(), 3);
        assert!(refs.iter().all(|r| r.is_call));

        // Callers of worker：最深容器 inner（2 次）、boss（1 次）；不误报 outer
        let callers = call_hierarchy(&conn, "worker", "callers", &RepoScope::default(), &[], &[]).unwrap();
        let names: Vec<(&str, i64)> = callers.iter().map(|c| (c.name.as_str(), c.call_count)).collect();
        assert!(names.contains(&("inner", 2)), "{names:?}");
        assert!(names.contains(&("boss", 1)), "{names:?}");
        assert!(!names.iter().any(|(n, _)| *n == "outer"), "{names:?}");

        // Callees of boss：只调 worker
        let callees = call_hierarchy(&conn, "boss", "callees", &RepoScope::default(), &[], &[]).unwrap();
        assert_eq!(callees.len(), 1);
        assert_eq!(callees[0].name, "worker");
        assert_eq!(callees[0].call_count, 1);
    }

    #[test]
    fn ext_and_repo_filters() {
        let (mut conn, root) = fixture(
            "filters",
            &[
                ("src/a.rs", "fn target_fn() {}\n"),
                ("src/b.py", "def target_fn():\n    pass\n"),
            ],
        );
        let repo_id = repo_id_for_path(&conn, root.to_str().unwrap()).unwrap();
        reindex_repo(&mut conn, &root, repo_id).unwrap();

        // @ext:rs 只命中 Rust 定义
        let hits = search_symbols(&conn, &RepoScope::default(), &["rs".into()], &[], &["target_fn".into()]).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].kind, "function");
        assert!(hits[0].file_path.ends_with("a.rs"));

        // @ext:py,go（parse_filters 拆分后为 ["py","go"]）命中 Python
        let hits = search_symbols(
            &conn,
            &RepoScope::default(),
            &["py".into(), "go".into()],
            &[],
            &["target_fn".into()],
        )
        .unwrap();
        assert_eq!(hits.len(), 1);
        assert!(hits[0].file_path.ends_with("b.py"));

        // @repo:不存在 → 空结果
        let scope = RepoScope {
            repo_paths: Some(vec!["/nonexistent/repo".into()]),
        };
        let hits = search_symbols(&conn, &scope, &[], &[], &["target_fn".into()]).unwrap();
        assert!(hits.is_empty());
    }

    #[test]
    fn search_stays_fast_on_synth_index() {
        // 10k 符号规模下 name LIKE 查询 < 100ms（验收标准：索引内 < 100ms）。
        let mut conn = Connection::open_in_memory().unwrap();
        crate::db::migrate(&mut conn).unwrap();
        conn.execute(
            "INSERT INTO workspaces (name, path, created_at, updated_at) VALUES ('ws', '/w', '', '')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO repositories (workspace_id, path, name, relative_path, created_at, updated_at) \
             VALUES (1, '/w/r', 'r', '', '', '')",
            [],
        )
        .unwrap();
        let now = chrono::Utc::now().to_rfc3339();
        let tx = conn.transaction().unwrap();
        {
            let mut stmt = tx
                .prepare(
                    "INSERT INTO symbols (repo_id, file_path, name, kind, line, updated_at) \
                 VALUES (1, 'f.rs', ?1, 'function', 1, ?2)",
                )
                .unwrap();
            for i in 0..10_000 {
                stmt.execute(params![format!("sym_{i:05}"), &now]).unwrap();
            }
        }
        tx.commit().unwrap();

        let start = std::time::Instant::now();
        let hits = search_symbols(&conn, &RepoScope::default(), &[], &[], &["sym_0999".into()]).unwrap();
        let elapsed = start.elapsed();
        assert!(!hits.is_empty());
        assert!(
            elapsed.as_millis() < 100,
            "search took {}ms, expected < 100ms",
            elapsed.as_millis()
        );
    }
}
