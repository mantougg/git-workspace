//! node / 包管理器可执行检测与版本探测（N-01，设计文档 §4.1）。
//!
//! - 一律走 `java/detect.rs::find_in_path`（`.exe → .cmd → .bat → 裸名`）：
//!   Windows 上 npm/pnpm/yarn 实体是 `.cmd` shim，直接 `Command::new("npm")`
//!   会命中不可执行的 Unix shim（os error 193，R-14 同款坑）。
//! - 版本探测仿 `maven/detect_exec.rs::probe_version`（超时 + 输出上限，
//!   复用其 `wait_with_timeout` / `needs_cmd_c`）；探测失败降级为
//!   「未知版本」（`version=None`），不报错。
//! - 检测不到可执行才报 `NodeNotFound` / `PackageManagerNotFound`
//!   可行动错误（§4.7：带 Suggested Actions 穿透 IPC）。
//! - 全程本地完成、无网络请求（全局约束 §10）。

use std::path::Path;
use std::process::Command;
use std::time::Duration;

use crate::error::{AppError, AppResult};
use crate::java::detect::find_in_path;
use crate::maven::detect_exec::{needs_cmd_c, wait_with_timeout};
use crate::node::decision::PackageManagerDecision;
use crate::node::model::{PackageManager, ToolDetection, ToolDetectionSource};

/// 版本探测超时（秒）。仿 `MVN_VERSION_TIMEOUT_SECS`；`node -v` / `<pm> -v`
/// 正常亚秒返回，超时多为 shim 异常。
const PROBE_TIMEOUT_SECS: u64 = 10;

/// 检测 `node` 可执行 + 版本；PATH 无 node → `NodeNotFound` 可行动错误。
pub fn detect_node() -> AppResult<ToolDetection> {
    let exe = find_in_path("node").ok_or_else(|| {
        AppError::NodeNotFound(
            "PATH 上未找到 node 可执行文件。请安装 Node.js LTS 并把 node 加入 PATH 后重试。"
                .to_string(),
        )
    })?;
    Ok(probe_tool(&exe))
}

/// 检测指定包管理器可执行 + 版本；PATH 无该可执行 → `PackageManagerNotFound`。
pub fn detect_package_manager(pm: PackageManager) -> AppResult<ToolDetection> {
    let exe = find_in_path(pm.executable_name()).ok_or_else(|| {
        AppError::PackageManagerNotFound(format!(
            "PATH 上未找到 {} 可执行文件。请安装 {}，或在配置中改选 npm。",
            pm.name(),
            pm.name()
        ))
    })?;
    Ok(probe_tool(&exe))
}

/// 把决策链结果解析为可执行绝对路径。
///
/// bun 只识别不执行（MVP 不支持，属 N-09）→ `PackageManagerNotFound`
/// 可行动错误引导改选；其余选中但 PATH 不可执行 → 同款错误并附决策来源。
pub fn resolve_package_manager(decision: &PackageManagerDecision) -> AppResult<ToolDetection> {
    if decision.manager == PackageManager::Bun {
        return Err(AppError::PackageManagerNotFound(format!(
            "决策链选中 bun（{}），当前版本不支持 bun 执行（属 N-09）。\
             请在配置中改选 npm，或改用 npm/pnpm/yarn 管理该工程。",
            decision.reason
        )));
    }
    detect_package_manager(decision.manager).map_err(|err| match err {
        AppError::PackageManagerNotFound(msg) => {
            AppError::PackageManagerNotFound(format!("{msg}（决策来源：{}）", decision.reason))
        }
        other => other,
    })
}

/// Resolve Node.js from the persistent registry before consulting PATH.
pub fn resolve_node_with_registry(
    conn: &rusqlite::Connection,
) -> AppResult<ToolDetection> {
    if let Some(entry) = crate::node::registry::find_valid_node(conn)? {
        return Ok(ToolDetection {
            executable: std::path::PathBuf::from(entry.executable_path),
            version: entry.version,
            raw_output: entry.raw_output,
            probe_ok: entry.is_valid,
            source: ToolDetectionSource::Registry,
        });
    }
    detect_node()
}

