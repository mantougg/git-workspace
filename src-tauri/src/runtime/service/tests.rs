use super::*;
use crate::maven::{self, RuntimeScope};
use crate::task::runtime::RuntimeTaskHandler;
use crate::models::task::{RuntimeOp, RuntimeTaskOptions, TaskType};
use crate::process::streaming::{OutputStream, StreamingExit};
use crate::runtime::build::runner::{FakeMavenRunner, FakeRun};
use crate::runtime::build::{BuildOutputSink, RunStrategy};
use crate::runtime::config::{CreateRuntimeConfigRequest, RuntimeApplicationConfig};
use crate::runtime::events::{
    VecEmitter, EVENT_BUILD_COMPLETED, EVENT_BUILD_PROGRESS, EVENT_BUILD_STARTED,
    EVENT_DEPENDENCY_RESOLVED, EVENT_ENVIRONMENT_COMPLETED,
    EVENT_ENVIRONMENT_PROGRESS, EVENT_HEALTH_CHANGED, EVENT_PROCESS_STARTED,
    EVENT_PROCESS_STOPPED, EVENT_PROJECT_DISCOVERED, EVENT_RESTART_COMPLETED,
    EVENT_RESTART_STARTED,
};
use crate::runtime::launch::launcher::FakeLaunchRunner;
use crate::runtime::launch::LifecycleStatus;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::atomic::AtomicUsize;
use std::time::Instant;
use crate::test_support::write;
// --------------------------------------------------------------
// fixtures（对齐 R-10 manager 测试的 MavenFixture 模式）
// --------------------------------------------------------------


struct Fixture {
    root: PathBuf,
    db: Arc<Mutex<Connection>>,
    workspace_id: i64,
}

