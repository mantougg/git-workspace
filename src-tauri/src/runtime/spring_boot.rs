//! Spring Boot application discovery (R-06).
//!
//! Detection deliberately stays below the Java language level: Maven metadata
//! identifies likely Boot modules, then a bounded text scan looks for a direct
//! `@SpringBootApplication` annotation. No Java AST or code index is built.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use moka::sync::Cache;
use rayon::prelude::*;
use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::maven::effective::EffectiveProject;
use crate::maven::model::{MavenDependency, MavenPlugin, MavenProject};
use crate::maven::parser::hex_hash;

const MAX_SOURCE_FILES: usize = 10_000;

/// A source-level Spring Boot main-class candidate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpringBootCandidate {
    /// Fully-qualified Java class name.
    pub class_name: String,
    /// Simple class name, useful for compact UI labels.
    pub simple_name: String,
    /// Maven artifact/module containing the source file.
    pub module: String,
    /// Absolute source file path.
    pub source_path: PathBuf,
}

/// Detection result for one Maven project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpringBootProject {
    /// Absolute `pom.xml` path.
    pub project_path: PathBuf,
    /// Maven artifact id used as the module label.
    pub module: String,
    pub spring_boot_plugin: bool,
    pub spring_boot_dependency: bool,
    pub is_spring_boot: bool,
    pub candidates: Vec<SpringBootCandidate>,
    pub default_main_class: Option<String>,
    pub source_files_scanned: usize,
    pub source_scan_truncated: bool,
}

/// Workspace-level Spring Boot detection response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpringBootWorkspaceResult {
    pub projects: Vec<SpringBootProject>,
    pub elapsed_ms: u128,
}

/// Bounded in-memory cache for source detection results.
///
/// The key includes the POM hash (and workspace-parent hashes) plus a content
/// fingerprint of every scanned Java source file. A changed POM or source file
/// therefore naturally bypasses the cache, matching R-01 cache semantics.
pub struct SpringBootDetectionCache {
    inner: Cache<String, SpringBootProject>,
}

impl SpringBootDetectionCache {
    pub fn new() -> Self {
        Self {
            inner: Cache::builder().max_capacity(2_048).build(),
        }
    }

    pub fn entry_count(&self) -> u64 {
        self.inner.run_pending_tasks();
        self.inner.entry_count()
    }

    pub fn invalidate_project(&self, project_path: &Path) {
        let prefix = path_key(project_path);
        let _ = self
            .inner
            .invalidate_entries_if(move |key, _| key.starts_with(&prefix));
    }

    pub fn invalidate_all(&self) {
        self.inner.invalidate_all();
    }
}

impl Default for SpringBootDetectionCache {
    fn default() -> Self {
        Self::new()
    }
}

/// Detect all Maven projects in a discovery result.
pub fn detect_spring_boot_workspace(
    projects: &[MavenProject],
    effective: &[EffectiveProject],
    cache: Option<&SpringBootDetectionCache>,
) -> SpringBootWorkspaceResult {
    detect_spring_boot_workspace_cancellable(projects, effective, cache, None)
}

/// Cancellable variant used by future Runtime task orchestration.
pub fn detect_spring_boot_workspace_cancellable(
    projects: &[MavenProject],
    effective: &[EffectiveProject],
    cache: Option<&SpringBootDetectionCache>,
    cancel: Option<&AtomicBool>,
) -> SpringBootWorkspaceResult {
    let started = Instant::now();
    let effective_by_path: HashMap<PathBuf, &EffectiveProject> = effective
        .iter()
        .map(|project| (canonical_or_original(&project.path), project))
        .collect();
    let by_gav = build_project_index(projects, effective);

    let mut results: Vec<SpringBootProject> = projects
        .par_iter()
        .filter_map(|project| {
            if is_cancelled(cancel) {
                return None;
            }
            let effective = effective_by_path
                .get(&canonical_or_original(&project.path))
                .copied();
            Some(detect_project(project, effective, &by_gav, cache))
        })
        .collect();
    results.sort_by(|left, right| left.project_path.cmp(&right.project_path));

    SpringBootWorkspaceResult {
        projects: results,
        elapsed_ms: started.elapsed().as_millis(),
    }
}