/// Resolve the selected package manager from the persistent registry before
/// falling back to the normal PATHEXT-aware PATH lookup.
pub fn resolve_package_manager_with_registry(
    conn: &rusqlite::Connection,
    decision: &PackageManagerDecision,
) -> AppResult<ToolDetection> {
    if decision.manager == PackageManager::Bun {
        return resolve_package_manager(decision);
    }
    if let Some(entry) = crate::node::registry::find_valid_package_manager(conn, decision.manager)? {
        return Ok(ToolDetection {
            executable: std::path::PathBuf::from(entry.executable_path),
            version: entry.version,
            raw_output: entry.raw_output,
            probe_ok: entry.is_valid,
            source: ToolDetectionSource::Registry,
        });
    }
    resolve_package_manager(decision)
}

/// 对可执行体跑 `-v` 探测版本。探测失败降级「未知版本」，不报错。
pub fn probe_tool(executable: &Path) -> ToolDetection {
    let (version, raw, ok) = probe_version(executable);
    ToolDetection {
        executable: executable.to_path_buf(),
        version,
        raw_output: raw,
        probe_ok: ok,
        source: ToolDetectionSource::Path,
    }
}

/// Fork `<exe> -v`：超时 + 输出上限（复用 maven 的 `wait_with_timeout`）。
/// 返回 `(version, raw, ok)`；任何失败都降级为 `(None, 原因, false)`。
fn probe_version(executable: &Path) -> (Option<String>, String, bool) {
    let mut cmd = build_probe_command(executable);
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());
    let child = match cmd.spawn() {
        Ok(c) => c,
        Err(err) => {
            log::warn!("tool probe {:?} failed to spawn: {}", executable, err);
            return (None, format!("probe error: {err}"), false);
        }
    };
    let combined = match wait_with_timeout(child, Duration::from_secs(PROBE_TIMEOUT_SECS)) {
        Ok(s) => s,
        Err(err) => {
            log::warn!("tool probe {:?} timed out or failed: {}", executable, err);
            return (None, format!("probe error: {err}"), false);
        }
    };
    match extract_version(&combined) {
        Some(version) => (Some(version), combined, true),
        None => (None, combined, false),
    }
}

/// 构造 `-v` 探测命令。Windows 下 `.cmd` / `.bat` 经 `cmd /C` 调用
/// （npm/pnpm/yarn 实体是 `.cmd` shim；§19 同款，`needs_cmd_c` 复用）。
fn build_probe_command(exe: &Path) -> Command {
    if cfg!(windows) && needs_cmd_c(exe) {
        let mut cmd = Command::new("cmd");
        cmd.arg("/C").arg(exe).arg("-v");
        cmd
    } else {
        let mut cmd = Command::new(exe);
        cmd.arg("-v");
        cmd
    }
}

