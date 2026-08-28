//! Build 流水线编排（R-09，§28）：
//! Validate JDK → Validate Maven → 依赖图 → Runtime Closure → Reactor →
//! Maven Build → Generate Classpath → LaunchPlan。
//!
//! 可观测性：构建输出经脱敏（T-08 共享规则 + 敏感环境值掩码，全局约束 §4）
//! 实时流式转发给 [`BuildOutputSink`]；引擎内部始终维护 [`RingTail`] 供
//! `BuildFailed.log_tail` 使用。取消传播与超时由
//! [`crate::process::streaming::spawn_streaming`] 杀进程树兜底。

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rusqlite::Connection;

use crate::error::{AppError, AppResult};
use crate::maven::closure::RuntimeClosureCache;
use crate::maven::executor;
use crate::maven::index::{DependencyGraph, DependencyGraphCache, MavenProjectNode};
use crate::maven::reactor::prepare_runtime_reactor;
use crate::process::streaming::{spawn_streaming, OutputStream, StreamingExit};
use crate::runtime::build::classpath;
use crate::runtime::build::runner::MavenRunner;
use crate::runtime::build::scheduler::BuildScheduler;
use crate::runtime::build::strategy;
use crate::runtime::build::{
    default_strategy, engine_for, BuildContext, BuildEngine, BuildOptions, BuildOutcome,
    BuildOutputSink, BuildRequest, RingTail, RunStrategy,
};
use crate::runtime::config;
use crate::runtime::logs::redact::{sensitive_env_values, LogRedactor};
use crate::runtime::script_approval::ScriptApprovalStore;

/// Maven Build Engine：[`engine_for("maven")`][engine_for] 返回的实现。
pub struct MavenBuildEngine;

impl BuildEngine for MavenBuildEngine {
    fn id(&self) -> &'static str {
        "maven"
    }

    fn build(
        &self,
        cx: &mut BuildContext<'_>,
        request: &BuildRequest,
        sink: &mut dyn BuildOutputSink,
        cancel: Option<&AtomicBool>,
    ) -> AppResult<BuildOutcome> {
        execute_build(
            cx.db,
            cx.workspace_root,
            cx.graph_cache,
            cx.closure_cache,
            cx.scheduler,
            cx.runner,
            request,
            cx.script_approvals,
            sink,
            cancel,
        )
    }
}

