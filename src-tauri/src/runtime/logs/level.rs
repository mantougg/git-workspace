//! 日志级别解析（R-11，§36 级别过滤）：识别主流日志格式（Logback /
//! Spring Boot 默认 pattern、Log4j2 默认 pattern、Maven `[INFO]` 风格、
//! 裸级别前缀），识别不出时返回 `None`——调用方降级为原文展示，
//! 且不被级别过滤误杀（stack trace 续行等无级别行保持可见）。

use std::sync::OnceLock;

use regex::Regex;

use crate::runtime::logs::LogLevel;

/// 有序模式表：先 anchore 的时间戳格式，后宽松前缀；首个命中即返回。
fn patterns() -> &'static [Regex] {
    static RE: OnceLock<Vec<Regex>> = OnceLock::new();
    RE.get_or_init(|| {
        const LEVEL: &str = r"(TRACE|DEBUG|INFO|WARNING|WARN|ERROR|FATAL)";
        vec![
            // Logback / Spring Boot 默认：
            // `2026-08-23 12:00:00.123  INFO 12345 --- [main] c.e.App : msg`
            // `2026-08-23T12:00:00.123+08:00  INFO ...`（ISO offset 变体）
            Regex::new(&format!(
                r"^\d{{4}}-\d{{2}}-\d{{2}}[T ]\d{{2}}:\d{{2}}:\d{{2}}[.,]\d{{1,6}}(?:\s*(?:[+-]\d{{2}}:?{{0,1}}\d{{2}}|Z))?\s+{LEVEL}\b"
            ))
            .unwrap(),
            // Log4j2 默认 pattern：`12:00:00.123 [main] INFO  c.e.App - msg`
            Regex::new(&format!(
                r"^\d{{2}}:\d{{2}}:\d{{2}}[.,]\d{{1,6}}\s+\[[^\]]*\]\s+{LEVEL}\b"
            ))
            .unwrap(),
            // 无线程段时间戳变体：`12:00:00.123 INFO msg`
            Regex::new(&format!(r"^\d{{2}}:\d{{2}}:\d{{2}}[.,]\d{{1,6}}\s+{LEVEL}\b")).unwrap(),
            // Maven / 方括号风格：`[INFO] Building app`、`[ERROR] COMPILATION ERROR`
            Regex::new(&format!(r"^\[{LEVEL}\]\s")).unwrap(),
            // 裸级别前缀：`INFO: msg` / `WARN msg`
            Regex::new(&format!(r"^{LEVEL}[\s:]")).unwrap(),
        ]
    })
}

/// 把级别 token 归一化为 [`LogLevel`]（`WARNING`→Warn、`FATAL`→Error）。
pub fn parse_level_token(token: &str) -> Option<LogLevel> {
    match token {
        "TRACE" => Some(LogLevel::Trace),
        "DEBUG" => Some(LogLevel::Debug),
        "INFO" => Some(LogLevel::Info),
        "WARN" | "WARNING" => Some(LogLevel::Warn),
        "ERROR" | "FATAL" => Some(LogLevel::Error),
        _ => None,
    }
}

/// 解析一行日志的级别；无法识别返回 `None`（降级为原文）。
pub fn parse_level(line: &str) -> Option<LogLevel> {
    for re in patterns() {
        if let Some(captures) = re.captures(line) {
            if let Some(token) = captures.get(1) {
                return parse_level_token(token.as_str());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_logback_spring_boot_default_pattern() {
        let line = "2026-08-23 12:00:00.123  INFO 12345 --- [main] com.example.App : started";
        assert_eq!(parse_level(line), Some(LogLevel::Info));
        let line = "2026-08-23 12:00:00.123 ERROR 12345 --- [http-nio-8080-exec-1] c.e.C : boom";
        assert_eq!(parse_level(line), Some(LogLevel::Error));
        // 级别右对齐 padding（WARN 后两空格）。
        let line = "2026-08-23 12:00:00.123  WARN 12345 --- [main] c.e.C : careful";
        assert_eq!(parse_level(line), Some(LogLevel::Warn));
    }

    #[test]
    fn parses_iso_offset_timestamp_variant() {
        let line = "2026-08-23T12:00:00.123+08:00 DEBUG 12345 --- [main] c.e.C : dbg";
        assert_eq!(parse_level(line), Some(LogLevel::Debug));
        let line = "2026-08-23T04:00:00.123Z  INFO 12345 --- [main] c.e.C : utc";
        assert_eq!(parse_level(line), Some(LogLevel::Info));
    }

    #[test]
    fn parses_log4j2_default_pattern() {
        let line = "12:00:00.123 [main] INFO  com.example.App - hello";
        assert_eq!(parse_level(line), Some(LogLevel::Info));
        let line = "12:00:00.123 [worker-2] ERROR com.example.App - failed";
        assert_eq!(parse_level(line), Some(LogLevel::Error));
        // 无线程段变体。
        assert_eq!(parse_level("12:00:00.123 TRACE enter"), Some(LogLevel::Trace));
    }

    #[test]
    fn parses_maven_bracket_style() {
        assert_eq!(parse_level("[INFO] Building app 1.0"), Some(LogLevel::Info));
        assert_eq!(parse_level("[WARNING] deprecated API"), Some(LogLevel::Warn));
        assert_eq!(parse_level("[ERROR] COMPILATION ERROR"), Some(LogLevel::Error));
    }

    #[test]
    fn parses_bare_level_prefix() {
        assert_eq!(parse_level("INFO: plain"), Some(LogLevel::Info));
        assert_eq!(parse_level("FATAL boom"), Some(LogLevel::Error));
    }

    #[test]
    fn unrecognized_lines_fall_back_to_none() {
        for line in [
            "",
            "just a normal sentence",
            "    at com.example.App.main(App.java:10)",
            "Started Application in 1.234 seconds (process running)",
            "Tomcat started on port 8080 (http) with context path ''",
            // 时间戳后不是级别 token（例如 banner 续行）不能误判。
            "2026-08-23 12:00:00.123 something-else entirely",
        ] {
            assert_eq!(parse_level(line), None, "line: {line:?}");
        }
    }
}
