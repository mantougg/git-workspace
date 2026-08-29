//! Runtime Watch 测试（R-17，B-07 拆分后归位同父模块 tests.rs）。
//!
//! §4.7 影响分析四条独立单测：
//! 1. `propagate_downstream_spreads_to_closure_descendants_only`——变更模块
//!    只扩散到允许的下游模块（闭包外不加入）；
//! 2. `propagate_downstream_ignores_external_sources_and_converges`——外部
//!    依赖不当成本地源码模块传播（环收敛）；
//! 3. `affected_modules_pure_maps_paths_and_propagates`——路径→模块映射与
//!    传播（纯函数，无 Task 提交依赖）；
//! 4. `same_batch_of_events_converges_into_single_task`——同批事件收敛为
//!    一个任务。

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::impact::propagate_downstream;
use super::*;
use crate::maven::closure::{RuntimeClosure, RuntimeScopeMode};
use crate::maven::index::DependencyGraphCache;
use crate::maven::model::PomCoordinates;
use crate::models::repository::ScannedRepo;
use crate::models::task::{RuntimeOp, RuntimeTaskOptions, TaskRequest, TaskType};
use crate::runtime::config::CreateRuntimeConfigRequest;
use crate::runtime::events::VecEmitter;

fn write(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, content).unwrap();
}

#[test]
fn ignore_path_filters_build_outputs() {
    assert!(ignore_path("/ws/repo/app/target/classes/App.class"));
    assert!(ignore_path("/ws/repo/.git/refs/main"));
    assert!(ignore_path("/ws/repo/app/node_modules/vue/index.js"));
    assert!(!ignore_path("/ws/repo/app/src/main/java/App.java"));
    assert!(!ignore_path("/ws/repo/app/pom.xml"));
}

// ------------------------------------------------------------------
// §72 反向传播（纯函数）
// ------------------------------------------------------------------

fn edge(from: i64, source: i64) -> crate::maven::index::DependencyEdge {
    crate::maven::index::DependencyEdge {
        dependency_id: 0,
        from_project_id: from,
        dependency: crate::maven::model::MavenDependency {
            group_id: "com.example".into(),
            artifact_id: "x".into(),
            version: Some("1.0.0".into()),
            scope: crate::maven::model::DependencyScope::Compile,
            optional: false,
            dep_type: "jar".into(),
            classifier: None,
            exclusions: Vec::new(),
        },
        source: crate::maven::resolver::DependencySource::WorkspaceSource,
        source_project_id: Some(source),
        resolved_path: None,
        reason: crate::maven::resolver::ResolutionReason::WorkspaceExactMatch,
    }
}

/// 链 common ← core ← auth ← boot：改 common 传播到全部；改 auth 只到
/// auth/boot；闭包外模块不扩散。
#[test]
fn propagate_downstream_spreads_to_closure_descendants_only() {
    let edges = vec![edge(2, 1), edge(3, 2), edge(4, 3)];
    let closure: HashSet<i64> = [1, 2, 3, 4].into_iter().collect();

    let mut affected: BTreeSet<i64> = [1].into_iter().collect();
    propagate_downstream(&mut affected, &edges, &closure);
    assert_eq!(affected, [1, 2, 3, 4].into_iter().collect());

    let mut affected: BTreeSet<i64> = [3].into_iter().collect();
    propagate_downstream(&mut affected, &edges, &closure);
    assert_eq!(affected, [3, 4].into_iter().collect());

    // 闭包外下游不扩散（edge 指向 closure 外）。
    let closure_small: HashSet<i64> = [1, 2].into_iter().collect();
    let mut affected: BTreeSet<i64> = [1].into_iter().collect();
    propagate_downstream(&mut affected, &edges, &closure_small);
    assert_eq!(affected, [1, 2].into_iter().collect());
}

/// 无 source_project_id 的边（Remote Maven）不参与传播；环收敛。
#[test]
fn propagate_downstream_ignores_external_sources_and_converges() {
    let mut cyclic = edge(1, 4);
    let external = crate::maven::index::DependencyEdge {
        source_project_id: None,
        ..cyclic.clone()
    };
    cyclic.dependency_id = 1;
    let edges = vec![cyclic, external];
    let closure: HashSet<i64> = [1, 4].into_iter().collect();
    let mut affected: BTreeSet<i64> = [4].into_iter().collect();
    propagate_downstream(&mut affected, &edges, &closure);
    assert_eq!(affected, [1, 4].into_iter().collect());
}

