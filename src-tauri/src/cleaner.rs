//! 工作区清理核心（T-36）：规则编译 / 单次遍历扫描 / 安全门禁 / 删除。
//!
//! 配置模型与语义迁移自独立工具 delete-targets（Go 版），差异点记录在
//! `docs/tasks/T-36-toolbox-cleaner.md`：无全局文件删除开关（kind 含文件即允许）、
//! 通配符仅 `*`/`?`、路径比较走 `pathutil::normalize_for_compare`（平台规范 §1）。

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::error::AppError;
use crate::pathutil::{fold_case_for_fs, normalize_for_compare, strip_windows_verbatim_prefix};

/// 规则匹配方式（一期：精确名 / 通配符；正则候补）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CleanerMatchMode {
    Exact,
    Wildcard,
}

/// 目标类型：目录 / 文件 / 文件和目录。
/// 文件删除无全局开关——kind 含文件即视为该规则显式允许删文件
/// （等价 Go 版规则级 `AllowFileDelete: true`）；目录规则绝不删文件。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CleanerKind {
    Dir,
    File,
    Any,
}

/// 单条匹配规则（IPC camelCase：pattern / matchMode / kind）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanerRule {
    pub pattern: String,
    pub match_mode: CleanerMatchMode,
    pub kind: CleanerKind,
}

/// 扫描请求（IPC camelCase：operationPaths / excludePaths / rules）。
/// 路径要求绝对路径；父路径必须存在且为目录。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanerScanRequest {
    pub operation_paths: Vec<String>,
    pub exclude_paths: Vec<String>,
    pub rules: Vec<CleanerRule>,
}

/// 扫描候选（仅服务端会话持有；IPC 侧用 [`CleanerItemInfo`]）。
#[derive(Debug, Clone)]
pub struct ScannedItem {
    /// 展示路径（剥离 verbatim 前缀）。
    pub display_path: String,
    /// IO 路径（canonicalize 产物；Windows 带 verbatim 前缀以规避 260 上限）。
    pub io_path: PathBuf,
    pub is_dir: bool,
    /// 文件大小扫描时直出；目录为 None，由大小线程异步补算。
    pub size: Option<u64>,
}

/// IPC 候选条目。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanerItemInfo {
    pub path: String,
    pub is_dir: bool,
    pub size: Option<u64>,
}

/// 扫描产出。
#[derive(Debug, Default)]
pub struct ScanOutcome {
    pub items: Vec<ScannedItem>,
    pub visited: u64,
    /// 规则命中次数（含被安全门禁拦下的）。
    pub matched: u64,
    pub warnings: Vec<String>,
    /// canonical 化后的排除路径快照，删除阶段复检复用（同 Go 清单元数据语义）。
    pub exclude_paths: Vec<PathBuf>,
}

/// 单项删除结果。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteItemResult {
    pub path: String,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

// ---------------------------------------------------------------------------
// 规则编译与匹配
// ---------------------------------------------------------------------------

struct CompiledRule {
    kind: CleanerKind,
    mode: CleanerMatchMode,
    /// 已按平台折叠大小写的匹配模式（平台规范 §1：大小写语义随文件系统）。
    pattern: String,
}

impl CompiledRule {
    fn allows_files(&self) -> bool {
        self.kind != CleanerKind::Dir
    }

    fn matches(&self, name: &str) -> bool {
        let name = fold_case_for_fs(name.to_string());
        match self.mode {
            CleanerMatchMode::Exact => name == self.pattern,
            CleanerMatchMode::Wildcard => wildcard_match(&self.pattern, &name),
        }
    }
}

fn compile_rules(rules: &[CleanerRule]) -> Result<Vec<CompiledRule>, AppError> {
    if rules.is_empty() {
        return Err(AppError::Other("请至少配置一条匹配规则".into()));
    }
    let mut compiled = Vec::with_capacity(rules.len());
    for (index, rule) in rules.iter().enumerate() {
        let pattern = rule.pattern.trim();
        if pattern.is_empty() {
            return Err(AppError::Other(format!("规则 #{} 名称为空", index + 1)));
        }
        if pattern == "." || pattern == ".." {
            return Err(AppError::Other(format!(
                "规则 #{} 不能是特殊目录名：{pattern}",
                index + 1
            )));
        }
        if pattern.contains('/') || pattern.contains('\\') {
            return Err(AppError::Other(format!(
                "规则 #{} 只能是文件/文件夹名，不能包含路径分隔符：{pattern}",
                index + 1
            )));
        }
        if rule.match_mode == CleanerMatchMode::Wildcard {
            // 防全量误删：裸 * 类模式禁止。
            if pattern.chars().all(|c| c == '*') {
                return Err(AppError::Other(format!(
                    "规则 #{} 为防全量误删，不允许纯 * 通配符：{pattern}",
                    index + 1
                )));
            }
            if pattern.contains('[') || pattern.contains(']') {
                return Err(AppError::Other(format!(
                    "规则 #{} 通配符暂不支持字符类 […]（一期仅支持 * 与 ?）：{pattern}",
                    index + 1
                )));
            }
        }
        compiled.push(CompiledRule {
            kind: rule.kind,
            mode: rule.match_mode,
            pattern: fold_case_for_fs(pattern.to_string()),
        });
    }
    Ok(compiled)
}

