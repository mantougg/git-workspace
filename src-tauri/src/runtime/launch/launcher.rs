//! Launcher（R-10，§29/§34）：把 R-09 的 [`LaunchPlan`] 组装成可 spawn 的
//! `java` / `mvn` 进程命令，并定义进程监督 seam（[`LaunchRunner`]）。
//!
//! - 命令组装是纯函数 [`launch_command`]：LaunchPlan 变体分别映射到
//!   `mvn spring-boot:run` / `java -jar` / `java -cp <deps> <main-class>` /
//!   包管理器 `run <script>`，
//!   并注入托管标记环境变量（[`MARKER_PROCESS_ID`] / [`MARKER_RUNTIME_NAME`]）。
//! - [`LaunchRunner`] 抽象「spawn + 流式转发 + 阻塞等待 + 信号控制」：
//!   生产实现 [`SystemLaunchRunner`] 复用 `process::streaming` /
//!   `process::kill_tree`；测试 Fake 回放脚本，不依赖本机 JDK。

use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::AtomicBool;
use std::sync::Mutex;

use crate::error::{AppError, AppResult};
use crate::maven::executor;
use crate::maven::detect_exec::needs_cmd_c;
use crate::process::streaming::{spawn_streaming_ext, OutputStream, StreamingExit};
use crate::runtime::build::LaunchPlan;
use crate::runtime::launch::{MARKER_PROCESS_ID, MARKER_RUNTIME_NAME};

/// 把 [`LaunchPlan`] 组装成可 spawn 的 [`Command`]，并注入托管标记。
///
/// 只做构造，不 spawn。`env` 原样透传（含未脱敏秘密）——命令对象与环境
/// 绝不跨 IPC。
pub fn launch_command(plan: &LaunchPlan, process_id: i64, runtime_name: &str) -> AppResult<Command> {
    let mut command = match plan {
        LaunchPlan::MavenGoal { request, env, .. } => executor::build_process(request, env),
        LaunchPlan::JavaJar {
            java_exec,
            jar_path,
            vm_options,
            program_arguments,
            env,
            working_dir,
            ..
        } => {
            let mut command = Command::new(java_exec);
            command
                .args(vm_options)
                .arg("-jar")
                .arg(jar_path)
                .args(program_arguments)
                .current_dir(working_dir);
            apply_env(&mut command, env);
            command
        }
        LaunchPlan::JavaClasspath {
            java_exec,
            classpath,
            main_class,
            vm_options,
            program_arguments,
            env,
            working_dir,
            ..
        } => {
            let joined = std::env::join_paths(classpath).map_err(|error| {
                AppError::ProcessStartFailed {
                    runtime: runtime_name.to_string(),
                    reason: format!(
                        "classpath 含非法路径字符，无法拼接启动参数：{error}。\
                         请检查本地仓库与模块路径"
                    ),
                }
            })?;
            let mut command = Command::new(java_exec);
            command
                .args(vm_options)
                .arg("-cp")
                .arg(joined)
                .arg(main_class)
                .args(program_arguments)
                .current_dir(working_dir);
            apply_env(&mut command, env);
            command
        }
        LaunchPlan::Script {
            executable,
            args,
            env,
            working_dir,
            ..
        } => {
            let mut command = if cfg!(windows) && needs_cmd_c(executable) {
                let mut command = Command::new("cmd");
                command.arg("/C").arg(executable);
                command
            } else {
                Command::new(executable)
            };
            command.args(args).current_dir(working_dir);
            apply_env(&mut command, env);
            command
        }
    };
    command.env(MARKER_PROCESS_ID, process_id.to_string());
    command.env(MARKER_RUNTIME_NAME, runtime_name);
    Ok(command)
}

/// LaunchPlan 的启动命令预览（§75 可预览/可追溯；R-09 构造时已生成）。
pub fn plan_preview(plan: &LaunchPlan) -> String {
    match plan {
        LaunchPlan::MavenGoal { preview, .. }
        | LaunchPlan::JavaJar { preview, .. }
        | LaunchPlan::JavaClasspath { preview, .. }
        | LaunchPlan::Script { preview, .. } => preview.clone(),
    }
}