/// 执行一次完整 Build（§28 流水线）。
///
/// - 配置经内部未脱敏路径加载（`env` 含真实秘密，绝不外泄到 IPC）。
/// - Maven 构建在 [`BuildScheduler`] 的 permit 内执行（全局约束 §6）。
/// - 构建完成后 best-effort 刷新 `~/.m2` 依赖来源映射（失败只记日志）。
/// - DB 访问按阶段短持锁（R-12）：Maven 子进程运行期间**不持有** SQLite
///   单连接写锁，多个构建可以真正并行到 §66 上限，UI 查询也不被长构建
///   阻塞（R-09 初版由调用方跨整个构建持锁）。
#[allow(clippy::too_many_arguments)]
pub fn execute_build(
    db: &Arc<Mutex<Connection>>,
    workspace_root: &Path,
    graph_cache: &DependencyGraphCache,
    closure_cache: &RuntimeClosureCache,
    scheduler: &BuildScheduler,
    runner: &dyn MavenRunner,
    request: &BuildRequest,
    script_approvals: &ScriptApprovalStore,
    sink: &mut dyn BuildOutputSink,
    cancel: Option<&AtomicBool>,
) -> AppResult<BuildOutcome> {
    // ---- 1. 加载未脱敏 Runtime 配置 + 校验 Build Engine id ----
    let mut config = {
        let conn = db.lock().unwrap();
        config::load_config_unredacted(&conn, request.workspace_id, &request.runtime_name)?
    };
    // R-10 Launcher 的 R-06 推断回退：配置缺省 mainClass 时以推断值覆盖
    // （内存生效，不改用户配置文件）。
    if let Some(main_class) = &request.options.main_class_override {
        if config.main_class.is_none() {
            config.main_class = Some(main_class.clone());
        }
    }
    // 只认 "maven"；mvnd（R-18）/ Gradle（R-22）将来在这里分发。
    let _engine = engine_for(config.build_engine.as_deref().unwrap_or("maven"))?;
    let strategy = request
        .options
        .strategy
        .unwrap_or_else(|| default_strategy(config.injected_profile().as_deref()));

    // ---- 2. Validate JDK（配置了才校验）----
    let jdk = match config.jdk.as_deref() {
        Some(spec) => {
            let conn = db.lock().unwrap();
            Some(crate::java::resolve::resolve_jdk_for_config(&conn, spec)?)
        }
        None => None,
    };

    // ---- 3. 依赖图 + 根项目匹配 ----
    let graph = {
        let conn = db.lock().unwrap();
        graph_cache.get_or_load(&conn, request.workspace_id)?.graph
    };
    let root = find_root_project(&graph, &config.project)?;

    // ---- 4. Validate Maven（经 runner seam，可测试）----
    // F-16：应用级本地仓库覆盖优先于 settings.xml 探测。
    let local_repository = crate::maven::settings::resolve_local_repository_effective(None);
    let resolved = runner.resolve_maven(&strategy::module_directory(root), &local_repository)?;

    // ---- 5. Runtime Closure + Reactor ----
    // Scope 来自 Runtime 配置（R-03 §15，缺省 Auto；R-13 起 UI 可调）。
    let closure = closure_cache
        .get_or_compute(&graph, root.project_id, &config.scope)?
        .closure;
    let reactor = prepare_runtime_reactor(&graph, &closure, workspace_root, &request.runtime_name)?;

    // ---- 6. 构建环境：五层合并环境 + JAVA_HOME（不强行改 PATH）----
    let mut env: Vec<(String, String)> = {
        let conn = db.lock().unwrap();
        config::resolve_environment(&conn, request.workspace_id, &request.runtime_name)?
            .into_iter()
            .collect()
    };
    if let Some(jdk) = &jdk {
        env.push(("JAVA_HOME".into(), jdk.home_path.clone()));
    }

    // 脱敏转发 sink + 内部 RingTail（BuildFailed 上下文）。
    let mut redacting = RedactingSink::new(sensitive_env_values(&env), sink);

    // ---- 6.5 R-14 §75 Command Safety：Pre-Build Script（确认后执行）----
    if let Some(script) = config.pre_build_script.as_deref() {
        run_user_script(
            script,
            "pre",
            workspace_root,
            script_approvals,
            request,
            &mut redacting,
            cancel,
        )?;
    }

    // ---- 7. Maven Build（限流闸内）----
    let build_request = strategy::build_maven_request(
        &resolved.executable,
        workspace_root,
        &reactor,
        strategy,
        &request.options,
        Some(resolved.local_repository.clone()),
    );
    let build_preview = executor::preview_command(&build_request);
    // R-12：等 permit 期间响应任务取消（排队取消），拿到 permit 后的构建
    // 取消由 runner 的 50ms 轮询负责。
    let _permit = match cancel {
        Some(flag) => scheduler
            .acquire_cancelable(flag)
            .ok_or_else(|| AppError::Task("build cancelled by user（排队等待构建位时取消）".into()))?,
        None => scheduler.acquire(),
    };
    let start = Instant::now();
    let exit = runner.run(
        &build_request,
        &env,
        &mut redacting,
        cancel,
        request.options.timeout,
    )?;
    check_exit(&exit, root, &redacting, "build", request.options.timeout)?;

    // ---- 8. Classpath Run：解析/复用 classpath 缓存 ----
    let mut dependency_classpath = None;
    if strategy == RunStrategy::ClasspathRun {
        dependency_classpath = Some(resolve_classpath(
            runner,
            workspace_root,
            &request.runtime_name,
            &reactor,
            root,
            &graph.fingerprint,
            &resolved.executable,
            &resolved.local_repository,
            &request.options,
            &env,
            &mut redacting,
            cancel,
        )?);
    }
    let build_duration_ms = start.elapsed().as_millis();

    // ---- 8.5 R-14 §75 Command Safety：Post-Build Script（确认后执行）----
    if let Some(script) = config.post_build_script.as_deref() {
        run_user_script(
            script,
            "post",
            workspace_root,
            script_approvals,
            request,
            &mut redacting,
            cancel,
        )?;
    }

    // ---- 9. LaunchPlan + 结果 ----
    let launch = strategy::launch_plan(
        strategy,
        &strategy::LaunchInputs {
            config: &config,
            root,
            reactor: &reactor,
            executable: &resolved.executable,
            workspace_root,
            local_repository: Some(resolved.local_repository.clone()),
            env: env.clone(),
            jdk: jdk.as_ref(),
            classpath: dependency_classpath,
        },
    )?;

    // 构建可能改变了 ~/.m2：best-effort 刷新，失败不影响构建结果。
    {
        let mut conn = db.lock().unwrap();
        if let Err(error) = crate::maven::refresh_dependency_sources(
            &mut conn,
            request.workspace_id,
            &resolved.local_repository,
        ) {
            log::warn!("R-09: post-build refresh_dependency_sources failed: {error}");
        }
    }

    Ok(BuildOutcome {
        strategy,
        reactor_kind: reactor.kind,
        reactor_pom: reactor.pom_path.clone(),
        modules_built: closure.projects.iter().map(strategy::module_ga).collect(),
        build_duration_ms,
        build_command_preview: build_preview,
        launch,
    })
}

