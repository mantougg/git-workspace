use std::sync::Arc;
use std::time::Duration;

use crate::error::{AppError, AppResult};
use crate::runtime::build::pipeline::execute_build;
use crate::runtime::build::{BuildOptions, BuildRequest, LaunchPlan};
use crate::runtime::config;
use crate::runtime::launch::launcher;
use crate::runtime::launch::store;
use crate::runtime::launch::{LifecycleStatus, RuntimeProcessInfo};

use super::*;

impl RuntimeProcessManager {
    /// 启动一个 Runtime：构建（或复用缓存）→ spawn → Running 判定。
    ///
    /// 返回时状态已稳定为 Running / Stopped / Failed 之一；失败路径返回
    /// `Err`（BuildFailed / ProcessStartFailed 等结构化错误）且 DB 行落 Failed。
    pub fn start(
        self: &Arc<Self>,
        workspace_id: i64,
        runtime_name: &str,
        options: StartOptions,
    ) -> AppResult<RuntimeProcessInfo> {
        self.ensure_sampler();
        // 重复启动守卫：同一 (workspace, runtime) 只允许一个活跃进程。
        {
            let conn = self.db.lock().unwrap();
            if let Some(active) = store::find_active(&conn, workspace_id, runtime_name)? {
                return Err(AppError::Conflict(format!(
                    "Runtime '{runtime_name}' 已在运行（进程记录 #{}，状态 {}）。\
                     请先 Stop，或使用 Restart。",
                    active.id,
                    active.status.as_str()
                )));
            }
        }

        let process_id = {
            let conn = self.db.lock().unwrap();
            store::insert_process(&conn, workspace_id, runtime_name)?
        };
        let handle = ActiveProcess::new(false, workspace_id, runtime_name);
        self.active
            .lock()
            .unwrap()
            .insert(process_id, handle.clone());

        let result = self.start_inner(process_id, workspace_id, runtime_name, &options, &handle);
        if result.is_err() {
            // start_inner 的失败路径已负责状态落库与 outcome 信号；这里只摘牌。
            self.active.lock().unwrap().remove(&process_id);
        }
        result
    }

