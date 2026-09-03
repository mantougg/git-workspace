//! Maven 项目发现（R-01）：Workspace 级 `pom.xml` 扫描。
//!
//! 复用 T-01 Scanner 的忽略规则（`.gitworkspaceignore` + 默认跳过目录 `target/` 等），
//! 不另起一套目录遍历框架（全局约束 §7）。发现 + 解析全程本地完成，禁止网络请求
//! （全局约束 §10）。扫描与解析走 rayon 并行 + 批量，但并发度受 IO 预算约束，
//! 不与 Git status 争抢（沿用 T-01 经验）。

use std::collections::{BTreeSet, HashMap};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

use crate::core::scanner::{is_skip_dir, IgnoreRules, RepoScanner};
use crate::maven::cache::PomCache;
use crate::maven::effective::{build_effective, build_index, EffectiveProject};
use crate::maven::model::{MavenProject, MavenProjectType, MavenReactor, MavenReactorModule, PomCoordinates};
use crate::maven::parser::{parse_pom_file, PomParseError};

/// 单个发现到的 POM 条目（解析后的 model + 所在仓库相对路径）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveredPom {
    pub project: MavenProject,
    /// 相对于 workspace 根的路径（用 `/` 分隔），供 UI / 索引使用。
    pub relative_path: String,
}

/// 单个仓库的发现结果（按 repo 维度汇报进度，沿用 T-01 风格）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoDiscoveryResult {
    pub repo_path: String,
    pub repo_name: String,
    pub poms: Vec<DiscoveredPom>,
    /// 解析失败的 pom（路径 + 错误），不影响其它项目的发现。
    pub errors: Vec<PomDiscoveryError>,
}

/// 单个 POM 的结构化发现/解析错误。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PomDiscoveryError {
    pub code: String,
    pub path: String,
    pub message: String,
}

/// Workspace 级发现汇总。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MavenDiscoveryResult {
    pub projects: Vec<MavenProject>,
    pub effective: Vec<EffectiveProject>,
    pub reactors: Vec<MavenReactor>,
    pub errors: Vec<PomDiscoveryError>,
    pub elapsed_ms: u128,
    pub stats: crate::maven::PomCacheStats,
}

/// Workspace 级 `pom.xml` 扫描 + 解析 + effective model 构建。
///
/// 仓库清单（T-01 Scanner 输出的 `.git` 仓库）内逐 repo 下钻，并始终以 workspace
/// 根补扫仓库边界之外的区域（R-27：Runtime 发现以 workspace 为边界、与 git 解耦，
/// 覆盖无 git / 混合工作区）。
///
/// - `workspace_root`：workspace 根目录。
/// - `scan_depth`：最大下钻深度（与 T-01 Scanner 一致语义）。
/// - `cache`：可选 POM Cache，传入则命中缓存跳过重新解析。
/// - `cancel`：可选取消标志。
pub fn discover_poms(
    workspace_root: &Path,
    scan_depth: usize,
    cache: Option<&PomCache>,
    cancel: Option<&AtomicBool>,
) -> MavenDiscoveryResult {
    let start = Instant::now();
    let scanner = RepoScanner::new(scan_depth);
    let repos = scanner.scan_cancellable(workspace_root, cancel);
    if is_cancelled(cancel) {
        return empty_result(start);
    }

    let repo_paths: Vec<String> = repos.into_iter().map(|repo| repo.path).collect();
    let repo_results = discover_poms_in_repos(&repo_paths, workspace_root, cache, cancel);
    if is_cancelled(cancel) {
        return empty_result(start);
    }

    let mut projects = Vec::new();
    let mut errors = Vec::new();
    for repo in repo_results {
        projects.extend(repo.poms.into_iter().map(|pom| pom.project));
        errors.extend(repo.errors);
    }

    // R-27 仓库边界外补扫：Runtime 发现以 workspace 为边界、与 git 解耦。除仓库清
    // 单扫描外，始终以 workspace 根为伪仓库补扫一遍——`collect_pom_paths` 对含
    // `.git` 的目录保持边界跳过，补扫只会额外命中非仓库区域的 pom；忽略规则 /
    // POM Cache / 取消语义全部复用，不新增配置。非仓库区域的零散 pom（备份目录
    // 等）可用 `.gitworkspaceignore` 排除。
    let root = [workspace_root.to_string_lossy().to_string()];
    for repo in discover_poms_in_repos(&root, workspace_root, cache, cancel) {
        projects.extend(repo.poms.into_iter().map(|pom| pom.project));
        errors.extend(repo.errors);
    }
    if is_cancelled(cancel) {
        return empty_result(start);
    }

    // 根目录本身是仓库时，根级 pom 会被仓库扫描与补扫各收集一次，按路径去重
    //（补扫的重复解析走 POM Cache，零成本）。
    let mut seen = std::collections::HashSet::new();
    projects.retain(|project| seen.insert(canonical_or_original(&project.path)));
    let mut seen_error_paths = std::collections::HashSet::new();
    errors.retain(|error| seen_error_paths.insert(error.path.clone()));

    projects.sort_by(|left, right| left.path.cmp(&right.path));
    errors.sort_by(|left, right| left.path.cmp(&right.path));

    let index = build_index(&projects);
    let mut effective: Vec<EffectiveProject> = projects
        .iter()
        .map(|project| build_effective(project, &index))
        .collect();
    classify_libraries(&mut effective);
    let reactors = build_reactors(&projects, &effective);

    let stats = cache.map(|c| c.stats()).unwrap_or_default();
    MavenDiscoveryResult {
        projects,
        effective,
        reactors,
        errors,
        elapsed_ms: start.elapsed().as_millis(),
        stats,
    }
}

