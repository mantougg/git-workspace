//! 路径分类（R-17，B-07 拆分）：归一化、模块目录归属判定与 `ignore_path`
//! 忽略规则。纯函数，全部路径比较经正斜杠归一化（全局约束 §6）。

use std::path::Path;

/// 路径忽略规则（§43）：`target/` / `.git/` / `node_modules/` 等构建产物与
/// 元数据。纯函数（单测覆盖）。
pub fn ignore_path(normalized_path: &str) -> bool {
    const IGNORED_SEGMENTS: &[&str] = &["/target/", "/.git/", "/node_modules/", "/.gitworkspace/"];
    for segment in IGNORED_SEGMENTS {
        if normalized_path.contains(segment) {
            return true;
        }
    }
    // .class 文件（增量编译产物落 target 已排除；防御双扩展名等）。
    normalized_path.ends_with(".class")
}

/// 归一化为正斜杠分隔的字符串形式（Windows 混合分隔符安全）。
pub(super) fn normalize_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// 依赖图项目的模块目录（pom 路径的父目录，正斜杠归一化）。
pub(super) fn module_dir(project_pom_path: &Path) -> String {
    project_pom_path
        .parent()
        .unwrap_or(Path::new(""))
        .to_string_lossy()
        .replace('\\', "/")
}

/// 归属判定：归一化路径是否落在模块目录内（前缀匹配）。
pub(super) fn path_in_module_dir(normalized: &str, module_dir: &str) -> bool {
    normalized.starts_with(&format!("{module_dir}/"))
}
