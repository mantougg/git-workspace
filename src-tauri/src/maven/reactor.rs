//! Existing and synthetic Maven Reactor planning (R-03).

use std::collections::{HashMap, HashSet};
use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use crate::maven::closure::RuntimeClosure;
use crate::maven::index::{DependencyGraph, MavenModuleLink, MavenProjectNode};

const GITWORKSPACE_IGNORE_ENTRY: &str = ".gitworkspace/";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RuntimeReactorKind {
    Existing,
    Synthetic,
}

/// Maven invocation inputs prepared without executing Maven.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeReactorPlan {
    pub kind: RuntimeReactorKind,
    pub pom_path: PathBuf,
    pub module_paths: Vec<PathBuf>,
    pub arguments: Vec<String>,
}

/// Reuse a complete existing reactor when possible; otherwise generate a minimal one.
pub fn prepare_runtime_reactor(
    graph: &DependencyGraph,
    closure: &RuntimeClosure,
    workspace_root: &Path,
    runtime_name: &str,
) -> AppResult<RuntimeReactorPlan> {
    validate_closure(graph, closure)?;
    let root_project = graph
        .projects
        .iter()
        .find(|project| project.project_id == closure.root_project_id)
        .ok_or_else(|| {
            AppError::ProjectNotFound(format!(
                "Runtime root project {} is missing from the dependency graph",
                closure.root_project_id
            ))
        })?;

    if closure.projects.len() == 1 {
        require_project_pom(root_project)?;
        return Ok(existing_plan(root_project, root_project, closure, false));
    }

    if let Some(reactor_id) = find_existing_reactor(graph, closure)? {
        validate_reactor_modules(graph, reactor_id)?;
        let reactor = graph
            .projects
            .iter()
            .find(|project| project.project_id == reactor_id)
            .ok_or_else(|| {
                AppError::ProjectNotFound(format!(
                    "Maven reactor project {reactor_id} is missing from the dependency graph"
                ))
            })?;
        require_project_pom(reactor)?;
        return Ok(existing_plan(reactor, root_project, closure, true));
    }

    generate_synthetic_reactor(closure, workspace_root, runtime_name)
}

fn validate_closure(graph: &DependencyGraph, closure: &RuntimeClosure) -> AppResult<()> {
    if graph.workspace_id != closure.workspace_id || graph.fingerprint != closure.graph_fingerprint
    {
        return Err(AppError::DependencyResolve(
            "Runtime Closure is stale; recompute it from the current dependency graph".into(),
        ));
    }
    let graph_projects: HashMap<i64, &MavenProjectNode> = graph
        .projects
        .iter()
        .map(|project| (project.project_id, project))
        .collect();
    for project in &closure.projects {
        let graph_project = graph_projects.get(&project.project_id).ok_or_else(|| {
            AppError::ProjectNotFound(format!(
                "Closure project {} is missing from the dependency graph",
                project.project_id
            ))
        })?;
        if *graph_project != project {
            return Err(AppError::DependencyResolve(format!(
                "Closure project {} does not match the current dependency graph",
                project.project_id
            )));
        }
        require_project_pom(project)?;
    }
    Ok(())
}

fn require_project_pom(project: &MavenProjectNode) -> AppResult<()> {
    if project.path.is_file() {
        Ok(())
    } else {
        Err(AppError::ProjectNotFound(format!(
            "Maven POM {} is missing",
            project.path.display()
        )))
    }
}

fn existing_plan(
    reactor: &MavenProjectNode,
    root_project: &MavenProjectNode,
    closure: &RuntimeClosure,
    use_project_list: bool,
) -> RuntimeReactorPlan {
    let pom_path = reactor.path.clone();
    let mut arguments = vec!["-f".into(), pom_path.to_string_lossy().into_owned()];
    if use_project_list {
        arguments.extend([
            "-pl".into(),
            format!(
                "{}:{}",
                root_project.coordinates.group_id, root_project.coordinates.artifact_id
            ),
            "-am".into(),
        ]);
    }
    RuntimeReactorPlan {
        kind: RuntimeReactorKind::Existing,
        pom_path,
        module_paths: closure.projects.iter().map(project_directory).collect(),
        arguments,
    }
}