/// 基于已有的仓库清单（T-01 Scanner 输出的 repo 路径）逐 repo 下钻发现 pom。
///
/// 适用于 workspace 已扫描过 Git 仓库、只想在这些 repo 内发现 Maven 项目的场景。
pub fn discover_poms_in_repos<T: AsRef<str> + Sync>(
    repos: &[T],
    workspace_root: &Path,
    cache: Option<&PomCache>,
    cancel: Option<&AtomicBool>,
) -> Vec<RepoDiscoveryResult> {
    repos
        .par_iter()
        .filter_map(|repo_path| {
            if is_cancelled(cancel) {
                return None;
            }
            let repo_path = repo_path.as_ref();
            let repo = Path::new(repo_path);
            let repo_name = repo
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown")
                .to_string();
            let pom_paths = collect_pom_paths(repo, workspace_root, cancel)?;

            let parsed: Vec<Result<DiscoveredPom, PomDiscoveryError>> = pom_paths
                .par_iter()
                .filter_map(|path| {
                    if is_cancelled(cancel) {
                        return None;
                    }
                    let parsed = if let Some(cache) = cache {
                        cache.get_or_parse(path)
                    } else {
                        parse_pom_file(path)
                    };
                    Some(match parsed {
                        Ok(project) => Ok(DiscoveredPom {
                            project,
                            relative_path: path
                                .parent()
                                .map(|parent| relative_string(parent, workspace_root))
                                .unwrap_or_default(),
                        }),
                        Err(error) => Err(PomDiscoveryError::from_parse_error(path, error)),
                    })
                })
                .collect();
            let mut poms = Vec::new();
            let mut errors = Vec::new();
            for result in parsed {
                match result {
                    Ok(pom) => poms.push(pom),
                    Err(error) => errors.push(error),
                }
            }

            Some(RepoDiscoveryResult {
                repo_path: repo_path.to_string(),
                repo_name,
                poms,
                errors,
            })
        })
        .collect()
}

fn collect_pom_paths(repo: &Path, workspace_root: &Path, cancel: Option<&AtomicBool>) -> Option<Vec<PathBuf>> {
    let workspace_ignore = IgnoreRules::load(workspace_root);
    let repo_ignore = IgnoreRules::load(repo);
    let mut walker = WalkDir::new(repo).follow_links(false).into_iter();
    let mut pom_paths = Vec::new();

    loop {
        if is_cancelled(cancel) {
            return None;
        }
        match walker.next() {
            Some(Ok(entry)) => {
                if entry.file_type().is_dir() {
                    let name = entry.file_name();
                    if entry.path() != repo
                        && (entry.path().join(".git").is_dir() || entry.path().join(".git").is_file())
                    {
                        // T-01 会把嵌套 repo 单独列入 inventory；父 repo 不跨边界重复扫描。
                        walker.skip_current_dir();
                        continue;
                    }
                    if name == OsStr::new(".git") || is_skip_dir(name) {
                        walker.skip_current_dir();
                        continue;
                    }
                    let repo_relative = relative_string(entry.path(), repo);
                    let workspace_relative = relative_string(entry.path(), workspace_root);
                    let name = name.to_string_lossy();
                    if repo_ignore.is_ignored(&name, &repo_relative)
                        || workspace_ignore.is_ignored(&name, &workspace_relative)
                    {
                        walker.skip_current_dir();
                    }
                } else if entry.file_type().is_file() && entry.file_name() == OsStr::new("pom.xml") {
                    pom_paths.push(entry.path().to_path_buf());
                }
            }
            Some(Err(error)) => log::warn!("Maven discovery walk error: {error}"),
            None => break,
        }
    }
    pom_paths.sort();
    Some(pom_paths)
}