/// LaunchPlan 的工作目录（MavenGoal = Maven 请求的工作目录）。
pub fn plan_working_dir(plan: &LaunchPlan) -> PathBuf {
    match plan {
        LaunchPlan::MavenGoal { request, .. } => request.working_dir.clone(),
        LaunchPlan::JavaJar { working_dir, .. }
        | LaunchPlan::JavaClasspath { working_dir, .. }
        | LaunchPlan::Script { working_dir, .. } => working_dir.clone(),
    }
}

fn apply_env(command: &mut Command, env: &[(String, String)]) {
    for (key, value) in env {
        command.env(key, value);
    }
}

/// 进程监督 seam（对照 R-09 `MavenRunner`）：spawn 一个长驻进程并监督到
/// 退出，期间输出按行转发、外部可经 pid 发优雅终止信号或置 `kill` 强杀。
pub trait LaunchRunner: Send + Sync {
    /// Spawn 并阻塞监督到进程退出 / 被强杀。
    ///
    /// - `pid_slot`：spawn 成功后**立即**写入 pid（外部 Stop/Kill 以此发信号，
    ///   不等 `run` 返回）。
    /// - `kill`：置位后杀整棵进程树（Force Kill / Stop grace 超时升级）。
    /// - 返回 [`StreamingExit`]；`cancelled=true` 表示走了 `kill` 路径。
    fn run(
        &self,
        command: &mut Command,
        kill: &AtomicBool,
        pid_slot: &Mutex<Option<u32>>,
        on_line: &mut dyn FnMut(OutputStream, &str),
    ) -> AppResult<StreamingExit>;

    /// 优雅终止信号（Unix SIGTERM；Windows 无语义返回 `false`，调用方升级
    /// 为 `kill` 强杀）。进程不存在返回 `false`。
    fn terminate(&self, pid: u32) -> bool;

    /// 存活核对；`start_time`（[`Self::start_time`]）防 PID 复用。
    fn alive(&self, pid: u32, start_time: Option<u64>) -> bool;

    /// 进程 start_time（epoch 秒）；进程不存在返回 `None`。
    fn start_time(&self, pid: u32) -> Option<u64>;
}

/// 生产实现：真实 spawn / sysinfo 信号与存活探测。
pub struct SystemLaunchRunner;

impl LaunchRunner for SystemLaunchRunner {
    fn run(
        &self,
        command: &mut Command,
        kill: &AtomicBool,
        pid_slot: &Mutex<Option<u32>>,
        on_line: &mut dyn FnMut(OutputStream, &str),
    ) -> AppResult<StreamingExit> {
        Ok(spawn_streaming_ext(
            command,
            Some(kill),
            None,
            Some(pid_slot),
            on_line,
        )?)
    }

    fn terminate(&self, pid: u32) -> bool {
        crate::process::terminate_process(pid)
    }

    fn alive(&self, pid: u32, start_time: Option<u64>) -> bool {
        crate::process::process_alive(pid, start_time)
    }

    fn start_time(&self, pid: u32) -> Option<u64> {
        crate::process::process_start_time(pid)
    }
}

#[cfg(test)]
pub mod fake {
    //! `FakeLaunchRunner`：不 spawn 真实进程，按脚本回放行/模拟驻留与信号。
    //! fake pid 从 900_000 起递增，避免与宿主真实 pid 混淆。

    use std::collections::HashMap;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    use super::*;

    /// fake 进程行为：立即退出，或驻留直到收到 terminate / kill。
    #[derive(Debug, Clone)]
    pub enum FakeBehavior {
        /// 回放完即以该码退出（`None` = 被信号终止无码）。
        Exit(Option<i32>),
        /// 驻留；`terminate` 后以 `on_terminate` 码退出（Spring Boot 优雅
        /// 关闭语义），`kill` 以 `cancelled=true` 无码退出。
        StayAlive { on_terminate: Option<i32> },
    }