    fn start_inner(
        self: &Arc<Self>,
        process_id: i64,
        workspace_id: i64,
        runtime_name: &str,
        options: &StartOptions,
        handle: &ActiveProcess,
    ) -> AppResult<RuntimeProcessInfo> {
        // ---- Preparing：配置加载 + R-06 mainClass 回退 + 缓存判定 ----
        self.transit(process_id, runtime_name, LifecycleStatus::Preparing, None)?;
        let prepared = match self.prepare(workspace_id, runtime_name, options) {
            Ok(prepared) => prepared,
            Err(error) => {
                self.abort_before_spawn(process_id, runtime_name, handle, None);
                return Err(error);
            }
        };

        // ---- R-11 日志会话：构建 / 运行输出统一接管（脱敏后落盘）----
        // 日志目录不可写等失败直接终止 Start（可行动错误），不带病运行。
        if let Err(error) = self.open_log_session(workspace_id, runtime_name, process_id) {
            self.abort_before_spawn(process_id, runtime_name, handle, None);
            return Err(error);
        }

        // ---- Resolving / Building（skip-build 命中缓存时跳过）----
        let (plan, strategy) = match prepared {
            Prepared::Cached(cached) => {
                log::info!("R-10: reusing cached launch artifacts for '{runtime_name}'");
                (cached.plan, cached.strategy)
            }
            Prepared::NeedBuild(build_options) => {
                self.transit(process_id, runtime_name, LifecycleStatus::Resolving, None)?;
                // execute_build 内部（图/闭包/Reactor → Maven）无法插桩；
                // 构建主体是 Maven 调用，紧邻置位 Building（模块文档说明）。
                self.transit(process_id, runtime_name, LifecycleStatus::Building, None)?;
                match self.run_build(
                    process_id,
                    workspace_id,
                    runtime_name,
                    build_options,
                    handle,
                ) {
                    Ok(built) => (built.plan, built.strategy),
                    Err(error) => {
                        // Stop/Kill 在构建期间介入：以停止语义收尾，不再算启动失败。
                        let current = self.current_status(process_id)?;
                        if current == LifecycleStatus::Stopping {
                            self.transit(process_id, runtime_name, LifecycleStatus::Stopped, None)?;
                            handle.signal_outcome(MonitorOutcome {
                                exit_code: None,
                                cancelled: true,
                                spawn_error: None,
                            });
                            self.active.lock().unwrap().remove(&process_id);
                            return self.info(process_id);
                        }
                        self.abort_before_spawn(process_id, runtime_name, handle, None);
                        return Err(error);
                    }
                }
            }
        };

        // ---- Starting：命令组装 + spawn ----
        self.transit(process_id, runtime_name, LifecycleStatus::Starting, None)?;
        let command = match launcher::launch_command(&plan, process_id, runtime_name) {
            Ok(command) => command,
            Err(error) => {
                self.abort_before_spawn(process_id, runtime_name, handle, None);
                return Err(error);
            }
        };
        {
            let conn = self.db.lock().unwrap();
            // preview / working_dir 先行落库；pid 在 spawn 后回填。
            if let Err(error) = store::set_launched_meta(
                &conn,
                process_id,
                strategy,
                &launcher::plan_preview(&plan),
                &launcher::plan_working_dir(&plan),
            ) {
                self.abort_before_spawn(process_id, runtime_name, handle, None);
                return Err(error);
            }
        }
        let detector_kind = if matches!(&plan, LaunchPlan::Script { .. }) {
            crate::runtime::config::RuntimeKind::Node
        } else {
            crate::runtime::config::RuntimeKind::SpringBoot
        };
        self.spawn_monitor(
            process_id,
            runtime_name.to_string(),
            command,
            handle,
            detector_kind,
        );

        // spawn 失败 / 拿到 pid 之前进程就没了 → outcome 先到。
        let pid = match self.wait_pid_or_outcome(handle, Duration::from_secs(10)) {
            PidWait::Pid(pid) => pid,
            PidWait::Exited => return self.finish_early_exit(process_id, runtime_name, handle),
            PidWait::Timeout => {
                let error = AppError::ProcessStartFailed {
                    runtime: runtime_name.to_string(),
                    reason: "spawn 后 10s 内未能确认进程 pid".into(),
                };
                self.abort_before_spawn(process_id, runtime_name, handle, None);
                return Err(error);
            }
        };
        {
            let start_time = self.deps.launch_runner.start_time(pid);
            *handle.pid_start_time.lock().unwrap() = start_time;
            let conn = self.db.lock().unwrap();
            store::set_pid(&conn, process_id, pid, start_time)?;
        }

        // ---- Running 判定：横幅命中提前翻转；否则宽限到期仍存活即 Running ----
        match self.wait_running_or_outcome(handle, options.start_grace) {
            RunWait::Running => {
                self.transit(process_id, runtime_name, LifecycleStatus::Running, None)?;
                // R-16：进入 Running 后开启健康探针（配置缺失时引擎内部 no-op）。
                if let Some(health) = &self.deps.health {
                    health.start_monitor(process_id, workspace_id, runtime_name);
                }
                self.info(process_id)
            }
            RunWait::Exited => self.finish_early_exit(process_id, runtime_name, handle),
            RunWait::GraceElapsed => {
                let start_time = *handle.pid_start_time.lock().unwrap();
                if self.deps.launch_runner.alive(pid, start_time) {
                    self.transit(process_id, runtime_name, LifecycleStatus::Running, None)?;
                    // R-16：宽限边界翻转 Running 同样开启探针。
                    if let Some(health) = &self.deps.health {
                        health.start_monitor(process_id, workspace_id, runtime_name);
                    }
                    self.info(process_id)
                } else {
                    // 宽限边界上刚好退出：等 monitor 收尾后按退出分类。
                    self.wait_outcome(handle, Duration::from_secs(5));
                    self.finish_early_exit(process_id, runtime_name, handle)
                }
            }
        }
    }

