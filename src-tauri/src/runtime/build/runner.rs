//! Maven 执行 seam（R-09）：把「跑一次 Maven 调用」抽象成可替换的 trait。
//!
//! - [`SpawningMavenRunner`] 是生产实现：`executor::build_process` 构造命令，
//!   `process::streaming::spawn_streaming` 流式转发输出、传播取消与超时。
//! - `#[cfg(test)]` 的 `FakeMavenRunner` 记录请求、回放预设输出/退出码，
//!   让流水线单测不依赖本机 Maven。

use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use crate::error::AppResult;
use crate::maven::exec_model::{MavenExecutionRequest, MavenSource, ResolvedMaven};
use crate::maven::executor;
use crate::process::streaming::{spawn_streaming, StreamingExit};
use crate::runtime::build::BuildOutputSink;

/// 一次 Maven 调用的执行体。
///
/// `Send + Sync`：R-10 Process Manager 经 `Arc<dyn MavenRunner>` 跨线程共享。
pub trait MavenRunner: Send + Sync {
    /// 为项目解析/校验 Maven 可执行体（流水线的 Validate Maven 步骤）。
    ///
    /// 默认实现走真实检测（会 fork `mvn -v`）；测试 fake 覆盖它以保持
    /// 单测环境无 Maven 依赖。
    fn resolve_maven(
        &self,
        project_dir: &Path,
        local_repository: &Path,
    ) -> AppResult<ResolvedMaven> {
        crate::maven::detect_exec::resolve_maven_for_project(project_dir, None, local_repository)
            .ok_or_else(|| {
                crate::error::AppError::MavenNotFound(format!(
                    "未在项目 {} 找到可用的 Maven（wrapper / 配置 / 系统三者皆缺）。\
                     请安装 Maven 或在 Settings 中配置 Maven 可执行路径。",
                    project_dir.display()
                ))
            })
    }

    /// R-18：带 Build Engine 偏好的解析。默认忽略偏好（测试 fake 不受影响）；
    /// 生产实现 [`SpawningMavenRunner`] 在 `Mvnd` 偏好下尝试 PATH 检测 mvnd，
    /// 未安装返回 `None`（调用方回退普通 mvn 并提示）。
    fn resolve_maven_for_engine(
        &self,
        project_dir: &Path,
        local_repository: &Path,
        _engine: BuildEngineHint,
    ) -> AppResult<Option<ResolvedMaven>> {
        Ok(Some(self.resolve_maven(project_dir, local_repository)?))
    }

    /// 执行一次 Maven 调用，把输出按行转发给 `sink`。
    fn run(
        &self,
        request: &MavenExecutionRequest,
        env: &[(String, String)],
        sink: &mut dyn BuildOutputSink,
        cancel: Option<&AtomicBool>,
        timeout: Option<Duration>,
    ) -> AppResult<StreamingExit>;
}

/// Build Engine 偏好（R-18）：决定可执行体解析走 mvn 还是 mvnd。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildEngineHint {
    Maven,
    Mvnd,
}

/// 生产实现：真实 spawn Maven 子进程。
pub struct SpawningMavenRunner;

impl MavenRunner for SpawningMavenRunner {
    /// R-18：`Mvnd` 偏好 → PATH 检测 mvnd（PATHEXT 语义在 find_in_path 内）；
    /// 未安装 / 探测失败返回 `None`，调用方回退普通 mvn。
    fn resolve_maven_for_engine(
        &self,
        project_dir: &Path,
        local_repository: &Path,
        engine: BuildEngineHint,
    ) -> AppResult<Option<ResolvedMaven>> {
        if engine == BuildEngineHint::Maven {
            return Ok(Some(self.resolve_maven(project_dir, local_repository)?));
        }
        let detection = crate::maven::mvnd::detect_mvnd();
        if !detection.available {
            return Ok(None);
        }
        let Some(path) = &detection.executable_path else {
            return Ok(None);
        };
        let mut exe =
            crate::maven::exec_model::MavenExecutable::new(path, MavenSource::System, None);
        exe.is_valid = true;
        exe.raw_version = detection.raw.clone();
        exe.full_version = detection.full_version.clone();
        Ok(Some(ResolvedMaven {
            executable: exe,
            local_repository: local_repository.to_path_buf(),
            uses_wrapper: false,
        }))
    }

    fn run(
        &self,
        request: &MavenExecutionRequest,
        env: &[(String, String)],
        sink: &mut dyn BuildOutputSink,
        cancel: Option<&AtomicBool>,
        timeout: Option<Duration>,
    ) -> AppResult<StreamingExit> {
        let mut command = executor::build_process(request, env);
        let exit = spawn_streaming(&mut command, cancel, timeout, &mut |stream, line| {
            sink.on_line(stream, line);
        })?;
        Ok(exit)
    }
}

#[cfg(test)]
pub mod fake {
    //! `FakeMavenRunner`：按调用顺序回放预设脚本，记录每一次请求供断言。
    //! `output_file_content` 用于模拟 `dependency:build-classpath`：扫描
    //! 请求里的 `-Dmdep.outputFile=...` 并把预设内容写进去。

    use std::sync::Mutex;

    use super::*;
    use crate::maven::exec_model::{MavenExecutable, MavenSource};
    use crate::process::streaming::OutputStream;