    #[derive(Debug, Clone)]
    pub struct FakeLaunch {
        pub lines: Vec<(OutputStream, String)>,
        pub behavior: FakeBehavior,
        /// 回放完行之后、退出/驻留之前的延迟（模拟「Running 后再崩溃」）。
        pub delay_after_lines: Option<Duration>,
    }

    impl Default for FakeLaunch {
        fn default() -> Self {
            Self {
                lines: Vec::new(),
                behavior: FakeBehavior::Exit(Some(0)),
                delay_after_lines: None,
            }
        }
    }

    struct FakeProc {
        start_time: u64,
        alive: bool,
        terminated: bool,
    }

    pub struct FakeLaunchRunner {
        script: Mutex<Vec<FakeLaunch>>,
        procs: Mutex<HashMap<u32, FakeProc>>,
        next_pid: AtomicU32,
        /// 记录每次 run 的命令预览（`Command` 不便直接断言时的最简观测口）。
        pub commands: Mutex<Vec<String>>,
    }

    impl FakeLaunchRunner {
        pub fn new(script: Vec<FakeLaunch>) -> Self {
            Self {
                script: Mutex::new(script),
                procs: Mutex::new(HashMap::new()),
                next_pid: AtomicU32::new(900_000),
                commands: Mutex::new(Vec::new()),
            }
        }

        /// 脚本耗尽后驻留（最常见的「应用起来了」场景）。
        pub fn staying_alive() -> Self {
            Self::new(Vec::new())
        }

        fn default_launch() -> FakeLaunch {
            FakeLaunch {
                lines: Vec::new(),
                behavior: FakeBehavior::StayAlive {
                    on_terminate: Some(0),
                },
                delay_after_lines: None,
            }
        }
    }

    impl LaunchRunner for FakeLaunchRunner {
        fn run(
            &self,
            command: &mut Command,
            kill: &AtomicBool,
            pid_slot: &Mutex<Option<u32>>,
            on_line: &mut dyn FnMut(OutputStream, &str),
        ) -> AppResult<StreamingExit> {
            let preview = format!("{:?}", command);
            self.commands.lock().unwrap().push(preview);

            let launch = {
                let mut script = self.script.lock().unwrap();
                if script.is_empty() {
                    Self::default_launch()
                } else {
                    script.remove(0)
                }
            };
            let pid = self.next_pid.fetch_add(1, Ordering::Relaxed);
            let start_time = pid as u64; // fake 时钟：start_time == pid，足够做匹配核对
            let on_terminate = match &launch.behavior {
                FakeBehavior::StayAlive { on_terminate } => *on_terminate,
                FakeBehavior::Exit(_) => None,
            };
            self.procs.lock().unwrap().insert(
                pid,
                FakeProc {
                    start_time,
                    alive: true,
                    terminated: false,
                },
            );
            *pid_slot.lock().unwrap() = Some(pid);

            for (stream, line) in &launch.lines {
                on_line(*stream, line);
            }
            if let Some(delay) = launch.delay_after_lines {
                std::thread::sleep(delay);
            }

            match launch.behavior {
                FakeBehavior::Exit(code) => {
                    self.procs.lock().unwrap().entry(pid).and_modify(|p| p.alive = false);
                    Ok(StreamingExit {
                        exit_code: code,
                        timed_out: false,
                        cancelled: false,
                    })
                }
                FakeBehavior::StayAlive { .. } => {
                    let deadline = Instant::now() + Duration::from_secs(30);
                    loop {
                        if kill.load(Ordering::Relaxed) {
                            self.procs.lock().unwrap().entry(pid).and_modify(|p| p.alive = false);
                            return Ok(StreamingExit {
                                exit_code: None,
                                timed_out: false,
                                cancelled: true,
                            });
                        }
                        let terminated = self
                            .procs
                            .lock()
                            .unwrap()
                            .get(&pid)
                            .map(|p| p.terminated)
                            .unwrap_or(true);
                        if terminated {
                            self.procs.lock().unwrap().entry(pid).and_modify(|p| p.alive = false);
                            return Ok(StreamingExit {
                                exit_code: on_terminate,
                                timed_out: false,
                                cancelled: false,
                            });
                        }
                        if Instant::now() > deadline {
                            return Err(AppError::Other(
                                "FakeLaunchRunner: stay-alive 脚本 30s 内未被 terminate/kill（测试遗漏 Stop?）"
                                    .into(),
                            ));
                        }
                        std::thread::sleep(Duration::from_millis(5));
                    }
                }
            }
        }