fn find_existing_reactor(
    graph: &DependencyGraph,
    closure: &RuntimeClosure,
) -> AppResult<Option<i64>> {
    let repository_id = match closure
        .projects
        .first()
        .and_then(|project| project.repository_id)
    {
        Some(repository_id)
            if closure
                .projects
                .iter()
                .all(|project| project.repository_id == Some(repository_id)) =>
        {
            repository_id
        }
        _ => return Ok(None),
    };

    let closure_ids: HashSet<i64> = closure
        .projects
        .iter()
        .map(|project| project.project_id)
        .collect();
    let links = module_links_by_parent(&graph.modules);
    let mut candidates = Vec::new();

    for project in graph.projects.iter().filter(|project| {
        project.repository_id == Some(repository_id) && project.packaging == "pom"
    }) {
        let reachable = known_reactor_projects(project.project_id, &links);
        if closure_ids.is_subset(&reachable) {
            candidates.push((
                reachable.len(),
                project.path.to_string_lossy().into_owned(),
                project.project_id,
            ));
        }
    }
    candidates.sort();
    Ok(candidates.first().map(|candidate| candidate.2))
}

fn module_links_by_parent(modules: &[MavenModuleLink]) -> HashMap<i64, Vec<&MavenModuleLink>> {
    let mut links: HashMap<i64, Vec<&MavenModuleLink>> = HashMap::new();
    for module in modules {
        links
            .entry(module.parent_project_id)
            .or_default()
            .push(module);
    }
    for children in links.values_mut() {
        children.sort_by(|left, right| left.declared_path.cmp(&right.declared_path));
    }
    links
}

fn known_reactor_projects(
    root_project_id: i64,
    links: &HashMap<i64, Vec<&MavenModuleLink>>,
) -> HashSet<i64> {
    let mut projects = HashSet::new();
    let mut pending = vec![root_project_id];
    while let Some(project_id) = pending.pop() {
        if !projects.insert(project_id) {
            continue;
        }
        if let Some(children) = links.get(&project_id) {
            pending.extend(
                children
                    .iter()
                    .filter_map(|module| module.module_project_id),
            );
        }
    }
    projects
}

fn validate_reactor_modules(graph: &DependencyGraph, root_project_id: i64) -> AppResult<()> {
    let links = module_links_by_parent(&graph.modules);
    let projects: HashMap<i64, &MavenProjectNode> = graph
        .projects
        .iter()
        .map(|project| (project.project_id, project))
        .collect();
    let mut visited = HashSet::new();
    let mut pending = vec![root_project_id];

    while let Some(parent_id) = pending.pop() {
        if !visited.insert(parent_id) {
            continue;
        }
        if let Some(children) = links.get(&parent_id) {
            for child in children {
                let child_id = child.module_project_id.ok_or_else(|| {
                    let parent = projects
                        .get(&parent_id)
                        .map(|project| project.path.display().to_string())
                        .unwrap_or_else(|| parent_id.to_string());
                    AppError::ProjectNotFound(format!(
                        "Maven module `{}` declared by {parent} is missing",
                        child.declared_path
                    ))
                })?;
                let child_project = projects.get(&child_id).ok_or_else(|| {
                    AppError::ProjectNotFound(format!(
                        "Maven module `{}` maps to missing project {child_id}",
                        child.declared_path
                    ))
                })?;
                require_project_pom(child_project)?;
                pending.push(child_id);
            }
        }
    }
    Ok(())
}

