//! 路径比较辅助（平台规范 §1，AGENTS.md）。
//!
//! 「展示用原始形式、比较必须归一化」的公共实现：Windows verbatim 前缀
//! 剥离、分隔符统一、大小写折叠（按文件系统语义）。此前该逻辑在
//! `maven/index/path.rs`、`node/scan.rs`、`core/git_status.rs` 各有一份
//! 仿写副本，本模块统一收口；各调用方的比较语义差异（是否折叠大小写、
//! 是否 canonicalize）由调用方按需组合。
//!
//! 注意：本模块产物仅用于**比较**，不用于展示或文件 IO。

/// 剥离 Windows verbatim 前缀（`\\?\` / `\\?\UNC\`）。
pub fn strip_windows_verbatim_prefix(path: &str) -> String {
    if let Some(rest) = path.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{rest}")
    } else if let Some(rest) = path.strip_prefix(r"\\?\") {
        rest.to_string()
    } else {
        path.to_string()
    }
}

/// 大小写折叠：仅在大小写不敏感文件系统平台（Windows / macOS）生效，
/// Linux 保留大小写（平台差异用编译期分支，平台规范 §5）。
pub fn fold_case_for_fs(path: String) -> String {
    #[cfg(any(windows, target_os = "macos"))]
    {
        path.to_lowercase()
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        path
    }
}

/// 归一化用于比较：剥 verbatim 前缀 → 分隔符统一 `/` → 按平台折叠大小写。
pub fn normalize_for_compare(path: &str) -> String {
    let stripped = strip_windows_verbatim_prefix(path);
    let unified = stripped.replace('\\', "/");
    fold_case_for_fs(unified)
}

/// 路径组件级匹配：`path` 与 `needle` 相等，或 `needle` 的**完整组件序列**
/// 是 `path` 的尾部（`api` 命中 `.../backend/api`，不命中 `.../myapi`；
/// 多段 needle `backend/api` 不命中 `.../xbackend/api`）。两侧先经
/// [`normalize_for_compare`]，needle 可为相对片段或绝对路径。
pub fn path_component_match(path: &str, needle: &str) -> bool {
    let path = normalize_for_compare(path);
    let needle = normalize_for_compare(needle);
    if needle.is_empty() {
        return false;
    }
    if path == needle {
        return true;
    }
    let needle_components: Vec<&str> = needle.split('/').filter(|s| !s.is_empty()).collect();
    if needle_components.is_empty() {
        return false;
    }
    let path_components: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    path_components.len() > needle_components.len()
        && path_components[path_components.len() - needle_components.len()..] == needle_components[..]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_verbatim_prefixes() {
        assert_eq!(strip_windows_verbatim_prefix(r"\\?\C:\Users"), r"C:\Users");
        assert_eq!(
            strip_windows_verbatim_prefix(r"\\?\UNC\server\share"),
            r"\\server\share"
        );
        assert_eq!(strip_windows_verbatim_prefix("/ws/a"), "/ws/a");
    }

    #[test]
    fn component_match_requires_full_component_suffix() {
        // PAF-18 回归：project `api` 不得匹配 `.../myapi`。
        assert!(path_component_match("/ws/backend/api", "api"));
        assert!(path_component_match("/ws/backend/api", "/ws/backend/api"));
        assert!(!path_component_match("/ws/backend/myapi", "api"));
        assert!(!path_component_match("/ws/backend/apix", "api"));
        assert!(!path_component_match("/ws/backend/api", ""));
    }

    #[test]
    fn component_match_normalizes_separators_and_verbatim() {
        // 等值匹配（verbatim 前缀 / 分隔符差异被归一化抹平）。
        assert!(path_component_match(r"\\?\D:\ws\backend\api", r"D:\ws\backend\api"));
        assert!(path_component_match("D:/ws/backend/api", r"D:\ws\backend\api"));
        // 相对多段 needle 的组件级后缀。
        assert!(path_component_match("/x/y/backend/api", "backend/api"));
        assert!(!path_component_match("/x/y/xbackend/api", "backend/api"));
        // needle 为项目根的绝对路径时是等值比较，不是前缀关系（调用方
        // 传入的两侧都是项目根目录，见 service/watch/git_link）。
        assert!(!path_component_match(r"\\?\D:\ws\backend\api\src\main.rs", r"D:\ws\backend\api"));
    }

    // 大小写折叠仅在 Windows / macOS 生效（Linux 大小写敏感）。
    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    fn component_match_folds_case_on_insensitive_filesystems() {
        assert!(path_component_match(r"d:\WS\A\pom.xml", "D:/ws/a"));
    }

    #[cfg(not(any(windows, target_os = "macos")))]
    #[test]
    fn component_match_keeps_case_on_sensitive_filesystems() {
        assert!(!path_component_match("/WS/a/pom.xml", "/ws/a"));
    }
}
