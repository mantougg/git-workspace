//! Synthetic Maven workspace generator (R-08).
//!
//! Generates a deterministic `repositories × modules` matrix of Maven projects
//! (§96): each repository is a multi-module project whose modules form an
//! in-repo dependency chain; the last module of every repository after the
//! first adds a cross-repository source dependency on `module-XXX-00` of the
//! previous repository, so Runtime Closures span repositories and force
//! Synthetic Reactor generation (R-03).
//!
//! Determinism: every byte of generated content is derived from the loop
//! indices (repository / module numbers) — no wall clock, no RNG. Two runs
//! with the same arguments produce an identical tree, which is the "fixed
//! seed" guarantee cross-run comparisons rely on.
//!
//! Each repository is `git init`-ed (no commits) so the T-01 RepoScanner —
//! which discovers repositories by `.git` markers — finds it.

use std::path::{Path, PathBuf};

use super::io_err;

/// A generated synthetic Maven workspace.
#[derive(Debug, Clone)]
pub struct SyntheticMavenWorkspace {
    pub root: PathBuf,
    pub repositories: usize,
    pub modules_per_repository: usize,
    /// Absolute path of every generated repository root (git-initialized).
    pub repository_paths: Vec<PathBuf>,
    /// Total number of `pom.xml` files (one parent + N modules per repository).
    pub project_count: usize,
    /// Number of cross-repository dependency edges.
    pub cross_repo_edges: usize,
}

/// Maven groupId of synthetic repository `repo`.
pub fn group_id(repo: usize) -> String {
    format!("com.bench.r{repo}")
}

/// Maven artifactId of module `module` in repository `repo`.
pub fn artifact_id(repo: usize, module: usize) -> String {
    format!("module-{repo:03}-{module:02}")
}

/// Directory name of module `module` inside its repository.
fn module_dir(module: usize) -> String {
    format!("module_{module:02}")
}

/// Generate the synthetic workspace under `root` (created if missing).
///
/// `repositories` and `modules_per_repository` come from the §96 matrix
/// (10 / 50 / 100 each). `modules_per_repository` must be at least 1.
pub fn generate_maven_workspace(
    root: &Path,
    repositories: usize,
    modules_per_repository: usize,
) -> std::io::Result<SyntheticMavenWorkspace> {
    assert!(
        repositories >= 1 && modules_per_repository >= 1,
        "matrix dimensions must be >= 1"
    );
    std::fs::create_dir_all(root)?;

    let mut repository_paths = Vec::with_capacity(repositories);
    for repo in 0..repositories {
        let repo_dir = root.join(format!("repo_{repo:04}"));
        write_repo(&repo_dir, repo, repositories, modules_per_repository)?;
        // T-01 RepoScanner discovers repositories via `.git` markers; an
        // init-ed repository without commits is enough (no history needed).
        git2::Repository::init(&repo_dir).map_err(io_err)?;
        repository_paths.push(repo_dir);
    }

    Ok(SyntheticMavenWorkspace {
        root: root.to_path_buf(),
        repositories,
        modules_per_repository,
        repository_paths,
        project_count: repositories * (modules_per_repository + 1),
        cross_repo_edges: repositories.saturating_sub(1),
    })
}

fn write_repo(
    repo_dir: &Path,
    repo: usize,
    repositories: usize,
    modules: usize,
) -> std::io::Result<()> {
    std::fs::create_dir_all(repo_dir)?;
    std::fs::write(repo_dir.join("pom.xml"), parent_pom(repo, modules))?;
    for module in 0..modules {
        let module_dir = repo_dir.join(module_dir(module));
        std::fs::create_dir_all(&module_dir)?;
        std::fs::write(
            module_dir.join("pom.xml"),
            module_pom(repo, module, repositories, modules),
        )?;
        write_java_source(&module_dir, repo, module)?;
    }
    Ok(())
}