fn detect_project(
    project: &MavenProject,
    effective: Option<&EffectiveProject>,
    by_gav: &HashMap<String, &MavenProject>,
    cache: Option<&SpringBootDetectionCache>,
) -> SpringBootProject {
    let module = module_name(project);
    let chain = parent_chain(project, by_gav);
    let spring_boot_plugin = chain
        .iter()
        .flat_map(|pom| pom.plugins.iter())
        .any(is_spring_boot_plugin)
        || chain.iter().any(|pom| {
            pom.parent.as_ref().is_some_and(|parent| {
                parent.group_id == "org.springframework.boot"
                    && parent.artifact_id == "spring-boot-starter-parent"
            })
        });
    let spring_boot_dependency = effective
        .map(|model| {
            model
                .effective_dependencies
                .iter()
                .any(is_spring_boot_dependency)
        })
        .unwrap_or_else(|| project.dependencies.iter().any(is_spring_boot_dependency));
    let is_spring_boot = spring_boot_plugin || spring_boot_dependency;

    if !is_spring_boot {
        return SpringBootProject {
            project_path: project.path.clone(),
            module,
            spring_boot_plugin,
            spring_boot_dependency,
            is_spring_boot,
            candidates: vec![],
            default_main_class: None,
            source_files_scanned: 0,
            source_scan_truncated: false,
        };
    }

    let source_root = project
        .path
        .parent()
        .unwrap_or_else(|| Path::new(""))
        .join("src/main/java");
    let files = collect_java_files(&source_root);
    let source_fingerprint = fingerprint_sources(&files);
    let parent_fingerprint = chain
        .iter()
        .map(|pom| pom.file_hash.as_str())
        .collect::<Vec<_>>()
        .join(":");
    let cache_key = format!(
        "{}:{}:{}",
        path_key(&project.path),
        parent_fingerprint,
        source_fingerprint
    );
    if let Some(cache) = cache {
        if let Some(cached) = cache.inner.get(&cache_key) {
            return cached;
        }
    }

    let mut candidates = Vec::new();
    for source in &files.files {
        let Ok(content) = std::fs::read_to_string(source) else {
            continue;
        };
        candidates.extend(scan_java_source(&content, source, &module));
    }
    candidates.sort_by(|left, right| {
        left.class_name
            .cmp(&right.class_name)
            .then_with(|| left.source_path.cmp(&right.source_path))
    });
    candidates.dedup_by(|left, right| left.class_name == right.class_name);

    let default_main_class = infer_default_main_class(
        &candidates,
        &module,
        effective.map(|model| &model.effective_properties),
    );
    let result = SpringBootProject {
        project_path: project.path.clone(),
        module,
        spring_boot_plugin,
        spring_boot_dependency,
        is_spring_boot,
        candidates,
        default_main_class,
        source_files_scanned: files.files.len(),
        source_scan_truncated: files.truncated,
    };
    if let Some(cache) = cache {
        cache.inner.insert(cache_key, result.clone());
    }
    result
}

fn build_project_index<'a>(
    projects: &'a [MavenProject],
    effective: &[EffectiveProject],
) -> HashMap<String, &'a MavenProject> {
    let by_path: HashMap<PathBuf, &'a MavenProject> = projects
        .iter()
        .map(|project| (canonical_or_original(&project.path), project))
        .collect();
    let mut index = HashMap::new();
    for model in effective {
        if let Some(project) = by_path.get(&canonical_or_original(&model.path)) {
            index.insert(
                format!("{}:{}:{}", model.group_id, model.artifact_id, model.version),
                *project,
            );
        }
    }
    // Discovery normally provides effective entries for every valid POM. Keep
    // a raw-coordinate fallback for direct unit callers and partial results.
    for project in projects {
        index.entry(project.coordinates().gav()).or_insert(project);
    }
    index
}

