use super::*;
use crate::db;
use crate::maven::{discover_poms, PomCache};
use crate::test_support::write;


fn fixture() -> (PathBuf, Connection, i64) {
    let root = std::env::temp_dir().join(format!(
        "gw_r02_{}",
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let mut conn = Connection::open_in_memory().unwrap();
    db::init_db(&mut conn).unwrap();
    conn.execute(
        "INSERT INTO workspaces (name, path, created_at, updated_at)
             VALUES ('w', ?1, 't', 't')",
        [path_key(&root)],
    )
    .unwrap();
    let workspace_id = conn.last_insert_rowid();

    for name in ["common", "core", "auth", "boot"] {
        let repo = root.join(format!("repo-{name}"));
        std::fs::create_dir_all(&repo).unwrap();
        git2::Repository::init(&repo).unwrap();
        conn.execute(
            "INSERT INTO repositories (
                    workspace_id, path, name, relative_path, created_at, updated_at
                 ) VALUES (?1, ?2, ?3, ?3, 't', 't')",
            params![workspace_id, path_key(&repo), name],
        )
        .unwrap();
    }
    (root, conn, workspace_id)
}

fn pom(artifact: &str, dependencies: &str) -> String {
    format!(
        r#"<project>
  <modelVersion>4.0.0</modelVersion>
  <groupId>com.example</groupId>
  <artifactId>{artifact}</artifactId>
  <version>1.0.0</version>
  <dependencies>{dependencies}</dependencies>
</project>"#
    )
}

fn dep(group: &str, artifact: &str, version: &str) -> String {
    format!(
        "<dependency><groupId>{group}</groupId><artifactId>{artifact}</artifactId><version>{version}</version></dependency>"
    )
}

fn write_fixture_poms(root: &Path, boot_extra: &str) {
    write(&root.join("repo-common/pom.xml"), &pom("common", ""));
    write(
        &root.join("repo-core/pom.xml"),
        &pom("core", &dep("com.example", "common", "1.0.0")),
    );
    write(
        &root.join("repo-auth/pom.xml"),
        &pom("auth", &dep("com.example", "common", "1.0.0")),
    );
    let boot_deps = format!(
        "{}{}{}{}",
        dep("com.example", "core", "1.0.0"),
        dep("com.example", "auth", "1.0.0"),
        dep("org.springframework.boot", "spring-boot", "3.3.0"),
        boot_extra
    );
    write(&root.join("repo-boot/pom.xml"), &pom("boot", &boot_deps));
}

#[test]
fn persists_cross_repo_graph_and_classifies_all_sources() {
    let (root, mut conn, workspace_id) = fixture();
    write_fixture_poms(&root, &dep("org.example", "remote-only", "1.0.0"));
    let local = root.join("m2");
    let spring = local.join("org/springframework/boot/spring-boot/3.3.0/spring-boot-3.3.0.jar");
    write(&spring, "jar");

    let discovery = discover_poms(&root, 6, None, None);
    let sync = sync_workspace_index(&mut conn, workspace_id, &discovery, &local).unwrap();
    assert_eq!(sync.inserted, 4);
    assert_eq!(sync.recomputed_projects, 4);

    let graph = query_dependency_graph(&conn, workspace_id).unwrap();
    assert_eq!(graph.projects.len(), 4);
    assert_eq!(graph.source_mappings.len(), 4);
    assert_eq!(
        graph
            .dependencies
            .iter()
            .filter(|edge| edge.source == DependencySource::WorkspaceSource)
            .count(),
        4
    );
    assert!(graph.dependencies.iter().any(|edge| {
        edge.dependency.artifact_id == "spring-boot"
            && edge.source == DependencySource::LocalRepository
    }));
    assert!(graph.dependencies.iter().any(|edge| {
        edge.dependency.artifact_id == "remote-only"
            && edge.source == DependencySource::RemoteRepository
    }));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn unchanged_models_reuse_graph_and_pom_change_invalidates_cache() {
    let (root, mut conn, workspace_id) = fixture();
    write_fixture_poms(&root, "");
    let local = root.join("m2");
    let pom_cache = PomCache::new();
    let first = discover_poms(&root, 6, Some(&pom_cache), None);
    sync_workspace_index(&mut conn, workspace_id, &first, &local).unwrap();

    let second = discover_poms(&root, 6, Some(&pom_cache), None);
    let sync = sync_workspace_index(&mut conn, workspace_id, &second, &local).unwrap();
    assert_eq!(sync.unchanged, 4);
    assert_eq!(sync.recomputed_projects, 0);

    let cache = DependencyGraphCache::new();
    assert_eq!(
        cache.inner.policy().max_capacity(),
        Some(GRAPH_CACHE_CAPACITY)
    );
    assert!(!cache.get_or_load(&conn, workspace_id).unwrap().cache_hit);
    assert!(cache.get_or_load(&conn, workspace_id).unwrap().cache_hit);

    let remote_graph = cache.get_or_load(&conn, workspace_id).unwrap().graph;
    let spring_dependency = remote_graph
        .dependencies
        .iter()
        .find(|edge| edge.dependency.artifact_id == "spring-boot")
        .unwrap()
        .dependency
        .clone();
    let spring_artifact = local_artifact_path(
        &local,
        &spring_dependency,
        spring_dependency.version.as_deref().unwrap(),
    );
    write(&spring_artifact, "jar");
    let unchanged = discover_poms(&root, 6, Some(&pom_cache), None);
    let sync = sync_workspace_index(&mut conn, workspace_id, &unchanged, &local).unwrap();
    assert_eq!(sync.recomputed_projects, 0);
    let local_lookup = cache.get_or_load(&conn, workspace_id).unwrap();
    assert!(!local_lookup.cache_hit);
    assert!(local_lookup.graph.dependencies.iter().any(|edge| {
        edge.dependency.artifact_id == "spring-boot"
            && edge.source == DependencySource::LocalRepository
    }));

    std::fs::remove_file(&spring_artifact).unwrap();
    let unchanged = discover_poms(&root, 6, Some(&pom_cache), None);
    let sync = sync_workspace_index(&mut conn, workspace_id, &unchanged, &local).unwrap();
    assert_eq!(sync.recomputed_projects, 0);
    assert!(cache
        .get_or_load(&conn, workspace_id)
        .unwrap()
        .graph
        .dependencies
        .iter()
        .any(|edge| {
            edge.dependency.artifact_id == "spring-boot"
                && edge.source == DependencySource::RemoteRepository
        }));

    write_fixture_poms(&root, &dep("org.example", "new-dependency", "2.0.0"));
    let changed = discover_poms(&root, 6, Some(&pom_cache), None);
    let sync = sync_workspace_index(&mut conn, workspace_id, &changed, &local).unwrap();
    assert_eq!(sync.updated, 1);
    assert_eq!(sync.recomputed_projects, 1);
    let refreshed = cache.get_or_load(&conn, workspace_id).unwrap();
    assert!(!refreshed.cache_hit);
    assert!(refreshed
        .graph
        .dependencies
        .iter()
        .any(|edge| edge.dependency.artifact_id == "new-dependency"));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn removed_project_reclassifies_dependents_without_stale_mapping() {
    let (root, mut conn, workspace_id) = fixture();
    write_fixture_poms(&root, "");
    let local = root.join("m2");
    let first = discover_poms(&root, 6, None, None);
    sync_workspace_index(&mut conn, workspace_id, &first, &local).unwrap();

    std::fs::remove_file(root.join("repo-common/pom.xml")).unwrap();
    let second = discover_poms(&root, 6, None, None);
    let sync = sync_workspace_index(&mut conn, workspace_id, &second, &local).unwrap();
    assert_eq!(sync.removed, 1);
    assert!(sync.mapping_changed);
    assert_eq!(sync.recomputed_projects, 3);

    let graph = query_dependency_graph(&conn, workspace_id).unwrap();
    assert!(!graph
        .source_mappings
        .iter()
        .any(|mapping| mapping.coordinates.artifact_id == "common"));
    assert!(graph.dependencies.iter().all(|edge| {
        edge.dependency.artifact_id != "common"
            || edge.source != DependencySource::WorkspaceSource
    }));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn migration_contains_exactly_the_r02_table_set() {
    let mut conn = Connection::open_in_memory().unwrap();
    db::init_db(&mut conn).unwrap();
    for table in [
        "maven_projects",
        "maven_dependencies",
        "maven_modules",
        "maven_artifacts",
        "maven_source_mappings",
        "runtime_projects",
        "runtime_dependencies",
    ] {
        let exists: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                [table],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(exists, 1, "missing R-02 table {table}");
    }
}

#[test]
fn runtime_dependency_unique_index_treats_null_as_one_value() {
    let mut conn = Connection::open_in_memory().unwrap();
    db::init_db(&mut conn).unwrap();
    conn.execute(
        "INSERT INTO workspaces (name, path, created_at, updated_at)
             VALUES ('w', '/w', 't', 't')",
        [],
    )
    .unwrap();
    let workspace_id = conn.last_insert_rowid();
    conn.execute(
        "INSERT INTO maven_projects (
                workspace_id, path, group_id, artifact_id, version, packaging,
                pom_hash, model_hash, last_scanned_at
             ) VALUES (?1, '/w/pom.xml', 'g', 'a', '1', 'jar', 'p', 'm', 't')",
        [workspace_id],
    )
    .unwrap();
    let project_id = conn.last_insert_rowid();
    conn.execute(
        "INSERT INTO runtime_projects (
                workspace_id, name, root_project_id, created_at, updated_at
             ) VALUES (?1, 'app', ?2, 't', 't')",
        params![workspace_id, project_id],
    )
    .unwrap();
    let runtime_id = conn.last_insert_rowid();
    let insert = || {
        conn.execute(
            "INSERT INTO runtime_dependencies (
                    runtime_project_id, maven_project_id, dependency_project_id,
                    scope, source_kind
                 ) VALUES (?1, ?2, NULL, 'compile', 'remoteRepository')",
            params![runtime_id, project_id],
        )
    };
    insert().unwrap();
    assert!(
        insert().is_err(),
        "NULL dependency ids must still deduplicate"
    );
}

#[test]
fn module_paths_are_canonicalized_before_linking() {
    let (root, mut conn, workspace_id) = fixture();
    write(
        &root.join("repo-common/pom.xml"),
        r#"<project>
  <modelVersion>4.0.0</modelVersion>
  <groupId>com.example</groupId>
  <artifactId>parent</artifactId>
  <version>1.0.0</version>
  <packaging>pom</packaging>
  <modules><module>./module-a</module></modules>
</project>"#,
    );
    write(
        &root.join("repo-common/module-a/pom.xml"),
        r#"<project>
  <modelVersion>4.0.0</modelVersion>
  <parent>
    <groupId>com.example</groupId>
    <artifactId>parent</artifactId>
    <version>1.0.0</version>
  </parent>
  <artifactId>module-a</artifactId>
</project>"#,
    );

    let discovery = discover_poms(&root, 6, None, None);
    sync_workspace_index(&mut conn, workspace_id, &discovery, &root.join("m2")).unwrap();
    let graph = query_dependency_graph(&conn, workspace_id).unwrap();
    assert_eq!(graph.modules.len(), 1);
    assert!(graph.modules[0].module_project_id.is_some());

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn lexical_path_normalization_removes_dot_segments() {
    let normalized = path_key(Path::new("alpha/./beta/../gamma/pom.xml"));
    assert!(normalized.ends_with("alpha/gamma/pom.xml"));
    assert!(!normalized.contains("/./"));
    assert!(!normalized.contains("beta/../"));
}

#[test]
fn windows_verbatim_prefix_is_not_persisted_or_exposed() {
    assert_eq!(
        strip_windows_verbatim_prefix(r"\\?\C:\workspace\pom.xml"),
        r"C:\workspace\pom.xml"
    );
    assert_eq!(
        strip_windows_verbatim_prefix(r"\\?\UNC\server\share\pom.xml"),
        r"\\server\share\pom.xml"
    );
}