    /// Preparing 阶段的准备工作：加载未脱敏配置（校验存在性）、R-06 推断
    /// mainClass（仅缺省时）、判定走缓存还是完整构建。
    fn prepare(
        &self,
        workspace_id: i64,
        runtime_name: &str,
        options: &StartOptions,
    ) -> AppResult<Prepared> {
        let (mut config, workspace_root) = {
            let conn = self.db.lock().unwrap();
            let config = config::load_config_unredacted(&conn, workspace_id, runtime_name)?;
            let root = config::workspace_root(&conn, workspace_id)?;
            (config, root)
        };

        // R-15 §82：环境覆盖项（内存生效，不改配置文件；应用层在五层合并的
        // Application 层之上追加，与「环境只存覆盖项」一致）。
        // N-09：按 kind 分流——JVM 形状的 jdk/profile 与 Spring Boot 端口
        // 参数只对 springBoot 生效；node 的端口覆盖走 PORT 环境变量
        // （dev server 惯例，不向 npm script 追加未知参数）。
        if let Some(overrides) = &options.overrides {
            match config.kind {
                crate::runtime::config::RuntimeKind::SpringBoot => {
                    if let Some(jdk) = &overrides.jdk {
                        config.jdk = Some(jdk.clone());
                    }
                    if let Some(profile) = &overrides.profile {
                        config.profile = Some(profile.clone());
                    }
                    config.environment.extend(overrides.environment.clone());
                    if let Some(port) = overrides.port {
                        config
                            .program_arguments
                            .retain(|arg| !arg.starts_with("--server.port="));
                        config
                            .vm_options
                            .retain(|arg| !arg.starts_with("-Dserver.port="));
                        config
                            .program_arguments
                            .push(format!("--server.port={port}"));
                    }
                }
                crate::runtime::config::RuntimeKind::Node => {
                    if overrides.jdk.is_some() || overrides.profile.is_some() {
                        log::warn!(
                            "R-15/N-09: node runtime '{runtime_name}' ignores jdk/profile overrides"
                        );
                    }
                    config.environment.extend(overrides.environment.clone());
                    if let Some(port) = overrides.port {
                        config.environment.insert("PORT".into(), port.to_string());
                    }
                }
            }
        }

        // R-14 §79：启动前端口预检——显式端口被占用直接返回 PortOccupied
        // （带占用方 PID / 进程名，§80 可行动提示），避免启动后崩溃。
        crate::runtime::launch::port_preflight::preflight(&config)?;

        let mut build_options = options.build_options.clone();
        if config.main_class.is_none() {
            // R-06 回退：检测候选并取默认推断；找不到时不硬失败——
            // MavenRun/PackageRun 不需要 mainClass，ClasspathRun 会在
            // LaunchPlan 构造处给出可行动错误。
            match self.infer_main_class(&workspace_root, &config.project) {
                Ok(Some(inferred)) => {
                    log::info!(
                        "R-10: mainClass inferred via R-06 for '{runtime_name}': {inferred}"
                    );
                    build_options.main_class_override = Some(inferred);
                }
                Ok(None) => log::debug!("R-10: no main class candidate for '{runtime_name}'"),
                Err(error) => log::warn!("R-10: main class detection failed: {error}"),
            }
        }

        if options.skip_build {
            let cached = self
                .launch_cache
                .lock()
                .unwrap()
                .get(&(workspace_id, runtime_name.to_string()))
                .map(|cached| CachedLaunch {
                    plan: cached.plan.clone(),
                    strategy: cached.strategy,
                });
            if let Some(cached) = cached {
                return Ok(Prepared::Cached(cached));
            }
            log::info!(
                "R-10: skip_build requested for '{runtime_name}' but no cached artifacts; \
                 falling back to a full build"
            );
        }
        Ok(Prepared::NeedBuild(build_options))
    }

