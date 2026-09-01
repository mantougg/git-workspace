//! `package.json` discovery and SQLite indexing (N-02, design §4.2).
//!
//! Discovery follows the Maven scanner's workspace-boundary semantics: Git
//! repositories are scanned first, then the workspace root is supplemented so
//! non-Git source trees are included. Nested repositories are treated as
//! boundaries and dependency/build output directories are skipped.

use std::collections::{HashMap, HashSet};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

use rayon::prelude::*;
use rusqlite::{params, Connection, Transaction};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use walkdir::WalkDir;

use crate::core::scanner::{is_skip_dir, IgnoreRules, RepoScanner};
use crate::error::{AppError, AppResult};
use crate::maven::parser::hex_hash;

use super::decision::{decide_package_manager, DecisionInput, LockfileSnapshot};
use super::model::NodeProjectNode;

/// Parsed package metadata. Dependencies are intentionally ignored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedPackageJson {
    pub name: String,
    pub version: String,
    pub scripts_json: String,
    pub package_manager: Option<String>,
}

/// A package parse failure does not abort discovery of other projects.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeDiscoveryError {
    pub code: String,
    pub path: String,
    pub message: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeDiscoveryStats {
    pub parsed: usize,
    pub cache_hits: usize,
}

/// Workspace discovery result before SQLite synchronization.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeDiscoveryResult {
    pub projects: Vec<NodeProjectNode>,
    pub errors: Vec<NodeDiscoveryError>,
    pub elapsed_ms: u128,
    pub stats: NodeDiscoveryStats,
}

#[derive(Clone)]
struct CachedPackage {
    hash: String,
    parsed: ParsedPackageJson,
}

/// Content-hash cache aligned with the Maven POM cache semantics.
pub struct NodePackageCache {
    entries: Mutex<HashMap<String, CachedPackage>>,
    parse_count: AtomicUsize,
}

impl Default for NodePackageCache {
    fn default() -> Self {
        Self::new()
    }
}

impl NodePackageCache {
    pub fn new() -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
            parse_count: AtomicUsize::new(0),
        }
    }

    pub fn parse_count(&self) -> usize {
        self.parse_count.load(Ordering::Relaxed)
    }

    fn get_or_parse(
        &self,
        path: &Path,
        bytes: &[u8],
    ) -> Result<(String, ParsedPackageJson, bool), NodeDiscoveryError> {
        let key = path_key(path);
        let hash = hex_hash(bytes);
        if let Ok(entries) = self.entries.lock() {
            if let Some(cached) = entries.get(&key).filter(|item| item.hash == hash) {
                return Ok((hash, cached.parsed.clone(), true));
            }
        }
        let parsed = parse_package_json_bytes(path, bytes)?;
        self.parse_count.fetch_add(1, Ordering::Relaxed);
        if let Ok(mut entries) = self.entries.lock() {
            entries.insert(
                key,
                CachedPackage {
                    hash: hash.clone(),
                    parsed: parsed.clone(),
                },
            );
        }
        Ok((hash, parsed, false))
    }
}

static GLOBAL_CACHE: OnceLock<NodePackageCache> = OnceLock::new();

pub fn global_package_cache() -> &'static NodePackageCache {
    GLOBAL_CACHE.get_or_init(NodePackageCache::new)
}

/// Discover and parse all package manifests in a workspace.
pub fn discover_package_jsons(
    workspace_root: &Path,
    scan_depth: usize,
    cache: Option<&NodePackageCache>,
    cancel: Option<&AtomicBool>,
) -> NodeDiscoveryResult {
    let started = Instant::now();
    let scanner = RepoScanner::new(scan_depth);
    let repos = scanner.scan_cancellable(workspace_root, cancel);
    if is_cancelled(cancel) {
        return empty_result(started);
    }

    let mut paths = HashSet::new();
    for repo in repos {
        if let Some(found) =
            collect_package_json_paths(Path::new(&repo.path), workspace_root, cancel)
        {
            paths.extend(found.into_iter().map(|path| path_key(&path)));
        }
    }
    // R-27 supplement: include non-Git directories and root-level manifests.
    if let Some(found) = collect_package_json_paths(workspace_root, workspace_root, cancel) {
        paths.extend(found.into_iter().map(|path| path_key(&path)));
    }
    if is_cancelled(cancel) {
        return empty_result(started);
    }

    let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
    let parsed: Vec<Result<(NodeProjectNode, bool), NodeDiscoveryError>> = paths
        .par_iter()
        .map(|manifest| parse_project(manifest, cache))
        .collect();
    let mut projects = Vec::new();
    let mut errors = Vec::new();
    let mut stats = NodeDiscoveryStats::default();
    for item in parsed {
        match item {
            Ok((project, cache_hit)) => {
                if !cache_hit {
                    stats.parsed += 1;
                } else {
                    stats.cache_hits += 1;
                }
                projects.push(project);
            }
            Err(error) => errors.push(error),
        }
    }
    projects.sort_by(|left, right| left.path.cmp(&right.path));
    errors.sort_by(|left, right| left.path.cmp(&right.path));
    NodeDiscoveryResult {
        projects,
        errors,
        elapsed_ms: started.elapsed().as_millis(),
        stats,
    }
}

