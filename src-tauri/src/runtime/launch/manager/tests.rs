use super::*;
use crate::process::streaming::OutputStream;
use crate::runtime::build::runner::{FakeMavenRunner, FakeRun};
use crate::runtime::build::BuildOptions;
use crate::runtime::build::RunStrategy;
use crate::runtime::config::{create_config, CreateRuntimeConfigRequest, RuntimeApplicationConfig, RuntimeKind};
use crate::runtime::launch::launcher::{FakeBehavior, FakeLaunch, FakeLaunchRunner};
use crate::runtime::launch::VecEventSink;
use crate::runtime::logs::LogPhase;
use crate::test_support::write;
use std::path::{Path, PathBuf};
use std::time::Instant;

// --------------------------------------------------------------
// fixtures
// --------------------------------------------------------------

fn unique_root(tag: &str) -> PathBuf {
    crate::test_support::temp_root("gw_r10", tag)
}

/// 最小 fixture：tempdir + workspace 行 + Runtime 配置（skip-build 路径
/// 不需要 Maven 索引）。
struct MiniFixture {
    root: PathBuf,
    db: Arc<Mutex<Connection>>,
    workspace_id: i64,
}

fn mini_fixture(name: &str) -> MiniFixture {
    let root = unique_root(name);
    std::fs::create_dir_all(&root).unwrap();
    let mut conn = Connection::open_in_memory().unwrap();
    crate::db::init_db(&mut conn).unwrap();
    conn.execute(
        "INSERT INTO workspaces (name, path, created_at, updated_at) VALUES ('w', ?1, 't', 't')",
        [root.to_string_lossy().to_string()],
    )
    .unwrap();
    let workspace_id = conn.last_insert_rowid();
    let db = Arc::new(Mutex::new(conn));
    {
        let conn = db.lock().unwrap();
        create_config(
            &conn,
            &CreateRuntimeConfigRequest {
                workspace_id,
                config: RuntimeApplicationConfig {
                    name: "app".into(),
                    project: "app".into(),
                    main_class: Some("com.example.Application".into()),
                    ..Default::default()
                },
            },
        )
        .unwrap();
    }
    MiniFixture { root, db, workspace_id }
}

/// Maven 构建路径 fixture：单仓 parent(pom) + lib(jar) + app(jar→lib)，
/// 同步依赖图索引（对照 R-09 pipeline 测试 fixture）。
struct MavenFixture {
    root: PathBuf,
    db: Arc<Mutex<Connection>>,
    workspace_id: i64,
}

fn maven_fixture(name: &str, spring_boot: bool) -> MavenFixture {
    let root = unique_root(name);
    std::fs::create_dir_all(&root).unwrap();
    write(
        &root.join("repo/pom.xml"),
        "<project><modelVersion>4.0.0</modelVersion><groupId>com.example</groupId>\
             <artifactId>parent</artifactId><version>1.0.0</version><packaging>pom</packaging>\
             <modules><module>lib</module><module>app</module></modules></project>",
    );
    write(
        &root.join("repo/lib/pom.xml"),
        "<project><modelVersion>4.0.0</modelVersion><parent><groupId>com.example</groupId>\
             <artifactId>parent</artifactId><version>1.0.0</version></parent>\
             <artifactId>lib</artifactId></project>",
    );
    let app_dependency = if spring_boot {
        "<dependencies><dependency><groupId>com.example</groupId><artifactId>lib</artifactId>\
             <version>1.0.0</version></dependency><dependency><groupId>org.springframework.boot</groupId>\
             <artifactId>spring-boot-starter</artifactId><version>3.2.5</version></dependency></dependencies>"
    } else {
        "<dependencies><dependency><groupId>com.example</groupId><artifactId>lib</artifactId>\
             <version>1.0.0</version></dependency></dependencies>"
    };
    write(
        &root.join("repo/app/pom.xml"),
        &format!(
            "<project><modelVersion>4.0.0</modelVersion><parent><groupId>com.example</groupId>\
                 <artifactId>parent</artifactId><version>1.0.0</version></parent>\
                 <artifactId>app</artifactId>{app_dependency}</project>"
        ),
    );
    if spring_boot {
        write(
            &root.join("repo/app/src/main/java/com/example/app/Application.java"),
            "package com.example.app;\n\
                 import org.springframework.boot.autoconfigure.SpringBootApplication;\n\
                 @SpringBootApplication\npublic class Application {\n    public static void main(String[] args) {}\n}\n",
        );
    }
    git2::Repository::init(root.join("repo")).unwrap();

    let mut conn = Connection::open_in_memory().unwrap();
    crate::db::init_db(&mut conn).unwrap();
    conn.execute(
        "INSERT INTO workspaces (name, path, created_at, updated_at) VALUES ('w', ?1, 't', 't')",
        [root.to_string_lossy().to_string()],
    )
    .unwrap();
    let workspace_id = conn.last_insert_rowid();
    crate::db::dao::upsert_repositories_batch(
        &mut conn,
        workspace_id,
        &[crate::models::repository::ScannedRepo {
            path: root.join("repo").to_string_lossy().to_string(),
            name: "repo".into(),
            relative_path: "repo".into(),
            git_dir_mtime: None,
        }],
    )
    .unwrap();
    let discovery = crate::maven::discover_poms(&root, 5, None, None);
    assert!(discovery.errors.is_empty(), "{:?}", discovery.errors);
    crate::maven::sync_workspace_index(&mut conn, workspace_id, &discovery, &root.join("m2")).unwrap();

    let config = RuntimeApplicationConfig {
        name: "app".into(),
        project: root.join("repo/app/pom.xml").to_string_lossy().to_string(),
        main_class: (!spring_boot).then(|| "com.example.app.Application".to_string()),
        ..Default::default()
    };
    create_config(&conn, &CreateRuntimeConfigRequest { workspace_id, config }).unwrap();
    MavenFixture {
        root,
        db: Arc::new(Mutex::new(conn)),
        workspace_id,
    }
}

fn test_manager(
    db: Arc<Mutex<Connection>>,
    launch_runner: Arc<dyn LaunchRunner>,
    maven_runner: Arc<dyn MavenRunner>,
    events: Arc<VecEventSink>,
    sample_interval: Duration,
) -> Arc<RuntimeProcessManager> {
    Arc::new(RuntimeProcessManager::with_deps(
        db,
        RuntimeProcessDeps {
            launch_runner,
            maven_runner,
            events,
            sample_interval,
            ..Default::default()
        },
    ))
}

fn lifecycle_chain(events: &VecEventSink, process_id: i64) -> Vec<(LifecycleStatus, LifecycleStatus)> {
    events
        .collected()
        .iter()
        .filter_map(|event| match event {
            RuntimeEvent::Lifecycle {
                process_id: id,
                from,
                to,
                ..
            } if *id == process_id => Some((*from, *to)),
            _ => None,
        })
        .collect()
}

