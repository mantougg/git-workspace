//! Maven Executor 抽象（R-05，§18）。
//!
//! 提供「命令构造 / 工作目录 / 环境注入 / 输出流转发」的统一接口，供 Build
//! Engine（R-09）调用。本任务只做命令构造与预览（参数可断言、可追溯）；
//! 实际进程启动、确认流与生命周期管理留给 R-09 / R-10（全局约束 §3）。
//!
//! 设计预留 mvnd（R-18）、Gradle（R-22）的扩展位，但不提前实现（任务文档
//! 「架构/性能注意点」）。

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::maven::exec_model::{MavenExecutable, MavenExecutionRequest};

/// 构造完整的 Maven 命令行（含 `cmd /c` 前缀，若需要）。
///
/// 返回可直接断言 / 展示给用户预览的字符串列表。Build Engine（R-09）拿到
/// 此列表后可直接 `Command::new(parts[0]).args(&parts[1..])` 启动（确认流在 R-09）。
///
/// 命令结构：
/// ```text
/// [cmd /c] <executable> <goals...> <-Dmaven.repo.local=...> <extra_args...>
/// ```
pub fn build_command(req: &MavenExecutionRequest) -> Vec<String> {
    let mut parts = Vec::new();
    let exe = Path::new(&req.executable);

    if cfg!(windows) && crate::maven::detect_exec::needs_cmd_c(exe) {
        parts.push("cmd".into());
        parts.push("/c".into());
        parts.push(req.executable.clone());
    } else {
        parts.push(req.executable.clone());
    }

    parts.extend(req.goals.iter().cloned());

    if let Some(local_repo) = &req.local_repository {
        parts.push(format!("-Dmaven.repo.local={}", local_repo.to_string_lossy()));
    }

    parts.extend(req.extra_args.iter().cloned());
    parts
}

/// 从一个已解析的 `MavenExecutable` 构造 `MavenExecutionRequest`。
///
/// 这是 Build Engine 的便捷入口：给定选中的 Maven、项目目录与 goals，组装出
/// 可预览 / 可执行的请求。
pub fn build_request(
    executable: &MavenExecutable,
    working_dir: &Path,
    goals: Vec<String>,
    extra_args: Vec<String>,
    local_repository: Option<PathBuf>,
) -> MavenExecutionRequest {
    let via_cmd_c = cfg!(windows) && crate::maven::detect_exec::needs_cmd_c(Path::new(&executable.executable_path));
    MavenExecutionRequest {
        working_dir: working_dir.to_path_buf(),
        executable: executable.executable_path.clone(),
        goals,
        extra_args,
        via_cmd_c,
        local_repository,
    }
}

/// 构造用于实际 spawn 的 `Command`（设置工作目录与基础环境）。
///
/// 环境注入（§18）：`JAVA_HOME`（若用户/项目绑定了 JDK）与 `MAVEN_OPTS`（若
/// 调用方传入）会注入子进程环境。输出流转发到 `stdout` / `stderr` inherit
/// 由调用方在 spawn 后配置；本函数只构造 Command 对象。
///
/// **不 spawn**；返回 `Command` 供 R-09 在确认后启动。
pub fn build_process(req: &MavenExecutionRequest, env: &[(String, String)]) -> Command {
    let parts = build_command(req);
    debug_assert!(!parts.is_empty(), "command must have at least the executable");
    let mut cmd = Command::new(&parts[0]);
    cmd.args(&parts[1..]);
    cmd.current_dir(&req.working_dir);
    for (k, v) in env {
        cmd.env(k, v);
    }
    cmd
}

/// 返回命令的可读预览字符串（供 UI「预览完整参数」展示，§75）。
pub fn preview_command(req: &MavenExecutionRequest) -> String {
    build_command(req).join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::maven::exec_model::MavenSource;

    fn exe(path: &str, source: MavenSource) -> MavenExecutable {
        MavenExecutable::new(path, source, None)
    }

    #[test]
    fn builds_command_with_goals_and_local_repo() {
        let req = MavenExecutionRequest {
            working_dir: PathBuf::from("/project"),
            executable: "/usr/bin/mvn".into(),
            goals: vec!["clean".into(), "install".into()],
            extra_args: vec!["-DskipTests".into()],
            local_repository: Some(PathBuf::from("/custom/.m2/repository")),
            via_cmd_c: false,
        };
        let parts = build_command(&req);
        assert_eq!(parts[0], "/usr/bin/mvn");
        assert!(parts.contains(&"clean".to_string()));
        assert!(parts.contains(&"install".to_string()));
        assert!(parts.contains(&"-DskipTests".to_string()));
        assert!(parts
            .iter()
            .any(|p| p.starts_with("-Dmaven.repo.local=/custom/.m2/repository")));
    }

    #[test]
    fn omits_local_repo_when_none() {
        let req = MavenExecutionRequest {
            working_dir: PathBuf::from("/p"),
            executable: "/usr/bin/mvn".into(),
            goals: vec!["validate".into()],
            extra_args: vec![],
            local_repository: None,
            via_cmd_c: false,
        };
        let parts = build_command(&req);
        assert!(
            !parts.iter().any(|p| p.starts_with("-Dmaven.repo.local")),
            "no local repo flag when unset"
        );
    }

    #[cfg(windows)]
    #[test]
    fn prepends_cmd_c_for_wrapper_cmd() {
        let req = MavenExecutionRequest {
            working_dir: PathBuf::from("C:/project"),
            executable: "C:/project/mvnw.cmd".into(),
            goals: vec!["clean".into()],
            extra_args: vec![],
            local_repository: None,
            via_cmd_c: true,
        };
        let parts = build_command(&req);
        assert_eq!(parts[0], "cmd");
        assert_eq!(parts[1], "/c");
        assert_eq!(parts[2], "C:/project/mvnw.cmd");
    }

    #[test]
    fn build_request_inherits_goals_and_args() {
        let exe = exe("/usr/bin/mvn", MavenSource::System);
        let req = build_request(
            &exe,
            Path::new("/project"),
            vec!["compile".into()],
            vec!["-Pprod".into()],
            Some(PathBuf::from("/repo")),
        );
        assert_eq!(req.working_dir, PathBuf::from("/project"));
        assert_eq!(req.goals, vec!["compile".to_string()]);
        assert_eq!(req.extra_args, vec!["-Pprod".to_string()]);
        assert_eq!(req.local_repository, Some(PathBuf::from("/repo")));
    }

    #[test]
    fn preview_command_is_space_joined() {
        let req = MavenExecutionRequest {
            working_dir: PathBuf::from("/p"),
            executable: "/usr/bin/mvn".into(),
            goals: vec!["clean".into()],
            extra_args: vec![],
            local_repository: None,
            via_cmd_c: false,
        };
        let preview = preview_command(&req);
        assert!(preview.starts_with("/usr/bin/mvn clean"));
    }
}