fn parse_project(
    manifest: &Path,
    cache: Option<&NodePackageCache>,
) -> Result<(NodeProjectNode, bool), NodeDiscoveryError> {
    let bytes = std::fs::read(manifest).map_err(|source| NodeDiscoveryError {
        code: "InvalidPackageJson".into(),
        path: manifest.display().to_string(),
        message: source.to_string(),
    })?;
    let (hash, parsed, cache_hit) = match cache {
        Some(cache) => cache.get_or_parse(manifest, &bytes)?,
        None => (
            hex_hash(&bytes),
            parse_package_json_bytes(manifest, &bytes)?,
            false,
        ),
    };
    let dir = manifest.parent().unwrap_or_else(|| Path::new(""));
    let decision = decide_package_manager(&DecisionInput {
        configured: None,
        package_json_field: parsed.package_manager.clone(),
        lockfiles: LockfileSnapshot::scan(dir),
    });
    Ok((
        NodeProjectNode {
            project_id: 0,
            repository_id: None,
            path: PathBuf::from(path_key(dir)),
            name: parsed.name,
            version: parsed.version,
            package_manager: Some(decision.manager.name().to_string()),
            scripts_json: parsed.scripts_json,
            pkg_hash: hash,
        },
        cache_hit,
    ))
}

fn parse_package_json_bytes(
    path: &Path,
    bytes: &[u8],
) -> Result<ParsedPackageJson, NodeDiscoveryError> {
    let value: Value = serde_json::from_slice(bytes).map_err(|source| NodeDiscoveryError {
        code: "InvalidPackageJson".into(),
        path: path.display().to_string(),
        message: source.to_string(),
    })?;
    let object = value.as_object().ok_or_else(|| NodeDiscoveryError {
        code: "InvalidPackageJson".into(),
        path: path.display().to_string(),
        message: "package.json root must be an object".into(),
    })?;
    let name = string_field(object, "name");
    let version = string_field(object, "version");
    let scripts_json = if object.get("scripts").and_then(Value::as_object).is_some() {
        extract_raw_object(bytes, "scripts").unwrap_or_else(|| "{}".into())
    } else {
        "{}".into()
    };
    let package_manager = object
        .get("packageManager")
        .and_then(Value::as_str)
        .map(str::to_string);
    Ok(ParsedPackageJson {
        name,
        version,
        scripts_json,
        package_manager,
    })
}

/// Return the raw object text for a top-level JSON key. `serde_json` validates
/// and parses the document; this small scanner only retains insertion order
/// and formatting for the scripts object used by the UI.
fn extract_raw_object(bytes: &[u8], key: &str) -> Option<String> {
    let text = std::str::from_utf8(bytes).ok()?;
    let marker = format!("\"{key}\"");
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    let mut key_start = None;
    let mut marker_pos = None;
    for (offset, ch) in text.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
                if depth == 1 {
                    if let Some(start) = key_start {
                        let is_key = text[offset + 1..]
                            .chars()
                            .find(|next| !next.is_whitespace())
                            == Some(':');
                        if is_key && text[start..offset + 1] == marker {
                            marker_pos = Some(start);
                            break;
                        }
                    }
                }
            }
            continue;
        }
        match ch {
            '"' => {
                in_string = true;
                key_start = Some(offset);
            }
            '{' => depth += 1,
            '}' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    let marker_pos = marker_pos?;
    let colon = text[marker_pos + marker.len()..].find(':')? + marker_pos + marker.len();
    let start = text[colon + 1..]
        .char_indices()
        .find(|(_, ch)| !ch.is_whitespace())
        .map(|(offset, _)| colon + 1 + offset)?;
    if text.as_bytes().get(start).copied()? != b'{' {
        return None;
    }
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for (offset, ch) in text[start..].char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        match ch {
            '"' => in_string = true,
            '{' => depth += 1,
            '}' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(text[start..start + offset + ch.len_utf8()].to_string());
                }
            }
            _ => {}
        }
    }
    None
}

