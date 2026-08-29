use std::collections::BTreeMap;
use std::fs;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use super::*;
use crate::runtime::build::runner::{FakeMavenRunner, FakeRun};
use crate::runtime::build::scheduler::BuildScheduler;
use crate::runtime::build::LaunchPlan;
use crate::runtime::config::{
    create_config, CreateRuntimeConfigRequest, RuntimeApplicationConfig,
};
use crate::test_support::write;

/// 单仓多模块 fixture：parent(pom) + lib(jar) + app(jar，依赖 lib)。
struct Fixture {
    root: PathBuf,
    db: Arc<Mutex<Connection>>,
    workspace_id: i64,
}


fn setup_fixture(name: &str) -> Fixture {
    let root = std::env::temp_dir().join(format!(
        "gw_r09_pipe_{name}_{}",
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
    ));
    fs::create_dir_all(&root).unwrap();
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
    // T-01 Scanner 靠 `.git` 标记发现仓库（同 maven_gen 的做法）。
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
    crate::maven::sync_workspace_index(
        &mut conn,
        workspace_id,
        &discovery,
        &root.join("m2"),
    )
    .unwrap();
    Fixture {
        root,
        db: Arc::new(Mutex::new(conn)),
        workspace_id,
    }
}

impl Fixture {
    fn app_pom_path(&self) -> String {
        self.root
            .join("repo/app/pom.xml")
            .to_string_lossy()
            .to_string()
    }

    fn create_runtime(
        &mut self,
        name: &str,
        mutate: impl FnOnce(&mut RuntimeApplicationConfig),
    ) {
        let mut config = RuntimeApplicationConfig {
            name: name.into(),
            project: self.app_pom_path(),
            main_class: Some("com.example.app.Application".into()),
            ..Default::default()
        };
        mutate(&mut config);
        create_config(
            &self.db.lock().unwrap(),
            &CreateRuntimeConfigRequest {
                workspace_id: self.workspace_id,
                config,
            },
        )
        .unwrap();
    }
}

/// 收集转发行的 sink，断言脱敏用。
struct VecSink(Vec<(OutputStream, String)>);
impl BuildOutputSink for VecSink {
    fn on_line(&mut self, stream: OutputStream, line: &str) {
        self.0.push((stream, line.to_string()));
    }
}

fn build_with(
    fixture: &mut Fixture,
    runner: &dyn MavenRunner,
    options: BuildOptions,
    sink: &mut dyn BuildOutputSink,
    cancel: Option<&AtomicBool>,
) -> AppResult<BuildOutcome> {
    let graph_cache = DependencyGraphCache::new();
    let closure_cache = RuntimeClosureCache::new();
    let scheduler = BuildScheduler::new(2);
    let request = BuildRequest {
        workspace_id: fixture.workspace_id,
        runtime_name: "app".into(),
        options,
    };
    let root = fixture.root.clone();
    execute_build(
        &fixture.db,
        &root,
        &graph_cache,
        &closure_cache,
        &scheduler,
        runner,
        &request,
        &ScriptApprovalStore::new(fixture.root.join("approvals.json")),
        sink,
        cancel,
    )
}

fn classpath_content(entries: &[PathBuf]) -> String {
    std::env::join_paths(entries)
        .unwrap()
        .to_string_lossy()
        .into_owned()
}

#[test]
fn classpath_run_full_pipeline_with_fake_maven() {
    let mut fixture = setup_fixture("classpath");
    fixture.create_runtime("app", |_| {});
    let cp = classpath_content(&[PathBuf::from("/m2/lib-dep.jar")]);
    let runner = FakeMavenRunner::new(vec![
        FakeRun {
            lines: vec![(OutputStream::Stdout, "[INFO] BUILD SUCCESS".into())],
            ..Default::default()
        },
        FakeRun {
            output_file_content: Some(cp),
            ..Default::default()
        },
    ]);
    let mut sink = VecSink(Vec::new());

    let outcome = build_with(
        &mut fixture,
        &runner,
        BuildOptions::default(),
        &mut sink,
        None,
    )
    .unwrap();

    assert_eq!(outcome.strategy, RunStrategy::ClasspathRun);
    assert_eq!(outcome.reactor_kind, crate::maven::reactor::RuntimeReactorKind::Existing);
    assert_eq!(
        outcome.modules_built,
        ["com.example:lib".to_string(), "com.example:app".to_string()]
    );
    assert!(outcome.build_command_preview.contains("compile"));
    assert!(outcome.build_command_preview.contains("-pl"));
    assert!(outcome.build_command_preview.contains("-am"));

    // 两次 Maven 调用：compile + process-classes/dependency:build-classpath。
    let requests = runner.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(
        requests[1].goals,
        ["process-classes", "dependency:build-classpath"]
    );

    // LaunchPlan：target/classes 在首元素，依赖 jar 随后。
    let LaunchPlan::JavaClasspath {
        classpath, main_class, ..
    } = &outcome.launch
    else {
        panic!("expected JavaClasspath");
    };
    assert_eq!(main_class, "com.example.app.Application");
    assert!(classpath[0].ends_with("target/classes"));
    assert!(classpath.contains(&PathBuf::from("/m2/lib-dep.jar")));

    // 缓存文件落在 .gitworkspace 下。
    let cache_dir = fixture
        .root
        .join(".gitworkspace/runtime/app/classpath");
    assert!(fs::read_dir(&cache_dir).unwrap().count() == 1);

    // 第二次构建：classpath 缓存命中，只有 compile 一次调用。
    let mut sink = VecSink(Vec::new());
    let outcome = build_with(
        &mut fixture,
        &runner,
        BuildOptions::default(),
        &mut sink,
        None,
    )
    .unwrap();
    assert_eq!(runner.request_count(), 3, "cache hit must skip maven");
    assert!(matches!(outcome.launch, LaunchPlan::JavaClasspath { .. }));
    let _ = fs::remove_dir_all(fixture.root);
}

