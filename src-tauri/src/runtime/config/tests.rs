//! Runtime 配置测试（R-07，B-06 拆分后归位同父模块 tests.rs）。

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use rusqlite::Connection;
use uuid::Uuid;

use super::storage::read_file;
use super::*;
use crate::db;

fn open_db() -> (Connection, PathBuf) {
    let mut conn = Connection::open_in_memory().unwrap();
    db::init_db(&mut conn).unwrap();
    let root = std::env::temp_dir().join(format!("gw_runtime_config_{}", Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    conn.execute(
        "INSERT INTO workspaces (name, path, created_at, updated_at) VALUES ('w', ?1, 't', 't')",
        [root.to_string_lossy().to_string()],
    )
    .unwrap();
    (conn, root)
}

fn sample(name: &str) -> RuntimeApplicationConfig {
    RuntimeApplicationConfig {
        name: name.into(),
        project: "repo-boot".into(),
        main_class: Some("com.example.Application".into()),
        jdk: Some("21".into()),
        profile: Some("dev".into()),
        vm_options: vec!["-Xmx1g".into()],
        program_arguments: vec!["--server.port=8080".into()],
        environment: BTreeMap::from([
            ("SERVER_PORT".into(), "8080".into()),
            ("DB_PASSWORD".into(), "secret".into()),
        ]),
        runtime_environment: BTreeMap::from([("RUNTIME_FLAG".into(), "on".into())]),
        ..Default::default()
    }
}

#[test]
fn crud_round_trip_uses_json_for_full_config_and_sqlite_for_list() {
    let (conn, root) = open_db();
    let request = CreateRuntimeConfigRequest {
        workspace_id: 1,
        config: sample("boot"),
    };
    let created = create_config(&conn, &request).unwrap();
    assert_eq!(created.environment["DB_PASSWORD"], MASKED_VALUE);
    let path = root.join(".gitworkspace/runtimes/boot.json");
    assert!(path.is_file());

    let summaries = list_configs(&conn, 1).unwrap();
    assert_eq!(summaries.len(), 1);
    assert_eq!(summaries[0].project, "repo-boot");

    let loaded = get_config(&conn, 1, "boot").unwrap();
    assert_eq!(loaded.environment["SERVER_PORT"], "8080");
    assert_eq!(loaded.environment["DB_PASSWORD"], MASKED_VALUE);

    delete_config(&conn, 1, "boot").unwrap();
    assert!(!path.exists());
    assert!(list_configs(&conn, 1).unwrap().is_empty());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn update_preserves_masked_secret_and_supports_rename() {
    let (conn, root) = open_db();
    create_config(
        &conn,
        &CreateRuntimeConfigRequest {
            workspace_id: 1,
            config: sample("old"),
        },
    )
    .unwrap();
    let mut update = sample("new");
    update
        .environment
        .insert("DB_PASSWORD".into(), MASKED_VALUE.into());
    update
        .environment
        .insert("SERVER_PORT".into(), "9090".into());
    update_config(
        &conn,
        &UpdateRuntimeConfigRequest {
            workspace_id: 1,
            name: "old".into(),
            config: update,
        },
    )
    .unwrap();
    let raw = read_file(&root.join(".gitworkspace/runtimes/new.json")).unwrap();
    assert!(raw.contains("secret"));
    assert!(!root.join(".gitworkspace/runtimes/old.json").exists());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn environment_layers_merge_application_over_runtime_workspace_global_system() {
    let layers = EnvironmentLayers {
        system: BTreeMap::from([("A".into(), "system".into())]),
        global: BTreeMap::from([("A".into(), "global".into()), ("G".into(), "1".into())]),
        workspace: BTreeMap::from([("A".into(), "workspace".into())]),
        runtime: BTreeMap::from([("A".into(), "runtime".into())]),
        application: BTreeMap::from([("A".into(), "application".into())]),
    };
    let merged = merge_environment(&layers);
    assert_eq!(merged["A"], "application");
    assert_eq!(merged["G"], "1");
}

#[test]
fn malformed_json_reports_path_line_and_column() {
    let (conn, root) = open_db();
    create_config(
        &conn,
        &CreateRuntimeConfigRequest {
            workspace_id: 1,
            config: sample("broken"),
        },
    )
    .unwrap();
    let path = root.join(".gitworkspace/runtimes/broken.json");
    fs::write(&path, "{\n  \"name\": \"broken\",\n  \"project\": ").unwrap();
    let error = get_config(&conn, 1, "broken").unwrap_err();
    let message = error.to_string();
    assert!(message.contains("broken.json"));
    assert!(message.contains("第"));
    assert!(message.contains("列"));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn old_json_defaults_new_fields_and_profiles_support_both_injection_forms() {
    let old: RuntimeApplicationConfig =
        serde_json::from_str(r#"{"name":"boot","project":"repo-boot","profile":"dev"}"#).unwrap();
    assert_eq!(old.schema_version, CURRENT_SCHEMA_VERSION);
    assert!(old.vm_options.is_empty());
    assert_eq!(
        old.with_default_profile_injection().vm_options,
        vec!["-Dspring.profiles.active=dev"]
    );

    let mut args = old.clone();
    args.program_arguments = vec!["--spring.profiles.active=test".into()];
    assert_eq!(args.injected_profile().as_deref(), Some("test"));
    args.program_arguments.clear();
    args.vm_options = vec!["-Dspring.profiles.active=prod".into()];
    assert_eq!(args.injected_profile().as_deref(), Some("prod"));
}