fn parent_chain<'a>(
    project: &'a MavenProject,
    by_gav: &HashMap<String, &'a MavenProject>,
) -> Vec<&'a MavenProject> {
    let mut chain = vec![project];
    let mut current = project.parent.as_ref();
    let mut visited = HashSet::from([project.coordinates().gav()]);
    while let Some(parent) = current {
        let key = format!(
            "{}:{}:{}",
            parent.group_id, parent.artifact_id, parent.version
        );
        if !visited.insert(key.clone()) {
            break;
        }
        let Some(parent_project) = by_gav.get(&key).copied() else {
            break;
        };
        chain.push(parent_project);
        current = parent_project.parent.as_ref();
    }
    chain
}

fn is_spring_boot_plugin(plugin: &MavenPlugin) -> bool {
    plugin.artifact_id == "spring-boot-maven-plugin"
        && (plugin.group_id.is_empty() || plugin.group_id == "org.springframework.boot")
}

fn is_spring_boot_dependency(dependency: &MavenDependency) -> bool {
    dependency.group_id == "org.springframework.boot"
        && dependency.artifact_id.starts_with("spring-boot")
}

fn module_name(project: &MavenProject) -> String {
    if !project.artifact_id.is_empty() {
        return project.artifact_id.clone();
    }
    project
        .path
        .parent()
        .and_then(|path| path.file_name())
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "unknown".to_string())
}

fn infer_default_main_class(
    candidates: &[SpringBootCandidate],
    module: &str,
    properties: Option<&std::collections::BTreeMap<String, String>>,
) -> Option<String> {
    if let Some(properties) = properties {
        for key in [
            "start-class",
            "spring-boot.run.main-class",
            "spring.boot.mainclass",
        ] {
            if let Some(value) = properties.get(key).filter(|value| !value.trim().is_empty()) {
                return Some(value.trim().to_string());
            }
        }
    }
    if candidates.len() == 1 {
        return Some(candidates[0].class_name.clone());
    }
    candidates
        .iter()
        .max_by(|left, right| {
            candidate_score(left, module)
                .cmp(&candidate_score(right, module))
                .then_with(|| right.class_name.cmp(&left.class_name))
        })
        .map(|candidate| candidate.class_name.clone())
}

fn candidate_score(candidate: &SpringBootCandidate, module: &str) -> u8 {
    let simple = candidate.simple_name.to_ascii_lowercase();
    let module_name = normalize_identifier(module);
    let mut score = 0;
    if simple.ends_with("application") {
        score += 50;
    }
    if normalize_identifier(&candidate.simple_name) == format!("{module_name}application") {
        score += 100;
    }
    score
}