/// Parent POM: packaging `pom`, module list, and a `dependencyManagement`
/// entry pinned via a property so the effective-model stage (R-01) has real
/// property substitution + version mediation work to do.
fn parent_pom(repo: usize, modules: usize) -> String {
    let module_entries: String = (0..modules)
        .map(|module| format!("    <module>{}</module>\n", module_dir(module)))
        .collect();
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<project xmlns="http://maven.apache.org/POM/4.0.0">
  <modelVersion>4.0.0</modelVersion>
  <groupId>{group}</groupId>
  <artifactId>repo-{repo:03}-parent</artifactId>
  <version>1.0.0</version>
  <packaging>pom</packaging>
  <properties>
    <commons.version>2.{repo}.0</commons.version>
    <maven.compiler.source>17</maven.compiler.source>
    <maven.compiler.target>17</maven.compiler.target>
  </properties>
  <dependencyManagement>
    <dependencies>
      <dependency>
        <groupId>org.bench.external</groupId>
        <artifactId>commons-external</artifactId>
        <version>${{commons.version}}</version>
      </dependency>
    </dependencies>
  </dependencyManagement>
  <modules>
{module_entries}  </modules>
</project>
"#,
        group = group_id(repo),
    )
}

/// Module POM: inherits from the repository parent (version via parent), and
/// carries the dependency edges that shape the graph:
/// - `module_00` depends on `org.bench.external:commons-external` (version
///   mediated by the parent's dependencyManagement → a Remote edge).
/// - every module after the first depends on its in-repo predecessor
///   (`${project.version}` → a Workspace Source edge chain).
/// - the last module of every repository after the first depends on
///   `module-XXX-00` of the previous repository (cross-repo Source edge).
fn module_pom(repo: usize, module: usize, repositories: usize, modules: usize) -> String {
    let mut dependencies = String::new();
    if module == 0 {
        dependencies.push_str(
            "    <dependency>\n\
             \x20     <groupId>org.bench.external</groupId>\n\
             \x20     <artifactId>commons-external</artifactId>\n\
             \x20   </dependency>\n",
        );
    }
    if module > 0 {
        dependencies.push_str(&format!(
            "    <dependency>\n\
             \x20     <groupId>{}</groupId>\n\
             \x20     <artifactId>{}</artifactId>\n\
             \x20     <version>${{project.version}}</version>\n\
             \x20   </dependency>\n",
            group_id(repo),
            artifact_id(repo, module - 1),
        ));
    }
    if module == modules - 1 && repo > 0 {
        let _ = repositories; // only the previous repository is referenced
        dependencies.push_str(&format!(
            "    <dependency>\n\
             \x20     <groupId>{}</groupId>\n\
             \x20     <artifactId>{}</artifactId>\n\
             \x20     <version>1.0.0</version>\n\
             \x20   </dependency>\n",
            group_id(repo - 1),
            artifact_id(repo - 1, 0),
        ));
    }
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<project xmlns="http://maven.apache.org/POM/4.0.0">
  <modelVersion>4.0.0</modelVersion>
  <parent>
    <groupId>{group}</groupId>
    <artifactId>repo-{repo:03}-parent</artifactId>
    <version>1.0.0</version>
  </parent>
  <artifactId>{artifact}</artifactId>
  <dependencies>
{dependencies}  </dependencies>
</project>
"#,
        group = group_id(repo),
        artifact = artifact_id(repo, module),
    )
}