fn generate_synthetic_reactor(
    closure: &RuntimeClosure,
    workspace_root: &Path,
    runtime_name: &str,
) -> AppResult<RuntimeReactorPlan> {
    validate_runtime_name(runtime_name)?;
    let workspace_root = fs::canonicalize(workspace_root).map_err(|error| {
        AppError::ProjectNotFound(format!(
            "workspace root {} is unavailable: {error}",
            workspace_root.display()
        ))
    })?;
    let reactor_dir = workspace_root
        .join(".gitworkspace")
        .join("runtime")
        .join(runtime_name);
    // R-14 §78 只读护栏：运行时生成物只落 workspace/.gitworkspace（含
    // 符号链接解析后的逃逸检查，替换原 starts_with 弱校验）。
    crate::runtime::guard::assert_workspace_write_path(&reactor_dir, &workspace_root, "Synthetic Reactor 生成")?;
    fs::create_dir_all(&reactor_dir)?;
    let reactor_dir = fs::canonicalize(&reactor_dir)?;
    crate::runtime::guard::assert_workspace_write_path(&reactor_dir, &workspace_root, "Synthetic Reactor 生成（canonical）")?;

    let mut module_paths = Vec::with_capacity(closure.projects.len());
    let mut module_entries = Vec::with_capacity(closure.projects.len());
    let mut seen = HashSet::new();
    for project in &closure.projects {
        let project_dir = fs::canonicalize(project_directory(project)).map_err(|error| {
            AppError::ProjectNotFound(format!(
                "Maven project {} is unavailable: {error}",
                project.path.display()
            ))
        })?;
        let relative = relative_path(&reactor_dir, &project_dir)?;
        let module = path_for_maven(&relative)?;
        if seen.insert(module.clone()) {
            module_paths.push(exposed_path(&project_dir));
            module_entries.push(module);
        }
    }

    ensure_gitworkspace_ignored(&workspace_root)?;
    let internal_pom_path = reactor_dir.join("pom.xml");
    reject_symlink(&internal_pom_path)?;
    let content = synthetic_pom(runtime_name, &module_entries);
    write_if_changed(&internal_pom_path, content.as_bytes())?;
    let pom_path = exposed_path(&internal_pom_path);

    Ok(RuntimeReactorPlan {
        kind: RuntimeReactorKind::Synthetic,
        pom_path: pom_path.clone(),
        module_paths,
        arguments: vec!["-f".into(), pom_path.to_string_lossy().into_owned()],
    })
}

fn exposed_path(path: &Path) -> PathBuf {
    let value = path.to_string_lossy();
    if let Some(rest) = value.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{rest}"));
    }
    if let Some(rest) = value.strip_prefix(r"\\?\") {
        return PathBuf::from(rest);
    }
    path.to_path_buf()
}

fn validate_runtime_name(runtime_name: &str) -> AppResult<()> {
    let valid = !runtime_name.is_empty()
        && runtime_name != "."
        && runtime_name != ".."
        && runtime_name
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "-_.".contains(character));
    if valid {
        Ok(())
    } else {
        Err(AppError::DependencyResolve(format!(
            "invalid runtime name `{runtime_name}`; use ASCII letters, digits, '.', '_' or '-'"
        )))
    }
}

fn project_directory(project: &MavenProjectNode) -> PathBuf {
    project
        .path
        .parent()
        .unwrap_or_else(|| Path::new(""))
        .to_path_buf()
}

fn relative_path(from: &Path, to: &Path) -> AppResult<PathBuf> {
    let (from_anchor, from_parts) = split_absolute_path(from)?;
    let (to_anchor, to_parts) = split_absolute_path(to)?;
    if !anchors_equal(&from_anchor, &to_anchor) {
        return Err(AppError::SourceMapping(format!(
            "cannot generate Synthetic Reactor module path across filesystem volumes: {} -> {}",
            from.display(),
            to.display()
        )));
    }

    let common = from_parts
        .iter()
        .zip(&to_parts)
        .take_while(|(left, right)| components_equal(left, right))
        .count();
    let mut relative = PathBuf::new();
    for _ in common..from_parts.len() {
        relative.push("..");
    }
    for part in &to_parts[common..] {
        relative.push(part);
    }
    if relative.as_os_str().is_empty() {
        relative.push(".");
    }
    Ok(relative)
}

fn split_absolute_path(path: &Path) -> AppResult<(OsString, Vec<OsString>)> {
    let mut anchor = OsString::new();
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => anchor.push(prefix.as_os_str()),
            Component::RootDir => anchor.push(OsStr::new("/")),
            Component::CurDir => {}
            Component::ParentDir => {
                return Err(AppError::SourceMapping(format!(
                    "path must be normalized before Reactor generation: {}",
                    path.display()
                )))
            }
            Component::Normal(part) => parts.push(part.to_os_string()),
        }
    }
    if anchor.is_empty() {
        return Err(AppError::SourceMapping(format!(
            "path must be absolute for Reactor generation: {}",
            path.display()
        )));
    }
    Ok((anchor, parts))
}

#[cfg(windows)]
fn anchors_equal(left: &OsStr, right: &OsStr) -> bool {
    left.to_string_lossy()
        .eq_ignore_ascii_case(&right.to_string_lossy())
}

#[cfg(not(windows))]
fn anchors_equal(left: &OsStr, right: &OsStr) -> bool {
    left == right
}

#[cfg(windows)]
fn components_equal(left: &OsStr, right: &OsStr) -> bool {
    left.to_string_lossy()
        .eq_ignore_ascii_case(&right.to_string_lossy())
}