fn normalize_identifier(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

struct JavaFiles {
    files: Vec<PathBuf>,
    truncated: bool,
}

fn collect_java_files(root: &Path) -> JavaFiles {
    if !root.is_dir() {
        return JavaFiles {
            files: vec![],
            truncated: false,
        };
    }
    let mut files = Vec::new();
    let mut walker = walkdir::WalkDir::new(root).follow_links(false).into_iter();
    while let Some(entry) = walker.next() {
        let Ok(entry) = entry else { continue };
        if entry.file_type().is_dir()
            && matches!(
                entry.file_name().to_str(),
                Some("target" | ".git" | "node_modules")
            )
        {
            walker.skip_current_dir();
            continue;
        }
        if entry.file_type().is_file()
            && entry.path().extension().and_then(|value| value.to_str()) == Some("java")
        {
            files.push(entry.path().to_path_buf());
            if files.len() >= MAX_SOURCE_FILES {
                break;
            }
        }
    }
    files.sort();
    JavaFiles {
        truncated: files.len() >= MAX_SOURCE_FILES,
        files,
    }
}

fn fingerprint_sources(files: &JavaFiles) -> String {
    let mut input = Vec::new();
    for path in &files.files {
        input.extend_from_slice(path.to_string_lossy().as_bytes());
        input.push(0);
        if let Ok(content) = std::fs::read(path) {
            input.extend_from_slice(hex_hash(&content).as_bytes());
        } else {
            input.extend_from_slice(b"missing");
        }
        input.push(0);
    }
    input.extend_from_slice(if files.truncated {
        b"truncated"
    } else {
        b"complete"
    });
    hex_hash(&input)
}

fn scan_java_source(content: &str, source_path: &Path, module: &str) -> Vec<SpringBootCandidate> {
    let cleaned = strip_java_comments_and_strings(content);
    let package =
        Regex::new(r"(?m)^\s*package\s+([A-Za-z_][A-Za-z0-9_]*(?:\.[A-Za-z_][A-Za-z0-9_]*)*)\s*;")
            .expect("package regex is valid")
            .captures(&cleaned)
            .map(|capture| capture[1].to_string());
    let class_regex = Regex::new(
        r"\b(?:public\s+|protected\s+|private\s+)?(?:abstract\s+|final\s+)?(?:class|record)\s+([A-Za-z_][A-Za-z0-9_]*)",
    )
    .expect("class regex is valid");

    let mut candidates = Vec::new();
    let mut cursor = 0;
    while let Some(relative) = cleaned[cursor..].find("@SpringBootApplication") {
        let start = cursor + relative;
        let before_ok = start == 0
            || !cleaned[..start]
                .chars()
                .next_back()
                .is_some_and(|character| character.is_ascii_alphanumeric() || character == '_');
        let after = start + "@SpringBootApplication".len();
        let after_ok = cleaned[after..]
            .chars()
            .next()
            .is_none_or(|character| !(character.is_ascii_alphanumeric() || character == '_'));
        if before_ok && after_ok {
            if let Some(capture) = class_regex.captures(&cleaned[after..]) {
                let simple_name = capture[1].to_string();
                let class_name = package
                    .as_deref()
                    .map(|package| format!("{package}.{simple_name}"))
                    .unwrap_or_else(|| simple_name.clone());
                candidates.push(SpringBootCandidate {
                    class_name,
                    simple_name,
                    module: module.to_string(),
                    source_path: source_path.to_path_buf(),
                });
            }
        }
        cursor = after;
    }
    candidates
}

/// Remove comments and string/character literals while preserving line breaks.
/// This prevents examples in comments or string constants from becoming apps.
fn strip_java_comments_and_strings(source: &str) -> String {
    #[derive(Clone, Copy)]
    enum State {
        Normal,
        LineComment,
        BlockComment,
        String,
        Char,
    }
    let chars: Vec<char> = source.chars().collect();
    let mut output = String::with_capacity(source.len());
    let mut state = State::Normal;
    let mut index = 0;
    while index < chars.len() {
        let character = chars[index];
        match state {
            State::Normal => match character {
                '/' if chars.get(index + 1) == Some(&'/') => {
                    output.push(' ');
                    output.push(' ');
                    state = State::LineComment;
                    index += 1;
                }
                '/' if chars.get(index + 1) == Some(&'*') => {
                    output.push(' ');
                    output.push(' ');
                    state = State::BlockComment;
                    index += 1;
                }
                '"' => {
                    output.push(' ');
                    state = State::String;
                }
                '\'' => {
                    output.push(' ');
                    state = State::Char;
                }
                _ => output.push(character),
            },
            State::LineComment => {
                if character == '\n' {
                    output.push('\n');
                    state = State::Normal;
                } else {
                    output.push(' ');
                }
            }
            State::BlockComment => {
                if character == '*' && chars.get(index + 1) == Some(&'/') {
                    output.push(' ');
                    output.push(' ');
                    state = State::Normal;
                    index += 1;
                } else if character == '\n' {
                    output.push('\n');
                } else {
                    output.push(' ');
                }
            }
            State::String | State::Char => {
                if character == '\\' {
                    output.push(' ');
                    if chars.get(index + 1) == Some(&'\n') {
                        output.push('\n');
                    } else if index + 1 < chars.len() {
                        output.push(' ');
                        index += 1;
                    }
                } else if (matches!(state, State::String) && character == '"')
                    || (matches!(state, State::Char) && character == '\'')
                {
                    output.push(' ');
                    state = State::Normal;
                } else if character == '\n' {
                    output.push('\n');
                    state = State::Normal;
                } else {
                    output.push(' ');
                }
            }
        }
        index += 1;
    }
    output
}

fn is_cancelled(cancel: Option<&AtomicBool>) -> bool {
    cancel.is_some_and(|flag| flag.load(std::sync::atomic::Ordering::Relaxed))
}

fn path_key(path: &Path) -> String {
    canonical_or_original(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn canonical_or_original(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::maven::effective::build_effective;
    use crate::maven::model::{MavenDependency, MavenPlugin};
    use crate::maven::parser::parse_pom;
    use std::collections::BTreeMap;

    fn project(path: PathBuf, artifact: &str, hash: &str) -> MavenProject {
        MavenProject {
            path,
            group_id: "com.example".into(),
            artifact_id: artifact.into(),
            version: "1.0.0".into(),
            packaging: "jar".into(),
            parent: None,
            modules: vec![],
            dependencies: vec![],
            dependency_management: vec![],
            profiles: vec![],
            properties: BTreeMap::new(),
            plugins: vec![],
            file_hash: hash.into(),
        }
    }

    fn detect(
        projects: &[MavenProject],
        cache: Option<&SpringBootDetectionCache>,
    ) -> SpringBootWorkspaceResult {
        let index = crate::maven::effective::build_index(projects);
        let effective: Vec<_> = projects
            .iter()
            .map(|project| build_effective(project, &index))
            .collect();
        detect_spring_boot_workspace(projects, &effective, cache)
    }

    #[test]
    fn scans_direct_annotation_and_prefers_start_class() {
        let dir = std::env::temp_dir().join(format!("gw_r06_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(dir.join("src/main/java/com/example")).unwrap();
        let pom = dir.join("pom.xml");
        std::fs::write(
            &pom,
            r#"<project><modelVersion>4.0.0</modelVersion><groupId>com.example</groupId><artifactId>admin</artifactId><version>1</version><properties><start-class>com.example.AdminApplication</start-class></properties><dependencies><dependency><groupId>org.springframework.boot</groupId><artifactId>spring-boot-starter</artifactId><version>3</version></dependency></dependencies></project>"#,
        )
        .unwrap();
        std::fs::write(
            dir.join("src/main/java/com/example/AdminApplication.java"),
            "package com.example;\n@SpringBootApplication\npublic class AdminApplication {}",
        )
        .unwrap();
        std::fs::write(
            dir.join("src/main/java/com/example/OtherApplication.java"),
            "package com.example;\n@SpringBootApplication\npublic class OtherApplication {}",
        )
        .unwrap();
        let parsed = parse_pom(&pom, &std::fs::read(&pom).unwrap(), "hash").unwrap();
        let result = detect(&[parsed], None);
        let app = &result.projects[0];
        assert!(app.is_spring_boot);
        assert!(app.spring_boot_dependency);
        assert_eq!(app.candidates.len(), 2);
        assert_eq!(
            app.default_main_class.as_deref(),
            Some("com.example.AdminApplication")
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn inherited_plugin_marks_child_as_boot_and_library_stays_empty() {
        let root = std::env::temp_dir().join(format!("gw_r06_parent_{}", uuid::Uuid::new_v4()));
        let parent_dir = root.join("parent");
        let child_dir = root.join("child");
        std::fs::create_dir_all(child_dir.join("src/main/java/com/example")).unwrap();
        let mut parent = project(parent_dir.join("pom.xml"), "parent", "parent-hash");
        parent.plugins.push(MavenPlugin {
            group_id: "org.springframework.boot".into(),
            artifact_id: "spring-boot-maven-plugin".into(),
            version: None,
        });
        let mut child = project(child_dir.join("pom.xml"), "child", "child-hash");
        child.parent = Some(crate::maven::model::MavenParent {
            group_id: "com.example".into(),
            artifact_id: "parent".into(),
            version: "1.0.0".into(),
            relative_path: Some("../parent/pom.xml".into()),
        });
        std::fs::write(
            child_dir.join("src/main/java/com/example/Application.java"),
            "package com.example;\n@SpringBootApplication\nclass Application {}",
        )
        .unwrap();
        let result = detect(&[parent, child], None);
        let app = result
            .projects
            .iter()
            .find(|project| project.module == "child")
            .unwrap();
        assert!(app.spring_boot_plugin);
        assert_eq!(app.candidates.len(), 1);
        assert!(result
            .projects
            .iter()
            .find(|project| project.module == "parent")
            .unwrap()
            .candidates
            .is_empty());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn comments_and_strings_are_not_candidates_and_cache_invalidates_on_source_change() {
        let dir = std::env::temp_dir().join(format!("gw_r06_cache_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(dir.join("src/main/java/com/example")).unwrap();
        let pom = dir.join("pom.xml");
        std::fs::write(&pom, "<project><modelVersion>4.0.0</modelVersion><groupId>g</groupId><artifactId>app</artifactId><version>1</version><build><plugins><plugin><groupId>org.springframework.boot</groupId><artifactId>spring-boot-maven-plugin</artifactId></plugin></plugins></build></project>").unwrap();
        let source = dir.join("src/main/java/com/example/Application.java");
        std::fs::write(&source, "package com.example; // @SpringBootApplication\nclass Application { String s = \"@SpringBootApplication\"; }").unwrap();
        let parsed = parse_pom(&pom, &std::fs::read(&pom).unwrap(), "hash").unwrap();
        let cache = SpringBootDetectionCache::new();
        let first = detect(std::slice::from_ref(&parsed), Some(&cache));
        assert!(first.projects[0].candidates.is_empty());
        std::fs::write(
            &source,
            "package com.example;\n@SpringBootApplication\nclass Application {}",
        )
        .unwrap();
        let second = detect(std::slice::from_ref(&parsed), Some(&cache));
        assert_eq!(second.projects[0].candidates.len(), 1);
        let mut pom_changed = parsed.clone();
        pom_changed.file_hash = "changed-pom-hash".into();
        let third = detect(std::slice::from_ref(&pom_changed), Some(&cache));
        assert_eq!(third.projects[0].candidates.len(), 1);
        assert_eq!(cache.entry_count(), 3);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn plain_library_does_not_trigger_source_scan() {
        let project = project(
            PathBuf::from("/tmp/library/pom.xml"),
            "library",
            "library-hash",
        );
        let result = detect(std::slice::from_ref(&project), None);
        let library = &result.projects[0];
        assert!(!library.is_spring_boot);
        assert!(library.candidates.is_empty());
        assert_eq!(library.source_files_scanned, 0);
    }

    #[test]
    fn dependency_helper_only_accepts_boot_group() {
        let dependency = MavenDependency {
            group_id: "org.springframework.boot".into(),
            artifact_id: "spring-boot-starter-web".into(),
            version: Some("3".into()),
            scope: Default::default(),
            optional: false,
            dep_type: "jar".into(),
            classifier: None,
            exclusions: vec![],
        };
        assert!(is_spring_boot_dependency(&dependency));
        let mut other = dependency.clone();
        other.group_id = "com.example".into();
        assert!(!is_spring_boot_dependency(&other));
    }
}
