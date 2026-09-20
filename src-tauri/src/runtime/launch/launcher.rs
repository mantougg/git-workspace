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
use crate::maven::detect_exec::needs_cmd_c;
use crate::maven::executor;
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
            let joined = std::env::join_paths(classpath).map_err(|error| AppError::ProcessStartFailed {
                runtime: runtime_name.to_string(),
                reason: format!(
                    "classpath 含非法路径字符，无法拼接启动参数：{error}。\
                         请检查本地仓库与模块路径"
                ),
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
    // N-07（unix）：启动子进程独立成组（`process_group(0)` = 新组长），Stop
    // 的 SIGTERM / 升级 SIGKILL 经 killpg 覆盖被 reparent 的孙子进程——npm
    // 收到 SIGTERM 后先于 vite 退出，parent 链 kill_tree 对「父死孙活」
    // 失效（设计文档 §9；Windows 无进程组语义，root cmd 存活期间 parent
    // 链完整，维持 kill_tree 路径不变）。
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
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

/// LaunchPlan 的 shell 可执行命令（TM-06 终端启动）。
///
/// 与 [`plan_preview`]（展示/落库用，空格 join、不加引号）不同，本函数用
/// 结构化字段重新组装：剥 Windows verbatim 前缀（`\\?\`，PowerShell/cmd
/// 不识别其作为命令名）、按 [`arg_needs_quoting`] 的字符集对每个 token
/// 加双引号。preview 字符串把含空格路径拆散后无法可靠还原，故必须在持有
/// `LaunchPlan` 结构化字段处组装。
pub fn plan_shell_command(plan: &LaunchPlan) -> String {
    match plan {
        LaunchPlan::MavenGoal { request, .. } => crate::maven::executor::build_command(request)
            .iter()
            .map(|part| shell_quote_arg(&crate::pathutil::strip_windows_verbatim_prefix(part)))
            .collect::<Vec<_>>()
            .join(" "),
        LaunchPlan::JavaJar {
            java_exec,
            jar_path,
            vm_options,
            program_arguments,
            ..
        } => {
            let mut parts = vec![shell_quote_path(java_exec)];
            parts.extend(vm_options.iter().map(|a| shell_quote_arg(a)));
            parts.push("-jar".into());
            parts.push(shell_quote_path(jar_path));
            parts.extend(program_arguments.iter().map(|a| shell_quote_arg(a)));
            parts.join(" ")
        }
        LaunchPlan::JavaClasspath {
            java_exec,
            classpath,
            main_class,
            vm_options,
            program_arguments,
            ..
        } => {
            let cp = std::env::join_paths(classpath)
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default();
            let mut parts = vec![shell_quote_path(java_exec)];
            parts.extend(vm_options.iter().map(|a| shell_quote_arg(a)));
            parts.push("-cp".into());
            parts.push(shell_quote_arg(&cp));
            parts.push(shell_quote_arg(main_class));
            parts.extend(program_arguments.iter().map(|a| shell_quote_arg(a)));
            parts.join(" ")
        }
        LaunchPlan::Script {
            executable, args, ..
        } => {
            let mut parts = vec![shell_quote_path(executable)];
            parts.extend(args.iter().map(|a| shell_quote_arg(a)));
            parts.join(" ")
        }
    }
}

/// 路径参数：剥 verbatim 前缀后按 [`shell_quote_arg`] 的字符集判定加引号
/// （`Program Files` / `.jar` 后缀都会命中）。
fn shell_quote_path(path: &std::path::Path) -> String {
    shell_quote_arg(&crate::pathutil::strip_windows_verbatim_prefix(
        &path.to_string_lossy(),
    ))
}

/// 普通参数：含 shell 不安全字符则加双引号（F-45）。
///
/// 命令字符串经 PTY 写入交互 shell 后会被当**源码**重解析，token 的形状直接
/// 决定它收到几个参数。PowerShell 参数模式下实测（本机 pwsh 7.6.6 + JDK 1.8）：
/// - 单个 `-` 前缀且含 `.` 的**裸** token 在第一个 `.` 处被拆成两个参数
///   （`-Dspring.output.ansi.enabled=always` → `-Dspring` +
///   `.output.ansi.enabled=always`，JVM 把后者当主类，报「找不到或无法加载
///   主类」）——Spring Boot 默认注入的 `-Dspring.*` 全覆盖命中；
/// - `,` 触发 ParserError；`;` `|` `&` 直接断开命令（`-cp a.jar;b.jar` 的
///   Windows 路径分隔符同样命中）；`$` 变量展开、反引号转义；
/// - 加双引号即作为单个 token 原样传入；`-XX:TieredStopAtLevel=1`（无 `.`）
///   等良构 token 保持裸写。
/// cmd 与 POSIX sh 的引号首词/引号 token 同样是单参数语义，加引号无害。
fn shell_quote_arg(arg: &str) -> String {
    if arg_needs_quoting(arg) {
        // `""` 是双引号串内的字面引号转义（PowerShell 与 cmd CRT 一致）。
        format!("\"{}\"", arg.replace('"', "\"\""))
    } else {
        arg.to_string()
    }
}

/// 需要引号包裹的字符：空白/引号、PowerShell 参数模式的拆分与展开字符、
/// 以及 cmd 的元字符（`%` `^` `!` `&` `|` `<` `>`）。
fn arg_needs_quoting(arg: &str) -> bool {
    arg.chars()
        .any(|c| matches!(
            c,
            ' ' | '\t'
                | '\r'
                | '\n'
                | '"'
                | '\''
                | '`'
                | '$'
                | '.'
                | ','
                | ';'
                | '|'
                | '&'
                | '<'
                | '>'
                | '('
                | ')'
                | '{'
                | '}'
                | '@'
                | '#'
                | '~'
                | '%'
                | '^'
                | '!'
        ))
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
        Ok(spawn_streaming_ext(command, Some(kill), None, Some(pid_slot), on_line)?)
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
                behavior: FakeBehavior::StayAlive { on_terminate: Some(0) },
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
                                "FakeLaunchRunner: stay-alive 脚本 30s 内未被 terminate/kill（测试遗漏 Stop?）".into(),
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
            self.procs
                .lock()
                .unwrap()
                .get(&pid)
                .is_some_and(|proc| proc.alive && start_time.is_none_or(|t| t == proc.start_time))
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

    /// 按空格切分命令字符串，同时识别双引号包裹；返回 `(token, 是否整体加引号)`。
    fn split_quoted(command: &str) -> Vec<(String, bool)> {
        let mut tokens = Vec::new();
        let mut current = String::new();
        let mut quoted = false;
        let mut in_quotes = false;
        let mut has_content = false;
        for c in command.chars() {
            if c == ' ' && !in_quotes {
                if has_content {
                    tokens.push((std::mem::take(&mut current), quoted));
                    quoted = false;
                    has_content = false;
                }
                continue;
            }
            if c == '"' {
                in_quotes = !in_quotes;
                quoted = true;
            } else {
                current.push(c);
            }
            has_content = true;
        }
        if has_content {
            tokens.push((current, quoted));
        }
        tokens
    }

    #[test]
    fn shell_quote_arg_covers_powershell_argument_mode_hazards() {
        // PowerShell 参数模式：单 `-` 前缀 + 含 `.` 的裸 token 在第一个 `.`
        // 处被拆成两个参数；`,` 直接 ParserError；`;`/`|`/`&` 断开命令；
        // `$`/反引号展开转义（F-45）。
        for arg in [
            "-Dspring.output.ansi.enabled=always",
            "-Dfoo=bar.baz",
            "--server.port=8080",
            "-Dfoo=a,b",
            "a.jar;b.jar",
            "-Dfoo=a|b",
            "-Dfoo=$HOME",
            "-Dfoo=a`tb",
        ] {
            assert_eq!(
                shell_quote_arg(arg),
                format!("\"{arg}\""),
                "应被整体加引号：{arg}"
            );
        }
        // 良构 token 保持裸写（PowerShell / cmd / sh 都不拆）。
        for arg in ["-jar", "-cp", "-XX:TieredStopAtLevel=1", "-Dfoo=bar", "mvn"] {
            assert_eq!(shell_quote_arg(arg), arg, "裸写即可：{arg}");
        }
        // 含空格仍走引号；内含双引号按 `""` 转义（PowerShell 与 cmd CRT 一致）。
        assert_eq!(shell_quote_arg("a b"), "\"a b\"");
        assert_eq!(shell_quote_arg("a\"b"), "\"a\"\"b\"");
    }

    /// F-45 原始案例的 LaunchPlan：jdk-1.8 在 Program Files（含空格）下，
    /// classpath 启动 + Spring Boot 默认注入的 `-Dspring.*`。
    fn f45_classpath_plan() -> LaunchPlan {
        LaunchPlan::JavaClasspath {
            java_exec: PathBuf::from(r"C:\Program Files\Java\jdk-1.8\bin\java.exe"),
            classpath: vec![PathBuf::from(
                r"D:\AWork\Code\IPD\.gitworkspace\runtime\IPD原型后端\classpath\pathing-e2459fdcec83117c.jar",
            )],
            main_class: "com.jxdinfo.hussar.example.HussarApplication".into(),
            vm_options: vec![
                "-XX:TieredStopAtLevel=1".into(),
                "-Dspring.output.ansi.enabled=always".into(),
                "-Dcom.sun.management.jmxremote".into(),
                "-Dspring.jmx.enabled=true".into(),
                "-Dspring.liveBeansView.mbeanDomain".into(),
                "-Dspring.application.admin.enabled=true".into(),
                "-Dmanagement.endpoints.jmx.exposure.include=*".into(),
                "-Dfile.encoding=UTF-8".into(),
            ],
            program_arguments: vec![],
            env: vec![],
            working_dir: PathBuf::from(r"D:\AWork\Code\IPD\docs\03原型\hussar-web"),
            preview: String::new(),
        }
    }

    /// F-45 回归：组装出的命令行里不存在「会被 PowerShell 拆开却没加引号」
    /// 的 token（原 bug 下 `-Dspring` + `.output.ansi.enabled=always` 被当
    /// 两个参数，JVM 把后者当主类）。
    #[test]
    fn plan_shell_command_classpath_quotes_every_unsafe_token() {
        let cmd = plan_shell_command(&f45_classpath_plan());
        for (token, quoted) in split_quoted(&cmd) {
            assert!(
                quoted || !arg_needs_quoting(&token),
                "未加引号的危险 token：{token}；完整命令：{cmd}"
            );
        }
    }

    /// 引号只是包裹：剥掉引号后的 argv 序列必须与 `launch_command` 一致。
    #[test]
    fn plan_shell_command_classpath_token_sequence() {
        let cmd = plan_shell_command(&f45_classpath_plan());
        let tokens: Vec<String> = split_quoted(&cmd).into_iter().map(|(t, _)| t).collect();
        assert_eq!(tokens[0], r"C:\Program Files\Java\jdk-1.8\bin\java.exe");
        assert_eq!(tokens[1], "-XX:TieredStopAtLevel=1");
        // 8 个 vm option（idx 1..=8）之后是 `-cp`、classpath、主类
        assert_eq!(tokens[2], "-Dspring.output.ansi.enabled=always");
        assert_eq!(tokens[8], "-Dfile.encoding=UTF-8");
        assert_eq!(tokens[9], "-cp");
        assert_eq!(
            tokens[10],
            r"D:\AWork\Code\IPD\.gitworkspace\runtime\IPD原型后端\classpath\pathing-e2459fdcec83117c.jar"
        );
        assert_eq!(tokens[11], "com.jxdinfo.hussar.example.HussarApplication");
        assert_eq!(tokens.len(), 12);
    }

    /// 危险的程序参数同样逐 token 加引号（Node 服务 / 任意 `Script` plan）。
    #[test]
    fn plan_shell_command_script_quotes_dangerous_arguments() {
        let plan = LaunchPlan::Script {
            executable: PathBuf::from(r"C:\tools\npm.cmd"),
            args: vec!["run".into(), "dev".into(), "--".into(), "--host=a,b".into()],
            env: vec![],
            working_dir: PathBuf::from(r"D:\ws\web"),
            preview: String::new(),
        };
        let cmd = plan_shell_command(&plan);
        assert_eq!(cmd, r#""C:\tools\npm.cmd" run dev -- "--host=a,b""#);
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