/// 通配符匹配：`*` 任意序列、`?` 单字符；双指针回溯，O(nm) 最坏。
fn wildcard_match(pattern: &str, name: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let n: Vec<char> = name.chars().collect();
    let (mut pi, mut ni) = (0usize, 0usize);
    // (星号后的 pattern 下标, 星号已消费的 name 下标)
    let mut star: Option<(usize, usize)> = None;
    while ni < n.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == n[ni]) {
            pi += 1;
            ni += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some((pi + 1, ni));
            pi += 1;
        } else if let Some((sp, sn)) = star {
            // 回退：让最近的 * 多消费一个字符。
            pi = sp;
            ni = sn + 1;
            star = Some((sp, sn + 1));
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

// ---------------------------------------------------------------------------
// 路径键与安全门禁
// ---------------------------------------------------------------------------

/// 比较键：归一化（verbatim 剥离 + `/` 统一 + 平台大小写折叠）后去尾部分隔符。
fn path_key(p: &Path) -> String {
    normalize_for_compare(&p.to_string_lossy())
        .trim_end_matches('/')
        .to_string()
}

fn display_of(p: &Path) -> String {
    strip_windows_verbatim_prefix(&p.to_string_lossy())
}

/// `key` 命中任一基准（相等或为其后代）。
fn is_path_in_any_key(key: &str, bases: &[String]) -> bool {
    bases.iter().filter(|b| !b.is_empty()).any(|b| {
        key == b.as_str()
            || (key.len() > b.len() && key.starts_with(b.as_str()) && key.as_bytes()[b.len()] == b'/')
    })
}

/// `key`（目录）的内部包含任一子路径。
fn contains_any_key(key: &str, children: &[String]) -> bool {
    children.iter().filter(|c| !c.is_empty()).any(|c| {
        c.len() > key.len() && c.starts_with(key) && c.as_bytes()[key.len()] == b'/'
    })
}

/// 磁盘/共享根判定（入参为 path_key 产物）：`C:`、`/`、UNC 的 `//host[/share]`。
fn is_root_key(key: &str) -> bool {
    let k = key.trim_end_matches('/');
    if k.is_empty() {
        return true; // "/"
    }
    let bytes = k.as_bytes();
    if k.len() == 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        return true; // "c:"
    }
    if let Some(rest) = k.strip_prefix("//") {
        let segments = rest.split('/').filter(|s| !s.is_empty()).count();
        return segments <= 2; // //host 或 //host/share
    }
    false
}

// ---------------------------------------------------------------------------
// 扫描
// ---------------------------------------------------------------------------

