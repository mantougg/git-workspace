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
    pub(super) fn open_log_session(&self, workspace_id: i64, runtime_name: &str, process_id: i64) -> AppResult<()> {
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
/// Node 横幅 / 端口检测：从运行输出中检测 dev server 就绪横幅与 localhost URL 端口。
/// 检测一律在 [`strip_ansi`] 后的文本上进行：vite 8 起管道输出仍带 ANSI 色码，
/// 端口数字会被加粗序列从中间劈开（F-32）。日志会话存储的仍是原始行。
pub(super) fn startup_banner(kind: RuntimeKind, line: &str) -> bool {
    static SPRING: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    static NODE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let line = strip_ansi(line);
    match kind {
        RuntimeKind::SpringBoot => SPRING
            .get_or_init(|| regex::Regex::new(r"Started \S+ in [\d.]+ seconds").unwrap())
            .is_match(&line),
        // Node dev servers: vite prints "VITE vX.x  ready", webpack prints
        // "compiled successfully", Next prints "Ready in"; match broadly so
        // the process is flagged Running as soon as the dev server is ready.
        RuntimeKind::Node => NODE
            .get_or_init(|| {
                regex::Regex::new(r"(?i)(?:VITE\b.*ready|ready in \d|compiled successfully|listening on)").unwrap()
            })
            .is_match(&line),
    }
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
    let line = strip_ansi(line);
    let regex = match kind {
        RuntimeKind::SpringBoot => {
            SPRING.get_or_init(|| regex::Regex::new(r"started on port(?:\(s\))?:?\s+(\d+)").unwrap())
        }
        RuntimeKind::Node => NODE.get_or_init(|| {
            regex::Regex::new(r"https?://(?:localhost|127\.0\.0\.1|0\.0\.0\.0|\[::1\]):(\d+)").unwrap()
        }),
    };
    let mut ports = Vec::new();
    for captures in regex.captures_iter(&line) {
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

/// 剥除 ANSI 转义序列（F-32）。vite 8 在管道（非 TTY）输出下仍打印彩色
/// 横幅（`CI=true`/`TERM=dumb` 亦发色，仅 NO_COLOR 关闭），`Local:`
/// 行的端口号被 `\x1b[1m5176\x1b[22m` 劈开。覆盖两类序列：
/// - CSI：`ESC [` … `@`~``（含 SGR 色码）
/// - OSC：`ESC ]` … `BEL` 或 `ESC \`（终端标题一类）
/// 无转义时返回原切片（零拷贝）。
pub(super) fn strip_ansi(line: &str) -> std::borrow::Cow<'_, str> {
    if !line.contains('\u{1b}') {
        return std::borrow::Cow::Borrowed(line);
    }
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(pos) = rest.find('\u{1b}') {
        out.push_str(&rest[..pos]);
        rest = &rest[pos + 1..];
        let Some(after_esc) = rest.chars().next() else {
            break;
        };
        match after_esc {
            '[' => {
                // CSI：参数与中间字节（0x30..=0x3F）到最终字节（0x40..=0x7E）。
                let end = rest
                    .char_indices()
                    .skip(1)
                    .find(|(_, ch)| ('@'..='~').contains(ch))
                    .map(|(idx, ch)| idx + ch.len_utf8());
                match end {
                    Some(end) => rest = &rest[end..],
                    None => {
                        rest = "";
                        break;
                    }
                }
            }
            ']' => {
                // OSC：BEL 或 ST（ESC \）结尾。
                let end = rest
                    .find('\u{7}')
                    .map(|idx| idx + 1)
                    .or_else(|| rest.find("\u{1b}\\").map(|idx| idx + 2));
                match end {
                    Some(end) => rest = &rest[end..],
                    None => {
                        rest = "";
                        break;
                    }
                }
            }
            _ => {
                // 其余两字符转义（如 ESC c）跳过 ESC 后的这一个字符。
                rest = &rest[after_esc.len_utf8()..];
            }
        }
    }
    out.push_str(rest);
    std::borrow::Cow::Owned(out)
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
            startup_port(RuntimeKind::SpringBoot, "Tomcat started on port(s): 8080 (http)"),
            Some(8080)
        );
        assert!(!startup_banner(RuntimeKind::Node, "Started Application in 3.2 seconds"));
    }

    #[test]
    fn node_startup_banner_detects_dev_server_ready() {
        assert!(startup_banner(RuntimeKind::Node, "  VITE v5.4.10  ready in 456 ms"));
        assert!(startup_banner(RuntimeKind::Node, "Ready in 1.2s"));
        assert!(startup_banner(RuntimeKind::Node, "compiled successfully"));
        assert!(!startup_banner(RuntimeKind::Node, "Installing dependencies..."));
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
        assert_eq!(startup_port(RuntimeKind::Node, "compiled successfully"), None);
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

    /// F-32 回归：vite 8 起的 dev server 在管道（非 TTY）输出下仍打印彩色
    /// 横幅（实测 `CI=true TERM=dumb` 亦发色，仅 NO_COLOR 关闭），端口号被
    /// `\x1b[1m5176\x1b[22m` 一类加粗序列从中间劈开。以下字节序列取自真实
    /// vite 8.2.2 vanilla 模板输出。
    #[test]
    fn node_detectors_parse_ansi_colored_vite8_output() {
        let banner =
            "  \u{1b}[32m\u{1b}[1mVITE\u{1b}[22m v8.2.2\u{1b}[39m  \u{1b}[2mready in \u{1b}[0m\u{1b}[1m317\u{1b}[22m\u{1b}[2m\u{1b}[0m ms\u{1b}[22m";
        assert!(startup_banner(RuntimeKind::Node, banner));
        let local = "  \u{1b}[32m➜\u{1b}[39m  \u{1b}[1mLocal\u{1b}[22m:   \u{1b}[36mhttp://localhost:\u{1b}[1m5176\u{1b}[22m/\u{1b}[39m";
        assert_eq!(startup_ports(RuntimeKind::Node, local), vec![5176]);
        // OSC 序列（终端标题）同样不得干扰端口提取。
        assert_eq!(
            startup_ports(RuntimeKind::Node, "\u{1b}]0;vite\u{7}http://localhost:3000"),
            vec![3000]
        );
    }

    #[test]
    fn strip_ansi_removes_csi_and_osc_sequences() {
        // vite 8.2.2 真实 Local 行（od 实测字节）。
        assert_eq!(
            strip_ansi("\u{1b}[36mhttp://localhost:\u{1b}[1m5176\u{1b}[22m/"),
            "http://localhost:5176/"
        );
        // OSC BEL / ST 两种结尾。
        assert_eq!(strip_ansi("\u{1b}]0;vite\u{7}ok"), "ok");
        assert_eq!(strip_ansi("\u{1b}]0;vite\u{1b}\\ok"), "ok");
        // 非法 UTF-8 已被 lossy 解码为 U+FFFD（F-12 链路），不得误伤。
        assert_eq!(strip_ansi("\u{FFFD}gb"), "\u{FFFD}gb");
        // 无转义：零拷贝借用原串。
        assert!(matches!(strip_ansi("plain"), std::borrow::Cow::Borrowed("plain")));
        // 行尾截断的未完成序列：丢弃残余，不 panic。
        assert_eq!(strip_ansi("ok\u{1b}[3"), "ok");
    }
}