/// §4.7 影响分析（纯函数）：路径→模块映射 + 闭包内传播；闭包外路径返回
/// `None`。不涉及任何 Task 提交（代码走查对应单测）。
#[test]
fn affected_modules_pure_maps_paths_and_propagates() {
    let projects = vec![
        crate::maven::index::MavenProjectNode {
            project_id: 1,
            repository_id: None,
            path: PathBuf::from("/ws/repo/lib/pom.xml"),
            coordinates: PomCoordinates {
                group_id: "com.example".into(),
                artifact_id: "lib".into(),
                version: "1.0.0".into(),
            },
            packaging: "jar".into(),
            pom_hash: "h1".into(),
        },
        crate::maven::index::MavenProjectNode {
            project_id: 2,
            repository_id: None,
            path: PathBuf::from("/ws/repo/app/pom.xml"),
            coordinates: PomCoordinates {
                group_id: "com.example".into(),
                artifact_id: "app".into(),
                version: "1.0.0".into(),
            },
            packaging: "jar".into(),
            pom_hash: "h2".into(),
        },
    ];
    let closure = RuntimeClosure {
        workspace_id: 1,
        root_project_id: 2,
        graph_fingerprint: "fp".into(),
        mode: RuntimeScopeMode::Auto,
        projects,
    };
    let graph = crate::maven::index::DependencyGraph {
        workspace_id: 1,
        fingerprint: "fp".into(),
        projects: Vec::new(),
        dependencies: vec![edge(2, 1)],
        modules: Vec::new(),
        source_mappings: Vec::new(),
    };

    // 改 lib 源码 → lib + app（反向传播）。
    let mut changed: BTreeSet<PathBuf> = [PathBuf::from("/ws/repo/lib/src/A.java")]
        .into_iter()
        .collect();
    let affected = super::impact::affected_modules(&closure, &graph, &changed).unwrap();
    assert_eq!(
        affected,
        BTreeSet::from(["com.example:lib".to_string(), "com.example:app".to_string()])
    );

    // 同批多文件（同模块多路径 + 跨模块）收敛为同一个受影响集合。
    changed.insert(PathBuf::from("/ws/repo/lib/src/B.java"));
    changed.insert(PathBuf::from("/ws/repo/app/src/Main.java"));
    let affected = super::impact::affected_modules(&closure, &graph, &changed).unwrap();
    assert_eq!(
        affected,
        BTreeSet::from(["com.example:lib".to_string(), "com.example:app".to_string()])
    );

    // 全部路径都在闭包外 → None（不触发任何任务）。
    let outside: BTreeSet<PathBuf> = [PathBuf::from("/elsewhere/src/X.java")]
        .into_iter()
        .collect();
    assert!(super::impact::affected_modules(&closure, &graph, &outside).is_none());
}

// ------------------------------------------------------------------
// 端到端（真 DB 索引 + 假提交端）：parent/lib/app 三模块
// ------------------------------------------------------------------

struct Fixture {
    root: PathBuf,
    db: Arc<Mutex<Connection>>,
    workspace_id: i64,
}