/// R-14 §75 Command Safety：执行用户 Pre/Post Build Script。
///
/// 规则：**默认禁止自动执行 shell script**——未确认（含脚本内容变更后）
/// 直接返回 [`AppError::ScriptConfirmationRequired`]，UI 弹出确认对话框；
/// 确认后执行并把输出（脱敏后）经 sink 转发，行首带 `[pre-build]` /
/// `[post-build]` 前缀；5 分钟超时 + 可取消；执行后记录 `last_executed_at`
/// （「确认后执行且记录」）。脚本失败 → [`AppError::ScriptFailed`]。
fn run_user_script(
    script: &str,
    script_type: &str,
    workspace_root: &Path,
    approvals: &ScriptApprovalStore,
    request: &BuildRequest,
    sink: &mut RedactingSink<'_>,
    cancel: Option<&AtomicBool>,
) -> AppResult<()> {
    const SCRIPT_TIMEOUT: Duration = Duration::from_secs(5 * 60);

    let hash = crate::runtime::script_approval::script_hash(script);
    if !approvals.is_approved(
        request.workspace_id,
        &request.runtime_name,
        script_type,
        &hash,
    ) {
        return Err(AppError::ScriptConfirmationRequired {
            workspace_id: request.workspace_id,
            runtime_name: request.runtime_name.clone(),
            script_type: script_type.to_string(),
            script_hash: hash,
            preview: crate::runtime::script_approval::script_preview(script),
        });
    }

    let mut command = user_script_command(script);
    command.current_dir(workspace_root);
    let prefix = format!("[{script_type}-build] ");
    let exit = spawn_streaming(&mut command, cancel, Some(SCRIPT_TIMEOUT), &mut |stream, line| {
        sink.on_line(stream, &format!("{prefix}{line}"));
    })?;
    approvals.record_execution(
        request.workspace_id,
        &request.runtime_name,
        script_type,
        &hash,
    )?;
    if exit.exit_code != Some(0) {
        return Err(AppError::ScriptFailed {
            script_type: script_type.to_string(),
            runtime: request.runtime_name.clone(),
            exit_code: exit.exit_code,
            log_tail: sink.tail(),
        });
    }
    Ok(())
}

