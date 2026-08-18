//! Effective Model 构建（R-01）。
//!
//! 在原始 POM 解析结果上构建 effective model：
//! - parent 继承链合并（groupId / version / properties / dependencyManagement）；
//! - `properties` 占位符替换（`${...}`）；
//! - `dependencyManagement` 版本/scope 落地为 effective dependency。
//!
//! 只覆盖 Runtime 所需字段，**不追求完整 Maven Model Builder**；复杂 profile 激活、
//! 远程 parent 解析交给 `mvn` 自身（全局约束 §1）。远程 parent 缺失时降级标记，
//! 不阻塞（全局约束 §10）。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::maven::model::{MavenDependency, MavenProject, MavenProjectType};
use crate::maven::parser::parse_pom_file;

/// 一组 workspace 内的 POM，按 GAV 坐标索引，用于 parent 继承链解析。
///
/// key = `groupId:artifactId:version`。
pub type PomIndex = BTreeMap<String, MavenProject>;

/// 一个 POM 的 effective model：合并 parent 链 + 占位符替换 + 版本落地后的依赖列表。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EffectiveProject {
    pub path: PathBuf,
    pub group_id: String,
    pub artifact_id: String,
    pub version: String,
    pub packaging: String,
    pub project_type: MavenProjectType,
    /// 已合并 parent 链 + 自身 properties 后的属性表。
    pub effective_properties: BTreeMap<String, String>,
    /// 版本/scope 已落地的依赖。
    pub effective_dependencies: Vec<MavenDependency>,
    /// 该 POM 是否能在本 workspace 内解析到 parent（用于 project_type 推断）。
    pub has_workspace_parent: bool,
    /// 远程 parent 缺失标记（降级，不阻塞）。
    pub remote_parent_missing: bool,
}