/// 单仓 parent(pom) + lib(jar) + app(jar→lib)，同步依赖图索引 + 配置。
fn maven_fixture(tag: &str) -> Fixture {
    let root = std::env::temp_dir().join(format!(
        "gw_r12_{tag}_{}",
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
    ));
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
    write(
        &root.join("repo/app/pom.xml"),
        "<project><modelVersion>4.0.0</modelVersion><parent><groupId>com.example</groupId>\
             <artifactId>parent</artifactId><version>1.0.0</version></parent>\
             <artifactId>app</artifactId><dependencies><dependency><groupId>com.example</groupId>\
             <artifactId>lib</artifactId><version>1.0.0</version></dependency></dependencies></project>",
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
    let discovery = maven::discover_poms(&root, 5, None, None);
    assert!(discovery.errors.is_empty(), "{:?}", discovery.errors);
    maven::sync_workspace_index(&mut conn, workspace_id, &discovery, &root.join("m2"))
        .unwrap();

    config::create_config(
        &conn,
        &CreateRuntimeConfigRequest {
            workspace_id,
            config: RuntimeApplicationConfig {
                name: "app".into(),
                project: root.join("repo/app/pom.xml").to_string_lossy().to_string(),
                main_class: Some("com.example.app.Application".into()),
                ..Default::default()
            },
        },
    )
    .unwrap();
    Fixture {
        root,
        db: Arc::new(Mutex::new(conn)),
        workspace_id,
    }
}

fn test_service(
    fixture: &Fixture,
    emitter: Arc<VecEmitter>,
    maven_runner: Arc<dyn MavenRunner>,
    launch_runner: Arc<dyn LaunchRunner>,
) -> Arc<RuntimeService> {
    RuntimeService::assemble(
        Arc::clone(&fixture.db),
        emitter,
        Arc::new(PomCache::new()),
        SchedulerConfig::default(),
        fixture.root.join("scheduler.json"),
        fixture.root.join("approvals.json"),
        RuntimeServiceOverrides {
            maven_runner,
            launch_runner,
            sample_interval: Duration::from_millis(50),
            ..Default::default()
        },
    )
}

fn runtime_task(op: RuntimeOp, workspace_id: i64, name: &str, options: RuntimeTaskOptions) -> TaskType {
    TaskType::Runtime {
        op,
        workspace_id,
        runtime_name: name.into(),
        options,
    }
}

// --------------------------------------------------------------
// tests
// --------------------------------------------------------------

/// §63/§65：Build 任务经 handler 执行成功，事件序列
/// build_started → build_progress(building) → build_completed(success)。
#[test]
fn build_op_succeeds_and_emits_event_sequence() {
    let fixture = maven_fixture("build");
    let emitter = Arc::new(VecEmitter::default());
    let maven = Arc::new(FakeMavenRunner::successful());
    let service = test_service(
        &fixture,
        Arc::clone(&emitter),
        maven,
        Arc::new(FakeLaunchRunner::staying_alive()),
    );

    let task = runtime_task(
        RuntimeOp::Build,
        fixture.workspace_id,
        "app",
        RuntimeTaskOptions {
            strategy: Some(RunStrategy::MavenRun),
            ..Default::default()
        },
    );
    let cancel = Arc::new(AtomicBool::new(false));
    let output = service.execute(&task, cancel).unwrap();
    assert!(output.unwrap().contains("构建完成"));

    assert_eq!(
        emitter.names(),
        vec![EVENT_BUILD_STARTED, EVENT_BUILD_PROGRESS, EVENT_BUILD_COMPLETED]
    );
    let completed = &emitter.collected()[2];
    assert_eq!(completed.payload["success"], serde_json::json!(true));
    assert!(completed.payload["durationMs"].is_number());
}

/// §65 验收主路径：Start 任务完整生命周期事件序列
/// build_* → process_* → health_changed，且阶段一一对应
/// Preparing/Resolving/Building/Starting。
#[test]
fn start_op_emits_full_lifecycle_sequence() {
    let fixture = maven_fixture("start");
    let emitter = Arc::new(VecEmitter::default());
    let service = test_service(
        &fixture,
        Arc::clone(&emitter),
        Arc::new(FakeMavenRunner::successful()),
        Arc::new(FakeLaunchRunner::staying_alive()),
    );

    let task = runtime_task(
        RuntimeOp::Start,
        fixture.workspace_id,
        "app",
        RuntimeTaskOptions {
            strategy: Some(RunStrategy::MavenRun),
            ..Default::default()
        },
    );
    let cancel = Arc::new(AtomicBool::new(false));
    service.execute(&task, cancel).unwrap();

    let names = emitter.names();
    assert_eq!(
        names,
        vec![
            EVENT_BUILD_STARTED,     // Preparing
            EVENT_BUILD_PROGRESS,    // preparing
            EVENT_BUILD_PROGRESS,    // resolving
            EVENT_BUILD_PROGRESS,    // building
            EVENT_BUILD_COMPLETED,   // 构建阶段结束
            EVENT_BUILD_PROGRESS,    // starting
            EVENT_PROCESS_STARTED,   // Running
            EVENT_HEALTH_CHANGED,    // up
        ]
    );
    let stages: Vec<String> = emitter
        .collected()
        .iter()
        .filter(|e| e.name == EVENT_BUILD_PROGRESS)
        .map(|e| e.payload["stage"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(stages, vec!["preparing", "resolving", "building", "starting"]);

    // 进程行进入 Running。
    let info = service
        .process_status(service.list_processes(fixture.workspace_id).unwrap()[0].process_id)
        .unwrap()
        .unwrap();
    assert_eq!(info.status, LifecycleStatus::Running);
}

/// §64/§66：Stop 任务停掉运行中的应用，事件
/// process_stopped + health_changed(down) 各一次。
#[test]
fn stop_op_stops_running_app() {
    let fixture = maven_fixture("stop");
    let emitter = Arc::new(VecEmitter::default());
    let service = test_service(
        &fixture,
        Arc::clone(&emitter),
        Arc::new(FakeMavenRunner::successful()),
        Arc::new(FakeLaunchRunner::staying_alive()),
    );
    let cancel = Arc::new(AtomicBool::new(false));
    service
        .execute(
            &runtime_task(
                RuntimeOp::Start,
                fixture.workspace_id,
                "app",
                RuntimeTaskOptions {
                    strategy: Some(RunStrategy::MavenRun),
                    ..Default::default()
                },
            ),
            cancel.clone(),
        )
        .unwrap();
    let before = emitter.names().len();

    service
        .execute(
            &runtime_task(RuntimeOp::Stop, fixture.workspace_id, "app", Default::default()),
            cancel,
        )
        .unwrap();

    let after: Vec<_> = emitter.names()[before..].to_vec();
    assert_eq!(
        after,
        vec![EVENT_PROCESS_STOPPED, EVENT_HEALTH_CHANGED]
    );
}

/// Restart 任务包裹 restart_started / restart_completed，内部 Start
/// 的生命周期事件照常发出（skip-build 路径）。
#[test]
fn restart_op_wraps_start_with_restart_events() {
    let fixture = maven_fixture("restart");
    let emitter = Arc::new(VecEmitter::default());
    let service = test_service(
        &fixture,
        Arc::clone(&emitter),
        Arc::new(FakeMavenRunner::successful()),
        Arc::new(FakeLaunchRunner::staying_alive()),
    );
    let cancel = Arc::new(AtomicBool::new(false));
    let start_task = runtime_task(
        RuntimeOp::Start,
        fixture.workspace_id,
        "app",
        RuntimeTaskOptions {
            strategy: Some(RunStrategy::MavenRun),
            ..Default::default()
        },
    );
    service.execute(&start_task, cancel.clone()).unwrap();
    let before = emitter.names().len();

    service
        .execute(
            &runtime_task(RuntimeOp::Restart, fixture.workspace_id, "app", Default::default()),
            cancel,
        )
        .unwrap();

    let after: Vec<_> = emitter.names()[before..].to_vec();
    assert_eq!(after.first(), Some(&EVENT_RESTART_STARTED));
    assert_eq!(after.last(), Some(&EVENT_RESTART_COMPLETED));
    assert!(after.contains(&EVENT_PROCESS_STOPPED));
    assert!(after.contains(&EVENT_PROCESS_STARTED));
    assert_eq!(
        emitter.collected().last().unwrap().payload["success"],
        serde_json::json!(true)
    );
}

/// §66 验收：取消进行中的 Start —— 构建中的 Maven 被取消快路径终止，
/// 进程行落到终态，任务以错误返回（worker 会标记 Cancelled）。
#[test]
fn cancel_during_start_aborts_build_and_finalizes() {
    let fixture = maven_fixture("cancel");
    let emitter = Arc::new(VecEmitter::default());
    // 构建挂起直到取消标志置位（FakeRun.duration 以 10ms 粒度检查取消）。
    let maven = Arc::new(FakeMavenRunner::new(vec![FakeRun {
        duration: Some(Duration::from_secs(30)),
        ..Default::default()
    }]));
    let service = test_service(
        &fixture,
        Arc::clone(&emitter),
        maven,
        Arc::new(FakeLaunchRunner::staying_alive()),
    );
    let task = runtime_task(
        RuntimeOp::Start,
        fixture.workspace_id,
        "app",
        RuntimeTaskOptions {
            strategy: Some(RunStrategy::MavenRun),
            ..Default::default()
        },
    );
    let cancel = Arc::new(AtomicBool::new(false));

    let started = Instant::now();
    let cancel2 = Arc::clone(&cancel);
    let canceller = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(200));
        cancel2.store(true, Ordering::Relaxed);
    });
    let result = service.execute(&task, cancel);
    canceller.join().unwrap();

    // 取消语义（R-10 设计）：watcher 先 signal_build_cancel 杀 Maven 构建，
    // 再 stop_runtime 置 Stopping —— start 以停止语义收尾返回 Ok
    // （若 Stopping 尚未可见则走 abort 路径返回 Err）。任务层的最终状态
    // 由 worker 的 cancel flag 兜底标记为 Cancelled（worker.rs 收尾检查）。
    let _ = &result; // Ok/Err 皆可，终态以进程行为准
    assert!(
        started.elapsed() < Duration::from_secs(15),
        "cancel must abort promptly, took {:?}",
        started.elapsed()
    );
    let rows = service.list_processes(fixture.workspace_id).unwrap();
    assert!(
        rows[0].status.is_terminal(),
        "row must be terminal after cancel, got {:?}",
        rows[0].status
    );
    assert!(
        matches!(
            rows[0].status,
            LifecycleStatus::Stopped | LifecycleStatus::Failed
        ),
        "cancelled start must end Stopped (stop semantics) or Failed (abort)"
    );
    // 构建确实被终止：build_completed(failure) 或 process_stopped 必居其一。
    let names = emitter.names();
    assert!(
        names.contains(&EVENT_BUILD_COMPLETED) || names.contains(&EVENT_PROCESS_STOPPED),
        "expected terminal events, got {names:?}"
    );
}

