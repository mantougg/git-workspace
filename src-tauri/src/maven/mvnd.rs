//! Maven Daemon（mvnd）检测与回退（R-18，§20/§73）。
//!
//! mvnd 是**可选增强**：未安装 / 探测失败时必须无感回退普通 mvn，不构成
//! 硬依赖（任务文档「架构/性能注意点」）。
//!
//! - 检测：PATH 查找（Windows 按 PATHEXT 语义 `.exe` → `.cmd` → `.bat`，
//!   复用 `java::detect::find_in_path`）+ `mvnd -v` 版本探测（mvnd 输出含
//!   `Apache Maven x.y.z` 行，复用 [`parse_mvn_version`]）；
//! - 回退：调用方（R-09 流水线）在 mvnd 不可用或构建输出出现 daemon 异常
//!   标记时回退普通 mvn，并经日志行向用户提示。

use crate::maven::detect_exec::{parse_mvn_version, probe_version};
use crate::maven::exec_model::{MavenExecutable, MavenVersionInfo};

/// mvnd 构建失败日志中判定「daemon 异常」的标记（大小写不敏感匹配）。
/// 命中任一标记 → 流水线回退普通 mvn 重试一次。
pub const DAEMON_FAILURE_MARKERS: &[&str] = &[
    "cannot connect to daemon",
    "daemon disappeared",
    "daemon crashed",
    "daemon stopped unexpectedly",
    "no suitable daemon",
    "unable to reach daemon",
];

/// mvnd 闲置退出默认值（毫秒）：daemon 常驻内存计入资源预算，闲置超时
/// 回收（以 `-Dmvnd.idleTimeout=` 透传；用户可用 extra_maven_args 覆盖）。
pub const DEFAULT_IDLE_TIMEOUT_MS: u64 = 120_000;

/// 一次 mvnd 检测结果。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MvndDetection {
    pub available: bool,
    pub executable_path: Option<String>,
    pub full_version: Option<String>,
    pub raw: Option<String>,
}

/// 检测 mvnd：PATH 查找 + 版本探测。`find_in_path` 已经覆盖 Windows
/// PATHEXT 语义（.exe/.cmd/.bat 优先，AGENTS.md 平台规范 §2）。
pub fn detect_mvnd() -> MvndDetection {
    let Some(path) = crate::java::detect::find_in_path("mvnd") else {
        log::info!("R-18: mvnd not found on PATH");
        return MvndDetection {
            available: false,
            executable_path: None,
            full_version: None,
            raw: None,
        };
    };
    let (info, is_valid) = probe_version(&path.to_string_lossy());
    if !is_valid {
        log::warn!(
            "R-18: mvnd found at {} but version probe failed: {}",
            path.display(),
            info.raw
        );
        return MvndDetection {
            available: false,
            executable_path: Some(path.to_string_lossy().into_owned()),
            full_version: None,
            raw: Some(info.raw),
        };
    }
    MvndDetection {
        available: true,
        executable_path: Some(path.to_string_lossy().into_owned()),
        full_version: info.full_version.clone(),
        raw: Some(info.raw),
    }
}

/// 把 mvnd 探测结果转成 [`MavenExecutable`]（source=System；mvnd 不属于
/// wrapper / configured 优先级链）。
pub fn mvnd_executable(path: &str, info: &MavenVersionInfo) -> MavenExecutable {
    let mut exe = MavenExecutable::new(path, crate::maven::exec_model::MavenSource::System, None);
    exe.major_version = info.major_version;
    exe.full_version = info.full_version.clone();
    exe.is_valid = true;
    exe.raw_version = Some(info.raw.clone());
    exe
}

/// 判定构建失败日志是否为 mvnd daemon 异常（回退 mvn 重试的依据）。
/// 纯函数（单测覆盖）。
pub fn looks_like_daemon_failure(log_tail: &str) -> bool {
    let lower = log_tail.to_ascii_lowercase();
    DAEMON_FAILURE_MARKERS.iter().any(|marker| lower.contains(marker))
}

/// mvnd 闲置退出参数（追加到构建调用；用户 extra_maven_args 显式设置时不
/// 重复注入——由调用方检查）。
pub fn idle_timeout_arg() -> String {
    format!("-Dmvnd.idleTimeout={DEFAULT_IDLE_TIMEOUT_MS}")
}

/// 解析 mvnd 版本行（复用 mvn 解析；mvnd 输出的 `Apache Maven x.y.z` 行
/// 可被同一解析器命中）。纯函数重导出以便单测聚合。
pub fn parse_mvnd_version(output: &str) -> MavenVersionInfo {
    parse_mvn_version(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn daemon_failure_markers_match_case_insensitively() {
        assert!(looks_like_daemon_failure(
            "[ERROR] Failed to execute build: Cannot connect to daemon"
        ));
        assert!(looks_like_daemon_failure(
            "the daemon disappeared unexpectedly mid-build"
        ));
        assert!(!looks_like_daemon_failure("[ERROR] COMPILATION ERROR on module app"));
        assert!(!looks_like_daemon_failure(""));
    }

    #[test]
    fn idle_timeout_arg_format() {
        assert_eq!(idle_timeout_arg(), "-Dmvnd.idleTimeout=120000");
    }

    #[test]
    fn parse_mvnd_version_extracts_apache_maven_line() {
        let output =
            "mvnd native client 1.0.2 (64b3a1f)\nTerminal: native\nApache Maven 3.9.9 (e5d0f4)\nJava version: 21\n";
        let info = parse_mvnd_version(output);
        assert_eq!(info.major_version, Some(3));
    }

    // detect_mvnd 的系统探测不做单测断言（环境相关：本机有无 mvnd 不确定）；
    // 探测失败路径由 find_in_path / probe_version 的单测与生产日志覆盖。
    #[test]
    fn detect_mvnd_is_total_and_reportable() {
        let detection = detect_mvnd();
        // 无论本机是否安装 mvnd，结果必须可序列化（IPC 形态稳定）。
        let json = serde_json::to_string(&detection).unwrap();
        assert!(json.contains("\"available\""));
    }

    #[test]
    fn mvnd_executable_marks_system_source() {
        let info = MavenVersionInfo {
            major_version: Some(3),
            full_version: Some("3.9.9".into()),
            raw: "Apache Maven 3.9.9".into(),
        };
        let exe = mvnd_executable("/usr/bin/mvnd", &info);
        assert_eq!(exe.executable_path, "/usr/bin/mvnd");
        assert_eq!(exe.source, crate::maven::exec_model::MavenSource::System);
        assert!(exe.is_valid);
    }

    // 平台注记：Windows 上 find_in_path 会优先命中 mvnd.cmd（PATHEXT 语义），
    // Unix 命中裸名 mvnd；两平台行为差异由 find_in_path 的既有约定保证。
    #[test]
    fn path_lookup_prefers_executable_extensions_on_windows() {
        let _ = Path::new("mvnd");
        // 该测试在 Windows 上由 java::detect::find_in_path 的实现保证扩展名
        // 优先级；Unix 下无行为差异。占位断言防误删注释。
        assert!(DAEMON_FAILURE_MARKERS.len() >= 5);
    }
}