/// 平台对应的 shell 执行器：Windows `cmd /C`，其他 `sh -c`。
fn user_script_command(script: &str) -> std::process::Command {
    #[cfg(windows)]
    {
        let mut command = std::process::Command::new("cmd");
        command.args(["/C", script]);
        command
    }
    #[cfg(not(windows))]
    {
        let mut command = std::process::Command::new("sh");
        command.args(["-c", script]);
        command
    }
}

/// 在依赖图中定位 Runtime 配置的根项目：path → artifactId → groupId:artifactId。
///
/// 路径匹配对 Windows 分隔符不敏感：R-02 索引把路径统一存为正斜杠
/// （`path_key`），而用户配置里的 project 可能是反斜杠——相等比较前
/// 两侧都归一化（Windows 真实 bug 修复，R-14）。
fn find_root_project<'a>(
    graph: &'a DependencyGraph,
    project: &str,
) -> AppResult<&'a MavenProjectNode> {
    let needle = project.replace('\\', "/");
    graph
        .projects
        .iter()
        .find(|node| normalize_path(&node.path.to_string_lossy()) == needle)
        .or_else(|| {
            graph
                .projects
                .iter()
                .find(|node| node.coordinates.artifact_id == project)
        })
        .or_else(|| {
            graph
                .projects
                .iter()
                .find(|node| strategy::module_ga(node) == project)
        })
        .ok_or_else(|| {
            AppError::ProjectNotFound(format!(
                "Runtime 配置的 project '{project}' 不在当前 Workspace 依赖图中；\
                 请重新选择 Runtime 的根项目"
            ))
        })
}

/// 路径归一化：Windows 反斜杠 → 正斜杠（与 R-02 `path_key` 一致）。
fn normalize_path(path: &str) -> String {
    path.replace('\\', "/")
}

/// Classpath Run 的 classpath 生成：缓存命中直接复用，否则驱动
/// `dependency:build-classpath` 写入缓存后读出。
#[allow(clippy::too_many_arguments)]
fn resolve_classpath(
    runner: &dyn MavenRunner,
    workspace_root: &Path,
    runtime_name: &str,
    reactor: &crate::maven::reactor::RuntimeReactorPlan,
    root: &MavenProjectNode,
    graph_fingerprint: &str,
    executable: &crate::maven::exec_model::MavenExecutable,
    local_repository: &Path,
    options: &BuildOptions,
    env: &[(String, String)],
    sink: &mut RedactingSink,
    cancel: Option<&AtomicBool>,
) -> AppResult<Vec<PathBuf>> {
    if let Some(entries) =
        classpath::cached_classpath(workspace_root, runtime_name, root, graph_fingerprint, local_repository)
    {
        log::info!(
            "R-09: classpath cache hit for {}",
            root.coordinates.artifact_id
        );
        return Ok(entries);
    }

    let dir = classpath::classpath_cache_dir(workspace_root, runtime_name);
    // R-14 §78 只读护栏：classpath 缓存只落 workspace/.gitworkspace。
    crate::runtime::guard::assert_workspace_write_path(&dir, workspace_root, "Classpath 缓存")?;
    let key = classpath::classpath_cache_key(root, graph_fingerprint, local_repository);
    let output_file =
        classpath::prepare_cache_write(&dir, &root.coordinates.artifact_id, &key)?;
    let request = classpath::build_classpath_request(
        executable,
        workspace_root,
        reactor,
        root,
        &output_file,
        options.offline,
        Some(local_repository.to_path_buf()),
    );
    let exit = runner.run(&request, env, sink, cancel, options.timeout)?;
    check_exit(&exit, root, sink, "dependency:build-classpath", options.timeout)?;

    if !output_file.is_file() {
        return Err(AppError::BuildFailed {
            module: strategy::module_ga(root),
            exit_code: Some(0),
            log_tail: format!(
                "{}\n[build] dependency:build-classpath 退出码为 0 但未生成 {}",
                sink.tail(),
                output_file.display()
            ),
        });
    }
    classpath::read_classpath_file(&output_file)
}