        fn terminate(&self, pid: u32) -> bool {
            let mut procs = self.procs.lock().unwrap();
            match procs.get_mut(&pid) {
                Some(proc) if proc.alive => {
                    proc.terminated = true;
                    true
                }
                _ => false,
            }
        }

        fn alive(&self, pid: u32, start_time: Option<u64>) -> bool {
            self.procs.lock().unwrap().get(&pid).is_some_and(|proc| {
                proc.alive && start_time.is_none_or(|t| t == proc.start_time)
            })
        }

        fn start_time(&self, pid: u32) -> Option<u64> {
            self.procs.lock().unwrap().get(&pid).map(|p| p.start_time)
        }
    }
}

#[cfg(test)]
pub use fake::{FakeBehavior, FakeLaunch, FakeLaunchRunner};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::maven::exec_model::MavenExecutionRequest;
    use std::path::Path;
    use std::sync::atomic::Ordering;
    use std::time::Duration;

    fn jar_plan() -> LaunchPlan {
        LaunchPlan::JavaJar {
            java_exec: PathBuf::from("/jdk21/bin/java"),
            jar_path: PathBuf::from("/ws/app/target/app-1.0.0.jar"),
            vm_options: vec!["-Xmx512m".into()],
            program_arguments: vec!["--server.port=8080".into()],
            env: vec![("DB_PASSWORD".into(), "secret".into())],
            working_dir: PathBuf::from("/ws/app"),
            preview: "java -jar app.jar".into(),
        }
    }

    #[test]
    fn jar_command_assembles_args_env_and_markers() {
        let command = launch_command(&jar_plan(), 42, "app").unwrap();
        let rendered = format!("{command:?}");
        assert!(rendered.contains("-Xmx512m"));
        assert!(rendered.contains("-jar"));
        assert!(rendered.contains("app-1.0.0.jar"));
        assert!(rendered.contains("--server.port=8080"));
        assert_eq!(command.get_current_dir(), Some(Path::new("/ws/app")));
        let envs: Vec<_> = command.get_envs().collect();
        assert!(envs.contains(&(
            std::ffi::OsStr::new("DB_PASSWORD"),
            Some(std::ffi::OsStr::new("secret"))
        )));
        // 孤儿托管标记：pid + runtime 名注入 env（reconcile 靠它认回进程）。
        assert!(envs.contains(&(
            std::ffi::OsStr::new("GITWORKSPACE_PROCESS_ID"),
            Some(std::ffi::OsStr::new("42"))
        )));
        assert!(envs.contains(&(
            std::ffi::OsStr::new("GITWORKSPACE_RUNTIME_NAME"),
            Some(std::ffi::OsStr::new("app"))
        )));
    }

    #[test]
    fn classpath_command_joins_paths_and_puts_main_class_last_before_program_args() {
        let plan = LaunchPlan::JavaClasspath {
            java_exec: PathBuf::from("java"),
            classpath: vec![PathBuf::from("/ws/app/target/classes"), PathBuf::from("/m2/a.jar")],
            main_class: "com.example.Application".into(),
            vm_options: vec![],
            program_arguments: vec!["--debug".into()],
            env: vec![],
            working_dir: PathBuf::from("/ws/app"),
            preview: String::new(),
        };
        let command = launch_command(&plan, 7, "app").unwrap();
        let rendered = format!("{command:?}");
        assert!(rendered.contains("-cp"));
        assert!(rendered.contains("target/classes"));
        assert!(rendered.contains("a.jar"));
        assert!(rendered.contains("com.example.Application"));
        assert!(rendered.contains("--debug"));
    }

    #[test]
    fn script_command_uses_platform_wrapper_and_preserves_arguments() {
        let executable = if cfg!(windows) {
            PathBuf::from(r"C:\tools\npm.cmd")
        } else {
            PathBuf::from("/usr/bin/npm")
        };
        let plan = LaunchPlan::Script {
            executable: executable.clone(),
            args: vec!["run".into(), "dev".into(), "--".into(), "--host".into()],
            env: vec![("PORT".into(), "5173".into())],
            working_dir: PathBuf::from("/ws/web"),
            preview: "npm run dev -- --host".into(),
        };
        let command = launch_command(&plan, 9, "web").unwrap();
        let args = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        if cfg!(windows) {
            assert_eq!(command.get_program(), Path::new("cmd"));
            assert_eq!(args.first().map(String::as_str), Some("/C"));
            assert_eq!(args.get(1), Some(&executable.to_string_lossy().into_owned()));
            assert_eq!(&args[2..], ["run", "dev", "--", "--host"]);
        } else {
            assert_eq!(command.get_program(), executable.as_path());
            assert_eq!(args, ["run", "dev", "--", "--host"]);
        }
        assert_eq!(command.get_current_dir(), Some(Path::new("/ws/web")));
        assert!(command
            .get_envs()
            .any(|(key, value)| key == "PORT" && value == Some(std::ffi::OsStr::new("5173"))));
    }

    #[test]
    fn maven_goal_delegates_to_executor() {
        let request = MavenExecutionRequest {
            working_dir: PathBuf::from("/ws/repo"),
            executable: "mvn".into(),
            goals: vec!["spring-boot:run".into()],
            extra_args: vec!["-pl".into(), "com.example:app".into()],
            via_cmd_c: false,
            local_repository: None,
        };
        let plan = LaunchPlan::MavenGoal {
            request,
            env: vec![],
            preview: "mvn spring-boot:run".into(),
        };
        let command = launch_command(&plan, 1, "app").unwrap();
        let rendered = format!("{command:?}");
        assert!(rendered.contains("mvn"));
        assert!(rendered.contains("spring-boot:run"));
        assert_eq!(command.get_current_dir(), Some(Path::new("/ws/repo")));
    }

    #[test]
    fn fake_runner_stays_alive_until_terminated_then_reports_exit_code() {
        let runner = FakeLaunchRunner::staying_alive();
        let kill = AtomicBool::new(false);
        let slot = Mutex::new(None);
        std::thread::scope(|scope| {
            let handle = scope.spawn(|| {
                let mut command = Command::new("java");
                runner.run(&mut command, &kill, &slot, &mut |_, _| {})
            });
            // 等 fake run 发布 pid。
            let pid = loop {
                if let Some(pid) = *slot.lock().unwrap() {
                    break pid;
                }
                std::thread::sleep(Duration::from_millis(5));
            };
            assert!(runner.alive(pid, Some(runner.start_time(pid).unwrap())));
            assert!(runner.terminate(pid));
            let exit = handle.join().unwrap().unwrap();
            assert_eq!(exit.exit_code, Some(0));
            assert!(!exit.cancelled);
            assert!(!runner.alive(pid, None));
        });
    }

    #[test]
    fn fake_runner_kill_path_reports_cancelled_without_code() {
        let runner = FakeLaunchRunner::staying_alive();
        let kill = AtomicBool::new(false);
        let slot = Mutex::new(None);
        std::thread::scope(|scope| {
            let handle = scope.spawn(|| {
                let mut command = Command::new("java");
                runner.run(&mut command, &kill, &slot, &mut |_, _| {})
            });
            let pid = loop {
                if let Some(pid) = *slot.lock().unwrap() {
                    break pid;
                }
                std::thread::sleep(Duration::from_millis(5));
            };
            kill.store(true, Ordering::Relaxed);
            let exit = handle.join().unwrap().unwrap();
            assert!(exit.cancelled);
            assert_eq!(exit.exit_code, None);
            assert!(!runner.alive(pid, None));
        });
    }
}