/// 单次遍历扫描（同 Go 版）：一次目录遍历同时匹配全部规则；命中可删目录后不深入
/// 其内部（内含排除路径的目录除外——不整体删除、继续扫描内部）；不跟随符号链接；
/// 遍历结束剔除嵌套候选（父项优先）。
///
/// `on_progress(visited, matched)` 每 512 个条目回调一次 + 结束时回调一次，
/// 节流由调用方负责。
pub fn scan(
    req: &CleanerScanRequest,
    protected: &[PathBuf],
    mut on_progress: impl FnMut(u64, u64),
) -> Result<ScanOutcome, AppError> {
    if req.operation_paths.is_empty() {
        return Err(AppError::Other("请至少配置一个父路径".into()));
    }
    let rules = compile_rules(&req.rules)?;
    let operation_paths = normalize_operation_paths(&req.operation_paths)?;
    let exclude_paths = canonicalize_loose(&req.exclude_paths, "排除路径")?;
    let exclude_keys: Vec<String> = exclude_paths.iter().map(|p| path_key(p)).collect();
    let protected_keys: Vec<String> = protected.iter().map(|p| path_key(p)).collect();

    let mut outcome = ScanOutcome {
        exclude_paths,
        ..Default::default()
    };
    let mut candidates: HashMap<String, ScannedItem> = HashMap::new();

    for op in &operation_paths {
        let op_key = path_key(op);
        if is_path_in_any_key(&op_key, &exclude_keys) {
            outcome
                .warnings
                .push(format!("父路径命中排除清单，已整体跳过：{}", display_of(op)));
            continue;
        }
        let mut stack: Vec<PathBuf> = vec![op.clone()];
        while let Some(dir) = stack.pop() {
            let read_dir = match fs::read_dir(&dir) {
                Ok(rd) => rd,
                Err(e) => {
                    outcome
                        .warnings
                        .push(format!("无法读取目录，已跳过：{}（{e}）", display_of(&dir)));
                    continue;
                }
            };
            for entry in read_dir {
                let entry = match entry {
                    Ok(e) => e,
                    Err(e) => {
                        outcome.warnings.push(format!(
                            "无法读取目录项，已跳过：{}（{e}）",
                            display_of(&dir)
                        ));
                        continue;
                    }
                };
                outcome.visited += 1;
                if outcome.visited % 512 == 0 {
                    on_progress(outcome.visited, outcome.matched);
                }

                let path = entry.path();
                let key = path_key(&path);
                if is_path_in_any_key(&key, &exclude_keys) {
                    continue;
                }
                // 符号链接目录 file_type().is_dir() 为 false → 不递归（同 Go
                // isDir && !ModeSymlink）。
                let is_dir = match entry.file_type() {
                    Ok(ft) => ft.is_dir(),
                    Err(e) => {
                        outcome.warnings.push(format!(
                            "无法读取路径属性，已跳过：{}（{e}）",
                            display_of(&path)
                        ));
                        continue;
                    }
                };
                let name = entry.file_name();
                let name = name.to_string_lossy();

                let mut hit = false;
                let mut allow_files = false;
                for rule in &rules {
                    if rule.kind == CleanerKind::Dir && !is_dir {
                        continue;
                    }
                    if rule.kind == CleanerKind::File && is_dir {
                        continue;
                    }
                    if !rule.matches(&name) {
                        continue;
                    }
                    hit = true;
                    if rule.allows_files() {
                        allow_files = true;
                    }
                }
                if !hit {
                    if is_dir {
                        stack.push(path);
                    }
                    continue;
                }
                outcome.matched += 1;

                if !is_dir && !allow_files {
                    // 防御性分支：kind=dir 规则已被上方类型过滤拦截，正常不可达；
                    // 文件仅在 file/any 规则下才可能进入候选。
                    continue;
                }
                if is_dir && contains_any_key(&key, &exclude_keys) {
                    // 目录内含排除路径：不整体删除，继续扫描内部（同 Go）。
                    stack.push(path);
                    continue;
                }
                if is_root_key(&key) {
                    outcome
                        .warnings
                        .push(format!("禁止删除磁盘根目录，已跳过：{}", display_of(&path)));
                    continue;
                }
                if is_path_in_any_key(&key, &protected_keys)
                    || (is_dir && contains_any_key(&key, &protected_keys))
                {
                    outcome.warnings.push(format!(
                        "禁止删除受保护路径（应用自身/数据目录），已跳过：{}",
                        display_of(&path)
                    ));
                    if is_dir {
                        stack.push(path);
                    }
                    continue;
                }

                let size = if is_dir {
                    None
                } else {
                    entry.metadata().ok().map(|m| m.len())
                };
                candidates.entry(key).or_insert(ScannedItem {
                    display_path: display_of(&path),
                    io_path: path,
                    is_dir,
                    size,
                });
            }
        }
    }
    on_progress(outcome.visited, outcome.matched);

    outcome.items = remove_nested(candidates);
    outcome.items.sort_by(|a, b| a.display_path.cmp(&b.display_path));
    Ok(outcome)
}

/// 父路径归一化：必须绝对路径、存在且为目录；canonicalize 解析符号链接
/// （同 Go EvalSymlinks），Windows 产物为 verbatim 路径。
fn normalize_operation_paths(paths: &[String]) -> Result<Vec<PathBuf>, AppError> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for raw in paths {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }
        let p = Path::new(trimmed);
        if !p.is_absolute() {
            return Err(AppError::Other(format!("父路径必须是绝对路径：{trimmed}")));
        }
        let canon = fs::canonicalize(p)
            .map_err(|e| AppError::Other(format!("父路径不可访问：{trimmed}（{e}）")))?;
        if !canon.is_dir() {
            return Err(AppError::Other(format!("父路径不是目录：{trimmed}")));
        }
        if seen.insert(path_key(&canon)) {
            out.push(canon);
        }
    }
    if out.is_empty() {
        return Err(AppError::Other("请至少配置一个有效的父路径".into()));
    }
    Ok(out)
}

/// 宽松归一化（排除路径用）：必须绝对路径；目标存在则 canonicalize，缺失时
/// 仅做组件级规整（`..` 无法离线解析，原样保留）。
fn canonicalize_loose(paths: &[String], label: &str) -> Result<Vec<PathBuf>, AppError> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for raw in paths {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }
        let p = Path::new(trimmed);
        if !p.is_absolute() {
            return Err(AppError::Other(format!("{label}必须是绝对路径：{trimmed}")));
        }
        let resolved = fs::canonicalize(p).unwrap_or_else(|_| clean_components(p));
        if seen.insert(path_key(&resolved)) {
            out.push(resolved);
        }
    }
    Ok(out)
}

