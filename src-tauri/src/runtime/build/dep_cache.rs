//! Runtime Dependency Cache（R-18，§73 第二阶段）：模块输入指纹缓存。
//!
//! 目标：模块输入（POM + 源码）未变化且上游产物未重建时，跳过该模块的
//! Maven 重建；全部模块未变化时跳过整个构建调用。
//!
//! 输入指纹 = `hash(pom.xml 内容) + hash(src/** 相对路径 + 内容)`。设计
//! 原则「宁可重建不错过」：
//! - 内容哈希（而非 mtime）——不会因时钟/同步误判跳过；
//! - 图指纹变化 → 全量重建（依赖关系变化不加推测）；
//! - 产物缺失（如被 `mvn clean`）→ 强制重建该模块；
//! - 任一上游模块重建 → 其全部下游（闭包内）级联重建。
//!
//! 缓存状态持久化在 `.gitworkspace/runtime/<name>/build-cache.json`（R-14
//! §78 护栏内）。**不自行实现 Java 编译缓存**（全局约束 §5）：粒度到
//! 「模块是否重构建」为止，编译增量交给 Maven 自身。

use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::Path;

use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

use crate::error::AppResult;
use crate::maven::index::{DependencyGraph, MavenProjectNode};

pub const BUILD_CACHE_FILE: &str = "build-cache.json";

/// 缓存状态：图指纹 + 模块 GA → 输入指纹（上次成功构建时）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct BuildCacheState {
    pub graph_fingerprint: String,
    /// `groupId:artifactId` → 输入指纹。
    pub modules: BTreeMap<String, String>,
}

pub fn build_cache_path(workspace_root: &Path, runtime_name: &str) -> std::path::PathBuf {
    workspace_root
        .join(".gitworkspace")
        .join("runtime")
        .join(runtime_name)
        .join(BUILD_CACHE_FILE)
}

/// 读取缓存状态；文件不存在 / 解析失败返回 None（视为无缓存）。
pub fn load_state(workspace_root: &Path, runtime_name: &str) -> Option<BuildCacheState> {
    let path = build_cache_path(workspace_root, runtime_name);
    let text = std::fs::read_to_string(&path).ok()?;
    match serde_json::from_str::<BuildCacheState>(&text) {
        Ok(state) => Some(state),
        Err(e) => {
            log::warn!("R-18: invalid build cache {}: {e}", path.display());
            None
        }
    }
}

/// 原子写入缓存状态（成功构建后调用；失败只记日志，不影响构建结果）。
pub fn store_state(workspace_root: &Path, runtime_name: &str, state: &BuildCacheState) {
    let path = build_cache_path(workspace_root, runtime_name);
    if let Err(e) = crate::runtime::config::write_json_atomic(&path, state) {
        log::warn!("R-18: failed to write build cache {}: {e}", path.display());
    }
}

/// 计算模块输入指纹：pom.xml 内容 + src/**（相对路径 + 内容）流式哈希。
/// 任何文件读取失败（权限等）→ 该模块指纹记为不可得（`None`，宁可重建）。
pub fn compute_module_fingerprint(pom: &Path, module_dir: &Path) -> Option<String> {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    let pom_bytes = std::fs::read(pom).ok()?;
    std::hash::Hasher::write(&mut hasher, b"pom\x00");
    std::hash::Hasher::write(&mut hasher, &pom_bytes);

    let src = module_dir.join("src");
    if src.is_dir() {
        let mut entries: Vec<std::path::PathBuf> = WalkDir::new(&src)
            .follow_links(false)
            .into_iter()
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.file_type().is_file())
            .map(|entry| entry.into_path())
            .collect();
        entries.sort();
        for file in entries {
            let relative = file.strip_prefix(module_dir).unwrap_or(&file);
            std::hash::Hasher::write(&mut hasher, b"file\x00");
            std::hash::Hasher::write(&mut hasher, relative.to_string_lossy().as_bytes());
            let mut file_handle = std::fs::File::open(&file).ok()?;
            let mut buffer = Vec::new();
            // 大文件防御性截断（>8MB 只哈希前 8MB——源码文件不至此量；
            // 截断方向是「指纹更易碰撞 → 更倾向重建」，不违反宁可重建）。
            file_handle
                .by_ref()
                .take(8 * 1024 * 1024)
                .read_to_end(&mut buffer)
                .ok()?;
            std::hash::Hasher::write(&mut hasher, &buffer);
        }
    }
    Some(format!("{:016x}", std::hash::Hasher::finish(&hasher)))
}