/// §63/§64：ResolveDependencies 同步索引并发 dependency_resolved 汇总；
/// 首次全量发现不发 project_discovered 洪泛。
#[test]
fn resolve_op_syncs_index_and_emits_summary() {
    let fixture = maven_fixture("resolve");
    let emitter = Arc::new(VecEmitter::default());
    let service = test_service(
        &fixture,
        Arc::clone(&emitter),
        Arc::new(FakeMavenRunner::successful()),
        Arc::new(FakeLaunchRunner::staying_alive()),
    );
    // 清空索引，模拟「首次解析」由本任务完成。
    {
        let conn = fixture.db.lock().unwrap();
        conn.execute("DELETE FROM maven_dependencies", []).unwrap();
        conn.execute("DELETE FROM maven_modules", []).unwrap();
        conn.execute("DELETE FROM maven_source_mappings", []).unwrap();
        conn.execute("DELETE FROM maven_projects", []).unwrap();
    }

    let task = runtime_task(
        RuntimeOp::ResolveDependencies,
        fixture.workspace_id,
        "",
        Default::default(),
    );
    let output = service
        .execute(&task, Arc::new(AtomicBool::new(false)))
        .unwrap();
    assert!(output.unwrap().contains("依赖解析完成"));

    let names = emitter.names();
    assert_eq!(names, vec![EVENT_DEPENDENCY_RESOLVED]);
    let payload = &emitter.collected()[0].payload;
    assert_eq!(payload["projects"], serde_json::json!(3));

    // 再同步一次：索引无变化，known 非空且无新项目 → 仍只有汇总事件。
    service
        .execute(&task, Arc::new(AtomicBool::new(false)))
        .unwrap();
    assert_eq!(
        emitter.names().iter().filter(|n| **n == EVENT_PROJECT_DISCOVERED).count(),
        0
    );

    // 查询侧：3 个项目、app → lib 依赖边。
    let projects = service.list_projects(fixture.workspace_id).unwrap();
    assert_eq!(projects.len(), 3);
    let inspection = service.inspect_project(fixture.workspace_id, "app").unwrap();
    assert_eq!(inspection.dependencies.len(), 1);
    let graph = service.dependency_graph(fixture.workspace_id, None, None).unwrap();
    assert!(!graph.truncated);
    assert_eq!(graph.total_dependencies, graph.dependencies.len());
}

/// 选项映射：skip_tests 缺省跟随 BuildOptions 默认（true），显式 false 生效。
#[test]
fn options_map_to_build_and_start_options() {
    let defaults = build_options_of(&RuntimeTaskOptions::default());
    assert!(defaults.skip_tests);
    let explicit = build_options_of(&RuntimeTaskOptions {
        skip_tests: Some(false),
        offline: true,
        ..Default::default()
    });
    assert!(!explicit.skip_tests);
    assert!(explicit.offline);

    let start = start_options_of(&RuntimeTaskOptions {
        skip_build: true,
        ..Default::default()
    });
    assert!(start.skip_build);
}

/// §66 可配置：set_scheduler_config 立即生效并持久化，重载后一致。
#[test]
fn scheduler_config_roundtrips_and_applies() {
    let fixture = maven_fixture("cfg");
    let emitter = Arc::new(VecEmitter::default());
    let service = test_service(
        &fixture,
        emitter,
        Arc::new(FakeMavenRunner::successful()),
        Arc::new(FakeLaunchRunner::staying_alive()),
    );
    assert_eq!(service.scheduler_config().max_concurrent_builds, 2);

    service
        .set_scheduler_config(&SchedulerConfig {
            max_concurrent_builds: 1,
            max_concurrent_resolves: 8,
        })
        .unwrap();
    assert_eq!(service.scheduler_config().max_concurrent_builds, 1);
    assert_eq!(service.scheduler_config().max_concurrent_resolves, 8);
    assert_eq!(service.build_scheduler.max(), 1);

    let loaded = SchedulerConfig::load(&fixture.root.join("scheduler.json"));
    assert_eq!(loaded.max_concurrent_builds, 1);
    assert_eq!(loaded.max_concurrent_resolves, 8);
}

