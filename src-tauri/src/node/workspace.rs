//! npm/yarn/pnpm workspaces 识别（N-09）。
//!
//! 只做「workspace 根与子包归属」判定，供安装路由（monorepo 依赖必须装在
//! 根）与列表展示使用；**不解读 script 语义、不路由启动命令**——子包内
//! `npm/pnpm/bun run <script>` 由包管理器自行解析（全局约束 §1）。
//!
//! - npm / yarn：根 `package.json` 的 `workspaces` 字段（字符串数组或
//!   `{ "packages": [...] }`）。
//! - pnpm：根目录 `pnpm-workspace.yaml` 的 `packages:` 列表（朴素行解析，
//!   只取 `- <pattern>` 项）。
//!
//! 模式匹配支持常见形态：`packages/*`、`apps/web`、`*`、`**`；不引入 glob
//! 依赖，`*`/`**` 按「相对路径段」匹配（纯函数，样例单测）。

use std::path::{Path, PathBuf};

/// 从 package.json 原文提取 `workspaces` 模式列表；无字段返回 `None`。
pub fn extract_workspaces(manifest_json: &str) -> Option<Vec<String>> {
    let manifest: serde_json::Value = serde_json::from_str(manifest_json).ok()?;
    match manifest.get("workspaces")? {
        serde_json::Value::Array(items) => Some(patterns_from(items)),
        serde_json::Value::Object(map) => Some(patterns_from(map.get("packages")?.as_array()?)),
        _ => None,
    }
}

fn patterns_from(items: &[serde_json::Value]) -> Vec<String> {
    items
        .iter()
        .filter_map(|item| item.as_str())
        .map(str::to_string)
        .collect()
}

/// 从 pnpm-workspace.yaml 朴素提取 `packages` 列表（`- <pattern>` 行）。
/// 只覆盖常见单行列表形态；解析失败返回 `None`。
pub fn extract_pnpm_workspace_packages(yaml: &str) -> Option<Vec<String>> {
    let mut in_packages = false;
    let mut patterns = Vec::new();
    for line in yaml.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("packages:") {
            in_packages = true;
            // 同行内联列表形态：packages: ['packages/*']
            let inline = rest.trim().trim_matches(|c| c == '[' || c == ']');
            for item in inline.split(',') {
                let value = item.trim().trim_matches(|c| c == '\'' || c == '"');
                if !value.is_empty() {
                    patterns.push(value.to_string());
                }
            }
            continue;
        }
        if in_packages {
            if let Some(value) = trimmed.strip_prefix("- ") {
                let value = value.trim().trim_matches(|c| c == '\'' || c == '"');
                if !value.is_empty() {
                    patterns.push(value.to_string());
                }
                continue;
            }
            if line.starts_with(' ') || line.starts_with('\t') {
                continue;
            }
            // 下一个顶层键：packages 列表结束。
            in_packages = false;
        }
    }
    (!patterns.is_empty()).then_some(patterns)
}

/// 相对路径是否命中 workspace 模式。`*` 匹配单段，`**` 匹配任意段；
/// 其余段按字面相等。分隔符两侧统一按 `/` 归一化（path_key 语义）。
pub fn path_matches_workspace_pattern(relative_path: &str, pattern: &str) -> bool {
    let relative = relative_path.replace('\\', "/");
    let normalized_pattern = pattern.replace('\\', "/");
    let segments: Vec<&str> = relative.split('/').filter(|s| !s.is_empty()).collect();
    let pattern_segments: Vec<&str> = normalized_pattern
        .split('/')
        .filter(|s| !s.is_empty() && *s != "./")
        .collect();
    match_segments(&segments, &pattern_segments)
}

fn match_segments(path: &[&str], pattern: &[&str]) -> bool {
    match pattern.split_first() {
        None => path.is_empty(),
        Some((&"**", rest)) => {
            // `**` 吞 0..n 段。
            (0..=path.len()).any(|skip| match_segments(&path[skip..], rest))
        }
        Some((first, rest)) => {
            let Some((head, tail)) = path.split_first() else {
                return false;
            };
            (*first == "*" || *first == *head) && match_segments(tail, rest)
        }
    }
}

/// N-09：安装目录路由——workspace 子包的依赖统一装在根（依赖提升与
/// lockfile 都在根）；独立工程装在自身目录。
pub fn install_dir_for(project_dir: &Path) -> PathBuf {
    find_workspace_root(project_dir, 4).unwrap_or_else(|| project_dir.to_path_buf())
}

/// 从 project 目录逐级向上（最多 `max_depth` 层）找 workspace 根：
/// 目录的 package.json 带 `workspaces`（npm/yarn）或存在 `pnpm-workspace.yaml`
/// （pnpm），且 project 相对路径命中任一模式。project 自身是根时返回 `None`
/// （安装本就在正确位置）。
pub fn find_workspace_root(project_dir: &Path, max_depth: usize) -> Option<PathBuf> {
    let canonical = std::fs::canonicalize(project_dir).unwrap_or_else(|_| project_dir.to_path_buf());
    let mut current = canonical.as_path();
    for _ in 0..max_depth {
        let parent = current.parent()?;
        current = parent;
        let relative = canonical.strip_prefix(current).ok()?.to_string_lossy().to_string();
        if let Some(patterns) = root_workspaces_of(current) {
            if patterns
                .iter()
                .any(|pattern| path_matches_workspace_pattern(&relative, pattern))
            {
                return Some(current.to_path_buf());
            }
        }
    }
    None
}