/// 纯字符串组件规整：去 `.` 与重复分隔符（不触碰 `..`）。
fn clean_components(p: &Path) -> PathBuf {
    use std::path::Component;
    let mut out = PathBuf::new();
    for component in p.components() {
        if !matches!(component, Component::CurDir) {
            out.push(component.as_os_str());
        }
    }
    out
}

/// 剔除嵌套候选：父目录已在清单内时子项不重复列（父项删除会带走整棵子树）。
fn remove_nested(candidates: HashMap<String, ScannedItem>) -> Vec<ScannedItem> {
    let mut keyed: Vec<(String, ScannedItem)> = candidates.into_iter().collect();
    // 短 key 在前：父目录先进入已收清单。
    keyed.sort_by(|a, b| a.0.len().cmp(&b.0.len()).then_with(|| a.0.cmp(&b.0)));
    let mut kept: Vec<(String, ScannedItem)> = Vec::with_capacity(keyed.len());
    for (key, item) in keyed {
        let covered = kept
            .iter()
            .any(|(parent_key, parent)| parent.is_dir && contains_any_key(parent_key, std::slice::from_ref(&key)));
        if !covered {
            kept.push((key, item));
        }
    }
    kept.into_iter().map(|(_, item)| item).collect()
}

// ---------------------------------------------------------------------------
// 删除（执行阶段逐条复检，同 Go 清单元数据语义）
// ---------------------------------------------------------------------------

/// 按清单执行删除。逐项复检：磁盘根目录 / 排除清单（命中或目录内含）/
/// 受保护路径（命中或目录内含）。单项失败不中断，原因写入结果。
pub fn execute(
    items: &[ScannedItem],
    exclude_paths: &[PathBuf],
    protected: &[PathBuf],
) -> Vec<DeleteItemResult> {
    let exclude_keys: Vec<String> = exclude_paths.iter().map(|p| path_key(p)).collect();
    let protected_keys: Vec<String> = protected.iter().map(|p| path_key(p)).collect();
    items
        .iter()
        .map(|item| {
            let key = path_key(&item.io_path);
            match recheck(&key, item.is_dir, &exclude_keys, &protected_keys)
                .and_then(|()| remove_item(item))
            {
                Ok(()) => DeleteItemResult {
                    path: item.display_path.clone(),
                    ok: true,
                    reason: None,
                },
                Err(reason) => DeleteItemResult {
                    path: item.display_path.clone(),
                    ok: false,
                    reason: Some(reason),
                },
            }
        })
        .collect()
}

fn recheck(
    key: &str,
    is_dir: bool,
    exclude_keys: &[String],
    protected_keys: &[String],
) -> Result<(), String> {
    if is_root_key(key) {
        return Err("禁止删除磁盘根目录".into());
    }
    if is_path_in_any_key(key, exclude_keys) {
        return Err("命中排除清单，已阻止删除".into());
    }
    if is_dir && contains_any_key(key, exclude_keys) {
        return Err("目录内部包含排除路径，已阻止整体删除".into());
    }
    if is_path_in_any_key(key, protected_keys) || (is_dir && contains_any_key(key, protected_keys)) {
        return Err("禁止删除受保护路径（应用自身/数据目录）".into());
    }
    Ok(())
}

fn remove_item(item: &ScannedItem) -> Result<(), String> {
    let meta = fs::symlink_metadata(&item.io_path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            "路径不存在".to_string()
        } else {
            e.to_string()
        }
    })?;
    if item.is_dir {
        return fs::remove_dir_all(&item.io_path).map_err(|e| e.to_string());
    }
    // 只读文件（Windows readonly 属性）先尝试去只读位再删（同 Go chmod +0200）。
    if meta.permissions().readonly() {
        let mut perms = meta.permissions();
        perms.set_readonly(false);
        let _ = fs::set_permissions(&item.io_path, perms);
    }
    fs::remove_file(&item.io_path).map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// 目录大小（异步补算；符号链接按自身大小计入、不跟随）
// ---------------------------------------------------------------------------

pub fn compute_size(io_path: &Path, is_dir: bool) -> u64 {
    if !is_dir {
        return fs::symlink_metadata(io_path).map(|m| m.len()).unwrap_or(0);
    }
    let mut total = 0u64;
    let mut stack = vec![io_path.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(read_dir) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in read_dir.flatten() {
            match entry.file_type() {
                Ok(ft) if ft.is_dir() => stack.push(entry.path()),
                Ok(_) => total += entry.metadata().map(|m| m.len()).unwrap_or(0),
                Err(_) => {}
            }
        }
    }
    total
}