#[cfg(not(windows))]
fn components_equal(left: &OsStr, right: &OsStr) -> bool {
    left == right
}

fn path_for_maven(path: &Path) -> AppResult<String> {
    path.to_str()
        .map(|value| value.replace('\\', "/"))
        .ok_or_else(|| {
            AppError::SourceMapping(format!(
                "Maven module path is not valid Unicode: {}",
                path.display()
            ))
        })
}

fn synthetic_pom(runtime_name: &str, modules: &[String]) -> String {
    let modules = modules
        .iter()
        .map(|module| format!("    <module>{}</module>", xml_escape(module)))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<project xmlns="http://maven.apache.org/POM/4.0.0"
         xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"
         xsi:schemaLocation="http://maven.apache.org/POM/4.0.0 https://maven.apache.org/xsd/maven-4.0.0.xsd">
  <modelVersion>4.0.0</modelVersion>
  <groupId>dev.gitworkspace.runtime</groupId>
  <artifactId>{}-runtime-reactor</artifactId>
  <version>1</version>
  <packaging>pom</packaging>
  <modules>
{}
  </modules>
</project>
"#,
        xml_escape(runtime_name),
        modules
    )
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// Ensure generated Runtime files remain outside user repositories' Git status.
pub fn ensure_gitworkspace_ignored(workspace_root: &Path) -> AppResult<bool> {
    let gitignore = workspace_root.join(".gitignore");
    reject_symlink(&gitignore)?;
    let existing = match fs::read_to_string(&gitignore) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error.into()),
    };
    if existing.lines().any(|line| {
        matches!(
            line.trim(),
            ".gitworkspace" | ".gitworkspace/" | "/.gitworkspace" | "/.gitworkspace/"
        )
    }) {
        return Ok(false);
    }

    let newline = if existing.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let mut updated = existing;
    if !updated.is_empty() && !updated.ends_with('\n') && !updated.ends_with('\r') {
        updated.push_str(newline);
    }
    updated.push_str(GITWORKSPACE_IGNORE_ENTRY);
    updated.push_str(newline);
    write_if_changed(&gitignore, updated.as_bytes())?;
    Ok(true)
}