    pub struct FakeRun {
        /// 逐行回放的输出。
        pub lines: Vec<(OutputStream, String)>,
        pub exit_code: Option<i32>,
        /// 模拟长耗时：以 10ms 粒度睡眠并检查取消标记；`None` 立即退出。
        pub duration: Option<Duration>,
        /// 若请求带 `-Dmdep.outputFile`，写入这段内容。
        pub output_file_content: Option<String>,
    }

    impl Default for FakeRun {
        fn default() -> Self {
            Self {
                lines: Vec::new(),
                exit_code: Some(0),
                duration: None,
                output_file_content: None,
            }
        }
    }

    pub struct FakeMavenRunner {
        pub requests: Mutex<Vec<MavenExecutionRequest>>,
        pub envs: Mutex<Vec<Vec<(String, String)>>>,
        pub script: Mutex<Vec<FakeRun>>,
    }

    impl FakeMavenRunner {
        pub fn new(script: Vec<FakeRun>) -> Self {
            Self {
                requests: Mutex::new(Vec::new()),
                envs: Mutex::new(Vec::new()),
                script: Mutex::new(script),
            }
        }

        /// 全部脚本消费完后再次调用时返回的默认成功结果。
        pub fn successful() -> Self {
            Self::new(Vec::new())
        }

        pub fn request_count(&self) -> usize {
            self.requests.lock().unwrap().len()
        }

        pub fn requests(&self) -> Vec<MavenExecutionRequest> {
            self.requests.lock().unwrap().clone()
        }

        pub fn envs(&self) -> Vec<Vec<(String, String)>> {
            self.envs.lock().unwrap().clone()
        }
    }

    impl MavenRunner for FakeMavenRunner {
        fn resolve_maven(
            &self,
            _project_dir: &Path,
            local_repository: &Path,
        ) -> AppResult<ResolvedMaven> {
            Ok(ResolvedMaven {
                executable: MavenExecutable::new("fake-mvn", MavenSource::System, None),
                local_repository: local_repository.to_path_buf(),
                uses_wrapper: false,
            })
        }

        fn run(
            &self,
            request: &MavenExecutionRequest,
            env: &[(String, String)],
            sink: &mut dyn BuildOutputSink,
            cancel: Option<&AtomicBool>,
            _timeout: Option<Duration>,
        ) -> AppResult<StreamingExit> {
            self.requests.lock().unwrap().push(request.clone());
            self.envs.lock().unwrap().push(env.to_vec());
            let step = {
                let mut script = self.script.lock().unwrap();
                if script.is_empty() {
                    FakeRun::default()
                } else {
                    script.remove(0)
                }
            };

            for (stream, line) in &step.lines {
                sink.on_line(*stream, line);
            }
            if let Some(content) = &step.output_file_content {
                if let Some(output) = request
                    .extra_args
                    .iter()
                    .find_map(|arg| arg.strip_prefix("-Dmdep.outputFile="))
                {
                    std::fs::write(output, content).unwrap();
                }
            }

            if let Some(duration) = step.duration {
                let start = std::time::Instant::now();
                while start.elapsed() < duration {
                    if cancel.is_some_and(|flag| flag.load(std::sync::atomic::Ordering::Relaxed)) {
                        return Ok(StreamingExit {
                            exit_code: None,
                            timed_out: false,
                            cancelled: true,
                        });
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
            }

            Ok(StreamingExit {
                exit_code: step.exit_code,
                timed_out: false,
                cancelled: false,
            })
        }
    }
}

#[cfg(test)]
pub use fake::{FakeMavenRunner, FakeRun};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::streaming::OutputStream;
    use crate::runtime::build::RingTail;

    #[cfg(unix)]
    #[test]
    fn spawning_runner_streams_real_output() {
        let request = MavenExecutionRequest {
            working_dir: std::env::temp_dir(),
            executable: "sh".into(),
            goals: vec!["-c".into(), "echo hello-from-fake-mvn".into()],
            extra_args: vec![],
            via_cmd_c: false,
            local_repository: None,
        };
        let mut tail = RingTail::new();
        let exit = SpawningMavenRunner
            .run(&request, &[], &mut tail, None, None)
            .unwrap();
        assert_eq!(exit.exit_code, Some(0));
        assert!(tail.tail().contains("hello-from-fake-mvn"));
        let _ = OutputStream::Stdout;
    }

    #[test]
    fn fake_runner_records_requests_and_replays_script() {
        let runner = FakeMavenRunner::new(vec![
            FakeRun {
                lines: vec![(OutputStream::Stdout, "building".into())],
                exit_code: Some(1),
                ..Default::default()
            },
            FakeRun::default(),
        ]);
        let request = MavenExecutionRequest {
            working_dir: std::path::PathBuf::from("/p"),
            executable: "fake-mvn".into(),
            goals: vec!["compile".into()],
            extra_args: vec![],
            via_cmd_c: false,
            local_repository: None,
        };
        let mut tail = RingTail::new();
        let first = runner
            .run(&request, &[("A".into(), "1".into())], &mut tail, None, None)
            .unwrap();
        assert_eq!(first.exit_code, Some(1));
        assert_eq!(tail.tail(), "building");

        // 脚本消费完后默认成功。
        let second = runner.run(&request, &[], &mut tail, None, None).unwrap();
        assert_eq!(second.exit_code, Some(0));

        assert_eq!(runner.request_count(), 2);
        assert_eq!(runner.requests()[0].goals, ["compile"]);
        assert_eq!(runner.envs()[0], [("A".to_string(), "1".to_string())]);
    }
}