#[test]
fn maven_run_preview_scopes_to_app_without_am() {
    let mut fixture = setup_fixture("mavenrun");
    fixture.create_runtime("app", |_| {});
    let runner = FakeMavenRunner::successful();
    let mut sink = VecSink(Vec::new());

    let outcome = build_with(
        &mut fixture,
        &runner,
        BuildOptions {
            strategy: Some(RunStrategy::MavenRun),
            ..Default::default()
        },
        &mut sink,
        None,
    )
    .unwrap();

    let LaunchPlan::MavenGoal { request, preview, .. } = &outcome.launch else {
        panic!("expected MavenGoal");
    };
    assert_eq!(request.goals, ["spring-boot:run"]);
    assert!(preview.contains("spring-boot:run"));
    assert!(preview.contains("-pl"));
    assert!(!preview.contains("-am"));
    let _ = fs::remove_dir_all(fixture.root);
}

#[test]
fn package_run_launch_uses_packaged_jar() {
    let mut fixture = setup_fixture("packagerun");
    fixture.create_runtime("app", |config| {
        config.profile = Some("prod".into());
    });
    // FakeMavenRunner 不真打包：预置 jar 产物。
    let jar = fixture.root.join("repo/app/target/app-1.0.0.jar");
    write(&jar, "jar");
    let runner = FakeMavenRunner::successful();
    let mut sink = VecSink(Vec::new());

    // prod profile → 不显式指定时默认 PackageRun。
    let outcome = build_with(
        &mut fixture,
        &runner,
        BuildOptions::default(),
        &mut sink,
        None,
    )
    .unwrap();

    assert_eq!(outcome.strategy, RunStrategy::PackageRun);
    // package 构建带 -DskipTests（默认 skip_tests = true）。
    assert!(runner.requests()[0]
        .extra_args
        .contains(&"-DskipTests".to_string()));
    let LaunchPlan::JavaJar { jar_path, vm_options, .. } = &outcome.launch else {
        panic!("expected JavaJar");
    };
    assert_eq!(jar_path, &jar);
    assert!(vm_options.contains(&"-Dspring.profiles.active=prod".to_string()));
    let _ = fs::remove_dir_all(fixture.root);
}

#[test]
fn failed_build_returns_structured_build_failed() {
    let mut fixture = setup_fixture("failed");
    fixture.create_runtime("app", |_| {});
    let runner = FakeMavenRunner::new(vec![FakeRun {
        lines: vec![
            (OutputStream::Stderr, "[ERROR] COMPILATION ERROR".into()),
            (
                OutputStream::Stderr,
                "[ERROR]   com.example:app ......... FAILURE [ 0.2 s]".into(),
            ),
        ],
        exit_code: Some(1),
        ..Default::default()
    }]);
    let mut sink = VecSink(Vec::new());

    let error = build_with(
        &mut fixture,
        &runner,
        BuildOptions::default(),
        &mut sink,
        None,
    )
    .unwrap_err();

    let AppError::BuildFailed {
        module,
        exit_code,
        log_tail,
    } = &error
    else {
        panic!("expected BuildFailed, got {error:?}");
    };
    assert_eq!(module, "com.example:app", "reactor summary names the module");
    assert_eq!(*exit_code, Some(1));
    assert!(log_tail.contains("COMPILATION ERROR"));
    assert_eq!(error.code(), "BuildFailed");
    let _ = fs::remove_dir_all(fixture.root);
}

#[test]
fn cancelled_build_maps_to_task_error() {
    let mut fixture = setup_fixture("cancel");
    fixture.create_runtime("app", |_| {});
    let runner = FakeMavenRunner::new(vec![FakeRun {
        duration: Some(Duration::from_secs(60)),
        ..Default::default()
    }]);
    let cancel = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&cancel);
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(150));
        flag.store(true, Ordering::Relaxed);
    });
    let mut sink = VecSink(Vec::new());
    let start = Instant::now();

    let error = build_with(
        &mut fixture,
        &runner,
        BuildOptions::default(),
        &mut sink,
        Some(&cancel),
    )
    .unwrap_err();

    assert_eq!(error.code(), "TaskError");
    assert!(error.to_string().contains("cancelled"));
    assert!(start.elapsed() < Duration::from_secs(10));
    let _ = fs::remove_dir_all(fixture.root);
}