fn reject_symlink(path: &Path) -> AppResult<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(AppError::Permission(format!(
            "refusing to write generated Runtime file through symlink {}",
            path.display()
        ))),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn write_if_changed(path: &Path, content: &[u8]) -> AppResult<bool> {
    match fs::read(path) {
        Ok(existing) if existing == content => return Ok(false),
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    fs::write(path, content)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use super::*;
    use crate::maven::closure::{compute_runtime_closure, RuntimeScope};
    use crate::maven::index::{DependencyEdge, MavenModuleLink};
    use crate::maven::model::{DependencyScope, MavenDependency, PomCoordinates};
    use crate::maven::resolver::{DependencySource, ResolutionReason};

    fn temp_workspace(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "gw_r03_{name}_{}",
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn write(path: &Path, content: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, content).unwrap();
    }

    fn commit_fixture_pom(repository_root: &Path) {
        let repository = git2::Repository::init(repository_root).unwrap();
        let mut index = repository.index().unwrap();
        index.add_path(Path::new("pom.xml")).unwrap();
        index.write().unwrap();
        let tree_id = index.write_tree().unwrap();
        let tree = repository.find_tree(tree_id).unwrap();
        let signature =
            git2::Signature::now("GitWorkspace Test", "test@gitworkspace.local").unwrap();
        repository
            .commit(Some("HEAD"), &signature, &signature, "fixture", &tree, &[])
            .unwrap();
    }

    fn project(
        id: i64,
        repository_id: i64,
        path: PathBuf,
        artifact: &str,
        packaging: &str,
    ) -> MavenProjectNode {
        MavenProjectNode {
            project_id: id,
            repository_id: Some(repository_id),
            path,
            coordinates: PomCoordinates {
                group_id: "com.example".into(),
                artifact_id: artifact.into(),
                version: "1.0.0".into(),
            },
            packaging: packaging.into(),
            pom_hash: format!("hash-{id}"),
        }
    }

    fn dependency(from: i64, to: i64, artifact: &str) -> DependencyEdge {
        DependencyEdge {
            dependency_id: from * 10 + to,
            from_project_id: from,
            dependency: MavenDependency {
                group_id: "com.example".into(),
                artifact_id: artifact.into(),
                version: Some("1.0.0".into()),
                scope: DependencyScope::Compile,
                optional: false,
                dep_type: "jar".into(),
                classifier: None,
                exclusions: vec![],
            },
            source: DependencySource::WorkspaceSource,
            source_project_id: Some(to),
            resolved_path: None,
            reason: ResolutionReason::WorkspaceExactMatch,
        }
    }

    fn graph(
        projects: Vec<MavenProjectNode>,
        dependencies: Vec<DependencyEdge>,
        modules: Vec<MavenModuleLink>,
    ) -> DependencyGraph {
        DependencyGraph {
            workspace_id: 1,
            fingerprint: "graph-v1".into(),
            projects,
            dependencies,
            modules,
            source_mappings: vec![],
        }
    }

    fn simple_pom(artifact: &str, dependency: Option<&str>) -> String {
        let dependency = dependency
            .map(|artifact| format!(
                "<dependencies><dependency><groupId>com.example</groupId><artifactId>{artifact}</artifactId><version>1.0.0</version></dependency></dependencies>"
            ))
            .unwrap_or_default();
        format!(
            "<project><modelVersion>4.0.0</modelVersion><groupId>com.example</groupId><artifactId>{artifact}</artifactId><version>1.0.0</version>{dependency}</project>"
        )
    }

    #[test]
    fn standard_multi_module_project_reuses_existing_reactor_without_writes() {
        let root = temp_workspace("existing");
        let parent = root.join("repo/pom.xml");
        let common = root.join("repo/common/pom.xml");
        let app = root.join("repo/app/pom.xml");
        write(
            &parent,
            "<project><modelVersion>4.0.0</modelVersion><groupId>com.example</groupId><artifactId>parent</artifactId><version>1.0.0</version><packaging>pom</packaging><modules><module>common</module><module>app</module></modules></project>",
        );
        write(&common, &simple_pom("common", None));
        write(&app, &simple_pom("app", Some("common")));
        let graph = graph(
            vec![
                project(1, 1, parent.clone(), "parent", "pom"),
                project(2, 1, common, "common", "jar"),
                project(3, 1, app, "app", "jar"),
            ],
            vec![dependency(3, 2, "common")],
            vec![
                MavenModuleLink {
                    parent_project_id: 1,
                    module_project_id: Some(2),
                    declared_path: "common".into(),
                },
                MavenModuleLink {
                    parent_project_id: 1,
                    module_project_id: Some(3),
                    declared_path: "app".into(),
                },
            ],
        );
        let closure = compute_runtime_closure(&graph, 3, &RuntimeScope::Auto).unwrap();
        let plan = prepare_runtime_reactor(&graph, &closure, &root, "app").unwrap();

        assert_eq!(plan.kind, RuntimeReactorKind::Existing);
        assert_eq!(plan.pom_path, parent);
        assert_eq!(plan.arguments[2..], ["-pl", "com.example:app", "-am"]);
        assert!(!root.join(".gitworkspace").exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn cross_repo_reactor_is_idempotent_and_never_changes_source_poms() {
        let root = temp_workspace("synthetic");
        let common_pom = root.join("repo-common/pom.xml");
        let app_pom = root.join("repo-app/pom.xml");
        let common_content = simple_pom("common", None);
        let app_content = simple_pom("app", Some("common"));
        write(&common_pom, &common_content);
        write(&app_pom, &app_content);
        commit_fixture_pom(common_pom.parent().unwrap());
        commit_fixture_pom(app_pom.parent().unwrap());
        let graph = graph(
            vec![
                project(1, 1, common_pom.clone(), "common", "jar"),
                project(2, 2, app_pom.clone(), "app", "jar"),
            ],
            vec![dependency(2, 1, "common")],
            vec![],
        );
        let closure = compute_runtime_closure(&graph, 2, &RuntimeScope::Auto).unwrap();

        let first = prepare_runtime_reactor(&graph, &closure, &root, "app").unwrap();
        let first_content = fs::read(&first.pom_path).unwrap();
        let second = prepare_runtime_reactor(&graph, &closure, &root, "app").unwrap();

        assert_eq!(first.kind, RuntimeReactorKind::Synthetic);
        assert_eq!(first, second);
        assert_eq!(first_content, fs::read(&second.pom_path).unwrap());
        assert_eq!(fs::read_to_string(common_pom).unwrap(), common_content);
        assert_eq!(fs::read_to_string(app_pom).unwrap(), app_content);
        for repository_root in [root.join("repo-common"), root.join("repo-app")] {
            let repository = git2::Repository::open(repository_root).unwrap();
            assert!(repository.statuses(None).unwrap().is_empty());
        }
        assert_eq!(
            fs::read_to_string(root.join(".gitignore")).unwrap(),
            ".gitworkspace/\n"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn missing_existing_reactor_module_is_actionable() {
        let root = temp_workspace("missing-module");
        write(
            &root.join("repo/pom.xml"),
            "<project><modelVersion>4.0.0</modelVersion><groupId>com.example</groupId><artifactId>parent</artifactId><version>1.0.0</version><packaging>pom</packaging><modules><module>common</module><module>app</module><module>missing</module></modules></project>",
        );
        write(
            &root.join("repo/common/pom.xml"),
            &simple_pom("common", None),
        );
        write(
            &root.join("repo/app/pom.xml"),
            &simple_pom("app", Some("common")),
        );
        let graph = graph(
            vec![
                project(1, 1, root.join("repo/pom.xml"), "parent", "pom"),
                project(2, 1, root.join("repo/common/pom.xml"), "common", "jar"),
                project(3, 1, root.join("repo/app/pom.xml"), "app", "jar"),
            ],
            vec![dependency(3, 2, "common")],
            vec![
                MavenModuleLink {
                    parent_project_id: 1,
                    module_project_id: Some(2),
                    declared_path: "common".into(),
                },
                MavenModuleLink {
                    parent_project_id: 1,
                    module_project_id: Some(3),
                    declared_path: "app".into(),
                },
                MavenModuleLink {
                    parent_project_id: 1,
                    module_project_id: None,
                    declared_path: "missing".into(),
                },
            ],
        );
        let closure = compute_runtime_closure(&graph, 3, &RuntimeScope::Auto).unwrap();
        let error = prepare_runtime_reactor(&graph, &closure, &root, "app").unwrap_err();
        assert_eq!(error.code(), "ProjectNotFound");
        assert!(error.to_string().contains("missing"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn stale_graph_with_deleted_pom_is_actionable() {
        let root = temp_workspace("deleted-pom");
        let graph = graph(
            vec![project(1, 1, root.join("repo-app/pom.xml"), "app", "jar")],
            vec![],
            vec![],
        );
        let closure = compute_runtime_closure(&graph, 1, &RuntimeScope::Auto).unwrap();
        let error = prepare_runtime_reactor(&graph, &closure, &root, "app").unwrap_err();

        assert_eq!(error.code(), "ProjectNotFound");
        assert!(error.to_string().contains("pom.xml"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn gitignore_maintenance_is_idempotent() {
        let root = temp_workspace("gitignore");
        write(&root.join(".gitignore"), "target/\r\n");
        assert!(ensure_gitworkspace_ignored(&root).unwrap());
        assert!(!ensure_gitworkspace_ignored(&root).unwrap());
        assert_eq!(
            fs::read_to_string(root.join(".gitignore")).unwrap(),
            "target/\r\n.gitworkspace/\r\n"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn generated_cross_repo_reactor_passes_real_maven_validate_when_available() {
        let maven = if cfg!(windows) { "mvn.cmd" } else { "mvn" };
        if Command::new(maven).arg("-version").output().is_err() {
            eprintln!("skipping real Maven validation because `{maven}` is unavailable");
            return;
        }

        let root = temp_workspace("maven-validate");
        let common_pom = root.join("repo-common/pom.xml");
        let app_pom = root.join("repo-app/pom.xml");
        write(&common_pom, &simple_pom("common", None));
        write(&app_pom, &simple_pom("app", Some("common")));
        let graph = graph(
            vec![
                project(1, 1, common_pom, "common", "jar"),
                project(2, 2, app_pom, "app", "jar"),
            ],
            vec![dependency(2, 1, "common")],
            vec![],
        );
        let closure = compute_runtime_closure(&graph, 2, &RuntimeScope::Auto).unwrap();
        let plan = prepare_runtime_reactor(&graph, &closure, &root, "app").unwrap();
        let output = Command::new(maven)
            .args(["-q", "-o", "-f"])
            .arg(&plan.pom_path)
            .arg("validate")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "mvn validate failed:\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(windows)]
    #[test]
    fn cross_volume_paths_return_source_mapping_error() {
        let error =
            relative_path(Path::new(r"C:\workspace\runtime"), Path::new(r"D:\repo")).unwrap_err();
        assert_eq!(error.code(), "SourceMappingFailed");
    }
}