/// 从 `node -v` / `<pm> -v` 输出提取版本串（纯函数）：
/// 首个以数字或 `v`+数字开头的非空行，去 `v` 前缀。
///
/// 样例取真实工具输出：`v22.14.0`（node）、`10.9.2`（npm）、
/// `9.15.9`（pnpm）、`1.22.22` / `4.9.2`（yarn）。解析不到返回 `None`。
pub fn extract_version(output: &str) -> Option<String> {
    for line in output.lines() {
        let trimmed = line.trim();
        let stripped = trimmed.strip_prefix('v').unwrap_or(trimmed);
        if stripped.chars().next().is_some_and(|c| c.is_ascii_digit()) {
            return Some(stripped.to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::java::detect::{executable_candidates, find_executable_in_dirs};
    use std::path::PathBuf;

    #[test]
    fn candidate_order_windows_prefers_extensions() {
        // PATHEXT 语义纯函数：.exe → .cmd → .bat → 裸名（R-14 顺序）。
        assert_eq!(
            executable_candidates("npm", true),
            vec!["npm.exe", "npm.cmd", "npm.bat", "npm"]
        );
    }

    #[test]
    fn candidate_order_unix_bare_only() {
        assert_eq!(executable_candidates("npm", false), vec!["npm"]);
    }

    #[test]
    fn finds_executable_with_platform_candidate_order() {
        let tmp = std::env::temp_dir().join(format!(
            "gw_node_cand_{}",
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        std::fs::create_dir_all(&tmp).unwrap();
        // 裸名（Unix shim）与 .cmd 并存：Windows 必须命中 .cmd，Unix 命中裸名。
        std::fs::write(tmp.join("npm"), b"#!/bin/sh\n").unwrap();
        std::fs::write(tmp.join("npm.cmd"), b"@echo off\n").unwrap();
        let found =
            find_executable_in_dirs("npm", std::slice::from_ref(&tmp)).expect("npm must be found");
        let name = found.file_name().unwrap().to_string_lossy().to_string();
        if cfg!(windows) {
            assert_eq!(
                name, "npm.cmd",
                "Windows 必须先命中 .cmd 候选（os error 193 防线）"
            );
        } else {
            assert_eq!(name, "npm", "Unix 回退裸名");
        }
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn extracts_versions_from_real_tool_outputs() {
        // 样例取真实工具输出原文（00 约束 §5）。
        assert_eq!(extract_version("v22.14.0\n"), Some("22.14.0".to_string()));
        assert_eq!(extract_version("10.9.2\n"), Some("10.9.2".to_string()));
        assert_eq!(extract_version("9.15.9"), Some("9.15.9".to_string()));
        assert_eq!(extract_version("1.22.22\n"), Some("1.22.22".to_string()));
        // 警告行在前、版本行在后。
        assert_eq!(
            extract_version("npm warn cli some warning\n10.9.2\n"),
            Some("10.9.2".to_string())
        );
        // 非版本输出 → None（探测失败降级，不报错）。
        assert_eq!(extract_version("not a version\nrandom text"), None);
        assert_eq!(extract_version(""), None);
    }

    #[test]
    fn bun_decision_is_actionable_error_not_executable() {
        let decision = PackageManagerDecision {
            manager: PackageManager::Bun,
            source: crate::node::decision::DecisionSource::Lockfile,
            reason: "lockfile 推断：bun.lockb".to_string(),
        };
        let err = resolve_package_manager(&decision).unwrap_err();
        match err {
            AppError::PackageManagerNotFound(msg) => {
                assert!(msg.contains("bun"), "error must name bun: {msg}");
                assert!(
                    msg.contains("npm"),
                    "error must suggest switching to npm: {msg}"
                );
            }
            other => panic!("expected PackageManagerNotFound, got {other:?}"),
        }
    }

    /// 真实环境冒烟：检测到 node 并返回版本；无环境 skip 并打印原因
    ///（AGENTS.md 平台规范 §4，不硬失败）。
    #[test]
    fn real_node_probe_smoke() {
        let detection = match detect_node() {
            Ok(d) => d,
            Err(AppError::NodeNotFound(msg)) => {
                eprintln!("N-01: no `node` on PATH; skipping real probe smoke ({msg})");
                return;
            }
            Err(other) => panic!("unexpected error: {other:?}"),
        };
        assert!(detection.executable.is_file());
        if !detection.probe_ok {
            eprintln!(
                "N-01: node found at {:?} but probe failed (raw={:?}); skipping version assert",
                detection.executable, detection.raw_output
            );
            return;
        }
        assert!(
            detection.version.is_some(),
            "real node must yield a version"
        );
        eprintln!(
            "N-01 real probe: node={:?} version={:?}",
            detection.executable, detection.version
        );
    }

    /// 真实环境冒烟：检测到 npm 并返回版本；无环境 skip 并打印原因。
    #[test]
    fn real_npm_probe_smoke() {
        let detection = match detect_package_manager(PackageManager::Npm) {
            Ok(d) => d,
            Err(AppError::PackageManagerNotFound(msg)) => {
                eprintln!("N-01: no `npm` on PATH; skipping real probe smoke ({msg})");
                return;
            }
            Err(other) => panic!("unexpected error: {other:?}"),
        };
        assert!(detection.executable.is_file());
        if cfg!(windows) {
            // Windows 上 npm 实体必须是 .cmd shim（find_in_path 候选顺序保证）。
            let name = detection
                .executable
                .file_name()
                .unwrap()
                .to_string_lossy()
                .to_ascii_lowercase();
            assert!(
                name.ends_with(".cmd") || name.ends_with(".exe") || name.ends_with(".bat"),
                "Windows npm must resolve to an executable extension, got {name}"
            );
        }
        if !detection.probe_ok {
            eprintln!(
                "N-01: npm found at {:?} but probe failed (raw={:?}); skipping version assert",
                detection.executable, detection.raw_output
            );
            return;
        }
        assert!(detection.version.is_some());
        eprintln!(
            "N-01 real probe: npm={:?} version={:?}",
            detection.executable, detection.version
        );
    }

    /// 决策链选中但 PATH 不可执行 → PackageManagerNotFound 且附决策来源。
    /// 用 yarn/pnpm 这类大概率未安装的 pm；若本机恰好已装则 skip。
    #[test]
    fn resolve_reports_decision_source_when_missing() {
        for pm in [PackageManager::Pnpm, PackageManager::Yarn] {
            if find_in_path(pm.executable_name()).is_some() {
                eprintln!(
                    "N-01: {} installed on this machine; skipping missing-pm assert",
                    pm.name()
                );
                continue;
            }
            let decision = PackageManagerDecision {
                manager: pm,
                source: crate::node::decision::DecisionSource::Configured,
                reason: format!("配置显式指定 {}", pm.name()),
            };
            let err = resolve_package_manager(&decision).unwrap_err();
            match err {
                AppError::PackageManagerNotFound(msg) => {
                    assert!(msg.contains(pm.name()), "error must name the pm: {msg}");
                    assert!(
                        msg.contains("决策来源"),
                        "error must carry decision source: {msg}"
                    );
                }
                other => panic!("expected PackageManagerNotFound, got {other:?}"),
            }
        }
    }

    /// PATH 兜底语义：node/npm 检测必须走 find_in_path（裸名禁止直接 spawn）。
    #[test]
    fn path_lookup_uses_find_in_path() {
        // 构造一个只在注入目录里存在的「node」——find_executable_in_dirs 能找到，
        // 但 PATH 上没有时 detect_node 必须报 NodeNotFound 而非误命中。
        let tmp = std::env::temp_dir().join(format!(
            "gw_node_iso_{}",
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        std::fs::create_dir_all(&tmp).unwrap();
        let name = if cfg!(windows) { "node.exe" } else { "node" };
        std::fs::write(tmp.join(name), b"").unwrap();
        let found = find_executable_in_dirs("node", std::slice::from_ref(&tmp));
        assert!(found.is_some(), "injected dir must resolve node candidate");
        let _ = std::fs::remove_dir_all(&tmp);
        // PATH 上无 node 时 detect_node 的错误形态（本机有 node 则跳过该断言）。
        if find_in_path("node").is_none() {
            assert!(matches!(detect_node(), Err(AppError::NodeNotFound(_))));
        } else {
            eprintln!("N-01: node present on PATH; skipping NodeNotFound assert");
        }
    }

    /// 探测不存在的可执行：spawn 失败必须降级而非 panic。
    #[test]
    fn probe_missing_executable_degrades() {
        let missing = PathBuf::from(if cfg!(windows) {
            "C:/definitely/not/here/node.exe"
        } else {
            "/definitely/not/here/node"
        });
        let detection = probe_tool(&missing);
        assert!(!detection.probe_ok);
        assert!(
            detection.version.is_none(),
            "probe failure degrades to 未知版本"
        );
        assert!(detection.raw_output.contains("probe error"));
    }
}