/// One minimal Java source file per module so the tree resembles a real
/// project and later build benchmarks (R-09+) have something to compile.
fn write_java_source(module_dir: &Path, repo: usize, module: usize) -> std::io::Result<()> {
    let package_dir = module_dir
        .join("src")
        .join("main")
        .join("java")
        .join("com")
        .join("bench")
        .join(format!("r{repo}"))
        .join(format!("m{module}"));
    std::fs::create_dir_all(&package_dir)?;
    std::fs::write(
        package_dir.join("App.java"),
        format!(
            "package com.bench.r{repo}.m{module};\n\nfinal class App {{\n    static int id() {{\n        return {repo} * 1000 + {module};\n    }}\n}}\n"
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree_snapshot(root: &Path) -> Vec<(String, String)> {
        let mut entries: Vec<(String, String)> = Vec::new();
        for entry in walkdir::WalkDir::new(root).sort_by_file_name() {
            let entry = entry.unwrap();
            if !entry.file_type().is_file() {
                continue;
            }
            // `.git` contents are not part of the deterministic payload.
            let relative = entry.path().strip_prefix(root).unwrap();
            if relative.components().next().and_then(|c| c.as_os_str().to_str()) == Some(".git")
                || relative.to_string_lossy().contains("/.git/")
                || relative.to_string_lossy().contains("/.git\\")
            {
                continue;
            }
            let content = std::fs::read(entry.path()).unwrap();
            entries.push((
                relative.to_string_lossy().replace('\\', "/"),
                crate::maven::parser::hex_hash(&content),
            ));
        }
        entries.sort();
        entries
    }

    #[test]
    fn generation_is_deterministic() {
        let base = std::env::temp_dir().join(format!("gw_mavengen_det_{}", std::process::id()));
        let first = base.join("first");
        let second = base.join("second");
        let _ = std::fs::remove_dir_all(&base);

        generate_maven_workspace(&first, 3, 4).unwrap();
        generate_maven_workspace(&second, 3, 4).unwrap();

        assert_eq!(tree_snapshot(&first), tree_snapshot(&second));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn generated_tree_matches_matrix_shape() {
        let root = std::env::temp_dir().join(format!("gw_mavengen_shape_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);

        let spec = generate_maven_workspace(&root, 10, 10).unwrap();
        assert_eq!(spec.repository_paths.len(), 10);
        assert_eq!(spec.project_count, 10 * 11);
        assert_eq!(spec.cross_repo_edges, 9);
        for repo_dir in &spec.repository_paths {
            assert!(repo_dir.join(".git").is_dir());
            assert!(repo_dir.join("pom.xml").is_file());
            for module in 0..10 {
                assert!(repo_dir
                    .join(module_dir(module))
                    .join("pom.xml")
                    .is_file());
            }
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The generated workspace must be discoverable by R-01 and resolvable by
    /// R-02: project count matches, in-repo chains and the cross-repo edge all
    /// resolve to Workspace Source, and the external dependency stays Remote.
    #[test]
    fn generated_workspace_roundtrips_through_discovery_and_index() {
        let root = std::env::temp_dir().join(format!("gw_mavengen_rt_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let spec = generate_maven_workspace(&root, 3, 3).unwrap();

        let discovery = crate::maven::discover_poms(&root, 5, None, None);
        assert_eq!(discovery.projects.len(), spec.project_count);
        assert!(
            discovery.errors.is_empty(),
            "synthetic POMs must parse cleanly: {:?}",
            discovery.errors
        );

        let db_path = root.join("bench.db");
        let mut conn = rusqlite::Connection::open(&db_path).unwrap();
        crate::db::init_db(&mut conn).unwrap();
        conn.execute(
            "INSERT INTO workspaces (name, path, created_at, updated_at) VALUES ('w', ?1, 't', 't')",
            [root.to_string_lossy().to_string()],
        )
        .unwrap();
        let workspace_id = conn.last_insert_rowid();
        let scanned: Vec<crate::models::repository::ScannedRepo> = spec
            .repository_paths
            .iter()
            .map(|path| crate::models::repository::ScannedRepo {
                path: path.to_string_lossy().to_string(),
                name: path.file_name().unwrap().to_string_lossy().to_string(),
                relative_path: path.file_name().unwrap().to_string_lossy().to_string(),
                git_dir_mtime: None,
            })
            .collect();
        crate::db::dao::upsert_repositories_batch(&mut conn, workspace_id, &scanned).unwrap();

        let local_repo = root.join("m2");
        let sync = crate::maven::sync_workspace_index(&mut conn, workspace_id, &discovery, &local_repo)
            .unwrap();
        assert_eq!(sync.inserted, spec.project_count);

        let graph = crate::maven::query_dependency_graph(&conn, workspace_id).unwrap();
        use crate::maven::DependencySource;
        let source_edges = graph
            .dependencies
            .iter()
            .filter(|edge| edge.source == DependencySource::WorkspaceSource)
            .count();
        let remote_edges = graph
            .dependencies
            .iter()
            .filter(|edge| edge.source == DependencySource::RemoteRepository)
            .count();
        // In-repo chains: 2 per repository × 3 repos; cross-repo: 2.
        assert_eq!(source_edges, 3 * 2 + 2);
        // One external dependency per repository (module_00 → commons-external).
        assert_eq!(remote_edges, 3);

        let _ = std::fs::remove_dir_all(&root);
    }
}