// ---------------------------------------------------------------------------
// 测试（fixture 一律 std::env::temp_dir()，平台规范 §5）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture {
        root: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir()
                .join(format!("gw-cleaner-test-{}", uuid::Uuid::new_v4().simple()));
            fs::create_dir_all(&root).unwrap();
            Self { root }
        }
        fn dir(&self, rel: &str) -> PathBuf {
            let p = self.root.join(rel);
            fs::create_dir_all(&p).unwrap();
            p
        }
        fn file(&self, rel: &str, content: &[u8]) -> PathBuf {
            let p = self.root.join(rel);
            if let Some(parent) = p.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(&p, content).unwrap();
            p
        }
        /// 与扫描侧同源的归一化期望键（canonical 化 + 归一化比较键）。
        fn key(&self, rel: &str) -> String {
            path_key(&fs::canonicalize(self.root.join(rel)).unwrap())
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn rule(pattern: &str, mode: CleanerMatchMode, kind: CleanerKind) -> CleanerRule {
        CleanerRule {
            pattern: pattern.into(),
            match_mode: mode,
            kind,
        }
    }

    fn scan_quiet(req: &CleanerScanRequest) -> Result<ScanOutcome, AppError> {
        scan(req, &[], |_, _| {})
    }

    fn candidate_keys(outcome: &ScanOutcome) -> HashSet<String> {
        outcome
            .items
            .iter()
            .map(|i| path_key(&i.io_path))
            .collect()
    }

    // ---- 通配符匹配器 ----

    #[test]
    fn wildcard_matches_star_and_question() {
        assert!(wildcard_match("*空环境*.zip", "备份空环境-v2.zip"));
        assert!(wildcard_match("*.zip", "a.zip"));
        assert!(!wildcard_match("*.zip", "a.zipx"));
        assert!(wildcard_match("a?c", "abc"));
        assert!(!wildcard_match("a?c", "ac"));
        assert!(wildcard_match("a*b*c", "aXbYc"));
        assert!(!wildcard_match("a*b*c", "aXbY"));
        // 无通配符时退化为全等
        assert!(wildcard_match("node_modules", "node_modules"));
        assert!(!wildcard_match("node_modules", "node_modules2"));
        // 星号可匹配空串
        assert!(wildcard_match("*.log", ".log"));
        assert!(wildcard_match("a*", "a"));
    }

    // ---- 规则编译守卫 ----

    #[test]
    fn compile_rejects_unsafe_patterns() {
        let bad = [
            rule("", CleanerMatchMode::Exact, CleanerKind::Dir),
            rule(".", CleanerMatchMode::Exact, CleanerKind::Dir),
            rule("..", CleanerMatchMode::Exact, CleanerKind::Dir),
            rule("a/b", CleanerMatchMode::Exact, CleanerKind::Dir),
            rule("a\\b", CleanerMatchMode::Exact, CleanerKind::Dir),
            rule("*", CleanerMatchMode::Wildcard, CleanerKind::Dir),
            rule("**", CleanerMatchMode::Wildcard, CleanerKind::Any),
            rule("[ab].txt", CleanerMatchMode::Wildcard, CleanerKind::File),
        ];
        for (index, r) in bad.iter().enumerate() {
            assert!(compile_rules(&[r.clone()]).is_err(), "bad rule #{index} must fail");
        }
        assert!(compile_rules(&[]).is_err());
        assert!(compile_rules(&[rule("node_modules", CleanerMatchMode::Exact, CleanerKind::Dir)]).is_ok());
        assert!(compile_rules(&[rule("*.tmp", CleanerMatchMode::Wildcard, CleanerKind::File)]).is_ok());
        // 精确模式下 * 只是普通字符，不受纯 * 守卫限制
        assert!(compile_rules(&[rule("*", CleanerMatchMode::Exact, CleanerKind::Dir)]).is_ok());
    }

    #[test]
    #[cfg(any(windows, target_os = "macos"))]
    fn exact_match_folds_case_on_insensitive_filesystems() {
        let rules = compile_rules(&[rule("NODE_MODULES", CleanerMatchMode::Exact, CleanerKind::Dir)]).unwrap();
        assert!(rules[0].matches("node_modules"));
    }

    #[test]
    #[cfg(not(any(windows, target_os = "macos")))]
    fn exact_match_keeps_case_on_sensitive_filesystems() {
        let rules = compile_rules(&[rule("NODE_MODULES", CleanerMatchMode::Exact, CleanerKind::Dir)]).unwrap();
        assert!(!rules[0].matches("node_modules"));
    }

    // ---- 路径键 / 安全门禁 ----

    #[test]
    fn root_key_detection() {
        assert!(is_root_key(""));
        assert!(is_root_key("c:"));
        assert!(is_root_key("//host"));
        assert!(is_root_key("//host/share"));
        assert!(!is_root_key("c:/a"));
        assert!(!is_root_key("//host/share/dir"));
        assert!(!is_root_key("/usr"));
        assert!(!is_root_key("/usr/local"));
    }

    #[test]
    fn path_membership_checks_normalize_separators() {
        let bases = vec!["d:/ws/keep".to_string()];
        assert!(is_path_in_any_key("d:/ws/keep", &bases));
        assert!(is_path_in_any_key("d:/ws/keep/inner", &bases));
        assert!(!is_path_in_any_key("d:/ws/keep2", &bases));
        assert!(!is_path_in_any_key("d:/ws", &bases));

        let children = vec!["d:/ws/env/inner".to_string()];
        assert!(contains_any_key("d:/ws/env", &children));
        assert!(!contains_any_key("d:/ws/env2", &children));
        assert!(!contains_any_key("d:/ws/env/inner", &children));
    }

    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    fn path_membership_folds_case_and_verbatim() {
        let bases = vec![path_key(Path::new(r"\\?\D:\WS\Keep"))];
        assert!(is_path_in_any_key(&path_key(Path::new(r"D:\ws\keep\inner")), &bases));
    }

    #[test]
    fn remove_nested_keeps_parent_dir_only() {
        let mut candidates = HashMap::new();
        for (rel, is_dir) in [("a", true), ("a/b", true), ("a/b/c.txt", false), ("x", true)] {
            let p = PathBuf::from(format!("/root/{rel}"));
            candidates.insert(
                path_key(&p),
                ScannedItem {
                    display_path: p.to_string_lossy().into(),
                    io_path: p,
                    is_dir,
                    size: None,
                },
            );
        }
        let items = remove_nested(candidates);
        let keys: HashSet<String> = items.iter().map(|i| path_key(&i.io_path)).collect();
        assert_eq!(keys.len(), 2);
        assert!(keys.contains(&path_key(Path::new("/root/a"))));
        assert!(keys.contains(&path_key(Path::new("/root/x"))));
    }

    // ---- 扫描端到端 ----

    /// 典型工作区树：
    /// root/app/node_modules（命中，且不深入其内部的 inner/node_modules）
    /// root/lib/node_modules（命中）
    /// root/keep/node_modules（父目录整体排除）
    /// root/docs/readme.md（文件规则命中）
    /// root/docs/weird.md/（目录名像文件，文件规则不得命中目录）
    fn build_workspace_fixture() -> Fixture {
        let fx = Fixture::new();
        fx.file("app/node_modules/pkg/index.js", b"1");
        fx.file("app/node_modules/inner/node_modules/deep.js", b"1");
        fx.file("app/src/main.rs", b"fn main() {}");
        fx.file("lib/node_modules/x.js", b"1");
        fx.file("keep/node_modules/secret.js", b"1");
        fx.file("docs/readme.md", b"# readme");
        fx.file("docs/weird.md/inside.txt", b"not a file target");
        fx
    }

    #[test]
    fn scan_matches_rules_and_respects_excludes() {
        let fx = build_workspace_fixture();
        let req = CleanerScanRequest {
            operation_paths: vec![fx.root.to_string_lossy().into()],
            exclude_paths: vec![fx.root.join("keep").to_string_lossy().into()],
            rules: vec![
                rule("node_modules", CleanerMatchMode::Exact, CleanerKind::Dir),
                rule("*.md", CleanerMatchMode::Wildcard, CleanerKind::File),
            ],
        };
        let outcome = scan_quiet(&req).unwrap();
        let keys = candidate_keys(&outcome);
        assert_eq!(keys.len(), 3, "items: {:?}", keys);
        assert!(keys.contains(&fx.key("app/node_modules")));
        assert!(keys.contains(&fx.key("lib/node_modules")));
        assert!(keys.contains(&fx.key("docs/readme.md")));
        // 父目录命中后不深入：inner/node_modules 不单独出现
        assert!(!keys.contains(&fx.key("app/node_modules/inner/node_modules")));
        // 排除目录内目标不出现
        assert!(!keys.contains(&fx.key("keep/node_modules")));
        // 文件规则不得命中目录
        assert!(!keys.contains(&fx.key("docs/weird.md")));
        // 文件候选扫描时已带大小
        let readme = outcome
            .items
            .iter()
            .find(|i| i.display_path.ends_with("readme.md"))
            .unwrap();
        assert!(!readme.is_dir);
        assert_eq!(readme.size, Some(8)); // b"# readme".len()
        assert!(outcome.visited >= 10);
        assert!(outcome.matched >= 3);
    }

    #[test]
    fn scan_dir_containing_exclude_is_not_deleted_wholesale_but_recursed() {
        let fx = Fixture::new();
        fx.file("proj/env/inner/keep.txt", b"keep");
        fx.file("proj/env/sub/env/nested.txt", b"nested");
        let req = CleanerScanRequest {
            operation_paths: vec![fx.root.to_string_lossy().into()],
            exclude_paths: vec![fx.root.join("proj/env/inner").to_string_lossy().into()],
            rules: vec![rule("env", CleanerMatchMode::Exact, CleanerKind::Dir)],
        };
        let outcome = scan_quiet(&req).unwrap();
        let keys = candidate_keys(&outcome);
        // proj/env 内含排除路径 → 不整体删除
        assert!(!keys.contains(&fx.key("proj/env")));
        // 但继续扫描内部 → 嵌套 env 命中
        assert!(keys.contains(&fx.key("proj/env/sub/env")));
        assert_eq!(keys.len(), 1);
    }

    #[test]
    fn scan_refuses_protected_paths() {
        let fx = Fixture::new();
        fx.file("app/node_modules/x.js", b"1");
        fx.file("protected_zone/node_modules/y.js", b"1");
        fx.file("zone/node_modules/z.js", b"1");
        let protected = vec![
            fs::canonicalize(fx.root.join("protected_zone")).unwrap(),
            fs::canonicalize(fx.root.join("zone")).unwrap(),
        ];
        let req = CleanerScanRequest {
            operation_paths: vec![fx.root.to_string_lossy().into()],
            exclude_paths: vec![],
            rules: vec![rule("node_modules", CleanerMatchMode::Exact, CleanerKind::Dir)],
        };
        let outcome = scan(&req, &protected, |_, _| {}).unwrap();
        let keys = candidate_keys(&outcome);
        // protected_zone/node_modules：命中受保护路径后代 → 拒绝
        // zone/node_modules：目录内含受保护路径（zone 本身是受保护目录）→ 拒绝
        assert_eq!(keys.len(), 1);
        assert!(keys.contains(&fx.key("app/node_modules")));
        assert!(!outcome.warnings.is_empty());
    }

    #[test]
    fn scan_dedupes_overlapping_operation_paths() {
        let fx = build_workspace_fixture();
        let req = CleanerScanRequest {
            operation_paths: vec![
                fx.root.to_string_lossy().into(),
                fx.root.join("app").to_string_lossy().into(),
            ],
            exclude_paths: vec![fx.root.join("keep").to_string_lossy().into()],
            rules: vec![rule("node_modules", CleanerMatchMode::Exact, CleanerKind::Dir)],
        };
        let outcome = scan_quiet(&req).unwrap();
        let keys = candidate_keys(&outcome);
        assert_eq!(keys.len(), 2);
        assert!(keys.contains(&fx.key("app/node_modules")));
        assert!(keys.contains(&fx.key("lib/node_modules")));
    }

    #[test]
    fn scan_rejects_invalid_operation_paths() {
        let fx = Fixture::new();
        let file_path = fx.file("a.txt", b"1");
        let rules = vec![rule("node_modules", CleanerMatchMode::Exact, CleanerKind::Dir)];

        let empty = CleanerScanRequest {
            operation_paths: vec![],
            exclude_paths: vec![],
            rules: rules.clone(),
        };
        assert!(scan_quiet(&empty).is_err());

        let relative = CleanerScanRequest {
            operation_paths: vec!["some/relative".into()],
            exclude_paths: vec![],
            rules: rules.clone(),
        };
        assert!(scan_quiet(&relative).is_err());

        let missing = CleanerScanRequest {
            operation_paths: vec![fx.root.join("not-exist").to_string_lossy().into()],
            exclude_paths: vec![],
            rules: rules.clone(),
        };
        assert!(scan_quiet(&missing).is_err());

        let file_as_op = CleanerScanRequest {
            operation_paths: vec![file_path.to_string_lossy().into()],
            exclude_paths: vec![],
            rules,
        };
        assert!(scan_quiet(&file_as_op).is_err());
    }

    #[test]
    fn scan_dir_rule_never_lists_files_and_file_rule_never_lists_dirs() {
        let fx = Fixture::new();
        fx.file("proj/build/output.bin", b"1");
        fx.file("proj/build.log", b"log");
        // build 作为目录规则：proj/build 命中；build.log 文件不得命中
        let req = CleanerScanRequest {
            operation_paths: vec![fx.root.to_string_lossy().into()],
            exclude_paths: vec![],
            rules: vec![rule("build", CleanerMatchMode::Exact, CleanerKind::Dir)],
        };
        let outcome = scan_quiet(&req).unwrap();
        let keys = candidate_keys(&outcome);
        assert_eq!(keys.len(), 1);
        assert!(keys.contains(&fx.key("proj/build")));

        // build 作为文件规则：proj/build 目录不得命中
        let req = CleanerScanRequest {
            operation_paths: vec![fx.root.to_string_lossy().into()],
            exclude_paths: vec![],
            rules: vec![rule("build", CleanerMatchMode::Exact, CleanerKind::File)],
        };
        let outcome = scan_quiet(&req).unwrap();
        assert!(outcome.items.is_empty());
    }

    // ---- 删除 ----

    #[test]
    fn execute_deletes_candidates_and_keeps_excluded() {
        let fx = build_workspace_fixture();
        let exclude = vec![fs::canonicalize(fx.root.join("keep")).unwrap()];
        let req = CleanerScanRequest {
            operation_paths: vec![fx.root.to_string_lossy().into()],
            exclude_paths: vec![fx.root.join("keep").to_string_lossy().into()],
            rules: vec![rule("node_modules", CleanerMatchMode::Exact, CleanerKind::Dir)],
        };
        let outcome = scan_quiet(&req).unwrap();
        assert_eq!(outcome.items.len(), 2);

        let results = execute(&outcome.items, &outcome.exclude_paths, &[]);
        assert!(results.iter().all(|r| r.ok), "results: {results:?}");
        assert!(!fx.root.join("app/node_modules").exists());
        assert!(!fx.root.join("lib/node_modules").exists());
        // 排除目录原样保留
        assert!(fx.root.join("keep/node_modules/secret.js").exists());
        drop(exclude);
    }

    #[test]
    fn execute_rechecks_root_and_protected() {
        let fx = Fixture::new();
        fx.file("zone/node_modules/x.js", b"1");

        // 磁盘根：从 temp_dir 推导当前平台根（Windows 盘符根 / Unix "/"）
        let root_path = std::env::temp_dir()
            .ancestors()
            .last()
            .map(Path::to_path_buf)
            .unwrap();
        let root_item = ScannedItem {
            display_path: root_path.to_string_lossy().into(),
            io_path: root_path,
            is_dir: true,
            size: None,
        };
        // 受保护目录：zone
        let protected_item = ScannedItem {
            display_path: "zone".into(),
            io_path: fx.root.join("zone"),
            is_dir: true,
            size: None,
        };
        let protected = vec![fs::canonicalize(fx.root.join("zone")).unwrap()];
        let results = execute(&[root_item, protected_item], &[], &protected);
        assert_eq!(results.len(), 2);
        assert!(!results[0].ok);
        assert!(results[0].reason.as_ref().unwrap().contains("根目录"));
        assert!(!results[1].ok);
        assert!(results[1].reason.as_ref().unwrap().contains("受保护"));
        // 未真实删除
        assert!(fx.root.join("zone/node_modules/x.js").exists());
    }

    #[test]
    fn execute_rechecks_dir_containing_exclude() {
        let fx = Fixture::new();
        fx.file("proj/env/inner/keep.txt", b"keep");
        let item = ScannedItem {
            display_path: "env".into(),
            io_path: fx.root.join("proj/env"),
            is_dir: true,
            size: None,
        };
        let exclude = vec![fs::canonicalize(fx.root.join("proj/env/inner")).unwrap()];
        let results = execute(&[item], &exclude, &[]);
        assert_eq!(results.len(), 1);
        assert!(!results[0].ok);
        assert!(results[0].reason.as_ref().unwrap().contains("排除"));
        assert!(fx.root.join("proj/env/inner/keep.txt").exists());
    }

    #[cfg(windows)]
    #[test]
    fn execute_removes_readonly_file() {
        let fx = Fixture::new();
        let file = fx.file("proj/locked.tmp", b"locked");
        let mut perms = fs::metadata(&file).unwrap().permissions();
        perms.set_readonly(true);
        fs::set_permissions(&file, perms).unwrap();

        let item = ScannedItem {
            display_path: file.to_string_lossy().into(),
            io_path: file.clone(),
            is_dir: false,
            size: Some(6),
        };
        let results = execute(&[item], &[], &[]);
        assert!(results[0].ok, "results: {results:?}");
        assert!(!file.exists());
    }

    #[test]
    fn execute_reports_missing_path_without_aborting() {
        let fx = Fixture::new();
        fx.file("real.tmp", b"1");
        let missing = ScannedItem {
            display_path: "missing".into(),
            io_path: fx.root.join("missing.tmp"),
            is_dir: false,
            size: None,
        };
        let real = ScannedItem {
            display_path: "real".into(),
            io_path: fx.root.join("real.tmp"),
            is_dir: false,
            size: Some(1),
        };
        let results = execute(&[missing, real], &[], &[]);
        assert_eq!(results.len(), 2);
        assert!(!results[0].ok);
        assert!(results[0].reason.as_ref().unwrap().contains("不存在"));
        assert!(results[1].ok);
        assert!(!fx.root.join("real.tmp").exists());
    }

    // ---- 大小统计 ----

    #[test]
    fn compute_size_sums_files_recursively() {
        let fx = Fixture::new();
        let dir = fx.dir("pkg");
        fx.file("pkg/a.bin", &[0u8; 10]);
        fx.file("pkg/sub/b.bin", &[0u8; 20]);
        fx.file("pkg/sub/deep/c.bin", &[0u8; 12]);
        assert_eq!(compute_size(&dir, true), 42);
        let file = fx.file("single.bin", &[0u8; 7]);
        assert_eq!(compute_size(&file, false), 7);
        // 缺失路径返回 0 而不是 panic
        assert_eq!(compute_size(&fx.root.join("missing"), false), 0);
    }
}