/// R-13 `closure_preview`：给定 Scope 返回闭包预览，Manual 剔除模块后
/// 收缩；缓存命中标记正确。
#[test]
fn closure_preview_computes_scope_and_reports_cache_hit() {
    let fixture = maven_fixture("closure");
    let emitter = Arc::new(VecEmitter::default());
    let service = test_service(
        &fixture,
        emitter,
        Arc::new(FakeMavenRunner::successful()),
        Arc::new(FakeLaunchRunner::staying_alive()),
    );
    let project = fixture
        .root
        .join("repo/app/pom.xml")
        .to_string_lossy()
        .to_string();
    let lib_project = fixture.root.join("repo/lib/pom.xml").to_string_lossy().to_string();

    // Auto：闭包 = app + lib（lib 是 app 的源码依赖，parent 不进闭包）。
    let auto = service
        .closure_preview(fixture.workspace_id, &project, &RuntimeScope::Auto)
        .unwrap();
    let auto_ids: Vec<i64> = auto.closure.projects.iter().map(|p| p.project_id).collect();
    assert!(
        auto_ids.contains(&auto.closure.root_project_id),
        "root must be inside the auto closure"
    );
    assert!(
        auto.closure.projects.len() >= 2,
        "app + lib expected in closure, got {:?}",
        auto_ids
    );

    // 二次计算（同 fingerprint + 同 scope）应命中缓存。
    let cached = service
        .closure_preview(fixture.workspace_id, &project, &RuntimeScope::Auto)
        .unwrap();
    assert!(cached.cache_hit, "second auto preview must hit the closure cache");

    // Manual 空集：闭包收缩为仅 root（root 不可被排除，R-03 语义）。
    let empty = service
        .closure_preview(
            fixture.workspace_id,
            &project,
            &RuntimeScope::Manual { project_ids: vec![] },
        )
        .unwrap();
    assert_eq!(empty.closure.projects.len(), 1);
    assert_eq!(empty.closure.projects[0].project_id, auto.closure.root_project_id);

    // Hybrid：include=[root]，排除 lib → 闭包仅 app。
    let lib_id = service
        .closure_preview(fixture.workspace_id, &lib_project, &RuntimeScope::Auto)
        .unwrap()
        .closure
        .root_project_id;
    let hybrid = service
        .closure_preview(
            fixture.workspace_id,
            &project,
            &RuntimeScope::Hybrid {
                include_project_ids: vec![auto.closure.root_project_id],
                exclude_project_ids: vec![lib_id],
            },
        )
        .unwrap();
    assert_eq!(hybrid.closure.projects.len(), 1);
    assert_eq!(hybrid.closure.projects[0].project_id, auto.closure.root_project_id);

    // 未知项目 → ProjectNotFound 可行动错误。
    let err = service
        .closure_preview(fixture.workspace_id, "no/such/project", &RuntimeScope::Auto)
        .unwrap_err();
    assert_eq!(err.code(), "ProjectNotFound");
}

/// environment 任务组装：start 覆盖全部配置；stop 只覆盖有活跃进程的。
#[test]
fn environment_requests_cover_configs() {
    let fixture = maven_fixture("env");
    let emitter = Arc::new(VecEmitter::default());
    let service = test_service(
        &fixture,
        emitter,
        Arc::new(FakeMavenRunner::successful()),
        Arc::new(FakeLaunchRunner::staying_alive()),
    );
    let start = service.start_environment_requests(fixture.workspace_id).unwrap();
    assert_eq!(start.len(), 1);
    assert!(matches!(
        start[0].task_type,
        TaskType::Runtime { op: RuntimeOp::Start, .. }
    ));

    // 未启动时 stop environment 为空；启动后覆盖。
    assert!(service
        .stop_environment_requests(fixture.workspace_id)
        .unwrap()
        .is_empty());
    let cancel = Arc::new(AtomicBool::new(false));
    service
        .execute(
            &runtime_task(
                RuntimeOp::Start,
                fixture.workspace_id,
                "app",
                RuntimeTaskOptions {
                    strategy: Some(RunStrategy::MavenRun),
                    ..Default::default()
                },
            ),
            cancel,
        )
        .unwrap();
    assert_eq!(
        service
            .stop_environment_requests(fixture.workspace_id)
            .unwrap()
            .len(),
        1
    );
}

// --------------------------------------------------------------
// R-16 §41：健康探针与进程生命周期集成
// --------------------------------------------------------------