#[test]
fn sensitive_environment_values_are_masked_in_stream() {
    let mut fixture = setup_fixture("mask");
    fixture.create_runtime("app", |config| {
        config.environment = BTreeMap::from([(
            "DB_PASSWORD".into(),
            "supersecret-value".into(),
        )]);
    });
    let runner = FakeMavenRunner::new(vec![
        FakeRun {
            lines: vec![(
                OutputStream::Stdout,
                "connecting with supersecret-value now".into(),
            )],
            ..Default::default()
        },
        FakeRun {
            output_file_content: Some(classpath_content(&[])),
            ..Default::default()
        },
    ]);
    let mut sink = VecSink(Vec::new());

    build_with(
        &mut fixture,
        &runner,
        BuildOptions::default(),
        &mut sink,
        None,
    )
    .unwrap();

    let forwarded = sink
        .0
        .iter()
        .map(|(_, line)| line.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(!forwarded.contains("supersecret-value"), "{forwarded}");
    assert!(forwarded.contains(config::MASKED_VALUE));
    let _ = fs::remove_dir_all(fixture.root);
}

#[test]
fn configured_jdk_injects_java_home_and_unknown_jdk_fails_fast() {
    let mut fixture = setup_fixture("jdk");
    crate::java::registry::upsert_jdk(
        &fixture.db.lock().unwrap(),
        &{
            let mut jdk = crate::java::model::JdkInstallation::new(
                "/jdk-21",
                crate::java::model::JdkDiscoverySource::System,
            );
            jdk.major_version = Some(21);
            jdk.is_valid = true;
            jdk
        },
    )
    .unwrap();
    fixture.create_runtime("app", |config| {
        config.jdk = Some("21".into());
    });
    let runner = FakeMavenRunner::successful();
    let mut sink = VecSink(Vec::new());
    build_with(
        &mut fixture,
        &runner,
        BuildOptions {
            strategy: Some(RunStrategy::MavenRun),
            ..Default::default()
        },
        &mut sink,
        None,
    )
    .unwrap();
    let env = &runner.envs()[0];
    assert!(env.contains(&("JAVA_HOME".to_string(), "/jdk-21".to_string())));

    // 未知 JDK：在任何 Maven 调用前失败。
    fixture.create_runtime("broken", |config| {
        config.jdk = Some("8".into());
    });
    let request = BuildRequest {
        workspace_id: fixture.workspace_id,
        runtime_name: "broken".into(),
        options: BuildOptions::default(),
    };
    let graph_cache = DependencyGraphCache::new();
    let closure_cache = RuntimeClosureCache::new();
    let scheduler = BuildScheduler::new(1);
    let root = fixture.root.clone();
    let mut sink = VecSink(Vec::new());
    let error = execute_build(
        &fixture.db,
        &root,
        &graph_cache,
        &closure_cache,
        &scheduler,
        &runner,
        &request,
        &ScriptApprovalStore::new(fixture.root.join("approvals.json")),
        &mut sink,
        None,
    )
    .unwrap_err();
    assert_eq!(error.code(), "JdkNotFound");
    assert_eq!(runner.request_count(), 1, "no maven run after JDK failure");
    let _ = fs::remove_dir_all(fixture.root);
}

#[test]
fn unknown_build_engine_and_unknown_project_are_actionable() {
    let mut fixture = setup_fixture("engine");
    fixture.create_runtime("gradle-app", |config| {
        config.build_engine = Some("gradle".into());
    });
    let runner = FakeMavenRunner::successful();
    let mut sink = VecSink(Vec::new());
    let graph_cache = DependencyGraphCache::new();
    let closure_cache = RuntimeClosureCache::new();
    let scheduler = BuildScheduler::new(1);
    let request = BuildRequest {
        workspace_id: fixture.workspace_id,
        runtime_name: "gradle-app".into(),
        options: BuildOptions::default(),
    };
    let root = fixture.root.clone();
    let error = execute_build(
        &fixture.db,
        &root,
        &graph_cache,
        &closure_cache,
        &scheduler,
        &runner,
        &request,
        &ScriptApprovalStore::new(fixture.root.join("approvals.json")),
        &mut sink,
        None,
    )
    .unwrap_err();
    assert_eq!(error.code(), "RuntimeConfigError");

    fixture.create_runtime("missing", |config| {
        config.project = "no-such-project".into();
    });
    let request = BuildRequest {
        workspace_id: fixture.workspace_id,
        runtime_name: "missing".into(),
        options: BuildOptions::default(),
    };
    let error = execute_build(
        &fixture.db,
        &fixture.root.clone(),
        &graph_cache,
        &closure_cache,
        &scheduler,
        &runner,
        &request,
        &ScriptApprovalStore::new(fixture.root.join("approvals.json")),
        &mut sink,
        None,
    )
    .unwrap_err();
    assert_eq!(error.code(), "ProjectNotFound");
    let _ = fs::remove_dir_all(fixture.root);
}

// ------------------------------------------------------------------
// R-14 §75 Command Safety：Pre/Post Script 确认流（不依赖 fixture，
// 平台 shell 直测：Windows `cmd /C`，其他 `sh -c`）
// ------------------------------------------------------------------

fn temp_approvals(tag: &str) -> (ScriptApprovalStore, PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "gw_r14_script_{tag}_{}",
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    (ScriptApprovalStore::new(dir.join("approvals.json")), dir)
}

fn dummy_request(workspace_id: i64, runtime_name: &str) -> BuildRequest {
    BuildRequest {
        workspace_id,
        runtime_name: runtime_name.into(),
        options: BuildOptions::default(),
    }
}

fn run_script_via(
    script: &str,
    script_type: &str,
    root: &Path,
    store: &ScriptApprovalStore,
    request: &BuildRequest,
    sink: &mut VecSink,
) -> AppResult<()> {
    run_user_script(
        script,
        script_type,
        root,
        store,
        request,
        &mut RedactingSink::new(vec![], sink),
        None,
    )
}

#[test]
fn unapproved_script_blocks_with_confirmation_error_and_does_not_run() {
    let (store, dir) = temp_approvals("unapproved");
    let mut sink = VecSink(Vec::new());
    let error = run_script_via(
        "echo must-not-run",
        "pre",
        &dir,
        &store,
        &dummy_request(1, "app"),
        &mut sink,
    )
    .unwrap_err();
    assert_eq!(error.code(), "ScriptConfirmationRequired");
    assert!(sink.0.is_empty(), "unapproved script must not execute");
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn approved_script_executes_forwarding_marked_output_and_records() {
    let (store, dir) = temp_approvals("approved");
    let hash = crate::runtime::script_approval::script_hash("echo pre-build-ran");
    store
        .approve(1, "app", "pre", &hash, "echo pre-build-ran")
        .unwrap();
    let mut sink = VecSink(Vec::new());
    run_script_via(
        "echo pre-build-ran",
        "pre",
        &dir,
        &store,
        &dummy_request(1, "app"),
        &mut sink,
    )
    .expect("approved script must run");
    assert!(
        sink.0
            .iter()
            .any(|(_, line)| line.contains("[pre-build]") && line.contains("pre-build-ran")),
        "script output must be forwarded with the [pre-build] prefix: {:?}",
        sink.0
    );
    // 「确认后执行且记录」。
    let entry = store
        .list()
        .into_iter()
        .find(|a| a.script_hash == hash)
        .expect("approval entry exists");
    assert!(entry.last_executed_at.is_some(), "execution must be recorded");
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn failing_script_maps_to_script_failed() {
    let (store, dir) = temp_approvals("failing");
    let hash = crate::runtime::script_approval::script_hash("exit 3");
    store.approve(1, "app", "post", &hash, "exit 3").unwrap();
    let mut sink = VecSink(Vec::new());
    let error = run_script_via(
        "exit 3",
        "post",
        &dir,
        &store,
        &dummy_request(1, "app"),
        &mut sink,
    )
    .unwrap_err();
    assert_eq!(error.code(), "ScriptFailed");
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn script_content_change_requires_reapproval() {
    let (store, dir) = temp_approvals("reapprove");
    let h1 = crate::runtime::script_approval::script_hash("echo v1");
    store.approve(1, "app", "pre", &h1, "echo v1").unwrap();
    assert!(store.is_approved(1, "app", "pre", &h1));
    assert!(
        !store.is_approved(1, "app", "pre", &crate::runtime::script_approval::script_hash("echo v2")),
        "script content change must invalidate the approval"
    );
    let _ = fs::remove_dir_all(dir);
}

// ------------------------------------------------------------------
// 真实 Maven 集成测试：`mvn` 不在 PATH 时 skip 并标注。
// 允许联网下载（不加 -o）；每用例独立 temp workspace。
// ------------------------------------------------------------------

const SPRING_BOOT_VERSION: &str = "3.2.5";
const INTEGRATION_TIMEOUT: Duration = Duration::from_secs(600);

fn maven_available() -> bool {
    let maven = if cfg!(windows) { "mvn.cmd" } else { "mvn" };
    std::process::Command::new(maven)
        .arg("-version")
        .output()
        .is_ok()
}

/// 统计 Maven 调用次数的 sink（每次 mvn 启动都会打印该横幅）。
struct MavenCountingSink {
    invocations: usize,
    tail: RingTail,
}

impl BuildOutputSink for MavenCountingSink {
    fn on_line(&mut self, stream: OutputStream, line: &str) {
        if line.contains("Scanning for projects") {
            self.invocations += 1;
        }
        self.tail.on_line(stream, line);
    }
}

fn spring_boot_parent_pom(modules: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<project xmlns="http://maven.apache.org/POM/4.0.0">
  <modelVersion>4.0.0</modelVersion>
  <groupId>com.r09</groupId>
  <artifactId>r09-parent</artifactId>
  <version>1.0.0</version>
  <packaging>pom</packaging>
  <modules>{modules}</modules>
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

fn lib_pom_standalone() -> String {
    r#"<?xml version="1.0" encoding="UTF-8"?>
<project xmlns="http://maven.apache.org/POM/4.0.0">
  <modelVersion>4.0.0</modelVersion>
  <groupId>com.r09</groupId>
  <artifactId>lib</artifactId>
  <version>1.0.0</version>
  <properties>
    <maven.compiler.source>17</maven.compiler.source>
    <maven.compiler.target>17</maven.compiler.target>
  </properties>
</project>
"#
    .to_string()
}

fn app_pom_standalone() -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<project xmlns="http://maven.apache.org/POM/4.0.0">
  <modelVersion>4.0.0</modelVersion>
  <groupId>com.r09</groupId>
  <artifactId>app</artifactId>
  <version>1.0.0</version>
  <properties>
    <maven.compiler.source>17</maven.compiler.source>
    <maven.compiler.target>17</maven.compiler.target>
  </properties>
  <dependencies>
    <dependency>
      <groupId>com.r09</groupId>
      <artifactId>lib</artifactId>
      <version>1.0.0</version>
    </dependency>
    <dependency>
      <groupId>org.springframework.boot</groupId>
      <artifactId>spring-boot-starter</artifactId>
    </dependency>
  </dependencies>
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
  <build>
    <plugins>
      <plugin>
        <groupId>org.springframework.boot</groupId>
        <artifactId>spring-boot-maven-plugin</artifactId>
        <executions>
          <execution><goals><goal>repackage</goal></goals></execution>
        </executions>
      </plugin>
    </plugins>
  </build>
</project>
"#
    )
}

fn app_pom_in_repo() -> String {
    r#"<?xml version="1.0" encoding="UTF-8"?>
<project xmlns="http://maven.apache.org/POM/4.0.0">
  <modelVersion>4.0.0</modelVersion>
  <parent>
    <groupId>com.r09</groupId>
    <artifactId>r09-parent</artifactId>
    <version>1.0.0</version>
  </parent>
  <artifactId>app</artifactId>
  <dependencies>
    <dependency>
      <groupId>com.r09</groupId>
      <artifactId>lib</artifactId>
      <version>1.0.0</version>
    </dependency>
    <dependency>
      <groupId>org.springframework.boot</groupId>
      <artifactId>spring-boot-starter</artifactId>
    </dependency>
  </dependencies>
  <build>
    <plugins>
      <plugin>
        <groupId>org.springframework.boot</groupId>
        <artifactId>spring-boot-maven-plugin</artifactId>
        <executions>
          <execution><goals><goal>repackage</goal></goals></execution>
        </executions>
      </plugin>
    </plugins>
  </build>
</project>
"#
    .to_string()
}

fn lib_pom_in_repo() -> String {
    r#"<?xml version="1.0" encoding="UTF-8"?>
<project xmlns="http://maven.apache.org/POM/4.0.0">
  <modelVersion>4.0.0</modelVersion>
  <parent>
    <groupId>com.r09</groupId>
    <artifactId>r09-parent</artifactId>
    <version>1.0.0</version>
  </parent>
  <artifactId>lib</artifactId>
</project>
"#
    .to_string()
}

fn write_java(dir: &Path, package_path: &str, class: &str, content: &str) {
    write(
        &dir.join("src/main/java").join(package_path).join(class),
        content,
    );
}

fn lib_source(dir: &Path) {
    write_java(
        dir,
        "com/r09/lib",
        "Lib.java",
        "package com.r09.lib;\n\npublic final class Lib {\n    private Lib() {}\n    public static String greet() { return \"hi\"; }\n}\n",
    );
}

fn app_source(dir: &Path) {
    write_java(
        dir,
        "com/r09/app",
        "Application.java",
        "package com.r09.app;\n\nimport org.springframework.boot.autoconfigure.SpringBootApplication;\n\n@SpringBootApplication\npublic class Application {\n    public static void main(String[] args) {\n        System.out.println(com.r09.lib.Lib.greet());\n    }\n}\n",
    );
}

fn register_workspace(root: &Path, repos: &[&str]) -> (Connection, i64) {
    let mut conn = Connection::open_in_memory().unwrap();
    crate::db::init_db(&mut conn).unwrap();
    conn.execute(
        "INSERT INTO workspaces (name, path, created_at, updated_at) VALUES ('w', ?1, 't', 't')",
        [root.to_string_lossy().to_string()],
    )
    .unwrap();
    let workspace_id = conn.last_insert_rowid();
    let scanned: Vec<crate::models::repository::ScannedRepo> = repos
        .iter()
        .map(|relative| crate::models::repository::ScannedRepo {
            path: root.join(relative).to_string_lossy().to_string(),
            name: relative.to_string(),
            relative_path: relative.to_string(),
            git_dir_mtime: None,
        })
        .collect();
    crate::db::dao::upsert_repositories_batch(&mut conn, workspace_id, &scanned).unwrap();
    let discovery = crate::maven::discover_poms(root, 6, None, None);
    assert!(discovery.errors.is_empty(), "{:?}", discovery.errors);
    crate::maven::sync_workspace_index(
        &mut conn,
        workspace_id,
        &discovery,
        &root.join("m2"),
    )
    .unwrap();
    (conn, workspace_id)
}

/// 单仓多模块 Spring Boot fixture：repo/(parent + lib + app)。
fn setup_single_repo_boot(name: &str) -> Fixture {
    let root = std::env::temp_dir().join(format!(
        "gw_r09_it_{name}_{}",
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
    ));
    fs::create_dir_all(&root).unwrap();
    write(
        &root.join("repo/pom.xml"),
        &spring_boot_parent_pom("<module>lib</module><module>app</module>"),
    );
    write(&root.join("repo/lib/pom.xml"), &lib_pom_in_repo());
    lib_source(&root.join("repo/lib"));
    write(&root.join("repo/app/pom.xml"), &app_pom_in_repo());
    app_source(&root.join("repo/app"));
    git2::Repository::init(root.join("repo")).unwrap();
    let (conn, workspace_id) = register_workspace(&root, &["repo"]);
    Fixture {
        root,
        db: Arc::new(Mutex::new(conn)),
        workspace_id,
    }
}

/// 跨仓 fixture：repo-lib(lib) + repo-app(app 跨仓依赖 lib) → Synthetic Reactor。
fn setup_cross_repo_boot(name: &str) -> Fixture {
    let root = std::env::temp_dir().join(format!(
        "gw_r09_it_{name}_{}",
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
    ));
    fs::create_dir_all(&root).unwrap();
    write(&root.join("repo-lib/pom.xml"), &lib_pom_standalone());
    lib_source(&root.join("repo-lib"));
    write(&root.join("repo-app/pom.xml"), &app_pom_standalone());
    app_source(&root.join("repo-app"));
    git2::Repository::init(root.join("repo-lib")).unwrap();
    git2::Repository::init(root.join("repo-app")).unwrap();
    let (conn, workspace_id) = register_workspace(&root, &["repo-lib", "repo-app"]);
    Fixture {
        root,
        db: Arc::new(Mutex::new(conn)),
        workspace_id,
    }
}

impl Fixture {
    fn create_boot_runtime(&mut self, app_pom: &str) {
        self.create_runtime("app", |config| {
            config.project = app_pom.to_string();
            config.main_class = Some("com.r09.app.Application".into());
        });
    }
}

fn real_build(
    fixture: &mut Fixture,
    strategy: RunStrategy,
    sink: &mut dyn BuildOutputSink,
    cancel: Option<&AtomicBool>,
) -> AppResult<BuildOutcome> {
    real_build_opts(
        fixture,
        BuildOptions {
            strategy: Some(strategy),
            timeout: Some(INTEGRATION_TIMEOUT),
            ..Default::default()
        },
        sink,
        cancel,
    )
}

fn real_build_opts(
    fixture: &mut Fixture,
    options: BuildOptions,
    sink: &mut dyn BuildOutputSink,
    cancel: Option<&AtomicBool>,
) -> AppResult<BuildOutcome> {
    let runner = crate::runtime::build::runner::SpawningMavenRunner;
    build_with(fixture, &runner, options, sink, cancel)
}

#[test]
fn package_run_builds_spring_boot_app_with_real_maven() {
    if !maven_available() {
        eprintln!("R-09: no `mvn` on PATH; skipping package_run integration test");
        return;
    }
    let mut fixture = setup_single_repo_boot("package");
    fixture.create_boot_runtime(&fixture.app_pom_path());
    let mut sink = MavenCountingSink {
        invocations: 0,
        tail: RingTail::new(),
    };

    let outcome = real_build(&mut fixture, RunStrategy::PackageRun, &mut sink, None)
        .unwrap_or_else(|error| panic!("package build failed: {error}\n{}", sink.tail.tail()));

    assert_eq!(outcome.strategy, RunStrategy::PackageRun);
    assert_eq!(
        outcome.reactor_kind,
        crate::maven::reactor::RuntimeReactorKind::Existing
    );
    let LaunchPlan::JavaJar { jar_path, .. } = &outcome.launch else {
        panic!("expected JavaJar");
    };
    assert!(jar_path.is_file(), "repackaged jar must exist: {jar_path:?}");
    let _ = fs::remove_dir_all(fixture.root);
}

#[test]
fn classpath_run_resolves_and_caches_classpath_with_real_maven() {
    if !maven_available() {
        eprintln!("R-09: no `mvn` on PATH; skipping classpath_run integration test");
        return;
    }
    let mut fixture = setup_single_repo_boot("classpath");
    fixture.create_boot_runtime(&fixture.app_pom_path());
    let mut sink = MavenCountingSink {
        invocations: 0,
        tail: RingTail::new(),
    };

    // 第一次：compile + dependency:build-classpath 两次 Maven 调用。
    let outcome = real_build(&mut fixture, RunStrategy::ClasspathRun, &mut sink, None)
        .unwrap_or_else(|error| panic!("classpath build failed: {error}\n{}", sink.tail.tail()));
    let after_first = sink.invocations;
    assert_eq!(after_first, 2, "compile + build-classpath");

    // 缓存文件含 spring-boot-starter jar 路径。
    let cache_dir = fixture.root.join(".gitworkspace/runtime/app/classpath");
    let cache_file = fs::read_dir(&cache_dir)
        .unwrap()
        .next()
        .expect("classpath cache file must exist")
        .unwrap()
        .path();
    let cached = classpath::read_classpath_file(&cache_file).unwrap();
    assert!(
        cached.iter().any(|entry| entry
            .to_string_lossy()
            .contains("spring-boot-starter")),
        "classpath must contain spring-boot-starter jar: {cached:?}"
    );

    // LaunchPlan：target/classes 在首元素。
    let LaunchPlan::JavaClasspath { classpath, .. } = &outcome.launch else {
        panic!("expected JavaClasspath");
    };
    assert!(classpath[0].ends_with("target/classes"));

    // 第二/三次：classpath 缓存稳定后命中，只剩 compile 一次调用。
    // （首次构建后的依赖来源刷新可能翻转图指纹 → 第二次可能仍重算；
    //   第三次必定命中。）关闭 R-18 依赖缓存，单独验证 R-09 classpath
    // 缓存语义；依赖缓存跳过行为由下方 dep-cache 专项测试覆盖。
    real_build_opts(
        &mut fixture,
        BuildOptions {
            strategy: Some(RunStrategy::ClasspathRun),
            timeout: Some(INTEGRATION_TIMEOUT),
            dependency_cache: false,
            ..Default::default()
        },
        &mut sink,
        None,
    )
    .unwrap_or_else(|error| panic!("second build failed: {error}\n{}", sink.tail.tail()));
    let before_third = sink.invocations;
    real_build_opts(
        &mut fixture,
        BuildOptions {
            strategy: Some(RunStrategy::ClasspathRun),
            timeout: Some(INTEGRATION_TIMEOUT),
            dependency_cache: false,
            ..Default::default()
        },
        &mut sink,
        None,
    )
    .unwrap_or_else(|error| panic!("third build failed: {error}\n{}", sink.tail.tail()));
    assert_eq!(
        sink.invocations - before_third,
        1,
        "third build must be compile-only (classpath cache hit)"
    );
    let _ = fs::remove_dir_all(fixture.root);
}

/// R-18 验收：未变化模块在二次构建中被跳过（构建日志可证
/// `[R-18] 依赖缓存命中`，Maven 调用数为 0）；修改源码后仅重建
/// 受影响子集。
#[test]
fn dependency_cache_skips_unchanged_modules_with_real_maven() {
    if !maven_available() {
        eprintln!("R-18: no `mvn` on PATH; skipping dependency cache integration test");
        return;
    }
    let mut fixture = setup_single_repo_boot("depcache");
    fixture.create_boot_runtime(&fixture.app_pom_path());
    let mut sink = MavenCountingSink {
        invocations: 0,
        tail: RingTail::new(),
    };

    // 第一次构建：全量（写入指纹缓存）。
    real_build(&mut fixture, RunStrategy::MavenRun, &mut sink, None)
        .unwrap_or_else(|error| panic!("first build failed: {error}\n{}", sink.tail.tail()));
    let after_first = sink.invocations;
    assert!(after_first >= 1);

    // 第二次构建：全部模块未变化 → 跳过 Maven 调用，日志可证。
    real_build(&mut fixture, RunStrategy::MavenRun, &mut sink, None)
        .unwrap_or_else(|error| panic!("second build failed: {error}\n{}", sink.tail.tail()));
    assert_eq!(
        sink.invocations,
        after_first,
        "unchanged modules must skip the maven build entirely"
    );
    assert!(
        sink.tail.tail().contains("[R-18] 依赖缓存命中"),
        "cache hit must be visible in build log: {}",
        sink.tail.tail()
    );

    // 修改 lib 源码 → 只重建 lib + app（-pl 子集，一次 Maven 调用）。
    let lib_java = fixture.root.join("repo/lib/src/main/java/com/r09/lib/Lib.java");
    std::fs::write(&lib_java, "package com.r09.lib;\n\npublic final class Lib {\n    public static String greet() { return \"hi v2\"; }\n}\n").unwrap();
    real_build(&mut fixture, RunStrategy::MavenRun, &mut sink, None)
        .unwrap_or_else(|error| panic!("incremental build failed: {error}\n{}", sink.tail.tail()));
    assert_eq!(
        sink.invocations,
        after_first + 1,
        "incremental build must be exactly one maven call (-pl subset)"
    );
    assert!(
        sink.tail.tail().contains("[R-18] 依赖缓存：仅重建"),
        "subset rebuild must be visible in build log: {}",
        sink.tail.tail()
    );
    // 第三次：增量构建后全部未变 → 再次跳过。
    real_build(&mut fixture, RunStrategy::MavenRun, &mut sink, None)
        .unwrap_or_else(|error| panic!("post-increment build failed: {error}\n{}", sink.tail.tail()));
    assert_eq!(sink.invocations, after_first + 1);
    let _ = fs::remove_dir_all(fixture.root);
}

/// R-17 验收：watch 影响分析给出的 affected_modules 必建子集——即使
/// R-18 指纹判定 SkipAll，也必须执行一次 `-pl` 增量构建（显式变更
/// 信号优先于「指纹未变」），且 `-pl` 含受影响模块。
#[test]
fn affected_modules_override_dependency_cache_skip_with_real_maven() {
    if !maven_available() {
        eprintln!("R-17: no `mvn` on PATH; skipping affected-modules integration test");
        return;
    }
    let mut fixture = setup_single_repo_boot("r17affected");
    fixture.create_boot_runtime(&fixture.app_pom_path());
    let mut sink = MavenCountingSink {
        invocations: 0,
        tail: RingTail::new(),
    };

    // 第一次构建：全量（写入指纹缓存）。
    real_build(&mut fixture, RunStrategy::MavenRun, &mut sink, None)
        .unwrap_or_else(|error| panic!("first build failed: {error}\n{}", sink.tail.tail()));

    // 不改任何文件，但 watch 报告 lib 源码变更（R-17 影响分析结果）→
    // 指纹本会 SkipAll；affected_modules 必须覆盖为一次 -pl 构建。
    let outcome = real_build_opts(
        &mut fixture,
        BuildOptions {
            strategy: Some(RunStrategy::MavenRun),
            timeout: Some(INTEGRATION_TIMEOUT),
            affected_modules: vec!["com.r09:lib".into()],
            ..Default::default()
        },
        &mut sink,
        None,
    )
    .unwrap_or_else(|error| panic!("affected build failed: {error}\n{}", sink.tail.tail()));
    assert_eq!(
        sink.invocations,
        2,
        "affected_modules must override the SkipAll verdict with exactly one maven call"
    );
    assert!(
        sink.tail.tail().contains("[R-17] 文件变更"),
        "R-17 affected rebuild must be visible in build log: {}",
        sink.tail.tail()
    );
    assert!(
        outcome.build_command_preview.contains("com.r09:lib"),
        "-pl subset must contain the affected module: {}",
        outcome.build_command_preview
    );
    let _ = fs::remove_dir_all(fixture.root);
}

#[test]
fn maven_run_builds_and_previews_spring_boot_run_with_real_maven() {
    if !maven_available() {
        eprintln!("R-09: no `mvn` on PATH; skipping maven_run integration test");
        return;
    }
    let mut fixture = setup_single_repo_boot("mavenrun");
    fixture.create_boot_runtime(&fixture.app_pom_path());
    let mut sink = MavenCountingSink {
        invocations: 0,
        tail: RingTail::new(),
    };

    // 只断言 build 成功 + preview；不真跑 spring-boot:run。
    let outcome = real_build(&mut fixture, RunStrategy::MavenRun, &mut sink, None)
        .unwrap_or_else(|error| panic!("maven-run build failed: {error}\n{}", sink.tail.tail()));
    let LaunchPlan::MavenGoal { preview, request, .. } = &outcome.launch else {
        panic!("expected MavenGoal");
    };
    assert!(preview.contains("spring-boot:run"));
    assert!(preview.contains("-pl"));
    assert!(!preview.contains("-am"));
    assert_eq!(request.goals, ["spring-boot:run"]);
    let _ = fs::remove_dir_all(fixture.root);
}

#[test]
fn cross_repo_synthetic_reactor_builds_with_real_maven() {
    if !maven_available() {
        eprintln!("R-09: no `mvn` on PATH; skipping synthetic reactor integration test");
        return;
    }
    let mut fixture = setup_cross_repo_boot("xrepo");
    let app_pom = fixture
        .root
        .join("repo-app/pom.xml")
        .to_string_lossy()
        .to_string();
    fixture.create_boot_runtime(&app_pom);
    let mut sink = MavenCountingSink {
        invocations: 0,
        tail: RingTail::new(),
    };

    let outcome = real_build(&mut fixture, RunStrategy::PackageRun, &mut sink, None)
        .unwrap_or_else(|error| {
            panic!("synthetic reactor build failed: {error}\n{}", sink.tail.tail())
        });
    assert_eq!(
        outcome.reactor_kind,
        crate::maven::reactor::RuntimeReactorKind::Synthetic
    );
    assert!(outcome
        .reactor_pom
        .starts_with(fixture.root.join(".gitworkspace/runtime/app")));
    let LaunchPlan::JavaJar { jar_path, .. } = &outcome.launch else {
        panic!("expected JavaJar");
    };
    assert!(jar_path.is_file());
    let _ = fs::remove_dir_all(fixture.root);
}

#[test]
fn cancelling_real_maven_build_kills_process_tree() {
    if !maven_available() {
        eprintln!("R-09: no `mvn` on PATH; skipping cancel integration test");
        return;
    }
    let mut fixture = setup_single_repo_boot("cancel");
    fixture.create_boot_runtime(&fixture.app_pom_path());
    let marker = fixture.root.to_string_lossy().to_string();
    let cancel = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&cancel);
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(700));
        flag.store(true, Ordering::Relaxed);
    });
    let mut sink = MavenCountingSink {
        invocations: 0,
        tail: RingTail::new(),
    };
    let start = Instant::now();

    let error = real_build(&mut fixture, RunStrategy::PackageRun, &mut sink, Some(&cancel))
        .unwrap_err();

    assert_eq!(error.code(), "TaskError", "cancel must map to Task error: {error}");
    assert!(
        start.elapsed() < INTEGRATION_TIMEOUT,
        "cancel must interrupt the build, not wait for completion"
    );

    // 兜底：sysinfo 查无该 temp workspace 相关的残留 mvn/java 进程。
    std::thread::sleep(Duration::from_millis(500));
    let mut system = sysinfo::System::new_with_specifics(
        sysinfo::RefreshKind::new().with_processes(sysinfo::ProcessRefreshKind::new()),
    );
    system.refresh_processes();
    let survivors: Vec<String> = system
        .processes()
        .values()
        .filter(|process| {
            process
                .cmd()
                .iter()
                .any(|arg| arg.contains(&marker))
        })
        .map(|process| format!("{:?} {:?}", process.pid(), process.cmd()))
        .collect();
    assert!(survivors.is_empty(), "build process tree must be gone: {survivors:?}");
    let _ = fs::remove_dir_all(fixture.root);
}