fn wait_for_status(
    manager: &RuntimeProcessManager,
    process_id: i64,
    status: LifecycleStatus,
    timeout: Duration,
) -> RuntimeProcessInfo {
    let deadline = Instant::now() + timeout;
    loop {
        let info = manager.get_process(process_id).unwrap().unwrap();
        if info.status == status {
            return info;
        }
        assert!(
            Instant::now() < deadline,
            "timeout waiting for {status:?}, last {info:?}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

const BANNER: &str = "Started Application in 1.234 seconds (process running)";
const TOMCAT: &str = "Tomcat started on port 8080 (http) with context path ''";

// --------------------------------------------------------------
// 全闭环（验收标准 1）：Start → Running → Stop → Stopped
// --------------------------------------------------------------

#[test]
fn start_stop_full_cycle_emits_lifecycle_events() {
    let fixture = maven_fixture("cycle", false);
    let events = Arc::new(VecEventSink::default());
    let maven = Arc::new(FakeMavenRunner::new(vec![
        FakeRun {
            lines: vec![(OutputStream::Stdout, "[INFO] BUILD SUCCESS".into())],
            ..Default::default()
        },
        FakeRun {
            output_file_content: Some(String::new()),
            ..Default::default()
        },
    ]));
    let launcher = Arc::new(FakeLaunchRunner::new(vec![FakeLaunch {
        lines: vec![
            (OutputStream::Stdout, TOMCAT.into()),
            (OutputStream::Stdout, BANNER.into()),
        ],
        behavior: FakeBehavior::StayAlive { on_terminate: Some(0) },
        ..Default::default()
    }]));
    let manager = test_manager(
        fixture.db.clone(),
        launcher,
        maven,
        events.clone(),
        Duration::from_millis(50),
    );

    let info = manager
        .start(fixture.workspace_id, "app", StartOptions::default())
        .unwrap();
    assert_eq!(info.status, LifecycleStatus::Running);
    assert!(info.pid.is_some());
    assert_eq!(info.run_strategy, Some(RunStrategy::ClasspathRun));
    assert!(info.command_preview.as_deref().unwrap().contains("java"));
    let _ = std::fs::remove_dir_all(&fixture.root);

    let stopped = manager.stop(info.process_id, None).unwrap();
    assert_eq!(stopped.status, LifecycleStatus::Stopped);
    assert_eq!(stopped.exit_code, Some(0));
    assert_eq!(stopped.ports, vec![8080], "端口探测来自启动日志");

    use LifecycleStatus::*;
    assert_eq!(
        lifecycle_chain(&events, info.process_id),
        vec![
            (Created, Preparing),
            (Preparing, Resolving),
            (Resolving, Building),
            (Building, Starting),
            (Starting, Running),
            (Running, Stopping),
            (Stopping, Stopped),
        ]
    );
    assert!(events.collected().iter().any(|e| matches!(
        e,
        RuntimeEvent::Ports { process_id, ports } if *process_id == info.process_id && ports.contains(&8080)
    )));
    assert!(events.collected().iter().any(|e| matches!(
        e,
        RuntimeEvent::Exited { process_id, exit_code: Some(0), crashed: false, .. } if *process_id == info.process_id
    )));
}

#[test]
fn node_start_detects_first_localhost_url_within_grace() {
    if crate::node::detect_node().is_err()
        || crate::node::detect_package_manager(crate::node::PackageManager::Npm).is_err()
    {
        eprintln!("N-05: node/npm unavailable; skipping Node monitor integration test");
        return;
    }
    let root = unique_root("node-detector");
    write(
        &root.join("web/package.json"),
        r#"{"name":"web","scripts":{"dev":"node -e \"console.log('ready')\""}}"#,
    );
    std::fs::create_dir_all(root.join("web/node_modules")).unwrap();
    let mut conn = Connection::open_in_memory().unwrap();
    crate::db::init_db(&mut conn).unwrap();
    conn.execute(
        "INSERT INTO workspaces (name, path, created_at, updated_at) VALUES ('w', ?1, 't', 't')",
        [root.to_string_lossy().to_string()],
    )
    .unwrap();
    let workspace_id = conn.last_insert_rowid();
    create_config(
        &conn,
        &CreateRuntimeConfigRequest {
            workspace_id,
            config: RuntimeApplicationConfig {
                name: "web".into(),
                project: root.join("web").to_string_lossy().into_owned(),
                main_class: Some("unused".into()),
                kind: RuntimeKind::Node,
                node_script: Some("dev".into()),
                node_package_manager: Some("npm".into()),
                ..Default::default()
            },
        },
    )
    .unwrap();
    let db = Arc::new(Mutex::new(conn));
    let events = Arc::new(VecEventSink::default());
    let launcher = Arc::new(FakeLaunchRunner::new(vec![FakeLaunch {
        lines: vec![
            (OutputStream::Stdout, "Local: http://localhost:5173/".into()),
            (OutputStream::Stdout, "Network: http://192.168.1.20:5173/".into()),
            (OutputStream::Stdout, "Inspector: http://127.0.0.1:9229/".into()),
        ],
        behavior: FakeBehavior::StayAlive { on_terminate: Some(0) },
        ..Default::default()
    }]));
    let manager = test_manager(
        db,
        launcher,
        Arc::new(FakeMavenRunner::successful()),
        events.clone(),
        Duration::from_millis(20),
    );
    let info = manager
        .start(
            workspace_id,
            "web",
            StartOptions {
                start_grace: Duration::from_millis(50),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(info.status, LifecycleStatus::Running);
    // F-34 起 Running 在首条 localhost 行命中时立即翻转，后续端口（9229）由
    // monitor 线程异步落库——info 快照与 set_ports 存在调度竞态（CI 实测只读
    // 到 [5173]）。与 wait_for_status 同模式轮询等端口落定。
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let ports = manager.get_process(info.process_id).unwrap().unwrap().ports;
        if ports == vec![5173, 9229] {
            break;
        }
        assert!(Instant::now() < deadline, "ports did not settle, last {ports:?}");
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(info.run_strategy, Some(RunStrategy::NodeScript));
    assert!(info.command_preview.as_deref().unwrap().contains("npm"));
    manager.stop(info.process_id, None).unwrap();
    assert!(events.collected().iter().any(|event| matches!(
        event,
        RuntimeEvent::Ports { process_id, ports }
            if *process_id == info.process_id && ports == &vec![5173, 9229]
    )));
    let _ = std::fs::remove_dir_all(root);
}

// --------------------------------------------------------------
// R-11：构建/运行输出统一进日志引擎（脱敏落盘 + 聚合事件 + 回查）
// --------------------------------------------------------------

#[test]
fn build_and_run_output_flow_into_masked_log_session() {
    let fixture = maven_fixture("logpipe", false);
    // 敏感环境变量（工作区层）：五层合并环境 → 日志脱敏秘密值来源。
    {
        let conn = fixture.db.lock().unwrap();
        crate::runtime::set_workspace_environment(
            &conn,
            fixture.workspace_id,
            std::collections::BTreeMap::from([("DB_PASSWORD".to_string(), "topsecret-value".to_string())]),
        )
        .unwrap();
    }
    let events = Arc::new(VecEventSink::default());
    let logs = Arc::new(RuntimeLogEngine::new());
    let maven = Arc::new(FakeMavenRunner::new(vec![
        FakeRun {
            lines: vec![(OutputStream::Stdout, "[INFO] BUILD SUCCESS".into())],
            ..Default::default()
        },
        FakeRun {
            output_file_content: Some(String::new()),
            ..Default::default()
        },
    ]));
    let launcher = Arc::new(FakeLaunchRunner::new(vec![FakeLaunch {
        lines: vec![
            (
                OutputStream::Stdout,
                "2026-08-23 12:00:00.123  INFO 1 --- [main] c.e.App : connecting with topsecret-value".into(),
            ),
            (OutputStream::Stdout, TOMCAT.into()),
            (OutputStream::Stdout, BANNER.into()),
        ],
        behavior: FakeBehavior::StayAlive { on_terminate: Some(0) },
        ..Default::default()
    }]));
    let manager = Arc::new(RuntimeProcessManager::with_deps(
        fixture.db.clone(),
        RuntimeProcessDeps {
            launch_runner: launcher,
            maven_runner: maven,
            events: events.clone(),
            logs: logs.clone(),
            sample_interval: Duration::from_millis(50),
            ..Default::default()
        },
    ));

    let info = manager
        .start(fixture.workspace_id, "app", StartOptions::default())
        .unwrap();
    assert_eq!(info.status, LifecycleStatus::Running);
    let stopped = manager.stop(info.process_id, None).unwrap();
    assert_eq!(stopped.status, LifecycleStatus::Stopped);
    assert_eq!(stopped.ports, vec![8080], "端口探测不受日志接管影响");

    // 落盘：构建 + 运行输出在同一文件，全程脱敏（磁盘无明文 secret）。
    let log_file = fixture
        .root
        .join(".gitworkspace/logs/app")
        .join(format!("{}.log", info.process_id));
    let on_disk = std::fs::read_to_string(&log_file).unwrap();
    assert!(on_disk.contains("[INFO] BUILD SUCCESS"), "构建输出进同一日志");
    assert!(on_disk.contains("Started Application"), "运行输出落盘");
    assert!(!on_disk.contains("topsecret-value"), "磁盘上不得有明文 secret");

    // 聚合事件：Build / Run 两阶段都经 RuntimeEvent::Logs 推送且已脱敏。
    let log_lines: Vec<_> = events
        .collected()
        .into_iter()
        .flat_map(|event| match event {
            RuntimeEvent::Logs { process_id, lines, .. } if process_id == info.process_id => lines,
            _ => Vec::new(),
        })
        .collect();
    assert!(log_lines.iter().any(|l| l.phase == LogPhase::Build));
    assert!(log_lines.iter().any(|l| l.phase == LogPhase::Run));
    assert!(log_lines.iter().all(|l| !l.line.contains("topsecret-value")));
    assert!(log_lines
        .iter()
        .any(|l| l.level == Some(crate::runtime::logs::LogLevel::Info)));

    // 进程结束后日志完整保留、可回查（R-11 验收标准）。
    let entries = logs
        .search(
            &fixture.root,
            "app",
            info.process_id,
            &crate::runtime::logs::LogFilter::default(),
        )
        .unwrap();
    assert_eq!(entries.len(), 4, "构建 1 行 + 运行 3 行全部可回查");
    let _ = std::fs::remove_dir_all(&fixture.root);
}

// --------------------------------------------------------------
// 构建失败 → Failed + BuildFailed 结构化错误
// --------------------------------------------------------------

#[test]
fn build_failure_marks_row_failed_and_returns_structured_error() {
    let fixture = maven_fixture("buildfail", false);
    let events = Arc::new(VecEventSink::default());
    let maven = Arc::new(FakeMavenRunner::new(vec![FakeRun {
        lines: vec![(OutputStream::Stderr, "[ERROR] COMPILATION ERROR".into())],
        exit_code: Some(1),
        ..Default::default()
    }]));
    let launcher = Arc::new(FakeLaunchRunner::staying_alive());
    let manager = test_manager(
        fixture.db.clone(),
        launcher,
        maven,
        events.clone(),
        Duration::from_millis(50),
    );

    let error = manager
        .start(fixture.workspace_id, "app", StartOptions::default())
        .unwrap_err();
    assert_eq!(error.code(), "BuildFailed");

    let rows = manager.list_processes(fixture.workspace_id).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].status, LifecycleStatus::Failed);
    use LifecycleStatus::*;
    assert_eq!(
        lifecycle_chain(&events, rows[0].process_id),
        vec![
            (Created, Preparing),
            (Preparing, Resolving),
            (Resolving, Building),
            (Building, Failed)
        ]
    );
    let _ = std::fs::remove_dir_all(&fixture.root);
}

// --------------------------------------------------------------
// Running 后崩溃 → Failed + 退出码（验收标准 4）
// --------------------------------------------------------------

#[test]
fn crash_after_running_marks_failed_with_exit_code() {
    let fixture = mini_fixture("crash");
    let events = Arc::new(VecEventSink::default());
    let launcher = Arc::new(FakeLaunchRunner::new(vec![FakeLaunch {
        lines: vec![(OutputStream::Stdout, BANNER.into())],
        behavior: FakeBehavior::Exit(Some(1)),
        delay_after_lines: Some(Duration::from_millis(300)),
    }]));
    let manager = test_manager(
        fixture.db.clone(),
        launcher,
        Arc::new(FakeMavenRunner::successful()),
        events.clone(),
        Duration::from_millis(50),
    );
    manager.seed_cached_launch(
        fixture.workspace_id,
        "app",
        crate::runtime::build::LaunchPlan::JavaJar {
            java_exec: PathBuf::from("java"),
            jar_path: PathBuf::from("/ws/app.jar"),
            vm_options: vec![],
            program_arguments: vec![],
            env: vec![],
            working_dir: fixture.root.clone(),
            preview: "java -jar app.jar".into(),
        },
        RunStrategy::PackageRun,
    );

    let info = manager
        .start(
            fixture.workspace_id,
            "app",
            StartOptions {
                skip_build: true,
                start_grace: Duration::from_secs(2),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(info.status, LifecycleStatus::Running);

    let failed = wait_for_status(
        &manager,
        info.process_id,
        LifecycleStatus::Failed,
        Duration::from_secs(3),
    );
    assert_eq!(failed.exit_code, Some(1));
    assert!(events.collected().iter().any(|e| matches!(
        e,
        RuntimeEvent::Exited { process_id, crashed: true, .. } if *process_id == info.process_id
    )));
    let _ = std::fs::remove_dir_all(&fixture.root);
}

// --------------------------------------------------------------
// 启动宽限期内退出 → ProcessStartFailed（可行动错误）
// --------------------------------------------------------------

#[test]
fn early_exit_maps_to_process_start_failed() {
    let fixture = mini_fixture("early");
    let launcher = Arc::new(FakeLaunchRunner::new(vec![FakeLaunch {
        lines: vec![],
        behavior: FakeBehavior::Exit(Some(2)),
        ..Default::default()
    }]));
    let manager = test_manager(
        fixture.db.clone(),
        launcher,
        Arc::new(FakeMavenRunner::successful()),
        Arc::new(VecEventSink::default()),
        Duration::from_millis(50),
    );
    manager.seed_cached_launch(
        fixture.workspace_id,
        "app",
        crate::runtime::build::LaunchPlan::JavaJar {
            java_exec: PathBuf::from("java"),
            jar_path: fixture.root.join("app.jar"),
            vm_options: vec![],
            program_arguments: vec![],
            env: vec![],
            working_dir: fixture.root.clone(),
            preview: "java -jar app.jar".into(),
        },
        RunStrategy::PackageRun,
    );

    let error = manager
        .start(
            fixture.workspace_id,
            "app",
            StartOptions {
                skip_build: true,
                ..Default::default()
            },
        )
        .unwrap_err();
    assert_eq!(error.code(), "ProcessStartFailed");
    assert!(error.to_string().contains("启动宽限期内退出"));
    let rows = manager.list_processes(fixture.workspace_id).unwrap();
    assert_eq!(rows[0].status, LifecycleStatus::Failed);
    assert_eq!(rows[0].exit_code, Some(2));
    let _ = std::fs::remove_dir_all(&fixture.root);
}

// --------------------------------------------------------------
// 重复启动守卫
// --------------------------------------------------------------

#[test]
fn duplicate_start_is_rejected_with_conflict() {
    let fixture = mini_fixture("dup");
    let launcher = Arc::new(FakeLaunchRunner::staying_alive());
    let manager = test_manager(
        fixture.db.clone(),
        launcher,
        Arc::new(FakeMavenRunner::successful()),
        Arc::new(VecEventSink::default()),
        Duration::from_millis(50),
    );
    manager.seed_cached_launch(
        fixture.workspace_id,
        "app",
        crate::runtime::build::LaunchPlan::JavaJar {
            java_exec: PathBuf::from("java"),
            jar_path: fixture.root.join("app.jar"),
            vm_options: vec![],
            program_arguments: vec![],
            env: vec![],
            working_dir: fixture.root.clone(),
            preview: "java -jar app.jar".into(),
        },
        RunStrategy::PackageRun,
    );

    let first = manager
        .start(
            fixture.workspace_id,
            "app",
            StartOptions {
                skip_build: true,
                start_grace: Duration::from_millis(200),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(first.status, LifecycleStatus::Running);

    let error = manager
        .start(fixture.workspace_id, "app", StartOptions::default())
        .unwrap_err();
    assert_eq!(error.code(), "ConflictError");
    assert!(error.to_string().contains("Restart"));

    manager.stop(first.process_id, None).unwrap();
    let _ = std::fs::remove_dir_all(&fixture.root);
}

// --------------------------------------------------------------
// Restart = Stop + Start 且复用最近构建产物（验收标准 2）
// --------------------------------------------------------------

#[test]
fn restart_reuses_cached_artifacts_without_rebuilding() {
    let fixture = maven_fixture("restart", false);
    let events = Arc::new(VecEventSink::default());
    let maven = Arc::new(FakeMavenRunner::successful());
    let launcher = Arc::new(FakeLaunchRunner::staying_alive());
    let manager = test_manager(
        fixture.db.clone(),
        launcher.clone(),
        maven.clone(),
        events.clone(),
        Duration::from_millis(50),
    );

    let options = || StartOptions {
        build_options: BuildOptions {
            strategy: Some(RunStrategy::MavenRun),
            ..Default::default()
        },
        start_grace: Duration::from_millis(200),
        ..Default::default()
    };
    let first = manager.start(fixture.workspace_id, "app", options()).unwrap();
    assert_eq!(first.status, LifecycleStatus::Running);
    assert_eq!(maven.request_count(), 1, "首次 start 构建一次");

    let second = manager.restart(fixture.workspace_id, "app", options()).unwrap();
    assert_eq!(second.status, LifecycleStatus::Running);
    assert_ne!(second.process_id, first.process_id, "restart 建新行");
    assert_eq!(maven.request_count(), 1, "restart 复用缓存产物，不再调 Maven");
    // skip-build 路径：Preparing 直达 Starting。
    use LifecycleStatus::*;
    let chain = lifecycle_chain(&events, second.process_id);
    assert_eq!(
        chain[..3],
        [(Created, Preparing), (Preparing, Starting), (Starting, Running)]
    );

    let first_row = manager.get_process(first.process_id).unwrap().unwrap();
    assert_eq!(first_row.status, LifecycleStatus::Stopped);
    manager.stop(second.process_id, None).unwrap();
    let _ = std::fs::remove_dir_all(&fixture.root);
}

// --------------------------------------------------------------
// R-06 mainClass 回退：配置缺省时自动推断进 LaunchPlan
// --------------------------------------------------------------

#[test]
fn missing_main_class_is_inferred_via_spring_boot_detection() {
    let fixture = maven_fixture("infer", true);
    // ClasspathRun 两次 Maven 调用：compile + dependency:build-classpath（写出缓存文件）。
    let maven = Arc::new(FakeMavenRunner::new(vec![
        FakeRun {
            lines: vec![(OutputStream::Stdout, "[INFO] BUILD SUCCESS".into())],
            ..Default::default()
        },
        FakeRun {
            output_file_content: Some("/m2/spring-boot-starter.jar".into()),
            ..Default::default()
        },
    ]));
    let launcher = Arc::new(FakeLaunchRunner::new(vec![FakeLaunch {
        lines: vec![(OutputStream::Stdout, BANNER.into())],
        behavior: FakeBehavior::StayAlive { on_terminate: Some(0) },
        ..Default::default()
    }]));
    let manager = test_manager(
        fixture.db.clone(),
        launcher.clone(),
        maven,
        Arc::new(VecEventSink::default()),
        Duration::from_millis(50),
    );

    let info = manager
        .start(
            fixture.workspace_id,
            "app",
            StartOptions {
                start_grace: Duration::from_millis(200),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(info.status, LifecycleStatus::Running);
    let preview = info.command_preview.unwrap();
    assert!(
        preview.contains("com.example.app.Application"),
        "推断的 mainClass 应进入启动命令预览: {preview}"
    );
    manager.stop(info.process_id, None).unwrap();
    let _ = std::fs::remove_dir_all(&fixture.root);
}

// --------------------------------------------------------------
// R-04 遗留验收：项目绑定 JDK 后，启动实际使用该 JDK 的 java
// --------------------------------------------------------------

#[test]
fn bound_jdk_is_used_for_launch_command() {
    let fixture = maven_fixture("jdkbind", false);
    {
        let conn = fixture.db.lock().unwrap();
        let mut jdk =
            crate::java::model::JdkInstallation::new("/jdk-21", crate::java::model::JdkDiscoverySource::System);
        jdk.major_version = Some(21);
        jdk.is_valid = true;
        crate::java::registry::upsert_jdk(&conn, &jdk).unwrap();
        let mut config = crate::runtime::config::load_config_unredacted(&conn, fixture.workspace_id, "app").unwrap();
        config.jdk = Some("21".into());
        crate::runtime::config::update_config(
            &conn,
            &crate::runtime::config::UpdateRuntimeConfigRequest {
                workspace_id: fixture.workspace_id,
                name: "app".into(),
                config,
            },
        )
        .unwrap();
    }
    let maven = Arc::new(FakeMavenRunner::new(vec![
        FakeRun {
            lines: vec![(OutputStream::Stdout, "[INFO] BUILD SUCCESS".into())],
            ..Default::default()
        },
        FakeRun {
            output_file_content: Some(String::new()),
            ..Default::default()
        },
    ]));
    let launcher = Arc::new(FakeLaunchRunner::new(vec![FakeLaunch {
        lines: vec![(OutputStream::Stdout, BANNER.into())],
        behavior: FakeBehavior::StayAlive { on_terminate: Some(0) },
        ..Default::default()
    }]));
    let manager = test_manager(
        fixture.db.clone(),
        launcher.clone(),
        maven,
        Arc::new(VecEventSink::default()),
        Duration::from_millis(50),
    );

    let info = manager
        .start(
            fixture.workspace_id,
            "app",
            StartOptions {
                start_grace: Duration::from_millis(200),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(info.status, LifecycleStatus::Running);
    let preview = info.command_preview.unwrap();
    assert!(
        preview.replace('\\', "/").starts_with("/jdk-21/bin/java"),
        "绑定 JDK 的 java 可执行路径应出现在启动命令开头（Windows 分隔符不敏感）: {preview}"
    );

    manager.stop(info.process_id, None).unwrap();
    let _ = std::fs::remove_dir_all(&fixture.root);
}

#[test]
fn classify_exit_table() {
    use LifecycleStatus::*;
    let clean = MonitorOutcome {
        exit_code: Some(0),
        cancelled: false,
        spawn_error: None,
    };
    let crash = MonitorOutcome {
        exit_code: Some(137),
        cancelled: false,
        spawn_error: None,
    };
    let signaled = MonitorOutcome {
        exit_code: None,
        cancelled: true,
        spawn_error: None,
    };
    let spawn_fail = MonitorOutcome {
        exit_code: None,
        cancelled: false,
        spawn_error: Some("io".into()),
    };

    assert_eq!(classify_exit(Running, &clean, false), (Stopped, false));
    assert_eq!(classify_exit(Running, &crash, false), (Failed, true));
    assert_eq!(classify_exit(Starting, &crash, false), (Failed, true));
    assert_eq!(classify_exit(Stopping, &crash, false), (Stopped, false));
    assert_eq!(classify_exit(Running, &signaled, false), (Failed, true));
    assert_eq!(
        classify_exit(Running, &signaled, true),
        (Stopped, false),
        "adopted 无码宽容"
    );
    assert_eq!(classify_exit(Starting, &spawn_fail, false), (Failed, false));
}

// --------------------------------------------------------------
// 真实进程（unix）：Force Kill / 优雅升级 / 孤儿接管 / 指标
// --------------------------------------------------------------

#[cfg(unix)]
mod real_process {
    use super::*;
    use crate::process::{process_alive, process_start_time};

    /// `sh -c <script>` 的 MavenGoal 计划（经 R-09 executor 组命令）。
    fn sh_plan(script: &str, working_dir: &Path) -> crate::runtime::build::LaunchPlan {
        crate::runtime::build::LaunchPlan::MavenGoal {
            request: crate::maven::exec_model::MavenExecutionRequest {
                working_dir: working_dir.to_path_buf(),
                executable: "sh".into(),
                goals: vec!["-c".into(), script.into()],
                extra_args: vec![],
                via_cmd_c: false,
                local_repository: None,
            },
            env: vec![],
            preview: format!("sh -c {script}"),
        }
    }

    fn real_manager(
        fixture: &MiniFixture,
        events: Arc<VecEventSink>,
        sample_interval: Duration,
    ) -> Arc<RuntimeProcessManager> {
        test_manager(
            fixture.db.clone(),
            Arc::new(crate::runtime::launch::SystemLaunchRunner),
            Arc::new(FakeMavenRunner::successful()),
            events,
            sample_interval,
        )
    }

    fn start_sh(manager: &Arc<RuntimeProcessManager>, fixture: &MiniFixture, script: &str) -> RuntimeProcessInfo {
        manager.seed_cached_launch(
            fixture.workspace_id,
            "app",
            sh_plan(script, &fixture.root),
            RunStrategy::MavenRun,
        );
        manager
            .start(
                fixture.workspace_id,
                "app",
                StartOptions {
                    skip_build: true,
                    start_grace: Duration::from_millis(300),
                    ..Default::default()
                },
            )
            .unwrap()
    }

    #[test]
    fn force_kill_requires_confirmation_and_leaves_no_orphan() {
        let fixture = mini_fixture("fkill");
        let manager = real_manager(&fixture, Arc::new(VecEventSink::default()), Duration::from_millis(50));
        let info = start_sh(&manager, &fixture, "sleep 300 & wait");
        assert_eq!(info.status, LifecycleStatus::Running);

        // 未确认 → 拒绝（全局约束 §3 二次确认）。
        let error = manager.kill(info.process_id, false).unwrap_err();
        assert_eq!(error.code(), "PermissionError");

        let stopped = manager.kill(info.process_id, true).unwrap();
        assert_eq!(stopped.status, LifecycleStatus::Stopped);

        // 进程树无孤儿残留（验收标准 5）。
        std::thread::sleep(Duration::from_millis(300));
        let mut system = sysinfo::System::new_with_specifics(
            sysinfo::RefreshKind::new().with_processes(sysinfo::ProcessRefreshKind::new()),
        );
        system.refresh_processes();
        let survivors: Vec<_> = system
            .processes()
            .values()
            .filter(|p| p.name() == "sleep" && p.cmd().iter().any(|a| a == "300"))
            .collect();
        assert!(survivors.is_empty(), "sleep 300 must be killed: {survivors:?}");
        let _ = std::fs::remove_dir_all(&fixture.root);
    }

    #[test]
    fn stop_escalates_to_tree_kill_when_sigterm_is_ignored() {
        let fixture = mini_fixture("escalate");
        let manager = real_manager(&fixture, Arc::new(VecEventSink::default()), Duration::from_millis(50));
        // 忽略 SIGTERM 的进程：grace 超时后必须升级杀树。
        let info = start_sh(&manager, &fixture, "trap '' TERM; while true; do sleep 0.05; done");
        let pid = info.pid.unwrap();

        let stopped = manager.stop(info.process_id, Some(Duration::from_millis(500))).unwrap();
        assert_eq!(stopped.status, LifecycleStatus::Stopped);
        std::thread::sleep(Duration::from_millis(200));
        assert!(
            !process_alive(pid, None),
            "SIGTERM-ignoring process must be tree-killed"
        );
        let _ = std::fs::remove_dir_all(&fixture.root);
    }

    /// F-12 回归（unix 变体）：忽略 SIGTERM 且两路输出均含非法 UTF-8
    /// （reader 全死、channel 断开）的进程——grace 升级时置位的
    /// force_kill 曾因 monitor 阻塞在 child.wait() 而无人消费。
    #[test]
    fn stop_kills_sigterm_ignoring_process_that_closed_streams() {
        let fixture = mini_fixture("f12unix");
        let manager = real_manager(&fixture, Arc::new(VecEventSink::default()), Duration::from_millis(50));
        let info = start_sh(
            &manager,
            &fixture,
            "trap '' TERM; printf '\\377\\376\\n'; printf '\\377\\376\\n' >&2; sleep 300",
        );
        let pid = info.pid.unwrap();

        let stopped = manager.stop(info.process_id, Some(Duration::from_millis(500))).unwrap();
        std::thread::sleep(Duration::from_millis(200));
        let alive = process_alive(pid, None);
        if alive {
            crate::process::kill_tree::kill_process_tree(pid);
        }
        assert_eq!(stopped.status, LifecycleStatus::Stopped);
        assert!(!alive, "F-12: reader 断开后升级杀树也必须生效");
        let _ = std::fs::remove_dir_all(&fixture.root);
    }

    #[test]
    fn graceful_stop_uses_sigterm_before_any_kill() {
        let fixture = mini_fixture("graceful");
        let events = Arc::new(VecEventSink::default());
        let manager = real_manager(&fixture, events.clone(), Duration::from_millis(50));
        // trap TERM → 记录并 exit 0：若先收到 SIGTERM 则优雅退出码 0。
        let info = start_sh(&manager, &fixture, "trap 'exit 0' TERM; while true; do sleep 0.1; done");

        let stopped = manager.stop(info.process_id, None).unwrap();
        assert_eq!(stopped.status, LifecycleStatus::Stopped);
        assert_eq!(stopped.exit_code, Some(0), "SIGTERM 触发 trap 优雅退出");
        assert!(events.collected().iter().any(|e| matches!(
            e,
            RuntimeEvent::Exited { process_id, crashed: false, .. } if *process_id == info.process_id
        )));
        let _ = std::fs::remove_dir_all(&fixture.root);
    }

    #[test]
    fn reconcile_adopts_live_orphan_and_fails_gone_rows() {
        let fixture = mini_fixture("orphan");
        // 会话 A：启动真实 sleep 后「崩溃」（drop manager，不 stop）。
        let manager_a = real_manager(&fixture, Arc::new(VecEventSink::default()), Duration::from_secs(3600));
        let info = start_sh(&manager_a, &fixture, "sleep 300");
        let pid = info.pid.unwrap();
        let pid_start = process_start_time(pid).unwrap();
        assert!(process_alive(pid, Some(pid_start)));
        drop(manager_a);

        // 同库补两类遗留行：死进程 Running 行 + 从未 spawn 的 Created 行。
        let (dead_id, created_id) = {
            let conn = fixture.db.lock().unwrap();
            let dead = store::insert_process(&conn, fixture.workspace_id, "dead-app").unwrap();
            for status in [
                LifecycleStatus::Preparing,
                LifecycleStatus::Resolving,
                LifecycleStatus::Building,
                LifecycleStatus::Starting,
                LifecycleStatus::Running,
            ] {
                store::transition_status(&conn, dead, status, None).unwrap();
            }
            store::set_pid(&conn, dead, 4_000_000, Some(1)).unwrap();
            let created = store::insert_process(&conn, fixture.workspace_id, "half-app").unwrap();
            (dead, created)
        };

        // 会话 B：reconcile 接管。
        let manager_b = real_manager(&fixture, Arc::new(VecEventSink::default()), Duration::from_secs(3600));
        let adopted = manager_b.reconcile_on_startup(fixture.workspace_id).unwrap();
        assert_eq!(adopted.len(), 1);
        assert_eq!(adopted[0].process_id, info.process_id);
        assert!(adopted[0].adopted);
        assert_eq!(adopted[0].status, LifecycleStatus::Running);

        let dead = manager_b.get_process(dead_id).unwrap().unwrap();
        assert_eq!(dead.status, LifecycleStatus::Failed);
        let created = manager_b.get_process(created_id).unwrap().unwrap();
        assert_eq!(created.status, LifecycleStatus::Failed);

        // 接管后可正常 Stop（SIGTERM 杀 sleep）。
        let stopped = manager_b.stop(info.process_id, Some(Duration::from_secs(5))).unwrap();
        assert_eq!(stopped.status, LifecycleStatus::Stopped);
        std::thread::sleep(Duration::from_millis(200));
        assert!(!process_alive(pid, Some(pid_start)), "adopted orphan must be stopped");
        let _ = std::fs::remove_dir_all(&fixture.root);
    }

    #[test]
    fn sampler_emits_metrics_for_live_process() {
        let fixture = mini_fixture("metrics");
        let events = Arc::new(VecEventSink::default());
        let manager = real_manager(&fixture, events.clone(), Duration::from_millis(30));
        let info = start_sh(&manager, &fixture, "sleep 60");

        std::thread::sleep(Duration::from_millis(250));
        let collected = events.collected();
        let metrics: Vec<_> = collected
            .iter()
            .filter(|e| matches!(e, RuntimeEvent::Metrics { process_id, .. } if *process_id == info.process_id))
            .collect();
        assert!(!metrics.is_empty(), "sampler must emit metrics events");
        if let RuntimeEvent::Metrics { memory_bytes, .. } = metrics[0] {
            assert!(*memory_bytes > 0);
        }

        manager.stop(info.process_id, None).unwrap();
        let _ = std::fs::remove_dir_all(&fixture.root);
    }
}

// --------------------------------------------------------------
// 真实进程（windows）：F-12 回归——reader 断开后 Stop 仍须杀树。
// Windows 无 SIGTERM（terminate 恒 false），停止全押在 force_kill
// 链路上，是本缺陷的必现平台。
// --------------------------------------------------------------

#[cfg(windows)]
mod real_process_windows {
    use super::*;
    use crate::process::process_alive;

    /// powershell 向 stdout/stderr 各写一段非法 UTF-8 后长驻：两个 reader
    /// 死亡曾令 monitor 阻塞在 child.wait()，force_kill 无人消费
    /// （F-12 复现路径，等价于 hussar JVM 的 GBK 中文日志输出）。
    fn gbk_output_plan(working_dir: &Path) -> crate::runtime::build::LaunchPlan {
        crate::runtime::build::LaunchPlan::MavenGoal {
            request: crate::maven::exec_model::MavenExecutionRequest {
                working_dir: working_dir.to_path_buf(),
                executable: "powershell".into(),
                goals: vec![
                    "-NoProfile".into(),
                    "-Command".into(),
                    "$b=[byte[]](255,254,10); \
                         [Console]::OpenStandardOutput().Write($b,0,3); \
                         [Console]::OpenStandardError().Write($b,0,3); \
                         Start-Sleep -Seconds 300"
                        .into(),
                ],
                extra_args: vec![],
                via_cmd_c: false,
                local_repository: None,
            },
            env: vec![],
            preview: "powershell invalid-utf8 sleep".into(),
        }
    }

    #[test]
    fn stop_kills_process_whose_output_streams_closed_early() {
        let fixture = mini_fixture("f12win");
        let manager = test_manager(
            fixture.db.clone(),
            Arc::new(crate::runtime::launch::SystemLaunchRunner),
            Arc::new(FakeMavenRunner::successful()),
            Arc::new(VecEventSink::default()),
            Duration::from_millis(50),
        );
        manager.seed_cached_launch(
            fixture.workspace_id,
            "app",
            gbk_output_plan(&fixture.root),
            RunStrategy::MavenRun,
        );
        // 非法字节杀死 reader → 等不到横幅，start_grace 到期按存活判 Running。
        let info = manager
            .start(
                fixture.workspace_id,
                "app",
                StartOptions {
                    skip_build: true,
                    start_grace: Duration::from_secs(3),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(info.status, LifecycleStatus::Running);
        let pid = info.pid.expect("spawn 后应有 pid");

        let stopped = manager.stop(info.process_id, Some(Duration::from_secs(2))).unwrap();
        std::thread::sleep(Duration::from_millis(300));
        let alive = process_alive(pid, None);
        if alive {
            crate::process::kill_tree::kill_process_tree(pid);
        }
        assert_eq!(stopped.status, LifecycleStatus::Stopped);
        assert!(!alive, "F-12: stop 后进程必须真实消失，不得残留孤儿");
        let _ = std::fs::remove_dir_all(&fixture.root);
    }
}

// --------------------------------------------------------------
// 真实 Maven + 真实 JVM 集成测试（验收标准 1/4 的端到端口径）。
// 需要 PATH 上的 `mvn`（自带 JDK）；缺失时跳过并标注。首次运行
// 会联网拉依赖（R-09 测试同款），属预期。
// --------------------------------------------------------------

mod real_maven {
    use super::*;

    const SPRING_BOOT_VERSION: &str = "3.2.5";
    const INTEGRATION_TIMEOUT: Duration = Duration::from_secs(600);

    fn maven_available() -> bool {
        let maven = if cfg!(windows) { "mvn.cmd" } else { "mvn" };
        std::process::Command::new(maven).arg("-version").output().is_ok()
    }

    fn parent_pom() -> String {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<project xmlns="http://maven.apache.org/POM/4.0.0">
  <modelVersion>4.0.0</modelVersion>
  <groupId>com.r10</groupId>
  <artifactId>r10-parent</artifactId>
  <version>1.0.0</version>
  <packaging>pom</packaging>
  <modules><module>app</module></modules>
  <properties>
    <maven.compiler.source>17</maven.compiler.source>
    <maven.compiler.target>17</maven.compiler.target>
    <project.build.sourceEncoding>UTF-8</project.build.sourceEncoding>
  </properties>
  <dependencyManagement>
    <dependencies>
      <dependency>
        <groupId>org.springframework.boot</groupId>
        <artifactId>spring-boot-dependencies</artifactId>
        <version>{SPRING_BOOT_VERSION}</version>
        <type>pom</type>
        <scope>import</scope>
      </dependency>
    </dependencies>
  </dependencyManagement>
</project>
"#
        )
    }

    fn app_pom() -> String {
        r#"<?xml version="1.0" encoding="UTF-8"?>
<project xmlns="http://maven.apache.org/POM/4.0.0">
  <modelVersion>4.0.0</modelVersion>
  <parent>
    <groupId>com.r10</groupId>
    <artifactId>r10-parent</artifactId>
    <version>1.0.0</version>
  </parent>
  <artifactId>app</artifactId>
  <dependencies>
    <dependency>
      <groupId>org.springframework.boot</groupId>
      <artifactId>spring-boot-starter-web</artifactId>
    </dependency>
  </dependencies>
</project>
"#
        .to_string()
    }

    /// 单仓 parent + app（spring-boot-starter-web 驻留应用）fixture。
    fn boot_fixture(
        name: &str,
        program_arguments: &[&str],
        vm_options: &[&str],
    ) -> (PathBuf, Arc<Mutex<Connection>>, i64) {
        let root = unique_root(name);
        std::fs::create_dir_all(&root).unwrap();
        write(&root.join("repo/pom.xml"), &parent_pom());
        write(&root.join("repo/app/pom.xml"), &app_pom());
        write(
            &root.join("repo/app/src/main/java/com/r10/app/Application.java"),
            "package com.r10.app;\n\n\
                 import org.springframework.boot.SpringApplication;\n\
                 import org.springframework.boot.autoconfigure.SpringBootApplication;\n\n\
                 @SpringBootApplication\n\
                 public class Application {\n    public static void main(String[] args) {\n        SpringApplication.run(Application.class, args);\n    }\n}\n",
        );
        git2::Repository::init(root.join("repo")).unwrap();

        let mut conn = Connection::open_in_memory().unwrap();
        crate::db::init_db(&mut conn).unwrap();
        conn.execute(
            "INSERT INTO workspaces (name, path, created_at, updated_at) VALUES ('w', ?1, 't', 't')",
            [root.to_string_lossy().to_string()],
        )
        .unwrap();
        let workspace_id = conn.last_insert_rowid();
        crate::db::dao::upsert_repositories_batch(
            &mut conn,
            workspace_id,
            &[crate::models::repository::ScannedRepo {
                path: root.join("repo").to_string_lossy().to_string(),
                name: "repo".into(),
                relative_path: "repo".into(),
                git_dir_mtime: None,
            }],
        )
        .unwrap();
        let discovery = crate::maven::discover_poms(&root, 5, None, None);
        assert!(discovery.errors.is_empty(), "{:?}", discovery.errors);
        crate::maven::sync_workspace_index(&mut conn, workspace_id, &discovery, &root.join("m2")).unwrap();
        // 真实集成测试前提：JDK 17+（Spring Boot 3.2.5 class 61）。把
        // 当前 JAVA_HOME 注册为 JDK 并绑定到配置——启动 JVM 与构建
        // 同源，避免「构建 17、运行 8」的版本错配（R-14 修复）。
        let bound_jdk = std::env::var("JAVA_HOME")
            .ok()
            .filter(|home| !home.is_empty())
            .inspect(|home| {
                let mut jdk = crate::java::model::JdkInstallation::new(
                    home.clone(),
                    crate::java::model::JdkDiscoverySource::System,
                );
                jdk.is_valid = true;
                crate::java::registry::upsert_jdk(&conn, &jdk).unwrap();
            });
        create_config(
            &conn,
            &CreateRuntimeConfigRequest {
                workspace_id,
                config: RuntimeApplicationConfig {
                    name: "app".into(),
                    project: root.join("repo/app/pom.xml").to_string_lossy().to_string(),
                    main_class: Some("com.r10.app.Application".into()),
                    jdk: bound_jdk.clone(),
                    program_arguments: program_arguments.iter().map(|arg| arg.to_string()).collect(),
                    vm_options: vm_options.iter().map(|opt| opt.to_string()).collect(),
                    ..Default::default()
                },
            },
        )
        .unwrap();
        (root, Arc::new(Mutex::new(conn)), workspace_id)
    }

    fn real_manager(db: Arc<Mutex<Connection>>, events: Arc<VecEventSink>) -> Arc<RuntimeProcessManager> {
        Arc::new(RuntimeProcessManager::with_deps(
            db,
            RuntimeProcessDeps {
                events,
                ..Default::default()
            },
        ))
    }

    fn classpath_options() -> StartOptions {
        StartOptions {
            build_options: BuildOptions {
                strategy: Some(RunStrategy::ClasspathRun),
                timeout: Some(INTEGRATION_TIMEOUT),
                ..Default::default()
            },
            // 真实 JVM + Spring 上下文的启动远慢于 fake；横幅命中会提前返回。
            start_grace: Duration::from_secs(120),
            ..Default::default()
        }
    }

    /// 验收标准 1 端到端：真实 Spring Boot 应用 Start → Running（横幅）
    /// → 端口探测 → Stop → Stopped，JVM 不残留。
    #[test]
    fn classpath_run_full_cycle_with_real_spring_boot_app() {
        if !maven_available() {
            eprintln!("R-10: no `mvn` on PATH; skipping real spring boot start test");
            return;
        }
        let (root, db, workspace_id) = boot_fixture("bootcycle", &["--server.port=0"], &[]);
        let events = Arc::new(VecEventSink::default());
        let manager = real_manager(db.clone(), events.clone());

        let info = manager
            .start(workspace_id, "app", classpath_options())
            .unwrap_or_else(|error| panic!("real start failed: {error}"));
        assert_eq!(info.status, LifecycleStatus::Running);
        let pid = info.pid.expect("real process must have a pid");

        // 端口来自启动日志正则（--server.port=0 → 随机端口）；Tomcat 端口
        // 行先于横幅，但 monitor 写库是异步的，这里轮询兜底。
        let deadline = Instant::now() + Duration::from_secs(10);
        let ports = loop {
            let ports = manager
                .get_process(info.process_id)
                .unwrap()
                .map(|row| row.ports)
                .unwrap_or_default();
            if !ports.is_empty() || Instant::now() > deadline {
                break ports;
            }
            std::thread::sleep(Duration::from_millis(200));
        };
        assert!(!ports.is_empty(), "启动日志应探测到随机端口");

        let stopped = manager.stop(info.process_id, Some(Duration::from_secs(30))).unwrap();
        assert_eq!(stopped.status, LifecycleStatus::Stopped);
        std::thread::sleep(Duration::from_millis(300));
        assert!(!crate::process::process_alive(pid, None), "stop 后 JVM 不应残留");

        use LifecycleStatus::*;
        assert_eq!(
            lifecycle_chain(&events, info.process_id),
            vec![
                (Created, Preparing),
                (Preparing, Resolving),
                (Resolving, Building),
                (Building, Starting),
                (Starting, Running),
                (Running, Stopping),
                (Stopping, Stopped),
            ]
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// F-04 端到端：「IDEA 启动」预设的 VM options 不影响真实启动——
    /// 用预设参数把 fixture Spring Boot 应用起到 Running 再停掉。
    /// 预设清单与前端 `src/config/launchPresets.ts` 保持一致（刻意排除
    /// idea_rt.jar javaagent 与 @arg_file 等 IDEA 私有项）。
    #[test]
    fn idea_preset_vm_options_boot_real_spring_boot_app() {
        if !maven_available() {
            eprintln!("F-04: no `mvn` on PATH; skipping IDEA preset boot test");
            return;
        }
        const IDEA_PRESET_VM_OPTIONS: &[&str] = &[
            "-XX:TieredStopAtLevel=1",
            "-Dspring.output.ansi.enabled=always",
            "-Dcom.sun.management.jmxremote",
            "-Dspring.jmx.enabled=true",
            "-Dspring.liveBeansView.mbeanDomain",
            "-Dspring.application.admin.enabled=true",
            "-Dmanagement.endpoints.jmx.exposure.include=*",
            "-Dfile.encoding=UTF-8",
        ];
        let (root, db, workspace_id) = boot_fixture("bootpreset", &["--server.port=0"], IDEA_PRESET_VM_OPTIONS);
        let manager = real_manager(db.clone(), Arc::new(VecEventSink::default()));

        let info = manager
            .start(workspace_id, "app", classpath_options())
            .unwrap_or_else(|error| panic!("F-04 preset start failed: {error}"));
        assert_eq!(info.status, LifecycleStatus::Running);

        let stopped = manager.stop(info.process_id, Some(Duration::from_secs(30))).unwrap();
        assert_eq!(stopped.status, LifecycleStatus::Stopped);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 验收标准 4 端到端：非法端口 → 启动期退出 → ProcessStartFailed
    /// + 行落 Failed 且带非零退出码。
    #[test]
    fn invalid_port_crashes_during_startup_and_marks_failed() {
        if !maven_available() {
            eprintln!("R-10: no `mvn` on PATH; skipping crash integration test");
            return;
        }
        let (root, db, workspace_id) = boot_fixture("bootcrash", &["--server.port=99999"], &[]);
        let manager = real_manager(db.clone(), Arc::new(VecEventSink::default()));

        let error = manager.start(workspace_id, "app", classpath_options()).unwrap_err();
        assert_eq!(error.code(), "ProcessStartFailed");

        let row = store::list_processes(&db.lock().unwrap(), workspace_id)
            .unwrap()
            .into_iter()
            .next()
            .expect("one process row");
        assert_eq!(row.status, LifecycleStatus::Failed);
        assert!(row.exit_code.is_some_and(|code| code != 0));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// F-12 真实场景复测（manual，需显式 opt-in）：
    /// `cargo test manual_hussar_stop -- --ignored --nocapture`
    /// 依赖本机 release.2 工作区（不存在则跳过）。链路：完整 mvn 构建
    /// → ClasspathRun（F-11 pathing jar）→ Running → stop(15s) → JVM
    /// 必须真实消失。修复前此场景 stop 返回成功但 JVM 残留（GBK 日志
    /// 杀死输出 reader，monitor 阻塞 wait 无法消费 force_kill）。
    #[test]
    #[ignore = "manual: 依赖本机 release.2 工作区（F-12 真实场景复测）"]
    fn manual_hussar_base_web_stop_kills_jvm() {
        let env_root = Path::new(r"D:\AWork\Code\9.6.0-release.2\env");
        let app_dir = env_root.join("hussar-base-web");
        if !app_dir.join("pom.xml").exists() || !maven_available() {
            eprintln!("F-12 manual: release.2 工作区或 mvn 不存在，跳过");
            return;
        }

        let mut conn = Connection::open_in_memory().unwrap();
        crate::db::init_db(&mut conn).unwrap();
        conn.execute(
            "INSERT INTO workspaces (name, path, created_at, updated_at) VALUES ('w', ?1, 't', 't')",
            [env_root.to_string_lossy().to_string()],
        )
        .unwrap();
        let workspace_id = conn.last_insert_rowid();
        crate::db::dao::upsert_repositories_batch(
            &mut conn,
            workspace_id,
            &[crate::models::repository::ScannedRepo {
                path: app_dir.to_string_lossy().to_string(),
                name: "hussar-base-web".into(),
                relative_path: "hussar-base-web".into(),
                git_dir_mtime: None,
            }],
        )
        .unwrap();
        let discovery = crate::maven::discover_poms(env_root, 5, None, None);
        // 生产同源：Maven 原生 ~/.m2 本地仓库（§73）。
        let m2 = PathBuf::from(std::env::var("USERPROFILE").expect("USERPROFILE")).join(".m2");
        crate::maven::sync_workspace_index(&mut conn, workspace_id, &discovery, &m2).unwrap();

        // 构建与运行同源 JDK（R-14）：hussar 场景绑 JAVA_HOME（temurin-8）。
        let jdk_home = std::env::var("JAVA_HOME").expect("manual 测试需要 JAVA_HOME（temurin-8）");
        let mut jdk =
            crate::java::model::JdkInstallation::new(jdk_home.clone(), crate::java::model::JdkDiscoverySource::System);
        jdk.is_valid = true;
        crate::java::registry::upsert_jdk(&conn, &jdk).unwrap();

        create_config(
            &conn,
            &CreateRuntimeConfigRequest {
                workspace_id,
                config: RuntimeApplicationConfig {
                    name: "app".into(),
                    project: app_dir.join("pom.xml").to_string_lossy().to_string(),
                    // 缺省 → R-06 自动推断 mainClass（F-05 链路，复现原场景）。
                    main_class: None,
                    jdk: Some(jdk_home),
                    ..Default::default()
                },
            },
        )
        .unwrap();

        let manager = real_manager(Arc::new(Mutex::new(conn)), Arc::new(VecEventSink::default()));
        let info = manager
            .start(workspace_id, "app", classpath_options())
            .unwrap_or_else(|error| panic!("F-12 manual: start failed: {error}"));
        assert_eq!(info.status, LifecycleStatus::Running);
        let pid = info.pid.expect("Running 应有 pid");

        let stopped = manager.stop(info.process_id, Some(Duration::from_secs(15))).unwrap();
        assert_eq!(stopped.status, LifecycleStatus::Stopped);
        std::thread::sleep(Duration::from_millis(500));
        assert!(
            !crate::process::process_alive(pid, None),
            "F-12: stop 后 JVM 必须真实消失（pid={pid}）"
        );
    }
}

// --------------------------------------------------------------
// N-07 端到端验收：真实 Vite 模板工程全闭环
// --------------------------------------------------------------

mod real_node_vite {
    use super::*;
    use std::net::TcpStream;

    /// N-07 验收（设计文档 §8 第 1 条）：`npm create vite` 产物全闭环——
    /// 发现 → 建配置 → 启动 → 端口正确识别 → 日志流 → 停止 + 端口真实释放。
    /// 与 `real_maven` 的 Spring Boot 真实闭环对称；依赖真实 node/npm 与
    /// 网络（脚手架 + 依赖安装），探测不到即 skip 并打印原因。
    #[test]
    fn real_vite_project_full_loop_with_port_release() {
        if crate::node::detect_node().is_err()
            || crate::node::detect_package_manager(crate::node::PackageManager::Npm).is_err()
        {
            eprintln!("N-07: node/npm unavailable; skipping real Vite e2e");
            return;
        }
        if !npm_registry_reachable() {
            eprintln!("N-07: npm registry unreachable; skipping real Vite e2e (network required)");
            return;
        }
        // 测试内 spawn 同样遵守可执行检测硬规则：find_in_path 解析绝对路径，
        // 不用裸名兜底（AGENTS.md §2）。
        let npm = crate::node::detect_package_manager(crate::node::PackageManager::Npm)
            .expect("npm must resolve via find_in_path")
            .executable;
        let root = unique_root("n07-vite");
        std::fs::create_dir_all(&root).unwrap();
        let web = root.join("web");

        // 1. 真实 Vite 模板工程（spec 固定样例：npm create vite 产物）。
        let scaffold = std::process::Command::new(&npm)
            .args(["create", "vite@latest", "web", "--", "--template", "vanilla"])
            .current_dir(&root)
            .output()
            .expect("spawn npm create vite");
        assert!(
            scaffold.status.success(),
            "npm create vite failed: {}{}",
            String::from_utf8_lossy(&scaffold.stdout),
            String::from_utf8_lossy(&scaffold.stderr)
        );
        // 2. 依赖安装：仅测试行为（产品启动链路禁止自动 install，全局约束 §2）。
        let install = std::process::Command::new(&npm)
            .args(["install", "--no-audit", "--no-fund", "--loglevel=error"])
            .current_dir(&web)
            .output()
            .expect("spawn npm install");
        assert!(
            install.status.success(),
            "npm install failed: {}{}",
            String::from_utf8_lossy(&install.stdout),
            String::from_utf8_lossy(&install.stderr)
        );

        // 3. 发现：package.json 扫描 + V17 索引 + 列表查询。
        let mut conn = Connection::open_in_memory().unwrap();
        crate::db::init_db(&mut conn).unwrap();
        conn.execute(
            "INSERT INTO workspaces (name, path, created_at, updated_at) VALUES ('w', ?1, 't', 't')",
            [root.to_string_lossy().to_string()],
        )
        .unwrap();
        let workspace_id = conn.last_insert_rowid();
        let discovery = crate::node::discovery::discover_package_jsons(&root, 5, None, None);
        assert!(discovery.errors.is_empty(), "{:?}", discovery.errors);
        let projects = crate::node::discovery::sync_node_projects(&mut conn, workspace_id, &discovery).unwrap();
        let web_project = projects
            .iter()
            .find(|project| project.name == "web")
            .unwrap_or_else(|| panic!("vite project must be discovered, got {projects:?}"));
        assert!(
            web_project.scripts_json.contains("dev"),
            "dev script must be visible: {}",
            web_project.scripts_json
        );

        // 4. 建配置：kind=Node + node_script=dev。
        create_config(
            &conn,
            &CreateRuntimeConfigRequest {
                workspace_id,
                config: RuntimeApplicationConfig {
                    name: "web".into(),
                    project: web.to_string_lossy().into_owned(),
                    kind: RuntimeKind::Node,
                    node_script: Some("dev".into()),
                    node_package_manager: Some("npm".into()),
                    ..Default::default()
                },
            },
        )
        .unwrap();

        // 5. 启动：真实构建链（NodeBuildEngine → LaunchPlan::Script）+ 真实
        //    spawn（SystemLaunchRunner）。grace 放宽到 30s：vite 首次依赖
        //    预构建可能明显慢于默认 5s。
        let db = Arc::new(Mutex::new(conn));
        let events = Arc::new(VecEventSink::default());
        let manager = test_manager(
            db.clone(),
            Arc::new(crate::runtime::launch::SystemLaunchRunner),
            Arc::new(FakeMavenRunner::successful()),
            events.clone(),
            Duration::from_millis(50),
        );
        let info = manager
            .start(
                workspace_id,
                "web",
                StartOptions {
                    start_grace: Duration::from_secs(30),
                    ..Default::default()
                },
            )
            .unwrap_or_else(|error| panic!("real node start failed: {error}"));
        assert_eq!(info.status, LifecycleStatus::Running);
        assert_eq!(info.run_strategy, Some(RunStrategy::NodeScript));
        assert!(
            info.command_preview.as_deref().unwrap_or_default().contains("npm"),
            "preview should mention npm: {:?}",
            info.command_preview
        );
        // preview 落库可查（设计文档 §6/§8：command_preview 入 runtime_processes）。
        let persisted_preview: Option<String> = {
            let conn = db.lock().unwrap();
            conn.query_row(
                "SELECT command_preview FROM runtime_processes WHERE id = ?1",
                [info.process_id],
                |row| row.get(0),
            )
            .ok()
        };
        assert!(
            persisted_preview.as_deref().unwrap_or_default().contains("npm"),
            "command_preview must be persisted, got {persisted_preview:?}"
        );

        // 6. 端口正确识别：探测到的端口必须真实在监听（vite 写库异步，轮询兜底）。
        let deadline = Instant::now() + Duration::from_secs(30);
        let ports = loop {
            let ports = manager
                .get_process(info.process_id)
                .unwrap()
                .map(|row| row.ports)
                .unwrap_or_default();
            if !ports.is_empty() || Instant::now() > deadline {
                break ports;
            }
            std::thread::sleep(Duration::from_millis(200));
        };
        assert!(!ports.is_empty(), "启动日志应探测到 vite 端口");
        let port = ports[0];
        assert!(
            loopback_reachable(port),
            "探测端口 {port} 必须真实可连（IPv4 或 IPv6 环回，vite 8 仅绑 ::1）"
        );

        // 7. 日志流：真实 vite 输出经日志引擎（含脱敏）进入事件流。
        let log_deadline = Instant::now() + Duration::from_secs(10);
        let has_vite_log = loop {
            let hit = events.collected().iter().any(|event| match event {
                RuntimeEvent::Logs { lines, .. } => lines
                    .iter()
                    .any(|line| line.line.contains("VITE") || line.line.contains("Local:")),
                _ => false,
            });
            if hit || Instant::now() > log_deadline {
                break hit;
            }
            std::thread::sleep(Duration::from_millis(200));
        };
        assert!(has_vite_log, "日志流应包含真实 vite 输出（VITE 横幅 / Local: 行）");

        // 8. 停止 + 端口真实释放（§9：kill_tree 整树终止 npm → vite 孙子进程）。
        let stopped = manager.stop(info.process_id, Some(Duration::from_secs(15))).unwrap();
        assert_eq!(stopped.status, LifecycleStatus::Stopped);
        let release_deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let still_listening = loopback_reachable(port);
            if !still_listening {
                break;
            }
            assert!(
                Instant::now() < release_deadline,
                "Stop 后端口 {port} 必须真实释放（kill_tree 整树终止）"
            );
            std::thread::sleep(Duration::from_millis(200));
        }
        use LifecycleStatus::*;
        assert_eq!(
            lifecycle_chain(&events, info.process_id),
            vec![
                (Created, Preparing),
                (Preparing, Resolving),
                (Resolving, Building),
                (Building, Starting),
                (Starting, Running),
                (Running, Stopping),
                (Stopping, Stopped),
            ]
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// npm registry 可达性探测（DNS + TCP connect，3s 超时）。
    fn npm_registry_reachable() -> bool {
        use std::net::ToSocketAddrs;
        match ("registry.npmjs.org", 443).to_socket_addrs() {
            Ok(mut addrs) => match addrs.next() {
                Some(addr) => TcpStream::connect_timeout(&addr, Duration::from_secs(3)).is_ok(),
                None => false,
            },
            Err(_) => false,
        }
    }

    /// vite 8 起在 Node 17+ verbatim DNS 下仅绑定 `::1`（IPv6 环回），连接
    /// `127.0.0.1` 必然失败（F-32）。探测同时尝试两种环回族，任一可达即可。
    fn loopback_reachable(port: u16) -> bool {
        use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
        [
            SocketAddr::new(IpAddr::from(Ipv4Addr::LOCALHOST), port),
            SocketAddr::new(IpAddr::from(Ipv6Addr::LOCALHOST), port),
        ]
        .iter()
        .any(|addr| TcpStream::connect(addr).is_ok())
    }
}
