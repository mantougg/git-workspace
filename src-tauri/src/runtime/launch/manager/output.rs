use std::sync::Arc;

use crate::error::AppResult;
use crate::process::streaming::OutputStream;
use crate::runtime::build::BuildOutputSink;
use crate::runtime::config;
use crate::runtime::config::RuntimeKind;
use crate::runtime::logs::redact::sensitive_env_values;
use crate::runtime::logs::{LogPhase, LogSession};

use super::*;

/// R-11：开启本次 Start 的日志会话（构建 + 运行输出统一进同一文件）。
/// 脱敏秘密值取自五层合并环境（与构建/启动环境同源）；仅在内存持有。
impl RuntimeProcessManager {
    pub(super) fn open_log_session(
        &self,
        workspace_id: i64,
        runtime_name: &str,
        process_id: i64,
    ) -> AppResult<()> {
        let (workspace_root, secrets) = {
            let conn = self.db.lock().unwrap();
            let root = config::workspace_root(&conn, workspace_id)?;
            let env = config::resolve_environment(&conn, workspace_id, runtime_name)?;
            let env: Vec<(String, String)> = env.into_iter().collect();
            (root, sensitive_env_values(&env))
        };
        self.deps.logs.open_session(
            &workspace_root,
            runtime_name,
            process_id,
            secrets,
            self.deps.events.clone(),
        )?;
        Ok(())
    }
}

/// R-11 构建输出挂接点：把构建阶段的行转发进本次 Start 的日志会话。
/// 行已被流水线 RedactingSink 脱敏；会话侧的再脱敏是幂等防御。
/// 会话不存在（防御分支）时静默丢弃。
pub(super) struct BuildLogSink {
    pub(super) session: Option<Arc<LogSession>>,
}

impl BuildOutputSink for BuildLogSink {
    fn on_line(&mut self, stream: OutputStream, line: &str) {
        if let Some(session) = &self.session {
            session.log(LogPhase::Build, stream, line);
        }
    }
}

/// 检测启动横幅 / 端口（只读日志流，不做端口扫描；端口管理归 R-16）。
/// Node 没有可靠的 ready banner，因此从运行输出中的 localhost URL 逐个收集端口。
pub(super) fn startup_banner(kind: RuntimeKind, line: &str) -> bool {
    static SPRING: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    kind == RuntimeKind::SpringBoot
        && SPRING
            .get_or_init(|| regex::Regex::new(r"Started \S+ in [\d.]+ seconds").unwrap())
            .is_match(line)
}

#[allow(dead_code)]
pub(super) fn startup_port(kind: RuntimeKind, line: &str) -> Option<u16> {
    startup_ports(kind, line).into_iter().next()
}

/// 从一行启动输出中提取全部端口。一个 Node dev server 可能同时输出
/// 应用端口、调试端口或多个本地服务 URL，不能只取首个匹配项。
pub(super) fn startup_ports(kind: RuntimeKind, line: &str) -> Vec<u16> {
    static SPRING: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    static NODE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let regex = match kind {
        RuntimeKind::SpringBoot => SPRING
            .get_or_init(|| regex::Regex::new(r"started on port(?:\(s\))?:?\s+(\d+)").unwrap()),
        RuntimeKind::Node => NODE.get_or_init(|| {
            regex::Regex::new(r"https?://(?:localhost|127\.0\.0\.1|0\.0\.0\.0|\[::1\]):(\d+)")
                .unwrap()
        }),
    };
    let mut ports = Vec::new();
    for captures in regex.captures_iter(line) {
        let Some(port) = captures
            .get(1)
            .and_then(|value| value.as_str().parse::<u16>().ok())
            .filter(|port| *port > 0)
        else {
            continue;
        };
        if !ports.contains(&port) {
            ports.push(port);
        }
    }
    ports
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spring_boot_detectors_preserve_existing_banner_and_port_shapes() {
        assert!(startup_banner(
            RuntimeKind::SpringBoot,
            "Started Application in 3.2 seconds (process running)"
        ));
        assert_eq!(
            startup_port(
                RuntimeKind::SpringBoot,
                "Tomcat started on port(s): 8080 (http)"
            ),
            Some(8080)
        );
        assert!(!startup_banner(
            RuntimeKind::Node,
            "Started Application in 3.2 seconds"
        ));
    }

    #[test]
    fn node_detector_accepts_real_dev_server_output_and_localhost_only() {
        let samples = [
            "  Local:   http://localhost:5173/",
            "Project is running at http://127.0.0.1:8080/",
            "- Local: http://localhost:3000",
        ];
        assert_eq!(startup_port(RuntimeKind::Node, samples[0]), Some(5173));
        assert_eq!(startup_port(RuntimeKind::Node, samples[1]), Some(8080));
        assert_eq!(startup_port(RuntimeKind::Node, samples[2]), Some(3000));
        assert_eq!(
            startup_port(RuntimeKind::Node, "Network: http://192.168.1.20:5173/"),
            None
        );
        assert_eq!(
            startup_port(RuntimeKind::Node, "compiled successfully"),
            None
        );
    }

    #[test]
    fn node_detector_collects_all_localhost_ports_from_one_line() {
        assert_eq!(
            startup_ports(
                RuntimeKind::Node,
                "Local: http://localhost:5173/ Inspector: http://127.0.0.1:9229/"
            ),
            vec![5173, 9229]
        );
        assert_eq!(
            startup_ports(
                RuntimeKind::Node,
                "Local: http://localhost:5173/ duplicate: http://localhost:5173/"
            ),
            vec![5173]
        );
    }
}