fn empty_result(start: Instant) -> MavenDiscoveryResult {
    MavenDiscoveryResult {
        projects: vec![],
        effective: vec![],
        reactors: vec![],
        errors: vec![],
        elapsed_ms: start.elapsed().as_millis(),
        stats: crate::maven::PomCacheStats::default(),
    }
}

fn is_cancelled(cancel: Option<&AtomicBool>) -> bool {
    cancel.is_some_and(|flag| flag.load(Ordering::Relaxed))
}

fn relative_string(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .map(|relative| relative.to_string_lossy().replace('\\', "/"))
        .unwrap_or_default()
}

fn classify_libraries(projects: &mut [EffectiveProject]) {
    let referenced: BTreeSet<PomCoordinates> = projects
        .iter()
        .flat_map(|project| &project.effective_dependencies)
        .filter_map(|dependency| {
            dependency.version.as_ref().map(|version| PomCoordinates {
                group_id: dependency.group_id.clone(),
                artifact_id: dependency.artifact_id.clone(),
                version: version.clone(),
            })
        })
        .collect();

    for project in projects {
        let coordinates = PomCoordinates {
            group_id: project.group_id.clone(),
            artifact_id: project.artifact_id.clone(),
            version: project.version.clone(),
        };
        if project.project_type == MavenProjectType::Standalone
            && project.packaging == "jar"
            && referenced.contains(&coordinates)
        {
            project.project_type = MavenProjectType::Library;
        }
    }
}

fn build_reactors(projects: &[MavenProject], effective: &[EffectiveProject]) -> Vec<MavenReactor> {
    let by_path: HashMap<PathBuf, PomCoordinates> = effective
        .iter()
        .map(|project| {
            (
                canonical_or_original(&project.path),
                PomCoordinates {
                    group_id: project.group_id.clone(),
                    artifact_id: project.artifact_id.clone(),
                    version: project.version.clone(),
                },
            )
        })
        .collect();

    let mut reactors = Vec::new();
    for project in projects.iter().filter(|project| !project.modules.is_empty()) {
        let parent_path = canonical_or_original(&project.path);
        let parent = effective
            .iter()
            .find(|effective| canonical_or_original(&effective.path) == parent_path)
            .map(|effective| PomCoordinates {
                group_id: effective.group_id.clone(),
                artifact_id: effective.artifact_id.clone(),
                version: effective.version.clone(),
            })
            .unwrap_or_else(|| project.coordinates());
        let parent_dir = project.path.parent().unwrap_or_else(|| Path::new(""));
        let modules = project
            .modules
            .iter()
            .map(|module| {
                let declared = parent_dir.join(&module.path);
                let pom_path = if declared.file_name() == Some(OsStr::new("pom.xml")) {
                    declared
                } else {
                    declared.join("pom.xml")
                };
                let pom_path = canonical_or_original(&pom_path);
                MavenReactorModule {
                    declared_path: module.path.clone(),
                    project: by_path.get(&pom_path).cloned(),
                    pom_path,
                }
            })
            .collect();
        reactors.push(MavenReactor {
            parent_path,
            parent,
            modules,
        });
    }
    reactors.sort_by(|left, right| left.parent_path.cmp(&right.parent_path));
    reactors
}