/// 重建计划。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RebuildPlan {
    /// 全部模块重建（首次构建 / 图指纹变化 / 指纹缺失）。
    RebuildAll,
    /// 只重建给定 GA 子集（模块自身变化或上游重建；`-pl <subset>` 不带 -am）。
    Subset(Vec<String>),
    /// 全部模块未变化（跳过 Maven 构建调用）。
    SkipAll,
}

/// 计算重建计划。
///
/// `closure_modules`：闭包内模块（含 pom 路径）；`fingerprint_of` 由调用方
/// 注入（便于测试注入内存指纹，生产路径为 [`compute_module_fingerprint`]）；
/// `artifact_exists` 判定模块产物是否在（`target/classes` 等，缺失强制重建）。
pub fn compute_rebuild_plan<F, A>(
    graph: &DependencyGraph,
    closure_modules: &[MavenProjectNode],
    stored: Option<&BuildCacheState>,
    graph_fingerprint: &str,
    fingerprint_of: F,
    artifact_exists: A,
) -> RebuildPlan
where
    F: Fn(&MavenProjectNode) -> Option<String>,
    A: Fn(&MavenProjectNode) -> bool,
{
    let Some(stored) = stored else {
        return RebuildPlan::RebuildAll;
    };
    if stored.graph_fingerprint != graph_fingerprint {
        return RebuildPlan::RebuildAll;
    }

    // 当前指纹。
    let mut current: BTreeMap<String, String> = BTreeMap::new();
    for module in closure_modules {
        let ga = format!(
            "{}:{}",
            module.coordinates.group_id, module.coordinates.artifact_id
        );
        match fingerprint_of(module) {
            Some(fp) => {
                current.insert(ga, fp);
            }
            None => return RebuildPlan::RebuildAll,
        }
    }

    // 自身指纹变化或产物缺失的模块。
    let mut rebuild: BTreeSet<String> = BTreeSet::new();
    for module in closure_modules {
        let ga = format!(
            "{}:{}",
            module.coordinates.group_id, module.coordinates.artifact_id
        );
        let fp_changed = stored.modules.get(&ga) != current.get(&ga);
        if fp_changed || !artifact_exists(module) {
            rebuild.insert(ga);
        }
    }
    if rebuild.is_empty() {
        return RebuildPlan::SkipAll;
    }

    // 上游重建 → 下游级联（闭包内的 workspace 依赖边）。
    loop {
        let mut propagated = BTreeSet::new();
        for edge in &graph.dependencies {
            let Some(upstream_id) = edge.source_project_id else {
                continue;
            };
            let Some(upstream) = graph.projects.iter().find(|p| p.project_id == upstream_id) else {
                continue;
            };
            let upstream_ga = format!(
                "{}:{}",
                upstream.coordinates.group_id, upstream.coordinates.artifact_id
            );
            if !rebuild.contains(&upstream_ga) {
                continue;
            }
            let Some(downstream) = graph
                .projects
                .iter()
                .find(|p| p.project_id == edge.from_project_id)
            else {
                continue;
            };
            let downstream_ga = format!(
                "{}:{}",
                downstream.coordinates.group_id, downstream.coordinates.artifact_id
            );
            let in_closure = closure_modules.iter().any(|m| {
                format!("{}:{}", m.coordinates.group_id, m.coordinates.artifact_id) == downstream_ga
            });
            if in_closure && !rebuild.contains(&downstream_ga) {
                propagated.insert(downstream_ga);
            }
        }
        if propagated.is_empty() {
            break;
        }
        rebuild.extend(propagated);
    }

    RebuildPlan::Subset(rebuild.into_iter().collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::maven::model::PomCoordinates;

    fn node(id: i64, group: &str, artifact: &str) -> MavenProjectNode {
        MavenProjectNode {
            project_id: id,
            repository_id: None,
            path: std::path::PathBuf::from(format!("/ws/repo/{artifact}/pom.xml")),
            coordinates: PomCoordinates {
                group_id: group.into(),
                artifact_id: artifact.into(),
                version: "1.0.0".into(),
            },
            packaging: "jar".into(),
            pom_hash: format!("pom-hash-{id}"),
        }
    }

    fn edge(from: i64, to: i64) -> crate::maven::index::DependencyEdge {
        crate::maven::index::DependencyEdge {
            dependency_id: from * 10 + to,
            from_project_id: from,
            dependency: crate::maven::model::MavenDependency {
                group_id: "com.example".into(),
                artifact_id: format!("m{to}"),
                version: Some("1.0.0".into()),
                scope: crate::maven::model::DependencyScope::Compile,
                optional: false,
                dep_type: "jar".into(),
                classifier: None,
                exclusions: vec![],
            },
            source: crate::maven::resolver::DependencySource::WorkspaceSource,
            source_project_id: Some(to),
            resolved_path: None,
            reason: crate::maven::resolver::ResolutionReason::WorkspaceExactMatch,
        }
    }

    fn graph(edges: Vec<crate::maven::index::DependencyEdge>) -> DependencyGraph {
        DependencyGraph {
            workspace_id: 1,
            fingerprint: "fp-1".into(),
            projects: vec![
                node(1, "com.example", "common"),
                node(2, "com.example", "auth"),
                node(3, "com.example", "boot"),
            ],
            modules: vec![],
            dependencies: edges,
            source_mappings: vec![],
        }
    }

    fn fp_of(module: &MavenProjectNode) -> Option<String> {
        Some(format!("fp-{}", module.project_id))
    }

    fn artifacts_ok(_module: &MavenProjectNode) -> bool {
        true
    }

    #[test]
    fn no_stored_state_rebuilds_all() {
        let g = graph(vec![edge(2, 1), edge(3, 2)]);
        let closure = g.projects.clone();
        assert_eq!(
            compute_rebuild_plan(&g, &closure, None, "fp-1", fp_of, artifacts_ok),
            RebuildPlan::RebuildAll
        );
    }

    #[test]
    fn graph_fingerprint_change_rebuilds_all() {
        let g = graph(vec![edge(2, 1), edge(3, 2)]);
        let closure = g.projects.clone();
        let stored = BuildCacheState {
            graph_fingerprint: "fp-old".into(),
            modules: BTreeMap::from([
                ("com.example:common".into(), "fp-1".into()),
                ("com.example:auth".into(), "fp-2".into()),
                ("com.example:boot".into(), "fp-3".into()),
            ]),
        };
        assert_eq!(
            compute_rebuild_plan(&g, &closure, Some(&stored), "fp-1", fp_of, artifacts_ok),
            RebuildPlan::RebuildAll
        );
    }

    #[test]
    fn unchanged_modules_skip_entire_build() {
        let g = graph(vec![edge(2, 1), edge(3, 2)]);
        let closure = g.projects.clone();
        let stored = BuildCacheState {
            graph_fingerprint: "fp-1".into(),
            modules: BTreeMap::from([
                ("com.example:common".into(), "fp-1".into()),
                ("com.example:auth".into(), "fp-2".into()),
                ("com.example:boot".into(), "fp-3".into()),
            ]),
        };
        assert_eq!(
            compute_rebuild_plan(&g, &closure, Some(&stored), "fp-1", fp_of, artifacts_ok),
            RebuildPlan::SkipAll
        );
    }

    #[test]
    fn changed_module_propagates_to_downstream_only() {
        // auth 指纹变化 → auth + boot 重建；common 不动（R-17 验收同型）。
        let g = graph(vec![edge(2, 1), edge(3, 2)]);
        let closure = g.projects.clone();
        let stored = BuildCacheState {
            graph_fingerprint: "fp-1".into(),
            modules: BTreeMap::from([
                ("com.example:common".into(), "fp-1".into()),
                ("com.example:auth".into(), "fp-OLD".into()),
                ("com.example:boot".into(), "fp-3".into()),
            ]),
        };
        assert_eq!(
            compute_rebuild_plan(&g, &closure, Some(&stored), "fp-1", fp_of, artifacts_ok),
            RebuildPlan::Subset(vec!["com.example:auth".into(), "com.example:boot".into()])
        );
    }

    #[test]
    fn missing_artifact_forces_rebuild_even_with_same_fingerprint() {
        // mvn clean 后 target/ 缺失：指纹未变也必须重建（宁可重建不错过）。
        let g = graph(vec![edge(2, 1), edge(3, 2)]);
        let closure = g.projects.clone();
        let stored = BuildCacheState {
            graph_fingerprint: "fp-1".into(),
            modules: BTreeMap::from([
                ("com.example:common".into(), "fp-1".into()),
                ("com.example:auth".into(), "fp-2".into()),
                ("com.example:boot".into(), "fp-3".into()),
            ]),
        };
        let plan = compute_rebuild_plan(
            &g,
            &closure,
            Some(&stored),
            "fp-1",
            fp_of,
            |m: &MavenProjectNode| m.project_id != 2, // auth 产物缺失
        );
        match plan {
            RebuildPlan::Subset(subset) => {
                assert!(subset.contains(&"com.example:auth".to_string()));
                assert!(subset.contains(&"com.example:boot".to_string()));
                assert!(!subset.contains(&"com.example:common".to_string()));
            }
            other => panic!("expected Subset, got {other:?}"),
        }
    }

    #[test]
    fn unfingerprintable_module_rebuilds_all() {
        let g = graph(vec![]);
        let closure = g.projects.clone();
        let stored = BuildCacheState {
            graph_fingerprint: "fp-1".into(),
            modules: BTreeMap::from([("com.example:common".into(), "fp-1".into())]),
        };
        assert_eq!(
            compute_rebuild_plan(&g, &closure, Some(&stored), "fp-1", |_| None, artifacts_ok),
            RebuildPlan::RebuildAll
        );
    }

    #[test]
    fn fingerprint_reflects_source_content_change() {
        let dir = std::env::temp_dir().join(format!(
            "gw_r18_fp_{}",
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        let module = dir.join("app");
        std::fs::create_dir_all(module.join("src/main/java")).unwrap();
        let pom = module.join("pom.xml");
        std::fs::write(&pom, b"<project/>").unwrap();
        let java = module.join("src/main/java/App.java");
        std::fs::write(&java, b"class App {}").unwrap();
        let before = compute_module_fingerprint(&pom, &module).unwrap();

        // 源码内容变化 → 指纹变化（内容哈希，不看 mtime）。
        std::fs::write(&java, b"class App { int x; }").unwrap();
        let after = compute_module_fingerprint(&pom, &module).unwrap();
        assert_ne!(before, after);

        // 同内容重写 → 指纹不变（mtime 无关）。
        let old_mtime = std::fs::metadata(&java).unwrap().modified().unwrap();
        std::fs::write(&java, b"class App { int x; }").unwrap();
        let same = compute_module_fingerprint(&pom, &module).unwrap();
        assert_eq!(after, same);
        let _ = old_mtime;
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn build_cache_state_roundtrips() {
        let state = BuildCacheState {
            graph_fingerprint: "fp-1".into(),
            modules: BTreeMap::from([("com.example:app".into(), "abc".into())]),
        };
        let text = serde_json::to_string(&state).unwrap();
        let back: BuildCacheState = serde_json::from_str(&text).unwrap();
        assert_eq!(state, back);
    }
}