/// 为单个 POM 构建 effective model。
///
/// - `index`：同 workspace 内所有已解析 POM 的 GAV 索引（不含自身）。
/// - 远程 parent（不在 workspace 内）缺失时降级标记 `remote_parent_missing=true`，
///   继承链到此为止（不发起网络请求，全局约束 §10）。
pub fn build_effective(pom: &MavenProject, index: &PomIndex) -> EffectiveProject {
    // 1. 收集继承链：从自身向上追溯 parent（仅在 workspace 内可解析的）。
    let mut chain: Vec<&MavenProject> = Vec::new();
    chain.push(pom);
    let mut current = pom.parent.as_ref();
    let mut has_workspace_parent = false;
    let mut remote_parent_missing = false;
    let mut visited: std::collections::HashSet<String> = std::collections::HashSet::new();
    visited.insert(pom.coordinates().gav());

    while let Some(parent) = current {
        let gav = format!(
            "{}:{}:{}",
            parent.group_id, parent.artifact_id, parent.version
        );
        if visited.contains(&gav) {
            // 防止循环（理论上不应出现）。
            break;
        }
        visited.insert(gav.clone());
        if let Some(parent_pom) = index.get(&gav) {
            has_workspace_parent = true;
            chain.push(parent_pom);
            current = parent_pom.parent.as_ref();
        } else {
            // parent 不在 workspace 内：远程 parent，降级标记，停止继承。
            remote_parent_missing = true;
            break;
        }
    }

    // 2. 合并 properties（父在前，子覆盖父）。
    let mut effective_properties: BTreeMap<String, String> = BTreeMap::new();
    // 反向遍历：从最远的祖先到自身，让自身覆盖。
    for pom in chain.iter().rev() {
        for (k, v) in &pom.properties {
            effective_properties.insert(k.clone(), v.clone());
        }
    }

    // 3. groupId / version 继承。
    let raw_group_id = if !pom.group_id.is_empty() {
        pom.group_id.clone()
    } else {
        chain
            .iter()
            .skip(1)
            .find_map(|p| {
                if !p.group_id.is_empty() {
                    Some(p.group_id.clone())
                } else {
                    None
                }
            })
            .or_else(|| pom.parent.as_ref().map(|p| p.group_id.clone()))
            .unwrap_or_default()
    };
    let raw_version = if !pom.version.is_empty() {
        pom.version.clone()
    } else {
        chain
            .iter()
            .skip(1)
            .find_map(|p| {
                if !p.version.is_empty() {
                    Some(p.version.clone())
                } else {
                    None
                }
            })
            .or_else(|| pom.parent.as_ref().map(|p| p.version.clone()))
            .unwrap_or_default()
    };

    // 坐标和 properties 都允许相互引用；有限轮次收敛，循环引用保持原样。
    let mut group_id = resolve_placeholders(
        &raw_group_id,
        &effective_properties,
        &raw_group_id,
        &raw_version,
    );
    let mut version =
        resolve_placeholders(&raw_version, &effective_properties, &group_id, &raw_version);
    for _ in 0..8 {
        let previous = effective_properties.clone();
        for (key, value) in &previous {
            effective_properties.insert(
                key.clone(),
                resolve_placeholders(value, &previous, &group_id, &version),
            );
        }
        let next_group =
            resolve_placeholders(&group_id, &effective_properties, &group_id, &version);
        let next_version =
            resolve_placeholders(&version, &effective_properties, &next_group, &version);
        if previous == effective_properties && next_group == group_id && next_version == version {
            break;
        }
        group_id = next_group;
        version = next_version;
    }

    // 4. 占位符替换：properties + 内建变量。
    let resolve = |s: &str| resolve_placeholders(s, &effective_properties, &group_id, &version);

    // 5. 合并 dependencyManagement。祖先先写、子级覆盖，符合 Maven 继承语义。
    let mut managed: BTreeMap<String, MavenDependency> = BTreeMap::new();
    for ancestor in chain.iter().rev() {
        for dependency in &ancestor.dependency_management {
            let mut dependency = dependency.clone();
            resolve_dependency_coordinates(&mut dependency, &resolve);
            managed.insert(dependency_key(&dependency), dependency);
        }
    }

    // 6. 父 dependencies 会被子 POM 继承；同坐标依赖由子级覆盖。
    let mut inherited: Vec<MavenDependency> = Vec::new();
    for ancestor in chain.iter().rev() {
        for dependency in &ancestor.dependencies {
            let mut dependency = dependency.clone();
            resolve_dependency_coordinates(&mut dependency, &resolve);
            let key = dependency_key(&dependency);
            if let Some(existing) = inherited
                .iter_mut()
                .find(|candidate| dependency_key(candidate) == key)
            {
                *existing = dependency;
            } else {
                inherited.push(dependency);
            }
        }
    }

    let mut effective_dependencies = Vec::with_capacity(inherited.len());
    for mut dependency in inherited {
        dependency.version = dependency
            .version
            .as_ref()
            .map(|value| resolve(value))
            .or_else(|| {
                managed
                    .get(&dependency_key(&dependency))
                    .and_then(|managed| managed.version.as_ref())
                    .map(|value| resolve(value))
            });
        for exclusion in &mut dependency.exclusions {
            exclusion.group_id = resolve(&exclusion.group_id);
            exclusion.artifact_id = resolve(&exclusion.artifact_id);
            exclusion.version = resolve(&exclusion.version);
        }
        effective_dependencies.push(dependency);
    }

    let project_type = pom.project_type(has_workspace_parent);

    EffectiveProject {
        path: pom.path.clone(),
        group_id: group_id.clone(),
        artifact_id: resolve(&pom.artifact_id),
        version,
        packaging: pom.packaging.clone(),
        project_type,
        effective_properties,
        effective_dependencies,
        has_workspace_parent,
        remote_parent_missing,
    }
}