/// Maven 步骤退出结果的统一错误映射：取消 → Task；超时 / 非零 → BuildFailed。
fn check_exit(
    exit: &StreamingExit,
    root: &MavenProjectNode,
    sink: &RedactingSink,
    step: &str,
    timeout: Option<Duration>,
) -> AppResult<()> {
    if exit.cancelled {
        return Err(AppError::Task("build cancelled by user".into()));
    }
    let module = strategy::module_ga(root);
    if exit.timed_out {
        return Err(AppError::BuildFailed {
            module,
            exit_code: None,
            log_tail: format!(
                "{}\n[build] Maven {step} 超过 {:?} 未结束，进程树已终止",
                sink.tail(),
                timeout.unwrap_or_default()
            ),
        });
    }
    if exit.exit_code != Some(0) {
        let tail = sink.tail();
        return Err(AppError::BuildFailed {
            module: infer_failed_module(&tail, &module),
            exit_code: exit.exit_code,
            log_tail: tail,
        });
    }
    Ok(())
}

/// 从 Reactor 失败摘要推断失败模块（形如
/// `[ERROR]   com.example:app ...... FAILURE [ 0.2 s]`）；单模块构建没有该
/// 摘要，回退根模块。
fn infer_failed_module(tail: &str, fallback: &str) -> String {
    for line in tail.lines().rev() {
        let Some(rest) = line.trim().strip_prefix("[ERROR]") else {
            continue;
        };
        let rest = rest.trim();
        if !rest.contains("FAILURE") {
            continue;
        }
        if let Some(token) = rest.split_whitespace().next() {
            if token.contains(':') {
                return token.to_string();
            }
        }
    }
    fallback.to_string()
}

/// 脱敏转发 sink：行先经 [`LogRedactor`] 掩码（T-08 `mask_secrets` + 敏感
/// 环境值替换，R-11 起为共享实现）再进内部 [`RingTail`] 与调用方 sink。
struct RedactingSink<'a> {
    redactor: LogRedactor,
    tail: RingTail,
    inner: &'a mut dyn BuildOutputSink,
}

impl<'a> RedactingSink<'a> {
    fn new(secrets: Vec<String>, inner: &'a mut dyn BuildOutputSink) -> Self {
        Self {
            redactor: LogRedactor::new(secrets),
            tail: RingTail::new(),
            inner,
        }
    }

    fn tail(&self) -> String {
        self.tail.tail()
    }
}

impl BuildOutputSink for RedactingSink<'_> {
    fn on_line(&mut self, stream: OutputStream, line: &str) {
        let masked = self.redactor.mask(line);
        self.tail.on_line(stream, &masked);
        self.inner.on_line(stream, &masked);
    }
}

#[cfg(test)]
mod tests {
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

    /// 单仓多模块 fixture：parent(pom) + lib(jar) + app(jar，依赖 lib)。
    struct Fixture {
        root: PathBuf,
        db: Arc<Mutex<Connection>>,
        workspace_id: i64,
    }

    fn write(path: &Path, content: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, content).unwrap();
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
        let runner = crate::runtime::build::runner::SpawningMavenRunner;
        build_with(
            fixture,
            &runner,
            BuildOptions {
                strategy: Some(strategy),
                timeout: Some(INTEGRATION_TIMEOUT),
                ..Default::default()
            },
            sink,
            cancel,
        )
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
        //   第三次必定命中。）
        real_build(&mut fixture, RunStrategy::ClasspathRun, &mut sink, None)
            .unwrap_or_else(|error| panic!("second build failed: {error}\n{}", sink.tail.tail()));
        let before_third = sink.invocations;
        real_build(&mut fixture, RunStrategy::ClasspathRun, &mut sink, None)
            .unwrap_or_else(|error| panic!("third build failed: {error}\n{}", sink.tail.tail()));
        assert_eq!(
            sink.invocations - before_third,
            1,
            "third build must be compile-only (classpath cache hit)"
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
}