fn string_field(object: &serde_json::Map<String, Value>, key: &str) -> String {
    object
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn collect_package_json_paths(
    root: &Path,
    workspace_root: &Path,
    cancel: Option<&AtomicBool>,
) -> Option<Vec<PathBuf>> {
    let workspace_ignore = IgnoreRules::load(workspace_root);
    let local_ignore = IgnoreRules::load(root);
    let mut walker = WalkDir::new(root).follow_links(false).into_iter();
    let mut paths = Vec::new();
    loop {
        if is_cancelled(cancel) {
            return None;
        }
        match walker.next() {
            Some(Ok(entry)) => {
                if entry.file_type().is_dir() {
                    let name = entry.file_name();
                    if entry.path() != root
                        && (entry.path().join(".git").is_dir()
                            || entry.path().join(".git").is_file())
                    {
                        walker.skip_current_dir();
                        continue;
                    }
                    let name_text = name.to_string_lossy();
                    let relative = relative_string(entry.path(), root);
                    let workspace_relative = relative_string(entry.path(), workspace_root);
                    if name == OsStr::new(".git")
                        || is_skip_dir(name)
                        || name_text.starts_with('.')
                        || local_ignore.is_ignored(&name_text, &relative)
                        || workspace_ignore.is_ignored(&name_text, &workspace_relative)
                    {
                        walker.skip_current_dir();
                    }
                } else if entry.file_type().is_file()
                    && entry.file_name() == OsStr::new("package.json")
                {
                    paths.push(entry.path().to_path_buf());
                }
            }
            Some(Err(error)) => log::warn!("Node discovery walk error: {error}"),
            None => break,
        }
    }
    Some(paths)
}

/// Upsert discovery results and remove stale rows atomically.
pub fn sync_node_projects(
    conn: &mut Connection,
    workspace_id: i64,
    discovery: &NodeDiscoveryResult,
) -> AppResult<Vec<NodeProjectNode>> {
    let existing: HashSet<String> = {
        let mut stmt = conn.prepare("SELECT path FROM node_projects WHERE workspace_id = ?1")?;
        let rows = stmt.query_map([workspace_id], |row| row.get::<_, String>(0))?;
        rows.collect::<Result<HashSet<_>, _>>()?
    };
    let repository_roots = repository_roots(conn, workspace_id)?;
    let inputs: Vec<_> = discovery
        .projects
        .iter()
        .map(|project| {
            let mut project = project.clone();
            project.repository_id = find_repository_id(&project.path, &repository_roots);
            (path_key(&project.path), project)
        })
        .collect();
    let input_paths: HashSet<&str> = inputs.iter().map(|(path, _)| path.as_str()).collect();
    let stale: Vec<String> = existing
        .iter()
        .filter(|path| !input_paths.contains(path.as_str()))
        .cloned()
        .collect();
    let now = chrono::Utc::now().to_rfc3339();
    let tx = conn.transaction()?;
    delete_stale(&tx, workspace_id, &stale)?;
    let mut stmt = tx.prepare(
        "INSERT INTO node_projects
            (workspace_id, repository_id, path, name, version, package_manager,
             scripts_json, pkg_hash, last_scanned_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
         ON CONFLICT(workspace_id, path) DO UPDATE SET
            repository_id = excluded.repository_id,
            name = excluded.name,
            version = excluded.version,
            package_manager = excluded.package_manager,
            scripts_json = excluded.scripts_json,
            pkg_hash = excluded.pkg_hash,
            last_scanned_at = excluded.last_scanned_at",
    )?;
    for (path, project) in inputs {
        stmt.execute(params![
            workspace_id,
            project.repository_id,
            path,
            project.name,
            project.version,
            project.package_manager,
            project.scripts_json,
            project.pkg_hash,
            now,
        ])?;
    }
    drop(stmt);
    tx.commit()?;
    list_node_projects(conn, workspace_id)
}

pub fn list_node_projects(conn: &Connection, workspace_id: i64) -> AppResult<Vec<NodeProjectNode>> {
    let mut stmt = conn.prepare(
        "SELECT id, repository_id, path, name, version, package_manager, scripts_json, pkg_hash
         FROM node_projects WHERE workspace_id = ?1 ORDER BY path",
    )?;
    let rows = stmt.query_map([workspace_id], |row| {
        Ok(NodeProjectNode {
            project_id: row.get(0)?,
            repository_id: row.get(1)?,
            path: PathBuf::from(row.get::<_, String>(2)?),
            name: row.get(3)?,
            version: row.get(4)?,
            package_manager: row.get(5)?,
            scripts_json: row.get(6)?,
            pkg_hash: row.get(7)?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(AppError::from)
}

fn delete_stale(tx: &Transaction<'_>, workspace_id: i64, stale: &[String]) -> AppResult<()> {
    let mut stmt = tx.prepare("DELETE FROM node_projects WHERE workspace_id = ?1 AND path = ?2")?;
    for path in stale {
        stmt.execute(params![workspace_id, path])?;
    }
    Ok(())
}

fn repository_roots(conn: &Connection, workspace_id: i64) -> AppResult<Vec<(i64, String)>> {
    let mut stmt = conn.prepare(
        "SELECT id, path FROM repositories WHERE workspace_id = ?1 AND is_deleted = 0
         ORDER BY length(path) DESC",
    )?;
    let rows = stmt.query_map([workspace_id], |row| Ok((row.get(0)?, row.get(1)?)))?;
    rows.collect::<Result<Vec<_>, _>>().map_err(AppError::from)
}

fn find_repository_id(path: &Path, roots: &[(i64, String)]) -> Option<i64> {
    let project = comparable_path(path);
    roots.iter().find_map(|(id, root)| {
        let root = comparable_path(Path::new(root));
        let root = root.trim_end_matches('/');
        (project == root || project.starts_with(&format!("{root}/"))).then_some(*id)
    })
}

fn comparable_path(path: &Path) -> String {
    let value = path_key(path);
    if cfg!(windows) {
        value.to_ascii_lowercase()
    } else {
        value
    }
}

fn path_key(path: &Path) -> String {
    let normalized = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let value = normalized.to_string_lossy();
    let value = value
        .strip_prefix(r"\\?\UNC\")
        .map(|rest| format!(r"\\{rest}"))
        .or_else(|| value.strip_prefix(r"\\?\").map(str::to_string))
        .unwrap_or_else(|| value.to_string());
    value.replace('\\', "/")
}

fn relative_string(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .map(|relative| relative.to_string_lossy().replace('\\', "/"))
        .unwrap_or_default()
}

fn is_cancelled(cancel: Option<&AtomicBool>) -> bool {
    cancel.is_some_and(|flag| flag.load(Ordering::Relaxed))
}

fn empty_result(started: Instant) -> NodeDiscoveryResult {
    NodeDiscoveryResult {
        projects: vec![],
        errors: vec![],
        elapsed_ms: started.elapsed().as_millis(),
        stats: NodeDiscoveryStats::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, content: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    #[test]
    fn parses_scripts_without_dependencies_and_preserves_order() {
        let dir = std::env::temp_dir().join(format!("gw_node_parse_{}", uuid::Uuid::new_v4()));
        let path = dir.join("package.json");
        write(
            &path,
            r#"{"name":"web","version":"1.2.3","scripts":{"dev":"vite","build":"vite build"},"dependencies":{"x":"1"}}"#,
        );
        let parsed = parse_package_json_bytes(&path, &std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(parsed.name, "web");
        assert_eq!(
            parsed.scripts_json,
            r#"{"dev":"vite","build":"vite build"}"#
        );
        assert!(!parsed.scripts_json.contains("dependencies"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn discovery_skips_node_modules_dist_build_and_dotdirs() {
        let root = std::env::temp_dir().join(format!("gw_node_disc_{}", uuid::Uuid::new_v4()));
        write(
            &root.join("app/package.json"),
            r#"{"name":"app","scripts":{"dev":"vite"}}"#,
        );
        write(
            &root.join("node_modules/pkg/package.json"),
            r#"{"name":"bad"}"#,
        );
        write(&root.join("dist/package.json"), r#"{"name":"bad"}"#);
        write(&root.join(".hidden/package.json"), r#"{"name":"bad"}"#);
        let result = discover_package_jsons(&root, 5, None, None);
        assert_eq!(result.projects.len(), 1);
        assert_eq!(result.projects[0].name, "app");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn nested_git_repository_is_discovered_once() {
        let root = std::env::temp_dir().join(format!("gw_node_nested_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("repo/pkg")).unwrap();
        git2::Repository::init(root.join("repo")).unwrap();
        git2::Repository::init(root.join("repo/pkg")).unwrap();
        write(&root.join("repo/package.json"), r#"{"name":"repo"}"#);
        write(&root.join("repo/pkg/package.json"), r#"{"name":"pkg"}"#);
        let result = discover_package_jsons(&root, 5, None, None);
        assert_eq!(result.projects.len(), 2);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn cache_avoids_reparsing_unchanged_manifest() {
        let root = std::env::temp_dir().join(format!("gw_node_cache_{}", uuid::Uuid::new_v4()));
        let path = root.join("package.json");
        write(&path, r#"{"name":"app","scripts":{}}"#);
        let cache = NodePackageCache::new();
        discover_package_jsons(&root, 5, Some(&cache), None);
        assert_eq!(cache.parse_count(), 1);
        discover_package_jsons(&root, 5, Some(&cache), None);
        assert_eq!(cache.parse_count(), 1);
        std::fs::write(&path, r#"{"name":"app2","scripts":{}}"#).unwrap();
        discover_package_jsons(&root, 5, Some(&cache), None);
        assert_eq!(cache.parse_count(), 2);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn malformed_package_is_reported_without_aborting_scan() {
        let root = std::env::temp_dir().join(format!("gw_node_bad_{}", uuid::Uuid::new_v4()));
        write(&root.join("good/package.json"), r#"{"name":"good"}"#);
        write(&root.join("bad/package.json"), "not json");
        let result = discover_package_jsons(&root, 5, None, None);
        assert_eq!(result.projects.len(), 1);
        assert_eq!(result.errors.len(), 1);
        assert_eq!(result.errors[0].code, "InvalidPackageJson");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn sync_is_idempotent_and_replaces_stale_rows() {
        let root = std::env::temp_dir().join(format!("gw_node_sync_{}", uuid::Uuid::new_v4()));
        write(
            &root.join("app/package.json"),
            r#"{"name":"app","version":"1"}"#,
        );
        let mut conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::db::init_db(&mut conn).unwrap();
        conn.execute(
            "INSERT INTO workspaces (name, path, created_at, updated_at) VALUES ('w', ?1, 't', 't')",
            params![root.to_string_lossy().to_string()],
        )
        .unwrap();
        let workspace_id = conn.last_insert_rowid();
        let discovery = discover_package_jsons(&root, 5, None, None);
        let first = sync_node_projects(&mut conn, workspace_id, &discovery).unwrap();
        assert_eq!(first.len(), 1);
        let second = sync_node_projects(&mut conn, workspace_id, &discovery).unwrap();
        assert_eq!(second.len(), 1);
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM node_projects", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);
        let empty = NodeDiscoveryResult {
            projects: vec![],
            errors: vec![],
            elapsed_ms: 0,
            stats: NodeDiscoveryStats::default(),
        };
        sync_node_projects(&mut conn, workspace_id, &empty).unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM node_projects", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn discovery_fixture_stays_under_workspace_budget() {
        let root = std::env::temp_dir().join(format!("gw_node_perf_{}", uuid::Uuid::new_v4()));
        for index in 0..100 {
            write(
                &root.join(format!("module-{index}/package.json")),
                &format!(r#"{{"name":"module-{index}","scripts":{{"dev":"vite"}}}}"#),
            );
        }
        let result = discover_package_jsons(&root, 5, Some(&NodePackageCache::new()), None);
        assert_eq!(result.projects.len(), 100);
        assert!(
            result.elapsed_ms < 500,
            "discovery took {}ms",
            result.elapsed_ms
        );
        let _ = std::fs::remove_dir_all(root);
    }
}