/// 替换 `${...}` 占位符。支持内建变量 `project.version` / `project.groupId`，
/// 以及用户自定义 properties。未识别占位符原样保留。
fn resolve_placeholders(
    s: &str,
    props: &BTreeMap<String, String>,
    group_id: &str,
    version: &str,
) -> String {
    let mut out = s.to_string();
    let mut prev: Option<String> = None;
    // 最多迭代若干轮以解析嵌套引用（${x} 引用 ${y}）。
    for _ in 0..8 {
        if prev.as_deref() == Some(out.as_str()) {
            break;
        }
        prev = Some(out.clone());
        out = replace_all_placeholders(&out, |key| match key {
            "project.version" | "pom.version" => version.to_string(),
            "project.groupId" | "pom.groupId" => group_id.to_string(),
            _ => props.get(key).cloned().unwrap_or_default(),
        });
    }
    out
}

/// 扫描 `${...}` 占位符并逐个替换。未识别的占位符（lookup 返回空）原样保留。
fn replace_all_placeholders<F: Fn(&str) -> String>(s: &str, lookup: F) -> String {
    let mut out = String::with_capacity(s.len());
    let mut cursor = 0;
    while let Some(relative_start) = s[cursor..].find("${") {
        let start = cursor + relative_start;
        out.push_str(&s[cursor..start]);
        let key_start = start + 2;
        let Some(relative_end) = s[key_start..].find('}') else {
            out.push_str(&s[start..]);
            return out;
        };
        let end = key_start + relative_end;
        let value = lookup(&s[key_start..end]);
        if value.is_empty() {
            out.push_str(&s[start..=end]);
        } else {
            out.push_str(&value);
        }
        cursor = end + 1;
    }
    out.push_str(&s[cursor..]);
    out
}

fn resolve_dependency_coordinates<F: Fn(&str) -> String>(
    dependency: &mut MavenDependency,
    resolve: &F,
) {
    dependency.group_id = resolve(&dependency.group_id);
    dependency.artifact_id = resolve(&dependency.artifact_id);
    dependency.dep_type = resolve(&dependency.dep_type);
    dependency.classifier = dependency.classifier.as_ref().map(|value| resolve(value));
}

fn dependency_key(dependency: &MavenDependency) -> String {
    format!(
        "{}:{}:{}:{}",
        dependency.group_id,
        dependency.artifact_id,
        dependency.dep_type,
        dependency.classifier.as_deref().unwrap_or_default()
    )
}

/// 便捷：解析一个目录下的所有 pom.xml 并构建 effective model（用于测试与小规模场景）。
/// 大规模发现请用 [`crate::maven::discovery::discover_poms`]。
pub fn build_effective_for_dir(dir: &Path) -> Vec<EffectiveProject> {
    let mut poms: Vec<MavenProject> = Vec::new();
    walk_poms(dir, &mut poms);
    let index = build_index(&poms);
    poms.iter().map(|p| build_effective(p, &index)).collect()
}

fn walk_poms(dir: &Path, out: &mut Vec<MavenProject>) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if matches!(name, "target" | ".git" | "node_modules") {
                    continue;
                }
                walk_poms(&p, out);
            } else if p.file_name().and_then(|n| n.to_str()) == Some("pom.xml") {
                if let Ok(pom) = parse_pom_file(&p) {
                    out.push(pom);
                }
            }
        }
    }
}

