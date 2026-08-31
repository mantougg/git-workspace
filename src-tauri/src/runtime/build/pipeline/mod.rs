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
use crate::maven::reactor::RuntimeReactorKind;
use crate::process::streaming::{spawn_streaming, OutputStream, StreamingExit};
use crate::runtime::build::classpath;
use crate::runtime::build::dep_cache;
use crate::runtime::build::runner::MavenRunner;
use crate::runtime::build::scheduler::BuildScheduler;
use crate::runtime::build::strategy;
use crate::runtime::build::{
    default_strategy, engine_for, BuildContext, BuildEngine, BuildOptions, BuildOutcome,
    BuildOutputSink, BuildRequest, LaunchPlan, RingTail, RunStrategy,
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

/// Execute the Node.js direct launch path. A Node frontend has no Maven
/// reactor or compile phase: validate the toolchain and manifest, merge the
/// standard runtime environment, then return a script launch plan.
#[allow(clippy::too_many_arguments)]
pub(crate) fn execute_node_build(
    db: &Arc<Mutex<Connection>>,
    workspace_root: &Path,
    script_approvals: &ScriptApprovalStore,
    request: &BuildRequest,
    sink: &mut dyn BuildOutputSink,
    cancel: Option<&AtomicBool>,
) -> AppResult<BuildOutcome> {
    let started = Instant::now();
    let config = {
        let conn = db.lock().unwrap();
        config::load_config_unredacted(&conn, request.workspace_id, &request.runtime_name)?
    };
    if config.kind != crate::runtime::config::RuntimeKind::Node {
        return Err(AppError::RuntimeConfig(format!(
            "Runtime '{}' 不是 Node 类型，不能使用 node Build Engine",
            request.runtime_name
        )));
    }
    if cancel.is_some_and(|flag| flag.load(std::sync::atomic::Ordering::Relaxed)) {
        return Err(AppError::Task("node build cancelled by user".into()));
    }

    let mut project_dir = PathBuf::from(&config.project);
    if !project_dir.is_absolute() {
        project_dir = workspace_root.join(project_dir);
    }
    if project_dir.is_file() {
        project_dir = project_dir
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| workspace_root.to_path_buf());
    }
    let package_path = project_dir.join("package.json");
    let package_bytes = std::fs::read(&package_path).map_err(|error| {
        AppError::RuntimeConfig(format!(
            "无法读取 Node 项目的 package.json {}：{}。请确认 project 指向 package.json 所在目录",
            package_path.display(),
            error
        ))
    })?;
    let package: serde_json::Value = serde_json::from_slice(&package_bytes).map_err(|error| {
        AppError::RuntimeConfig(format!(
            "Node 项目的 package.json {} 无效：{}。请修复 JSON 后重试",
            package_path.display(),
            error
        ))
    })?;
    let scripts = package
        .get("scripts")
        .and_then(serde_json::Value::as_object)
        .ok_or_else(|| AppError::ScriptNotFound {
            project: config.project.clone(),
            script: config.node_script.clone(),
            available: vec![],
        })?;
    let script = config.node_script.as_deref().unwrap_or_default().trim();
    if !scripts.contains_key(script) {
        return Err(AppError::ScriptNotFound {
            project: config.project.clone(),
            script: Some(script.to_string()),
            available: scripts.keys().cloned().collect(),
        });
    }

    // Resolve node first so an invalid PATH is reported as NodeNotFound even
    // when the package manager shim happens to exist.
    crate::node::detect_node()?;
    let decision = crate::node::decide_package_manager(&crate::node::DecisionInput {
        configured: config.node_package_manager.clone(),
        package_json_field: package
            .get("packageManager")
            .and_then(serde_json::Value::as_str)
            .map(ToOwned::to_owned),
        lockfiles: crate::node::LockfileSnapshot::scan(&project_dir),
    });
    let package_manager = crate::node::resolve_package_manager(&decision)?;
    if !project_dir.join("node_modules").is_dir() {
        return Err(AppError::RuntimeConfig(format!(
            "Node 项目 {} 缺少 node_modules，未自动安装依赖。Suggested Action：在项目目录执行 node_install（或手动运行 {} install）后重试",
            project_dir.display(),
            decision.manager.name()
        )));
    }

    let env: Vec<(String, String)> = {
        let conn = db.lock().unwrap();
        config::resolve_environment(&conn, request.workspace_id, &request.runtime_name)?
            .into_iter()
            .collect()
    };
    let mut redacting = RedactingSink::new(sensitive_env_values(&env), sink);
    if let Some(pre) = config.pre_build_script.as_deref() {
        run_user_script(
            pre,
            "pre",
            workspace_root,
            script_approvals,
            request,
            &mut redacting,
            cancel,
        )?;
    }

    // 参数透传形状按包管理器区分（N-08）：npm 需 `--` 分隔，pnpm/yarn 直接透传。
    let args = crate::node::build_run_args(decision.manager, script, &config.program_arguments);
    let mut preview_parts = vec![package_manager.executable.to_string_lossy().into_owned()];
    preview_parts.extend(args.iter().cloned());
    let preview = preview_parts.join(" ");

    if let Some(post) = config.post_build_script.as_deref() {
        run_user_script(
            post,
            "post",
            workspace_root,
            script_approvals,
            request,
            &mut redacting,
            cancel,
        )?;
    }

    Ok(BuildOutcome {
        strategy: RunStrategy::NodeScript,
        reactor_kind: RuntimeReactorKind::Existing,
        reactor_pom: package_path,
        modules_built: vec![project_dir.to_string_lossy().into_owned()],
        build_duration_ms: started.elapsed().as_millis(),
        build_command_preview: preview.clone(),
        launch: LaunchPlan::Script {
            executable: package_manager.executable,
            args,
            env,
            working_dir: project_dir,
            preview,
        },
    })
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
    // Node 配置按技术栈直通 NodeBuildEngine；历史 Spring Boot 配置继续
    // 使用 build_engine（maven/mvnd）语义，不改变既有链路。
    let engine_id = if config.kind == crate::runtime::config::RuntimeKind::Node {
        "node"
    } else {
        config.build_engine.as_deref().unwrap_or("maven")
    };
    if engine_id == "node" {
        return execute_node_build(db, workspace_root, script_approvals, request, sink, cancel);
    }
    // 只认 "maven" / "mvnd"（R-18）；Gradle（R-22）将来在这里分发。
    let _engine = engine_for(engine_id)?;
    let engine_hint = if engine_id == "mvnd" {
        crate::runtime::build::runner::BuildEngineHint::Mvnd
    } else {
        crate::runtime::build::runner::BuildEngineHint::Maven
    };
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
    let project_dir = strategy::module_directory(root);
    // R-18：mvnd 偏好解析；不可用回退 mvn（可选增强，不构成硬依赖）。
    let mut resolved = match runner.resolve_maven_for_engine(
        &project_dir,
        &local_repository,
        engine_hint,
    )? {
        Some(resolved) => resolved,
        None => {
            log::warn!("R-18: build_engine=mvnd 但 mvnd 不可用，回退普通 Maven");
            // 固定提示行（无秘密），直接进 sink 供日志可证。
            sink.on_line(
                OutputStream::Stdout,
                "[R-18] mvnd 不可用（未安装或探测失败），本次构建回退普通 Maven",
            );
            runner.resolve_maven(&project_dir, &local_repository)?
        }
    };

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
    // R-18 §73 第二阶段：Runtime Dependency Cache——模块输入指纹未变则
    // 跳过重建；全部未变则跳过整个 Maven 构建调用。
    let mut build_subset: Option<Vec<String>> = None;
    let mut skip_build_call = false;
    if request.options.dependency_cache {
        let closure_modules: Vec<MavenProjectNode> = closure.projects.clone();
        let stored = dep_cache::load_state(workspace_root, &request.runtime_name);
        let plan = dep_cache::compute_rebuild_plan(
            &graph,
            &closure_modules,
            stored.as_ref(),
            &graph.fingerprint,
            |module| {
                dep_cache::compute_module_fingerprint(
                    &module.path,
                    &strategy::module_directory(module),
                )
            },
            |module| {
                // 产物存在性：jar 模块看 target/classes（compile 产物）。
                strategy::module_directory(module).join("target").join("classes").is_dir()
            },
        );
        match plan {
            dep_cache::RebuildPlan::SkipAll => {
                log::info!(
                    "R-18: dependency cache hit for '{}': all {} module(s) unchanged, \
                     skipping Maven build",
                    request.runtime_name,
                    closure.projects.len()
                );
                redacting.on_line(
                    OutputStream::Stdout,
                    &format!(
                        "[R-18] 依赖缓存命中：{} 个模块输入未变化，跳过 Maven 构建",
                        closure.projects.len()
                    ),
                );
                skip_build_call = true;
            }
            dep_cache::RebuildPlan::Subset(subset) => {
                log::info!(
                    "R-18: dependency cache for '{}': rebuilding {} of {} module(s): {}",
                    request.runtime_name,
                    subset.len(),
                    closure.projects.len(),
                    subset.join(", ")
                );
                redacting.on_line(
                    OutputStream::Stdout,
                    &format!(
                        "[R-18] 依赖缓存：仅重建 {}/{} 个模块（{}）",
                        subset.len(),
                        closure.projects.len(),
                        subset.join(", ")
                    ),
                );
                build_subset = Some(subset);
            }
            dep_cache::RebuildPlan::RebuildAll => {}
        }

        // R-17 §44：watch 影响分析的必建子集——显式变更信号优先于「指纹
        // 未变」判断：非空时撤销 SkipAll 并与指纹子集取并集；RebuildAll
        // 保持全量（全量已覆盖 affected 子集，不缩窄）。
        if !request.options.affected_modules.is_empty() {
            match build_subset.take() {
                Some(subset) => {
                    let mut merged: std::collections::BTreeSet<String> =
                        subset.into_iter().collect();
                    merged.extend(request.options.affected_modules.iter().cloned());
                    build_subset = Some(merged.into_iter().collect());
                }
                None if skip_build_call => {
                    log::info!(
                        "R-17: watch affected {} module(s) override dependency-cache SkipAll",
                        request.options.affected_modules.len()
                    );
                    redacting.on_line(
                        OutputStream::Stdout,
                        &format!(
                            "[R-17] 文件变更：重建受影响模块（{}）",
                            request.options.affected_modules.join(", ")
                        ),
                    );
                    skip_build_call = false;
                    build_subset = Some(request.options.affected_modules.clone());
                }
                // RebuildAll：保持全量构建。
                None => {}
            }
        }
    }

    let mut build_request = strategy::build_maven_request_with_subset(
        &resolved.executable,
        workspace_root,
        &reactor,
        strategy,
        &request.options,
        Some(resolved.local_repository.clone()),
        build_subset.as_deref(),
    );
    // R-18：mvnd daemon 闲置超时回收（用户显式设置时不重复注入）。
    if engine_hint == crate::runtime::build::runner::BuildEngineHint::Mvnd
        && !build_request
            .extra_args
            .iter()
            .any(|arg| arg.starts_with("-Dmvnd.idleTimeout="))
    {
        build_request
            .extra_args
            .push(crate::maven::mvnd::idle_timeout_arg());
    }
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
    let exit = if skip_build_call {
        StreamingExit {
            exit_code: Some(0),
            timed_out: false,
            cancelled: false,
        }
    } else {
        runner.run(
            &build_request,
            &env,
            &mut redacting,
            cancel,
            request.options.timeout,
        )?
    };
    if let Err(error) = check_exit(&exit, root, &redacting, "build", request.options.timeout) {
        // R-18：mvnd daemon 异常识别 → 回退普通 mvn 重试一次。
        let retry_with_maven = engine_hint == crate::runtime::build::runner::BuildEngineHint::Mvnd
            && crate::maven::mvnd::looks_like_daemon_failure(&redacting.tail());
        if !retry_with_maven {
            return Err(error);
        }
        log::warn!("R-18: mvnd daemon failure detected; retrying with plain Maven");
        redacting.on_line(
            OutputStream::Stdout,
            "[R-18] 检测到 mvnd daemon 异常，回退普通 Maven 重试一次",
        );
        resolved = runner.resolve_maven(&project_dir, &local_repository)?;
        let retry_request = strategy::build_maven_request_with_subset(
            &resolved.executable,
            workspace_root,
            &reactor,
            strategy,
            &request.options,
            Some(resolved.local_repository.clone()),
            build_subset.as_deref(),
        );
        let retry_exit = runner.run(
            &retry_request,
            &env,
            &mut redacting,
            cancel,
            request.options.timeout,
        )?;
        check_exit(&retry_exit, root, &redacting, "build", request.options.timeout)?;
    }
    // 构建成功：写入/刷新依赖缓存状态（R-18）。
    if request.options.dependency_cache && !skip_build_call {
        let mut modules = std::collections::BTreeMap::new();
        let mut all_fingerprinted = true;
        for module in &closure.projects {
            match dep_cache::compute_module_fingerprint(
                &module.path,
                &strategy::module_directory(module),
            ) {
                Some(fp) => {
                    modules.insert(strategy::module_ga(module), fp);
                }
                None => {
                    all_fingerprinted = false;
                    break;
                }
            }
        }
        if all_fingerprinted {
            dep_cache::store_state(
                workspace_root,
                &request.runtime_name,
                &dep_cache::BuildCacheState {
                    graph_fingerprint: graph.fingerprint.clone(),
                    modules,
                },
            );
        }
    }

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
mod tests;