    /// R-06 自动推断默认 mainClass：按 Runtime 配置的 project 匹配检测结果。
    /// 路径比较对 Windows 分隔符不敏感（配置可能是 `\`、`/` 或混合，R-14 修复）。
    fn infer_main_class(
        &self,
        workspace_root: &std::path::Path,
        project: &str,
    ) -> AppResult<Option<String>> {
        let discovery = crate::maven::discover_poms(workspace_root, 5, None, None);
        let result = crate::runtime::spring_boot::detect_spring_boot_workspace(
            &discovery.projects,
            &discovery.effective,
            None,
        );
        let needle = project.replace('\\', "/");
        let found = result.projects.iter().find(|candidate| {
            let path = candidate.project_path.to_string_lossy().replace('\\', "/");
            path == needle || candidate.module == project
        });
        Ok(found.and_then(|candidate| candidate.default_main_class.clone()))
    }

    /// 驱动 R-09 构建流水线。构建输出经 `BuildLogSink` 进入 R-11 日志会话
    /// （流水线 RedactingSink 已脱敏一次，会话侧再脱敏是幂等防御）；
    /// `BuildFailed.log_tail` 由流水线内部的 RingTail 保障。
    fn run_build(
        &self,
        process_id: i64,
        workspace_id: i64,
        runtime_name: &str,
        build_options: BuildOptions,
        handle: &ActiveProcess,
    ) -> AppResult<Built> {
        let workspace_root = {
            let conn = self.db.lock().unwrap();
            config::workspace_root(&conn, workspace_id)?
        };
        let request = BuildRequest {
            workspace_id,
            runtime_name: runtime_name.to_string(),
            options: build_options,
        };
        let outcome = {
            // R-12：不在整个构建期间持 DB 锁——execute_build 按阶段自行加锁，
            // Maven 运行期间锁是空闲的（并发构建 / UI 查询不被阻塞）。
            let mut sink = BuildLogSink {
                session: self.deps.logs.session(process_id),
            };
            execute_build(
                &self.db,
                &workspace_root,
                &self.deps.graph_cache,
                &self.deps.closure_cache,
                &self.deps.scheduler,
                &*self.deps.maven_runner,
                &request,
                &self.deps.script_approvals,
                &mut sink,
                Some(&handle.build_cancel),
            )?
        };
        self.launch_cache.lock().unwrap().insert(
            (workspace_id, runtime_name.to_string()),
            CachedLaunch {
                plan: outcome.launch.clone(),
                strategy: outcome.strategy,
            },
        );
        Ok(Built {
            plan: outcome.launch,
            strategy: outcome.strategy,
        })
    }

    /// spawn 前阶段（Preparing/Resolving/Building/Starting）的失败收尾：
    /// 若 Stop 已介入（Stopping）则尊重停止语义落 Stopped，否则落 Failed。
    /// 同时收口 R-11 日志会话（幂等；会话未开启时 no-op）。
    fn abort_before_spawn(
        &self,
        process_id: i64,
        runtime_name: &str,
        handle: &ActiveProcess,
        exit_code: Option<i32>,
    ) {
        self.deps.logs.finish_session(process_id);
        let current = self
            .current_status(process_id)
            .unwrap_or(LifecycleStatus::Failed);
        let to = if current == LifecycleStatus::Stopping {
            LifecycleStatus::Stopped
        } else {
            LifecycleStatus::Failed
        };
        if let Err(error) = self.transit(process_id, runtime_name, to, Some(exit_code)) {
            log::error!("R-10: abort transition failed for #{process_id}: {error}");
        }
        handle.signal_outcome(MonitorOutcome {
            exit_code,
            cancelled: false,
            spawn_error: None,
        });
    }
}