pub(crate) fn build_index(poms: &[MavenProject]) -> PomIndex {
    let mut idx = PomIndex::new();
    for p in poms {
        let group_id = if p.group_id.is_empty() {
            p.parent
                .as_ref()
                .map(|parent| parent.group_id.clone())
                .unwrap_or_default()
        } else {
            p.group_id.clone()
        };
        let version = if p.version.is_empty() {
            p.parent
                .as_ref()
                .map(|parent| parent.version.clone())
                .unwrap_or_default()
        } else {
            p.version.clone()
        };
        idx.insert(
            format!("{}:{}:{}", group_id, p.artifact_id, version),
            p.clone(),
        );
    }
    idx
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::maven::model::ManagedDependency;

    fn pom(
        group: &str,
        artifact: &str,
        version: &str,
        parent: Option<crate::maven::model::MavenParent>,
    ) -> MavenProject {
        MavenProject {
            path: PathBuf::from(format!("/tmp/{}.xml", artifact)),
            group_id: group.to_string(),
            artifact_id: artifact.to_string(),
            version: version.to_string(),
            packaging: "jar".to_string(),
            parent,
            modules: vec![],
            dependencies: vec![],
            dependency_management: vec![],
            profiles: vec![],
            properties: BTreeMap::new(),
            plugins: vec![],
            file_hash: "h".to_string(),
        }
    }

    #[test]
    fn parent_inherits_group_and_version() {
        // child 缺 groupId/version，从 workspace 内 parent 继承。
        let parent = pom("com.example", "parent", "2.0.0", None);
        let mut child = pom(
            "",
            "child",
            "",
            Some(crate::maven::model::MavenParent {
                group_id: "com.example".into(),
                artifact_id: "parent".into(),
                version: "2.0.0".into(),
                relative_path: None,
            }),
        );
        // child 的 dependencyManagement 提供版本
        child.dependency_management.push(ManagedDependency {
            group_id: "com.example".into(),
            artifact_id: "lib".into(),
            version: Some("1.5.0".into()),
            scope: Default::default(),
            optional: false,
            dep_type: "jar".into(),
            classifier: None,
            exclusions: vec![],
        });
        child.dependencies.push(MavenDependency {
            group_id: "com.example".into(),
            artifact_id: "lib".into(),
            version: None,
            scope: Default::default(),
            optional: false,
            dep_type: "jar".into(),
            classifier: None,
            exclusions: vec![],
        });

        let mut idx = PomIndex::new();
        idx.insert(parent.coordinates().gav(), parent);
        let eff = build_effective(&child, &idx);
        assert_eq!(eff.group_id, "com.example");
        assert_eq!(eff.version, "2.0.0");
        assert!(eff.has_workspace_parent);
        assert!(!eff.remote_parent_missing);
        // 版本从 dependencyManagement 落地
        assert_eq!(
            eff.effective_dependencies[0].version.as_deref(),
            Some("1.5.0")
        );
    }

    #[test]
    fn remote_parent_missing_is_degraded() {
        let child = pom(
            "",
            "child",
            "",
            Some(crate::maven::model::MavenParent {
                group_id: "org.springframework.boot".into(),
                artifact_id: "spring-boot-starter-parent".into(),
                version: "3.2.0".into(),
                relative_path: None,
            }),
        );
        let idx = PomIndex::new();
        let eff = build_effective(&child, &idx);
        assert!(!eff.has_workspace_parent);
        assert!(eff.remote_parent_missing, "remote parent must be flagged");
        assert_eq!(eff.group_id, "org.springframework.boot");
        assert_eq!(eff.version, "3.2.0");
    }

    #[test]
    fn child_management_overrides_parent_and_parent_dependencies_are_inherited() {
        let mut parent = pom("com.example", "parent", "1.0.0", None);
        parent.dependency_management.push(ManagedDependency {
            group_id: "com.example".into(),
            artifact_id: "managed".into(),
            version: Some("1.0.0".into()),
            scope: Default::default(),
            optional: false,
            dep_type: "jar".into(),
            classifier: None,
            exclusions: vec![],
        });
        parent.dependencies.push(MavenDependency {
            group_id: "com.example".into(),
            artifact_id: "inherited".into(),
            version: Some("3.0.0".into()),
            scope: Default::default(),
            optional: false,
            dep_type: "jar".into(),
            classifier: None,
            exclusions: vec![],
        });

        let mut child = pom(
            "",
            "child",
            "",
            Some(crate::maven::model::MavenParent {
                group_id: "com.example".into(),
                artifact_id: "parent".into(),
                version: "1.0.0".into(),
                relative_path: None,
            }),
        );
        child.dependency_management.push(ManagedDependency {
            group_id: "com.example".into(),
            artifact_id: "managed".into(),
            version: Some("2.0.0".into()),
            scope: Default::default(),
            optional: false,
            dep_type: "jar".into(),
            classifier: None,
            exclusions: vec![],
        });
        child.dependencies.push(MavenDependency {
            group_id: "com.example".into(),
            artifact_id: "managed".into(),
            version: None,
            scope: Default::default(),
            optional: false,
            dep_type: "jar".into(),
            classifier: None,
            exclusions: vec![],
        });

        let mut index = PomIndex::new();
        index.insert(parent.coordinates().gav(), parent);
        let effective = build_effective(&child, &index);

        assert_eq!(effective.effective_dependencies.len(), 2);
        assert_eq!(effective.effective_dependencies[0].artifact_id, "inherited");
        assert_eq!(
            effective.effective_dependencies[1].version.as_deref(),
            Some("2.0.0")
        );
    }

    #[test]
    fn inherited_coordinates_are_indexed_for_grandchildren() {
        let root = pom("com.example", "root", "1.0.0", None);
        let child = pom(
            "",
            "child",
            "",
            Some(crate::maven::model::MavenParent {
                group_id: "com.example".into(),
                artifact_id: "root".into(),
                version: "1.0.0".into(),
                relative_path: None,
            }),
        );
        let grandchild = pom(
            "",
            "grandchild",
            "",
            Some(crate::maven::model::MavenParent {
                group_id: "com.example".into(),
                artifact_id: "child".into(),
                version: "1.0.0".into(),
                relative_path: None,
            }),
        );

        let index = build_index(&[root, child, grandchild.clone()]);
        let effective = build_effective(&grandchild, &index);
        assert!(effective.has_workspace_parent);
        assert_eq!(effective.group_id, "com.example");
        assert_eq!(effective.version, "1.0.0");
    }

    #[test]
    fn placeholder_substitution_with_properties() {
        let mut p = pom("com.example", "app", "1.0.0", None);
        p.properties.insert("lib.version".into(), "2.3.0".into());
        p.dependencies.push(MavenDependency {
            group_id: "com.example".into(),
            artifact_id: "lib".into(),
            version: Some("${lib.version}".into()),
            scope: Default::default(),
            optional: false,
            dep_type: "jar".into(),
            classifier: None,
            exclusions: vec![],
        });
        p.dependency_management.push(ManagedDependency {
            group_id: "com.example".into(),
            artifact_id: "other".into(),
            version: Some("${project.version}".into()),
            scope: Default::default(),
            optional: false,
            dep_type: "jar".into(),
            classifier: None,
            exclusions: vec![],
        });
        p.dependencies.push(MavenDependency {
            group_id: "com.example".into(),
            artifact_id: "other".into(),
            version: None,
            scope: Default::default(),
            optional: false,
            dep_type: "jar".into(),
            classifier: None,
            exclusions: vec![],
        });

        let idx = PomIndex::new();
        let eff = build_effective(&p, &idx);
        assert_eq!(
            eff.effective_dependencies[0].version.as_deref(),
            Some("2.3.0")
        );
        assert_eq!(
            eff.effective_dependencies[1].version.as_deref(),
            Some("1.0.0")
        );
    }

    #[test]
    fn unknown_placeholder_preserved() {
        let mut p = pom("g", "a", "1", None);
        p.dependencies.push(MavenDependency {
            group_id: "g".into(),
            artifact_id: "d".into(),
            version: Some("${unknown.key}".into()),
            scope: Default::default(),
            optional: false,
            dep_type: "jar".into(),
            classifier: None,
            exclusions: vec![],
        });
        let idx = PomIndex::new();
        let eff = build_effective(&p, &idx);
        assert_eq!(
            eff.effective_dependencies[0].version.as_deref(),
            Some("${unknown.key}")
        );
    }

    #[test]
    fn multi_module_parent_type() {
        let mut p = pom("com.example", "root", "1.0.0", None);
        p.packaging = "pom".to_string();
        p.modules.push(crate::maven::model::MavenModule {
            path: "mod-a".into(),
        });
        let idx = PomIndex::new();
        let eff = build_effective(&p, &idx);
        assert_eq!(eff.project_type, MavenProjectType::Parent);
    }
}