/// 目录作为 workspace 根的 workspaces 模式：npm/yarn（package.json）优先，
/// pnpm（pnpm-workspace.yaml）回退；两者皆无 → `None`。
fn root_workspaces_of(dir: &Path) -> Option<Vec<String>> {
    if let Ok(manifest) = std::fs::read_to_string(dir.join("package.json")) {
        if let Some(patterns) = extract_workspaces(&manifest) {
            return Some(patterns);
        }
    }
    let yaml = std::fs::read_to_string(dir.join("pnpm-workspace.yaml")).ok()?;
    extract_pnpm_workspace_packages(&yaml)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_workspaces_array_and_packages_object() {
        assert_eq!(
            extract_workspaces(r#"{"name":"root","workspaces":["packages/*","apps/web"]}"#),
            Some(vec!["packages/*".to_string(), "apps/web".to_string()])
        );
        assert_eq!(
            extract_workspaces(r#"{"workspaces":{"packages":["libs/*"],"nohoist":["x"]}}"#),
            Some(vec!["libs/*".to_string()])
        );
        assert_eq!(extract_workspaces(r#"{"name":"app"}"#), None);
        assert_eq!(extract_workspaces("not json"), None);
    }

    #[test]
    fn extracts_pnpm_workspace_packages_list() {
        let yaml = "packages:\n  - 'packages/*'\n  - \"apps/*\"\n# comment\nbuild:\n  x: 1\n";
        assert_eq!(
            extract_pnpm_workspace_packages(yaml),
            Some(vec!["packages/*".to_string(), "apps/*".to_string()])
        );
        assert_eq!(extract_pnpm_workspace_packages("nothing: 1\n"), None);
    }

    #[test]
    fn matches_single_segment_and_double_star_patterns() {
        assert!(path_matches_workspace_pattern("packages/web", "packages/*"));
        assert!(path_matches_workspace_pattern("packages\\web", "packages/*"));
        assert!(!path_matches_workspace_pattern("packages/web/inner", "packages/*"));
        assert!(path_matches_workspace_pattern("packages/web/inner", "packages/**"));
        assert!(path_matches_workspace_pattern("apps/web", "*/*"));
        assert!(path_matches_workspace_pattern("web", "*"));
        assert!(!path_matches_workspace_pattern("a/b", "a"));
        assert!(path_matches_workspace_pattern("apps/web", "apps/web"));
    }

    #[test]
    fn finds_workspace_root_for_npm_and_pnpm_layouts() {
        let root = std::env::temp_dir().join(format!("gw_n09_ws_{}", uuid::Uuid::new_v4()));
        let packages = root.join("packages/web");
        std::fs::create_dir_all(&packages).unwrap();
        // npm 形态。
        std::fs::write(
            root.join("package.json"),
            r#"{"name":"root","workspaces":["packages/*"]}"#,
        )
        .unwrap();
        assert_eq!(
            find_workspace_root(&packages, 4),
            Some(root.clone()),
            "npm workspaces root should be found"
        );
        // pnpm 形态：去掉 package.json workspaces，改用 pnpm-workspace.yaml。
        std::fs::write(root.join("package.json"), r#"{"name":"root"}"#).unwrap();
        std::fs::write(root.join("pnpm-workspace.yaml"), "packages:\n  - packages/*\n").unwrap();
        assert_eq!(find_workspace_root(&packages, 4), Some(root.clone()));
        // 根自身不作为 workspace root（装依赖本就在正确位置）。
        assert_eq!(find_workspace_root(&root, 4), None);
        // 深度不足时找不到。
        let deep = packages.join("a/b/c");
        std::fs::create_dir_all(&deep).unwrap();
        assert_eq!(find_workspace_root(&deep, 2), None);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn install_dir_routes_workspace_subpackage_to_root() {
        let root = std::env::temp_dir().join(format!("gw_n09_inst_{}", uuid::Uuid::new_v4()));
        let packages = root.join("packages/web");
        std::fs::create_dir_all(&packages).unwrap();
        std::fs::write(
            root.join("package.json"),
            r#"{"name":"root","workspaces":["packages/*"]}"#,
        )
        .unwrap();
        assert_eq!(install_dir_for(&packages), root, "subpackage installs at root");
        let standalone = root.join("standalone");
        std::fs::create_dir_all(&standalone).unwrap();
        std::fs::write(standalone.join("package.json"), r#"{"name":"s"}"#).unwrap();
        assert_eq!(install_dir_for(&standalone), standalone, "standalone installs in place");
        let _ = std::fs::remove_dir_all(&root);
    }
}