/// 配置了 health_check 的应用：Start 后探针 Starting → Healthy；
/// Stop 后收口为 Stopped（进程退出 → finalize_exit → stop_monitor）。
#[test]
fn health_probe_transitions_with_lifecycle() {
    let fixture = maven_fixture("health");
    // 真实本地端口：探针 Port 方式连它（FakeLaunchRunner 不开端口，
    // 因此显式配置 port，不经启动日志探测）。
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    {
        let conn = fixture.db.lock().unwrap();
        config::update_config(
            &conn,
            &config::UpdateRuntimeConfigRequest {
                workspace_id: fixture.workspace_id,
                name: "app".into(),
                config: RuntimeApplicationConfig {
                    name: "app".into(),
                    project: fixture
                        .root
                        .join("repo/app/pom.xml")
                        .to_string_lossy()
                        .to_string(),
                    main_class: Some("com.example.app.Application".into()),
                    health_check: Some(crate::runtime::health::HealthCheckConfig {
                        kind: crate::runtime::health::HealthCheckKind::Port,
                        port: Some(port),
                        interval_ms: Some(500),
                        timeout_ms: Some(500),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
            },
        )
        .unwrap();
    }
    let emitter = Arc::new(VecEmitter::default());
    let service = test_service(
        &fixture,
        Arc::clone(&emitter),
        Arc::new(FakeMavenRunner::successful()),
        Arc::new(FakeLaunchRunner::staying_alive()),
    );

    service
        .execute(
            &runtime_task(
                RuntimeOp::Start,
                fixture.workspace_id,
                "app",
                RuntimeTaskOptions {
                    strategy: Some(RunStrategy::MavenRun),
                    ..Default::default()
                },
            ),
            Arc::new(AtomicBool::new(false)),
        )
        .unwrap();

    // 等待探针翻到 Healthy（首个探测在一个间隔内发生）。
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut healthy_seen = false;
    while Instant::now() < deadline {
        if let Some(snapshot) = service.get_health(
            service.list_processes(fixture.workspace_id).unwrap()[0].process_id,
        ) {
            if snapshot.phase == crate::runtime::events::HealthStatus::Healthy {
                healthy_seen = true;
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(healthy_seen, "probe must reach Healthy while running");

    // Stop：进程退出收口探针 → Stopped。
    service
        .execute(
            &runtime_task(RuntimeOp::Stop, fixture.workspace_id, "app", Default::default()),
            Arc::new(AtomicBool::new(false)),
        )
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut stopped_seen = false;
    while Instant::now() < deadline {
        if let Some(snapshot) = service.get_health(
            service.list_processes(fixture.workspace_id).unwrap()[0].process_id,
        ) {
            if snapshot.phase == crate::runtime::events::HealthStatus::Stopped {
                stopped_seen = true;
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(stopped_seen, "probe must be finalized to Stopped after exit");
    drop(listener);
}

/// 未配置 health_check 的应用：无探针快照（R-12 up/down 语义保持）。
#[test]
fn no_health_config_means_no_probe() {
    let fixture = maven_fixture("nohealth");
    let emitter = Arc::new(VecEmitter::default());
    let service = test_service(
        &fixture,
        emitter,
        Arc::new(FakeMavenRunner::successful()),
        Arc::new(FakeLaunchRunner::staying_alive()),
    );
    service
        .execute(
            &runtime_task(
                RuntimeOp::Start,
                fixture.workspace_id,
                "app",
                RuntimeTaskOptions {
                    strategy: Some(RunStrategy::MavenRun),
                    ..Default::default()
                },
            ),
            Arc::new(AtomicBool::new(false)),
        )
        .unwrap();
    std::thread::sleep(Duration::from_millis(300));
    let process_id = service.list_processes(fixture.workspace_id).unwrap()[0].process_id;
    assert!(
        service.get_health(process_id).is_none(),
        "without health_check config there must be no probe snapshot"
    );
}

// --------------------------------------------------------------
// R-15 §38/§39/§40：环境编排
// --------------------------------------------------------------

/// 记录 Maven 调用顺序的 runner（`-f` reactor pom 区分服务；顺序断言
/// 拓扑波次）。
struct OrderingRunner {
    workdirs: Mutex<Vec<String>>,
}

impl MavenRunner for OrderingRunner {
    fn resolve_maven(
        &self,
        _project_dir: &Path,
        local_repository: &Path,
    ) -> AppResult<crate::maven::ResolvedMaven> {
        Ok(crate::maven::ResolvedMaven {
            executable: crate::maven::MavenExecutable::new(
                "fake-mvn",
                crate::maven::MavenSource::System,
                None,
            ),
            local_repository: local_repository.to_path_buf(),
            uses_wrapper: false,
        })
    }

    fn run(
        &self,
        request: &crate::maven::MavenExecutionRequest,
        _env: &[(String, String)],
        sink: &mut dyn BuildOutputSink,
        _cancel: Option<&AtomicBool>,
        _timeout: Option<Duration>,
    ) -> AppResult<StreamingExit> {
        let reactor = request
            .extra_args
            .iter()
            .position(|arg| arg == "-f")
            .and_then(|i| request.extra_args.get(i + 1))
            .cloned()
            .unwrap_or_default();
        self.workdirs.lock().unwrap().push(reactor);
        sink.on_line(OutputStream::Stdout, "BUILD SUCCESS");
        Ok(StreamingExit {
            exit_code: Some(0),
            timed_out: false,
            cancelled: false,
        })
    }
}

fn env_service(name: &str, deps: &[&str]) -> crate::runtime::environment::EnvironmentService {
    crate::runtime::environment::EnvironmentService {
        runtime_name: name.into(),
        depends_on: deps.iter().map(|s| s.to_string()).collect(),
        jdk: None,
        profile: None,
        environment: Default::default(),
        port: None,
        external_notes: None,
        ready_timeout_seconds: None,
    }
}

/// Start Environment：无依赖服务并行（第一波），依赖服务按拓扑序串行；
/// 全部就绪后 completed(success) 汇总。
#[test]
fn environment_start_follows_topology_and_readies_all() {
    let fixture = maven_fixture("envstart");
    let emitter = Arc::new(VecEmitter::default());
    let lib_dir = fixture.root.join("repo/lib").to_string_lossy().to_string();
    let app_dir = fixture.root.join("repo/app").to_string_lossy().to_string();
    let ordering = Arc::new(OrderingRunner {
        workdirs: Mutex::new(Vec::new()),
    });
    let service = test_service(
        &fixture,
        Arc::clone(&emitter),
        ordering.clone(),
        Arc::new(FakeLaunchRunner::staying_alive()),
    );

    // 四个配置：common/lib、file/app（第一波无依赖）；auth/lib、gateway/app。
    for (name, pom) in [
        ("common", "repo/lib/pom.xml"),
        ("file", "repo/app/pom.xml"),
        ("auth", "repo/lib/pom.xml"),
        ("gateway", "repo/app/pom.xml"),
    ] {
        let conn = fixture.db.lock().unwrap();
        config::create_config(
            &conn,
            &CreateRuntimeConfigRequest {
                workspace_id: fixture.workspace_id,
                config: RuntimeApplicationConfig {
                    name: name.into(),
                    project: fixture.root.join(pom).to_string_lossy().to_string(),
                    main_class: Some("com.example.app.Application".into()),
                    // PackageRun：单次 Maven 调用 + jar 产物校验（见下），
                    // 避免假 runner 下的 ClasspathRun classpath 文件生成。
                    profile: Some("prod".into()),
                    ..Default::default()
                },
            },
        )
        .unwrap();
    }
    // PackageRun 需要 target jar 产物存在。
    for (dir, artifact) in [("repo/lib", "lib"), ("repo/app", "app")] {
        let target = fixture.root.join(dir).join("target");
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join(format!("{artifact}-1.0.0.jar")), b"jar").unwrap();
    }
    let environment = crate::runtime::environment::RuntimeEnvironment {
        schema_version: 1,
        name: "Development".into(),
        description: None,
        services: vec![
            env_service("gateway", &["auth"]),
            env_service("auth", &["common"]),
            env_service("common", &[]),
            env_service("file", &[]),
        ],
    };
    crate::runtime::environment::save_environment(&fixture.root, &environment).unwrap();

    let output = service
        .execute(
            &runtime_task(
                RuntimeOp::StartEnvironment,
                fixture.workspace_id,
                "Development",
                Default::default(),
            ),
            Arc::new(AtomicBool::new(false)),
        )
        .unwrap();
    assert!(output.unwrap().contains("4 Ready"));

    // Maven 调用顺序：波 0（common/file，并行）→ 波 1（auth）→ 波 2
    // （gateway）。auth 的构建必须晚于两个无依赖服务的构建完成。
    let workdirs = ordering.workdirs.lock().unwrap();
    assert_eq!(workdirs.len(), 4, "each service builds once");
    // lib 配置的 reactor 是单项目 pom；app 配置（依赖 lib）的 reactor 是
    // 父 pom（带 -pl app）。
    let kind_of = |reactor: &str| {
        if reactor.ends_with("/repo/lib/pom.xml") {
            "lib"
        } else {
            "app"
        }
    };
    assert_eq!(kind_of(&workdirs[2]), "lib", "wave 1 = auth (lib reactor)");
    assert_eq!(kind_of(&workdirs[3]), "app", "wave 2 = gateway (app reactor)");
    drop(workdirs);
    let _ = (&lib_dir, &app_dir);

    // 事件：completed(success=true)，4 服务全部 ready。
    let names = emitter.names();
    assert!(names.contains(&EVENT_ENVIRONMENT_PROGRESS));
    assert_eq!(names.last(), Some(&EVENT_ENVIRONMENT_COMPLETED));
    let collected = emitter.collected();
    let completed = collected.last().unwrap();
    assert_eq!(completed.payload["success"], serde_json::json!(true));
    assert_eq!(completed.payload["services"].as_array().unwrap().len(), 4);
    for outcome in completed.payload["services"].as_array().unwrap() {
        assert_eq!(outcome["state"], serde_json::json!("ready"), "{outcome}");
    }
}

/// 部分失败语义：单服务启动失败 → 其依赖方 Skipped，无依赖分支照常
/// Ready；completed(success=false) 正确汇总。
#[test]
fn environment_start_partial_failure_skips_dependents() {
    let fixture = maven_fixture("envfail");
    let emitter = Arc::new(VecEmitter::default());
    let service = test_service(
        &fixture,
        Arc::clone(&emitter),
        Arc::new(FakeMavenRunner::successful()),
        Arc::new(FakeLaunchRunner::staying_alive()),
    );
    for (name, pom) in [
        ("ok", "repo/lib/pom.xml"),
        ("broken", "repo/missing/pom.xml"), // prepare 阶段即失败
        ("dependent", "repo/app/pom.xml"),
    ] {
        let conn = fixture.db.lock().unwrap();
        config::create_config(
            &conn,
            &CreateRuntimeConfigRequest {
                workspace_id: fixture.workspace_id,
                config: RuntimeApplicationConfig {
                    name: name.into(),
                    project: fixture.root.join(pom).to_string_lossy().to_string(),
                    main_class: Some("com.example.app.Application".into()),
                    profile: Some("prod".into()),
                    ..Default::default()
                },
            },
        )
        .unwrap();
    }
    for (dir, artifact) in [("repo/lib", "lib"), ("repo/app", "app")] {
        let target = fixture.root.join(dir).join("target");
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join(format!("{artifact}-1.0.0.jar")), b"jar").unwrap();
    }
    let environment = crate::runtime::environment::RuntimeEnvironment {
        schema_version: 1,
        name: "Demo".into(),
        description: None,
        services: vec![
            env_service("dependent", &["broken"]),
            env_service("broken", &[]),
            env_service("ok", &[]),
        ],
    };
    crate::runtime::environment::save_environment(&fixture.root, &environment).unwrap();

    let output = service
        .execute(
            &runtime_task(
                RuntimeOp::StartEnvironment,
                fixture.workspace_id,
                "Demo",
                Default::default(),
            ),
            Arc::new(AtomicBool::new(false)),
        )
        .unwrap();
    // 部分成功：任务以 Ok 收尾（ok Ready），失败明细在汇总里。
    assert!(output.unwrap().contains("Failed"));
    let collected = emitter.collected();
    let completed = collected.last().unwrap();
    assert_eq!(completed.payload["success"], serde_json::json!(false));
    let states: std::collections::BTreeMap<String, String> = completed.payload["services"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| {
            (
                o["service"].as_str().unwrap().to_string(),
                o["state"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    assert_eq!(states["ok"], "ready");
    assert_eq!(states["broken"], "failed");
    assert_eq!(states["dependent"], "skipped");
}

/// Stop Environment：运行中的服务全部停止。
#[test]
fn environment_stop_stops_running_services() {
    let fixture = maven_fixture("envstop");
    let emitter = Arc::new(VecEmitter::default());
    let service = test_service(
        &fixture,
        Arc::clone(&emitter),
        Arc::new(FakeMavenRunner::successful()),
        Arc::new(FakeLaunchRunner::staying_alive()),
    );
    for (name, pom) in [("a", "repo/lib/pom.xml"), ("b", "repo/app/pom.xml")] {
        let conn = fixture.db.lock().unwrap();
        config::create_config(
            &conn,
            &CreateRuntimeConfigRequest {
                workspace_id: fixture.workspace_id,
                config: RuntimeApplicationConfig {
                    name: name.into(),
                    project: fixture.root.join(pom).to_string_lossy().to_string(),
                    ..Default::default()
                },
            },
        )
        .unwrap();
    }
    let environment = crate::runtime::environment::RuntimeEnvironment {
        schema_version: 1,
        name: "Local".into(),
        description: None,
        services: vec![env_service("a", &[]), env_service("b", &["a"])],
    };
    crate::runtime::environment::save_environment(&fixture.root, &environment).unwrap();

    // 先启动 a（b 不启动）。
    service
        .execute(
            &runtime_task(
                RuntimeOp::Start,
                fixture.workspace_id,
                "a",
                RuntimeTaskOptions {
                    strategy: Some(RunStrategy::MavenRun),
                    ..Default::default()
                },
            ),
            Arc::new(AtomicBool::new(false)),
        )
        .unwrap();
    let before = emitter.names().len();

    let output = service
        .execute(
            &runtime_task(
                RuntimeOp::StopEnvironment,
                fixture.workspace_id,
                "Local",
                Default::default(),
            ),
            Arc::new(AtomicBool::new(false)),
        )
        .unwrap();
    assert!(output.unwrap().contains("已停止"));
    let collected = emitter.collected();
    let completed = collected.last().unwrap();
    assert_eq!(completed.payload["environment"], serde_json::json!("Local"));
    let states: Vec<(String, String)> = completed.payload["services"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| {
            (
                o["service"].as_str().unwrap().to_string(),
                o["state"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    assert!(states.contains(&("a".into(), "stopped".into())));
    assert!(states.contains(&("b".into(), "stopped".into())));
    // a 真正被停止：进程事件发出。
    let after: Vec<_> = emitter.names()[before..].to_vec();
    assert!(after.contains(&EVENT_PROCESS_STOPPED));
}

// --------------------------------------------------------------
// §66 并发验收 + 真实 Maven 集成
// --------------------------------------------------------------

/// 记录并发峰值的 Maven runner：每次 run 睡 150ms 制造并发窗口。
struct CountingRunner {
    running: AtomicUsize,
    max_seen: AtomicUsize,
}

impl MavenRunner for CountingRunner {
    fn resolve_maven(
        &self,
        _project_dir: &Path,
        local_repository: &Path,
    ) -> AppResult<crate::maven::ResolvedMaven> {
        Ok(crate::maven::ResolvedMaven {
            executable: crate::maven::MavenExecutable::new(
                "fake-mvn",
                crate::maven::MavenSource::System,
                None,
            ),
            local_repository: local_repository.to_path_buf(),
            uses_wrapper: false,
        })
    }

    fn run(
        &self,
        _request: &crate::maven::MavenExecutionRequest,
        _env: &[(String, String)],
        sink: &mut dyn BuildOutputSink,
        _cancel: Option<&AtomicBool>,
        _timeout: Option<Duration>,
    ) -> AppResult<StreamingExit> {
        let current = self.running.fetch_add(1, Ordering::SeqCst) + 1;
        self.max_seen.fetch_max(current, Ordering::SeqCst);
        std::thread::sleep(Duration::from_millis(150));
        self.running.fetch_sub(1, Ordering::SeqCst);
        sink.on_line(OutputStream::Stdout, "BUILD SUCCESS");
        Ok(StreamingExit {
            exit_code: Some(0),
            timed_out: false,
            cancelled: false,
        })
    }
}

/// §66 验收：3 个并发 Build 任务经过共享 permit 池，并发 Maven 构建
/// 峰值不超过 2，其余排队执行且全部成功。
#[test]
fn concurrent_builds_are_capped_by_scheduler() {
    let fixture = maven_fixture("conc");
    let emitter = Arc::new(VecEmitter::default());
    let counting = Arc::new(CountingRunner {
        running: AtomicUsize::new(0),
        max_seen: AtomicUsize::new(0),
    });
    let service = test_service(
        &fixture,
        emitter,
        counting.clone(),
        Arc::new(FakeLaunchRunner::staying_alive()),
    );

    // 三个配置分别指向 parent / lib / app。
    for (name, pom) in [
        ("cfg-parent", "repo/pom.xml"),
        ("cfg-lib", "repo/lib/pom.xml"),
        ("cfg-app", "repo/app/pom.xml"),
    ] {
        let conn = fixture.db.lock().unwrap();
        config::create_config(
            &conn,
            &CreateRuntimeConfigRequest {
                workspace_id: fixture.workspace_id,
                config: RuntimeApplicationConfig {
                    name: name.into(),
                    project: fixture.root.join(pom).to_string_lossy().to_string(),
                    ..Default::default()
                },
            },
        )
        .unwrap();
    }

    let mut handles = Vec::new();
    for name in ["cfg-parent", "cfg-lib", "cfg-app"] {
        let service = Arc::clone(&service);
        handles.push(std::thread::spawn(move || {
            service.execute(
                &runtime_task(
                    RuntimeOp::Build,
                    fixture.workspace_id,
                    name,
                    RuntimeTaskOptions {
                        strategy: Some(RunStrategy::MavenRun),
                        ..Default::default()
                    },
                ),
                Arc::new(AtomicBool::new(false)),
            )
        }));
    }
    for handle in handles {
        let result = handle.join().unwrap();
        assert!(result.is_ok(), "queued build must succeed: {result:?}");
    }
    assert_eq!(
        counting.max_seen.load(Ordering::SeqCst),
        2,
        "build concurrency must stay at the §66 cap"
    );
}

// ---- 真实 Maven 集成（Synthetic Reactor 走真实 mvn；无 mvn 时跳过并标注）----

fn maven_available() -> bool {
    let maven = if cfg!(windows) { "mvn.cmd" } else { "mvn" };
    std::process::Command::new(maven)
        .arg("-version")
        .output()
        .is_ok()
}

/// 单仓 Spring Boot fixture（对齐 R-09 `setup_single_repo_boot`，
/// 坐标换成 com.r12）：repo/(parent + lib + app)，app 依赖 lib +
/// spring-boot-starter（外部依赖，靠 ~/.m2 缓存命中）。
fn spring_boot_fixture(tag: &str) -> Fixture {
    let root = std::env::temp_dir().join(format!(
        "gw_r12_it_{tag}_{}",
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
    ));
    std::fs::create_dir_all(&root).unwrap();
    write(
        &root.join("repo/pom.xml"),
        r#"<?xml version="1.0" encoding="UTF-8"?>
<project xmlns="http://maven.apache.org/POM/4.0.0">
  <modelVersion>4.0.0</modelVersion>
  <groupId>com.r12</groupId>
  <artifactId>r12-parent</artifactId>
  <version>1.0.0</version>
  <packaging>pom</packaging>
  <modules><module>lib</module><module>app</module></modules>
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
        <version>3.2.5</version>
        <type>pom</type>
        <scope>import</scope>
      </dependency>
    </dependencies>
  </dependencyManagement>
</project>
"#,
    );
    write(
        &root.join("repo/lib/pom.xml"),
        r#"<?xml version="1.0" encoding="UTF-8"?>
<project xmlns="http://maven.apache.org/POM/4.0.0">
  <modelVersion>4.0.0</modelVersion>
  <parent><groupId>com.r12</groupId><artifactId>r12-parent</artifactId><version>1.0.0</version></parent>
  <artifactId>lib</artifactId>
</project>
"#,
    );
    write(
        &root.join("repo/lib/src/main/java/com/r12/lib/Lib.java"),
        "package com.r12.lib;\n\npublic final class Lib {\n    private Lib() {}\n    public static String greet() { return \"hi\"; }\n}\n",
    );
    write(
        &root.join("repo/app/pom.xml"),
        r#"<?xml version="1.0" encoding="UTF-8"?>
<project xmlns="http://maven.apache.org/POM/4.0.0">
  <modelVersion>4.0.0</modelVersion>
  <parent><groupId>com.r12</groupId><artifactId>r12-parent</artifactId><version>1.0.0</version></parent>
  <artifactId>app</artifactId>
  <dependencies>
    <dependency><groupId>com.r12</groupId><artifactId>lib</artifactId><version>1.0.0</version></dependency>
    <dependency><groupId>org.springframework.boot</groupId><artifactId>spring-boot-starter</artifactId></dependency>
  </dependencies>
</project>
"#,
    );
    write(
        &root.join("repo/app/src/main/java/com/r12/app/Application.java"),
        "package com.r12.app;\n\nimport org.springframework.boot.autoconfigure.SpringBootApplication;\n\n@SpringBootApplication\npublic class Application {\n    public static void main(String[] args) {\n        System.out.println(com.r12.lib.Lib.greet());\n    }\n}\n",
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
    let discovery = maven::discover_poms(&root, 6, None, None);
    assert!(discovery.errors.is_empty(), "{:?}", discovery.errors);
    maven::sync_workspace_index(&mut conn, workspace_id, &discovery, &root.join("m2"))
        .unwrap();

    config::create_config(
        &conn,
        &CreateRuntimeConfigRequest {
            workspace_id,
            config: RuntimeApplicationConfig {
                name: "app".into(),
                project: root.join("repo/app/pom.xml").to_string_lossy().to_string(),
                main_class: Some("com.r12.app.Application".into()),
                ..Default::default()
            },
        },
    )
    .unwrap();
    Fixture {
        root,
        db: Arc::new(Mutex::new(conn)),
        workspace_id,
    }
}

/// R-12 端到端：Build 任务驱动真实 mvn 走完 Synthetic Reactor 构建
/// （ClasspathRun = compile + dependency:build-classpath），事件序列完整。
#[test]
fn build_op_with_real_maven_builds_synthetic_reactor() {
    if !maven_available() {
        eprintln!("R-12: no `mvn` on PATH; skipping real-maven integration test");
        return;
    }
    let fixture = spring_boot_fixture("realmvn");
    let emitter = Arc::new(VecEmitter::default());
    // 生产 runner：SpawningMavenRunner 驱动真实 mvn。
    let service = RuntimeService::assemble(
        Arc::clone(&fixture.db),
        emitter.clone(),
        Arc::new(PomCache::new()),
        SchedulerConfig::default(),
        fixture.root.join("scheduler.json"),
        fixture.root.join("approvals.json"),
        RuntimeServiceOverrides::default(),
    );

    let task = runtime_task(
        RuntimeOp::Build,
        fixture.workspace_id,
        "app",
        RuntimeTaskOptions {
            strategy: Some(RunStrategy::ClasspathRun),
            ..Default::default()
        },
    );
    let output = service
        .execute(&task, Arc::new(AtomicBool::new(false)))
        .unwrap_or_else(|e| panic!("real maven build failed: {e}"));
    assert!(output.unwrap().contains("构建完成"));

    // Synthetic Reactor 只落在 .gitworkspace/（用户项目只读，全局约束 §2）。
    assert!(fixture.root.join(".gitworkspace/runtime/app").exists());
    assert!(!fixture.root.join("repo/.gitworkspace").exists());

    let names = emitter.names();
    assert_eq!(
        names,
        vec![EVENT_BUILD_STARTED, EVENT_BUILD_PROGRESS, EVENT_BUILD_COMPLETED]
    );
    assert_eq!(
        emitter.collected()[2].payload["success"],
        serde_json::json!(true)
    );

    let _ = std::fs::remove_dir_all(&fixture.root);
}
