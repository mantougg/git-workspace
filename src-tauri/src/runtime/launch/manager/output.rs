use std::sync::Arc;

use crate::error::AppResult;
use crate::process::streaming::OutputStream;
use crate::runtime::build::BuildOutputSink;
use crate::runtime::config;
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

/// 启动横幅 / 端口探测正则（只读日志流，不做端口扫描；端口管理归 R-16）。
pub(super) fn startup_detectors() -> &'static (regex::Regex, regex::Regex) {
    static DETECTORS: std::sync::OnceLock<(regex::Regex, regex::Regex)> =
        std::sync::OnceLock::new();
    DETECTORS.get_or_init(|| {
        (
            // Spring Boot 启动完成横幅："Started Application in 3.2 seconds ..."。
            regex::Regex::new(r"Started \S+ in [\d.]+ seconds").unwrap(),
            // 内嵌容器端口："Tomcat started on port 8080 (http) ..." /
            // 旧版 "Tomcat started on port(s): 8080" / Netty 同构。
            regex::Regex::new(r"started on port(?:\(s\))?:?\s+(\d+)").unwrap(),
        )
    })
}