/// 单仓 parent(pom) + lib(jar) + app(jar→lib)，同步依赖图索引 + 配置。
fn watch_fixture(tag: &str) -> Fixture {
    let root = std::env::temp_dir().join(format!(
        "gw_r17_watch_{tag}_{}",
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
        &[ScannedRepo {
            path: root.join("repo").to_string_lossy().to_string(),
            name: "repo".into(),
            relative_path: "repo".into(),
            git_dir_mtime: None,
        }],
    )
    .unwrap();
    let discovery = crate::maven::discover_poms(&root, 5, None, None);
    assert!(discovery.errors.is_empty(), "{:?}", discovery.errors);
    crate::maven::sync_workspace_index(&mut conn, workspace_id, &discovery, &root.join("m2"))
        .unwrap();

    crate::runtime::config::create_config(
        &conn,
        &CreateRuntimeConfigRequest {
            workspace_id,
            config: crate::runtime::config::RuntimeApplicationConfig {
                name: "app".into(),
                project: root.join("repo/app/pom.xml").to_string_lossy().to_string(),
                main_class: Some("com.example.app.Application".into()),
                auto_restart: Some(true),
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

/// 直接构造引擎（不走 spawn：不起防抖/同步线程，注入假提交端）。
fn test_engine(
    fixture: &Fixture,
    emitter: Arc<VecEmitter>,
) -> (RuntimeWatchEngine, Arc<RecordingSubmitter>) {
    let submitter = RecordingSubmitter::new();
    let (event_tx, _event_rx) = std::sync::mpsc::channel::<Vec<PathBuf>>();
    let engine = RuntimeWatchEngine {
        db: Arc::clone(&fixture.db),
        graph_cache: Arc::new(DependencyGraphCache::new()),
        closure_cache: Arc::new(RuntimeClosureCache::new()),
        emitter,
        processes: Arc::new(crate::runtime::launch::RuntimeProcessManager::new(
            Arc::clone(&fixture.db),
        )),
        task_manager: Mutex::new(Some(Arc::clone(&submitter) as Arc<dyn WatchTaskSubmitter>)),
        watcher: Mutex::new(None),
        watched: Mutex::new(HashSet::new()),
        apps: Mutex::new(HashMap::new()),
        event_tx,
        stop: Arc::new(AtomicBool::new(false)),
    };
    (engine, submitter)
}

/// §43 忽略规则 + §72 影响分析 + RebuildRestart 提交 + file_changed 事件。
#[test]
fn source_change_submits_rebuild_restart_with_affected_subset() {
    let fixture = watch_fixture("rebuild");
    let emitter = Arc::new(VecEmitter::default());
    let (engine, submitter) = test_engine(&fixture, Arc::clone(&emitter));
    engine.register_app_for_test(fixture.workspace_id, "app", Vec::new());

    // 改 lib 源码 → 影响分析 = lib + app（反向传播）。
    let lib_source = fixture.root.join("repo/lib/src/main/java/Demo.java");
    engine.handle_events(&[lib_source]);

    let ops = submitter.ops();
    assert_eq!(ops.len(), 1, "one rebuild submitted: {ops:?}");
    let (op, ws, name, modules) = &ops[0];
    assert_eq!(*op, RuntimeOp::RebuildRestart);
    assert_eq!(*ws, fixture.workspace_id);
    assert_eq!(name, "app");
    assert_eq!(
        modules,
        &vec!["com.example:app".to_string(), "com.example:lib".to_string()]
    );
    assert!(emitter.names().contains(&EVENT_FILE_CHANGED));

    // target/ 产物变化被忽略：不提交、不发事件。
    let before = submitter.ops().len();
    engine.handle_events(&[fixture.root.join("repo/lib/target/classes/Demo.class")]);
    assert_eq!(submitter.ops().len(), before);
    assert_eq!(emitter.names().len(), 1);
}

/// 改 app 自身 → 只重建 app（不扩大到无关模块）。
#[test]
fn app_own_change_rebuilds_only_app() {
    let fixture = watch_fixture("app-only");
    let emitter = Arc::new(VecEmitter::default());
    let (engine, submitter) = test_engine(&fixture, emitter);
    engine.register_app_for_test(fixture.workspace_id, "app", Vec::new());

    engine.handle_events(&[fixture.root.join("repo/app/src/main/java/Application.java")]);

    let ops = submitter.ops();
    assert_eq!(ops.len(), 1);
    assert_eq!(ops[0].0, RuntimeOp::RebuildRestart);
    assert_eq!(ops[0].3, vec!["com.example:app".to_string()]);
}

/// pom.xml 变化 → 提交依赖模型重算（ResolveDependencies），不直接构建。
#[test]
fn pom_change_submits_dependency_resolve_not_build() {
    let fixture = watch_fixture("pom");
    let emitter = Arc::new(VecEmitter::default());
    let (engine, submitter) = test_engine(&fixture, emitter);
    engine.register_app_for_test(fixture.workspace_id, "app", Vec::new());

    engine.handle_events(&[fixture.root.join("repo/lib/pom.xml")]);

    let ops = submitter.ops();
    assert_eq!(ops.len(), 1);
    assert_eq!(ops[0].0, RuntimeOp::ResolveDependencies);
    assert!(ops[0].2.is_empty(), "resolve 任务不带 runtime 名");
}

/// §4.7 同批事件收敛为一个任务：一批多路径只提交一次 RebuildRestart。
#[test]
fn same_batch_of_events_converges_into_single_task() {
    let fixture = watch_fixture("converge");
    let emitter = Arc::new(VecEmitter::default());
    let (engine, submitter) = test_engine(&fixture, emitter);
    engine.register_app_for_test(fixture.workspace_id, "app", Vec::new());

    let batch = vec![
        fixture.root.join("repo/lib/src/main/java/A.java"),
        fixture.root.join("repo/lib/src/main/java/B.java"),
        fixture.root.join("repo/app/src/main/java/Main.java"),
    ];
    engine.handle_events(&batch);

    let ops = submitter.ops();
    assert_eq!(ops.len(), 1, "同批事件只提交一个任务: {ops:?}");
    assert_eq!(ops[0].0, RuntimeOp::RebuildRestart);
    assert_eq!(
        ops[0].3,
        vec!["com.example:app".to_string(), "com.example:lib".to_string()]
    );
}

/// 构建中收到新变化 → 排队合并（in_flight 合并语义），不重复提交。
#[test]
fn changes_during_in_flight_rebuild_are_queued_and_merged() {
    let fixture = watch_fixture("merge");
    let emitter = Arc::new(VecEmitter::default());
    let (engine, submitter) = test_engine(&fixture, emitter);
    engine.register_app_for_test(fixture.workspace_id, "app", Vec::new());

    let lib_a = fixture.root.join("repo/lib/src/main/java/A.java");
    let lib_b = fixture.root.join("repo/lib/src/main/java/B.java");
    engine.handle_events(std::slice::from_ref(&lib_a));
    // 在途期间第二波变化：入队，不重复提交。
    engine.handle_events(&[lib_b]);
    assert_eq!(submitter.ops().len(), 1, "merged into one submission");

    let apps = engine.apps.lock().unwrap();
    let state = apps
        .get(&(fixture.workspace_id, "app".to_string()))
        .expect("watched app state");
    assert!(state.in_flight);
    assert!(
        state.pending_modules.contains("com.example:lib"),
        "queued module for the next rebuild: {:?}",
        state.pending_modules
    );
}

/// §42 归队：在途任务完成后（进程回到 running）有待处理变更 → 再提交。
#[test]
fn queued_changes_resubmit_after_process_returns_to_running() {
    let fixture = watch_fixture("resubmit");
    let emitter = Arc::new(VecEmitter::default());
    let (engine, submitter) = test_engine(&fixture, emitter);
    engine.register_app_for_test(fixture.workspace_id, "app", Vec::new());

    let lib_a = fixture.root.join("repo/lib/src/main/java/A.java");
    engine.handle_events(&[lib_a]);
    assert_eq!(submitter.ops().len(), 1);
    // 在途期间又改一处 → 排队。
    engine.handle_events(&[fixture.root.join("repo/lib/src/main/java/B.java")]);
    assert_eq!(submitter.ops().len(), 1);

    // 模拟重启完成：进程行回到 running；等待归队冷却窗口。
    {
        let conn = fixture.db.lock().unwrap();
        conn.execute(
            "INSERT INTO runtime_processes (workspace_id, runtime_name, status, started_at, updated_at)
                 VALUES (?1, 'app', 'running', 't', 't')",
            [fixture.workspace_id],
        )
        .unwrap();
    }
    std::thread::sleep(RESTART_RESUBMIT_COOLDOWN + Duration::from_millis(100));
    engine.sync_running_apps();

    let ops = submitter.ops();
    assert_eq!(ops.len(), 2, "queued change resubmitted: {ops:?}");
    assert_eq!(ops[1].0, RuntimeOp::RebuildRestart);
    assert!(ops[1].3.contains(&"com.example:lib".to_string()));
}

/// watch 引擎把受影响 GA 子集写进 RuntimeTaskOptions.affectedModules，
/// 经 IPC serde round-trip 保持（契约防回归）。
#[test]
fn affected_modules_survive_ipc_serde() {
    let options = RuntimeTaskOptions {
        affected_modules: vec!["com.example:lib".into(), "com.example:app".into()],
        ..Default::default()
    };
    let json = serde_json::to_value(&options).unwrap();
    assert_eq!(
        json["affectedModules"],
        serde_json::json!(["com.example:lib", "com.example:app"])
    );
    let back: RuntimeTaskOptions = serde_json::from_value(json).unwrap();
    assert_eq!(back, options);

    // 旧客户端不带该字段 → 缺省空（向后兼容）。
    let legacy: RuntimeTaskOptions = serde_json::from_value(serde_json::json!({})).unwrap();
    assert!(legacy.affected_modules.is_empty());
}

/// 测试用假任务提交端：只记录提交的请求。
struct RecordingSubmitter {
    requests: Mutex<Vec<TaskRequest>>,
}

impl RecordingSubmitter {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            requests: Mutex::new(Vec::new()),
        })
    }

    fn ops(&self) -> Vec<(RuntimeOp, i64, String, Vec<String>)> {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .map(|r| match &r.task_type {
                TaskType::Runtime {
                    op,
                    workspace_id,
                    runtime_name,
                    options,
                } => (
                    *op,
                    *workspace_id,
                    runtime_name.clone(),
                    options.affected_modules.clone(),
                ),
                _ => panic!("unexpected non-runtime task"),
            })
            .collect()
    }
}

impl WatchTaskSubmitter for RecordingSubmitter {
    fn submit(&self, request: TaskRequest) -> crate::error::AppResult<String> {
        self.requests.lock().unwrap().push(request);
        Ok(format!("task-{}", self.requests.lock().unwrap().len()))
    }
}