fn canonical_or_original(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

impl PomDiscoveryError {
    fn from_parse_error(path: &Path, error: PomParseError) -> Self {
        Self {
            code: error.code().to_string(),
            path: path.display().to_string(),
            message: error.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::maven::model::MavenProjectType;

    fn write(path: &Path, content: &str) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, content).unwrap();
    }

    fn make_workspace() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "gw_disc_{}",
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        git2::Repository::init(&dir).unwrap();
        dir
    }

    /// R-27 fixture：不初始化 git 的纯目录工作区（模拟源码导出包）。
    fn make_plain_workspace() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "gw_disc_plain_{}",
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn discovers_multi_module_workspace() {
        let ws = make_workspace();
        write(
            &ws.join("root/pom.xml"),
            r#"<project>
  <modelVersion>4.0.0</modelVersion>
  <groupId>com.example</groupId>
  <artifactId>root</artifactId>
  <version>1.0.0</version>
  <packaging>pom</packaging>
  <modules>
    <module>mod-a</module>
  </modules>
</project>"#,
        );
        write(
            &ws.join("root/mod-a/pom.xml"),
            r#"<project>
  <modelVersion>4.0.0</modelVersion>
  <parent>
    <groupId>com.example</groupId>
    <artifactId>root</artifactId>
    <version>1.0.0</version>
  </parent>
  <artifactId>mod-a</artifactId>
</project>"#,
        );
        // target/ 下的 pom 应被忽略。
        write(
            &ws.join("root/target/pom.xml"),
            r#"<project>
  <modelVersion>4.0.0</modelVersion>
  <groupId>com.example</groupId>
  <artifactId>ignored</artifactId>
  <version>1.0.0</version>
</project>"#,
        );

        let result = discover_poms(&ws, 8, None, None);
        assert_eq!(result.projects.len(), 2, "target/ pom must be ignored");

        let root = result.projects.iter().find(|p| p.artifact_id == "root").unwrap();
        let mod_a = result.projects.iter().find(|p| p.artifact_id == "mod-a").unwrap();
        assert_eq!(root.project_type(false), MavenProjectType::Parent);
        assert_eq!(mod_a.project_type(true), MavenProjectType::MultiModule);

        // effective model：mod-a 继承自 workspace 内 root。
        let eff_mod_a = result.effective.iter().find(|e| e.artifact_id == "mod-a").unwrap();
        assert!(eff_mod_a.has_workspace_parent, "mod-a parent resolved in workspace");
        assert_eq!(eff_mod_a.group_id, "com.example", "groupId inherited");
        assert_eq!(eff_mod_a.version, "1.0.0", "version inherited");

        let eff_root = result.effective.iter().find(|e| e.artifact_id == "root").unwrap();
        assert_eq!(eff_root.project_type, MavenProjectType::Parent);
        assert_eq!(result.reactors.len(), 1);
        assert_eq!(result.reactors[0].modules.len(), 1);
        assert_eq!(
            result.reactors[0].modules[0]
                .project
                .as_ref()
                .map(|coordinates| coordinates.artifact_id.as_str()),
            Some("mod-a")
        );

        let _ = std::fs::remove_dir_all(&ws);
    }

    #[test]
    fn broken_pom_recorded_as_error_not_blocking() {
        let ws = make_workspace();
        write(
            &ws.join("good/pom.xml"),
            r#"<project>
  <modelVersion>4.0.0</modelVersion>
  <groupId>g</groupId>
  <artifactId>good</artifactId>
  <version>1</version>
</project>"#,
        );
        write(&ws.join("bad/pom.xml"), "<project><not closed");

        let result = discover_poms(&ws, 4, None, None);
        assert_eq!(result.projects.len(), 1, "good project discovered");
        assert_eq!(result.errors.len(), 1, "broken pom recorded as error");
        assert_eq!(result.errors[0].code, "InvalidPom");
        assert!(Path::new(&result.errors[0].path).ends_with(Path::new("bad/pom.xml")));

        let _ = std::fs::remove_dir_all(&ws);
    }

    #[test]
    fn gitworkspaceignore_is_respected() {
        let ws = make_workspace();
        std::fs::write(ws.join(".gitworkspaceignore"), "legacy/\n").unwrap();
        write(
            &ws.join("main/pom.xml"),
            r#"<project>
  <modelVersion>4.0.0</modelVersion>
  <groupId>g</groupId>
  <artifactId>main</artifactId>
  <version>1</version>
</project>"#,
        );
        write(
            &ws.join("legacy/sub/pom.xml"),
            r#"<project>
  <modelVersion>4.0.0</modelVersion>
  <groupId>g</groupId>
  <artifactId>legacy</artifactId>
  <version>1</version>
</project>"#,
        );

        let result = discover_poms(&ws, 8, None, None);
        assert_eq!(result.projects.len(), 1);
        assert_eq!(result.projects[0].artifact_id, "main");

        let _ = std::fs::remove_dir_all(&ws);
    }

    #[test]
    fn cancellation_returns_empty() {
        let ws = make_workspace();
        write(
            &ws.join("a/pom.xml"),
            r#"<project>
  <modelVersion>4.0.0</modelVersion>
  <groupId>g</groupId>
  <artifactId>a</artifactId>
  <version>1</version>
</project>"#,
        );
        let flag = AtomicBool::new(true);
        let result = discover_poms(&ws, 4, None, Some(&flag));
        assert!(result.projects.is_empty());
        let _ = std::fs::remove_dir_all(&ws);
    }

    /// R-27：无 `.git` 的工作区（源码导出包）通过补扫发现 Maven 项目。
    #[test]
    fn discovers_plain_workspace_without_git() {
        let ws = make_plain_workspace();
        write(
            &ws.join("app/pom.xml"),
            r#"<project>
  <modelVersion>4.0.0</modelVersion>
  <groupId>g</groupId>
  <artifactId>plain-app</artifactId>
  <version>1</version>
</project>"#,
        );

        let result = discover_poms(&ws, 4, None, None);
        assert_eq!(result.projects.len(), 1, "plain workspace pom discovered");
        assert_eq!(result.projects[0].artifact_id, "plain-app");
        let normalized = result.projects[0].path.to_string_lossy().replace('\\', "/");
        assert!(normalized.ends_with("app/pom.xml"));

        let _ = std::fs::remove_dir_all(&ws);
    }

    /// R-27：根目录本身是仓库时，根级 pom 经仓库扫描与补扫各收集一次，去重后不重复。
    #[test]
    fn root_repository_supplement_does_not_duplicate() {
        let ws = make_workspace();
        write(
            &ws.join("pom.xml"),
            r#"<project>
  <modelVersion>4.0.0</modelVersion>
  <groupId>g</groupId>
  <artifactId>root-app</artifactId>
  <version>1</version>
</project>"#,
        );
        write(
            &ws.join("mod-a/pom.xml"),
            r#"<project>
  <modelVersion>4.0.0</modelVersion>
  <groupId>g</groupId>
  <artifactId>mod-a</artifactId>
  <version>1</version>
</project>"#,
        );

        let result = discover_poms(&ws, 4, None, None);
        assert_eq!(result.projects.len(), 2, "no duplicates from supplement");

        let _ = std::fs::remove_dir_all(&ws);
    }

    /// R-27：仓库存在但没有任何 pom（如嵌套前端仓库）时，非 git 目录经补扫发现。
    #[test]
    fn discovers_plain_dirs_when_repositories_have_no_poms() {
        let ws = make_plain_workspace();
        let empty_repo = ws.join("frontend/pkg");
        std::fs::create_dir_all(&empty_repo).unwrap();
        git2::Repository::init(&empty_repo).unwrap();
        write(
            &ws.join("exported-app/pom.xml"),
            r#"<project>
  <modelVersion>4.0.0</modelVersion>
  <groupId>g</groupId>
  <artifactId>exported-app</artifactId>
  <version>1</version>
</project>"#,
        );

        let result = discover_poms(&ws, 6, None, None);
        assert_eq!(result.projects.len(), 1);
        assert_eq!(result.projects[0].artifact_id, "exported-app");

        let _ = std::fs::remove_dir_all(&ws);
    }

    /// R-27：取消标志在补扫路径同样生效。
    #[test]
    fn cancellation_skips_workspace_supplement_scan() {
        let ws = make_plain_workspace();
        write(
            &ws.join("a/pom.xml"),
            r#"<project>
  <modelVersion>4.0.0</modelVersion>
  <groupId>g</groupId>
  <artifactId>a</artifactId>
  <version>1</version>
</project>"#,
        );
        let flag = AtomicBool::new(true);
        let result = discover_poms(&ws, 4, None, Some(&flag));
        assert!(result.projects.is_empty());
        let _ = std::fs::remove_dir_all(&ws);
    }

    #[test]
    fn cache_accelates_second_discovery() {
        let ws = make_workspace();
        write(
            &ws.join("a/pom.xml"),
            r#"<project>
  <modelVersion>4.0.0</modelVersion>
  <groupId>g</groupId>
  <artifactId>a</artifactId>
  <version>1</version>
</project>"#,
        );
        let cache = PomCache::new();
        let r1 = discover_poms(&ws, 4, Some(&cache), None);
        let r2 = discover_poms(&ws, 4, Some(&cache), None);
        assert_eq!(r1.projects.len(), 1);
        assert_eq!(r2.projects.len(), 1);
        assert!(r2.stats.hits >= 1, "second discovery must hit cache");

        let _ = std::fs::remove_dir_all(&ws);
    }

    /// R-27：混合工作区（git 仓库 + 非 git 目录）一并发现，Library 分类不受影响。
    #[test]
    fn discovers_repo_and_plain_dirs_and_classifies_workspace_library() {
        let ws = std::env::temp_dir().join(format!(
            "gw_disc_repos_{}",
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        let app_repo = ws.join("repo-app");
        let lib_repo = ws.join("repo-lib");
        std::fs::create_dir_all(&app_repo).unwrap();
        std::fs::create_dir_all(&lib_repo).unwrap();
        git2::Repository::init(&app_repo).unwrap();
        git2::Repository::init(&lib_repo).unwrap();

        write(
            &app_repo.join("pom.xml"),
            r#"<project>
  <modelVersion>4.0.0</modelVersion>
  <groupId>com.example</groupId>
  <artifactId>app</artifactId>
  <version>1.0.0</version>
  <dependencies>
    <dependency>
      <groupId>com.example</groupId>
      <artifactId>lib</artifactId>
      <version>1.0.0</version>
    </dependency>
  </dependencies>
</project>"#,
        );
        write(
            &lib_repo.join("pom.xml"),
            r#"<project>
  <modelVersion>4.0.0</modelVersion>
  <groupId>com.example</groupId>
  <artifactId>lib</artifactId>
  <version>1.0.0</version>
</project>"#,
        );
        write(
            &ws.join("not-a-repository/pom.xml"),
            r#"<project>
  <modelVersion>4.0.0</modelVersion>
  <groupId>com.example</groupId>
  <artifactId>outside</artifactId>
  <version>1.0.0</version>
</project>"#,
        );

        let result = discover_poms(&ws, 4, None, None);
        assert_eq!(
            result.projects.len(),
            3,
            "repo poms and plain-dir pom are all discovered (R-27)"
        );
        assert!(result.projects.iter().any(|project| project.artifact_id == "outside"));
        assert_eq!(
            result
                .effective
                .iter()
                .find(|project| project.artifact_id == "app")
                .unwrap()
                .project_type,
            MavenProjectType::Standalone
        );
        assert_eq!(
            result
                .effective
                .iter()
                .find(|project| project.artifact_id == "lib")
                .unwrap()
                .project_type,
            MavenProjectType::Library
        );

        let _ = std::fs::remove_dir_all(&ws);
    }

    #[test]
    fn nested_repository_is_not_parsed_twice() {
        let ws = make_workspace();
        let nested = ws.join("nested");
        std::fs::create_dir_all(&nested).unwrap();
        git2::Repository::init(&nested).unwrap();
        write(
            &nested.join("pom.xml"),
            r#"<project>
  <modelVersion>4.0.0</modelVersion>
  <groupId>com.example</groupId>
  <artifactId>nested</artifactId>
  <version>1.0.0</version>
</project>"#,
        );

        let result = discover_poms(&ws, 4, None, None);
        assert_eq!(result.projects.len(), 1);
        assert_eq!(result.projects[0].artifact_id, "nested");

        let _ = std::fs::remove_dir_all(&ws);
    }

    /// Provisional R-01 probe. R-08 owns the durable benchmark baseline.
    #[test]
    #[ignore = "run explicitly in release mode for the R-01 performance budget"]
    fn discovery_and_cache_hit_performance_probe() {
        let ws = make_workspace();
        for index in 0..100 {
            write(
                &ws.join(format!("module-{index}/pom.xml")),
                &format!(
                    r#"<project>
  <modelVersion>4.0.0</modelVersion>
  <groupId>com.example</groupId>
  <artifactId>module-{index}</artifactId>
  <version>1.0.0</version>
</project>"#
                ),
            );
        }

        let cache = PomCache::new();
        let first = discover_poms(&ws, 4, Some(&cache), None);
        let second = discover_poms(&ws, 4, Some(&cache), None);
        let single_hit_started = Instant::now();
        cache
            .get_or_parse(&ws.join("module-0/pom.xml"))
            .expect("cached POM should remain readable");
        let single_hit_us = single_hit_started.elapsed().as_micros();
        eprintln!(
            "R-01 probe: discovery={}ms, workspace-cache-load={}ms, single-cache-hit={}us, hits={}",
            first.elapsed_ms, second.elapsed_ms, single_hit_us, second.stats.hits
        );
        assert!(first.elapsed_ms < 500, "discovery budget exceeded");
        assert!(single_hit_us < 50_000, "single POM cache-hit budget exceeded");
        // R-27 补扫会让每个 pom 在单次发现内二次命中缓存，这里只锁定「全部来自缓存」。
        assert!(
            second.stats.hits >= 100,
            "every POM served from cache on second discovery"
        );

        let _ = std::fs::remove_dir_all(&ws);
    }
}
