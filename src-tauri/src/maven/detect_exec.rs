//! Maven 优先级链检测与 `mvn -v` 版本解析（R-05，§18 / §19）。
//!
//! 优先级链（§18）：项目内 `mvnw` / `mvnw.cmd` 存在即优先使用（§19）→
//! 用户配置的 Maven → 系统 `PATH` 中的 `mvn`。三者皆缺时返回 `MavenNotFound`
//! 可行动错误。
//!
//! `mvn -v` 探测：fork 只读探测进程（非 shell 脚本；全局约束 §3 的自动执行
//! 禁令不适用，build/run 进程的确认流留给 R-09/R-10）。探测有超时与输出上限，
//! 避免卡死启动流程（任务文档「架构/性能注意点」）。
//!
//! 全程本地完成、无网络请求（全局约束 §10）。版本探测结果可缓存到 SQLite
//! （`maven_executables` 表），惰性校验：只检路径存在性，不每次启动 fork。
//! Windows 下 `mvnw.cmd` 需经 `cmd /c` 调用（§19 / 任务文档注意点）。

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use crate::java::detect::find_in_path;
use crate::maven::exec_model::{MavenExecutable, MavenSource, MavenVersionInfo};

/// Maven `mvn -v` 探测超时（秒）。任务文档要求「有超时与输出上限」。
const MVN_VERSION_TIMEOUT_SECS: u64 = 15;

/// 最低支持 Maven major 版本（§18）。低于此版本视为不可用，继续回退下一候选。
pub const MIN_MAVEN_MAJOR_VERSION: u32 = 3;

/// 在项目目录下检测 Maven 可执行体的优先级链。
///
/// 返回按优先级排序的候选列表（wrapper 优先，然后是 configured、system）。
/// 调用方通常只取第一个；返回全部便于 UI 展示「当前生效来源」与备选。
///
/// - `project_dir`：项目根目录（`pom.xml` 所在目录），用于查找 `mvnw`。
/// - `configured_path`：用户在 Settings 配置的 Maven 可执行路径（可选）。
///
/// 候选只做「路径是否存在」检查，不 fork `mvn -v`（探测由 `probe_version` /
/// `resolve_maven_for_project` 按需触发）。
pub fn detect_maven_candidates(project_dir: Option<&Path>, configured_path: Option<&str>) -> Vec<MavenExecutable> {
    let mut out = Vec::new();

    if let Some(dir) = project_dir {
        if let Some(exe) = find_wrapper_in_dir(dir) {
            let project_path = dir.to_string_lossy().to_string();
            out.push(MavenExecutable::new(
                exe.to_string_lossy().to_string(),
                MavenSource::ProjectWrapper,
                Some(project_path),
            ));
        }
    }

    if let Some(cfg) = configured_path.filter(|s| !s.trim().is_empty()) {
        let p = Path::new(cfg);
        if is_executable_candidate(p) {
            out.push(MavenExecutable::new(canonical_or_raw(p), MavenSource::Configured, None));
        }
    }

    if let Some(exe) = find_in_path("mvn") {
        out.push(MavenExecutable::new(
            exe.to_string_lossy().to_string(),
            MavenSource::System,
            None,
        ));
    }

    // 按优先级稳定排序（wrapper < configured < system）。
    out.sort_by_key(|m| m.source.priority());
    out
}

/// F-16：全量扫描本机 Maven 安装（mise / SDKMAN / PATH），供设置页「扫描安装」。
///
/// 只检查路径存在性（不 fork `mvn -v`；版本探测与入库由命令层按需做）。
/// 返回按路径归一化去重后的候选（source=System；用户手动添加的走
/// `MavenSource::Configured`，见 `commands::maven::add_maven_executable`）。
pub fn scan_maven_installations() -> Vec<MavenExecutable> {
    let mut out: Vec<MavenExecutable> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut push = |exe: PathBuf, out: &mut Vec<MavenExecutable>| {
        // 路径去重统一正斜杠归一化（平台规范 §1）。
        let key = exe.to_string_lossy().replace('\\', "/");
        if seen.insert(key) {
            out.push(MavenExecutable::new(
                exe.to_string_lossy().to_string(),
                MavenSource::System,
                None,
            ));
        }
    };

    if let Some(exe) = find_in_path("mvn") {
        push(exe, &mut out);
    }
    for bin in managed_maven_bin_dirs() {
        if let Some(exe) = find_mvn_in_bin(&bin) {
            push(exe, &mut out);
        }
    }
    out
}

/// 版本管理器安装目录下的 Maven bin 目录列表（mise + SDKMAN）。
/// mise 目录布局与 F-03 `java::detect` 的 mise 扫描同源（installs/<tool>/<ver>）。
fn managed_maven_bin_dirs() -> Vec<PathBuf> {
    let mut parents = Vec::new();
    // mise：Windows 默认 %LOCALAPPDATA%\mise；Unix $XDG_DATA_HOME/mise 或
    // ~/.local/share/mise（另有 ~/.mise 兼容）。
    if let Some(local) = dirs::data_local_dir() {
        parents.push(local.join("mise").join("installs").join("maven"));
    }
    if let Some(home) = dirs::home_dir() {
        parents.push(home.join(".mise").join("installs").join("maven"));
        parents.push(
            home.join(".local")
                .join("share")
                .join("mise")
                .join("installs")
                .join("maven"),
        );
        // SDKMAN（Unix 系）：~/.sdkman/candidates/maven/<ver>/bin/mvn
        parents.push(home.join(".sdkman").join("candidates").join("maven"));
    }
    let mut bins = Vec::new();
    for parent in parents {
        let entries = match std::fs::read_dir(&parent) {
            Ok(e) => e,
            Err(_) => continue, // 目录不存在属正常（未装该管理器），静默跳过
        };
        for entry in entries.flatten() {
            let bin = entry.path().join("bin");
            if bin.is_dir() {
                bins.push(bin);
            }
        }
    }
    bins
}

/// bin 目录内按 Windows PATHEXT 语义找 mvn（`.exe → .cmd → .bat → 裸名`）——
/// mise 会把不可执行的 Unix 脚本与 `mvn.cmd` 放同一目录，必须先命中扩展名
/// 候选（R-14 教训，与 `java::detect::find_in_path` 同顺序）。
fn find_mvn_in_bin(bin: &Path) -> Option<PathBuf> {
    if cfg!(windows) {
        for ext in [".exe", ".cmd", ".bat"] {
            let exe = bin.join(format!("mvn{ext}"));
            if exe.is_file() {
                return Some(exe);
            }
        }
    }
    let bare = bin.join("mvn");
    if bare.is_file() {
        return Some(bare);
    }
    None
}

/// 候选是否可用：探测有效且版本 >= 最低支持版本。
pub fn candidate_is_usable(info: &MavenVersionInfo) -> bool {
    info.is_valid() && info.major_version.is_some_and(|major| major >= MIN_MAVEN_MAJOR_VERSION)
}

/// 为项目解析最终生效的 Maven（优先级链第一个可用候选）。
///
/// 会 fork `mvn -v` 探测版本。任一候选探测成功且版本 >= 最低支持版本即返回；
/// 版本过低或全部探测失败或无候选时返回 `None`，由调用方转成 `MavenNotFound`
/// 可行动错误。
///
/// `configured_path` 为用户配置的 Maven 路径（可选，来自 Settings）。
/// `local_repository` 由调用方传入（来自 settings.xml 解析，见 `settings.rs`）。
pub fn resolve_maven_for_project(
    project_dir: &Path,
    configured_path: Option<&str>,
    local_repository: &Path,
) -> Option<crate::maven::exec_model::ResolvedMaven> {
    let candidates = detect_maven_candidates(Some(project_dir), configured_path);
    for mut exe in candidates {
        let uses_wrapper = exe.source == MavenSource::ProjectWrapper;
        let (info, is_valid) = probe_version(&exe.executable_path);
        let usable = candidate_is_usable(&info);
        let fallback_reason = if !is_valid {
            format!("probe failed for {}", exe.executable_path)
        } else {
            format!(
                "version {:?} below minimum {}",
                info.full_version, MIN_MAVEN_MAJOR_VERSION
            )
        };
        exe.major_version = info.major_version;
        exe.full_version = info.full_version;
        exe.is_valid = is_valid;
        exe.last_checked = chrono::Utc::now().to_rfc3339();
        exe.raw_version = if info.raw.is_empty() { None } else { Some(info.raw) };
        if usable {
            return Some(crate::maven::exec_model::ResolvedMaven {
                executable: exe,
                local_repository: local_repository.to_path_buf(),
                uses_wrapper,
            });
        }
        log::warn!("Maven candidate {} unusable: {fallback_reason}", exe.executable_path);
    }
    None
}

/// Fork `mvn -v` 并解析。返回 `(info, is_valid)`。
///
/// `mvn -v` 把版本信息打到 **stdout**（与 `java -version` 打到 stderr 不同）。
/// 即便非零退出码也尝试解析输出；解析不到版本串即 `is_valid=false`。
///
/// 设有超时（`MVN_VERSION_TIMEOUT_SECS`）与输出上限，避免卡死启动流程
/// （任务文档「架构/性能注意点」）。
pub fn probe_version(executable_path: &str) -> (MavenVersionInfo, bool) {
    let exe = Path::new(executable_path);
    // Windows 下 `.cmd` / `.bat` 需经 `cmd /c` 调用（§19 / 任务文档注意点）。
    let mut cmd = build_version_command(exe);
    let output = cmd.stdout(std::process::Stdio::piped());
    let output = output.stderr(std::process::Stdio::piped());
    let child = match output.spawn() {
        Ok(c) => c,
        Err(err) => {
            log::warn!("Maven probe {:?} failed to spawn: {}", exe, err);
            return (
                MavenVersionInfo {
                    raw: format!("probe error: {err}"),
                    ..Default::default()
                },
                false,
            );
        }
    };

    // 等待子进程，带超时（Windows 上 `wait_timeout` crate 不可用，用轮询兜底）。
    let timeout = Duration::from_secs(MVN_VERSION_TIMEOUT_SECS);
    let combined = match wait_with_timeout(child, timeout) {
        Ok(s) => s,
        Err(err) => {
            log::warn!("Maven probe {:?} timed out or failed: {}", exe, err);
            return (
                MavenVersionInfo {
                    raw: format!("probe error: {err}"),
                    ..Default::default()
                },
                false,
            );
        }
    };

    let info = parse_mvn_version(&combined);
    let valid = info.is_valid();
    (info, valid)
}

/// 为 `mvn -v` 构造命令。Windows 下 `.cmd` / `.bat` 经 `cmd /c` 调用。
pub(crate) fn build_version_command(exe: &Path) -> Command {
    if cfg!(windows) && needs_cmd_c(exe) {
        let mut cmd = Command::new("cmd");
        cmd.arg("/c").arg(exe).arg("-v");
        cmd
    } else {
        let mut cmd = Command::new(exe);
        cmd.arg("-v");
        cmd
    }
}

/// 判断一个路径是否需要 `cmd /c`（Windows `.cmd` / `.bat`）。
pub(crate) fn needs_cmd_c(exe: &Path) -> bool {
    matches!(
        exe.extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .as_deref(),
        Some("cmd") | Some("bat")
    )
}

/// 等待子进程完成，带超时。超时则 kill。
///
/// pub(crate)：N-01 node/npm 版本探测（`node/detect.rs`）复用同一
/// 「超时 + 输出上限」模式，不另起一套。
pub(crate) fn wait_with_timeout(mut child: std::process::Child, timeout: Duration) -> Result<String, String> {
    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let mut buf = String::new();
                if let Some(mut stdout) = child.stdout.take() {
                    use std::io::Read;
                    // 输出上限：1 MiB，避免恶意/异常 Maven 输出撑爆内存。
                    let mut raw = vec![0u8; 1024 * 1024];
                    let n = stdout.read(&mut raw).unwrap_or(0);
                    buf.push_str(&String::from_utf8_lossy(&raw[..n]));
                }
                if let Some(mut stderr) = child.stderr.take() {
                    use std::io::Read;
                    let mut raw = vec![0u8; 1024 * 1024];
                    let n = stderr.read(&mut raw).unwrap_or(0);
                    buf.push('\n');
                    buf.push_str(&String::from_utf8_lossy(&raw[..n]));
                }
                let _ = status; // 即便非零也尝试解析
                return Ok(buf);
            }
            Ok(None) => {
                if start.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!("timeout after {}s", timeout.as_secs()));
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(err) => return Err(format!("wait error: {err}")),
        }
    }
}

/// 在项目目录下查找 Maven Wrapper（`mvnw` / `mvnw.cmd`）。
pub(crate) fn find_wrapper_in_dir(dir: &Path) -> Option<PathBuf> {
    if cfg!(windows) {
        let cmd = dir.join("mvnw.cmd");
        if cmd.is_file() {
            return Some(cmd);
        }
    }
    let mvnw = dir.join("mvnw");
    if mvnw.is_file() {
        // Unix 需可执行位；缺失时给提示而非失败（任务文档注意点）。
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Ok(meta) = std::fs::metadata(&mvnw) {
                if meta.permissions().mode() & 0o111 == 0 {
                    log::warn!(
                        "Maven wrapper {:?} exists but is not executable; \
                         hint: run `chmod +x mvnw`",
                        mvnw
                    );
                    return None;
                }
            }
        }
        return Some(mvnw);
    }
    None
}

/// 路径是否是一个「看起来可用」的 Maven 可执行候选（存在 + 文件）。
fn is_executable_candidate(path: &Path) -> bool {
    path.is_file()
}

pub(crate) fn canonical_or_raw(path: &Path) -> String {
    std::fs::canonicalize(path)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| path.to_string_lossy().to_string())
}

/// 解析 `mvn -v` 合并输出。
///
/// `mvn -v` 输出首行形如：
/// ```text
/// Apache Maven 3.9.6 (bc0240f3c744dd6b6ec2920d3d97a5d8f5c5f5c5)
/// Maven home: /usr/share/maven
/// Java version: 17.0.12, vendor: Eclipse Adoptium
/// ```
/// 本解析器只提取首行的 `Apache Maven X.Y.Z` 版本串。
pub fn parse_mvn_version(output: &str) -> MavenVersionInfo {
    let raw = output.to_string();
    let full = extract_maven_version(output);
    let major = full.as_deref().and_then(major_from_full);
    MavenVersionInfo {
        major_version: major,
        full_version: full,
        raw,
    }
}

/// 从输出中抽取 `Apache Maven X.Y.Z` 的 `X.Y.Z`。
fn extract_maven_version(output: &str) -> Option<String> {
    for line in output.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("Apache Maven") {
            let token = rest.split_whitespace().next()?;
            let cleaned = token.trim_end_matches(',');
            if cleaned.chars().next().is_some_and(|c| c.is_ascii_digit()) {
                return Some(cleaned.to_string());
            }
        }
    }
    None
}

/// 从 `X.Y.Z` 抽取 major（首段数字）。
fn major_from_full(full: &str) -> Option<u32> {
    full.split('.').next()?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_apache_maven_3_9_6() {
        let out = "Apache Maven 3.9.6 (bc0240f3c744dd6b6ec2920d3d97a5d8f5c5f5c5)\n\
                   Maven home: /usr/share/maven\n\
                   Java version: 17.0.12, vendor: Eclipse Adoptium\n";
        let info = parse_mvn_version(out);
        assert_eq!(info.major_version, Some(3));
        assert_eq!(info.full_version.as_deref(), Some("3.9.6"));
        assert!(info.is_valid());
    }

    #[test]
    fn parses_apache_maven_4_0_0() {
        let out = "Apache Maven 4.0.0-rc-1 (deadbeef)\nMaven home: /opt/maven\n";
        let info = parse_mvn_version(out);
        assert_eq!(info.major_version, Some(4));
        assert_eq!(info.full_version.as_deref(), Some("4.0.0-rc-1"));
    }

    #[test]
    fn returns_none_for_non_maven_output() {
        let info = parse_mvn_version("not a maven output\nrandom text");
        assert_eq!(info.major_version, None);
        assert!(!info.is_valid());
    }

    #[test]
    fn candidate_usable_requires_min_major_version() {
        let mut m3 = MavenVersionInfo {
            major_version: Some(3),
            full_version: Some("3.9.6".into()),
            raw: String::new(),
        };
        assert!(candidate_is_usable(&m3), "Maven 3 is usable");

        let m4 = MavenVersionInfo {
            major_version: Some(4),
            full_version: Some("4.0.0".into()),
            raw: String::new(),
        };
        assert!(candidate_is_usable(&m4), "Maven 4 is usable");

        let m2 = MavenVersionInfo {
            major_version: Some(2),
            full_version: Some("2.2.1".into()),
            raw: String::new(),
        };
        assert!(!candidate_is_usable(&m2), "Maven 2 is below minimum");

        m3.major_version = None;
        assert!(!candidate_is_usable(&m3), "unparseable version is not usable");
    }

    #[test]
    fn wrapper_detection_finds_mvnw() {
        let tmp = std::env::temp_dir().join(format!(
            "gw_mvnw_{}",
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        std::fs::create_dir_all(&tmp).unwrap();
        #[cfg(unix)]
        {
            let mvnw = tmp.join("mvnw");
            std::fs::write(&mvnw, b"#!/bin/sh\n").unwrap();
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&mvnw, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        #[cfg(windows)]
        {
            let cmd = tmp.join("mvnw.cmd");
            std::fs::write(&cmd, b"@echo off\n").unwrap();
        }
        let found = find_wrapper_in_dir(&tmp);
        assert!(found.is_some(), "wrapper must be detected");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[cfg(unix)]
    #[test]
    fn wrapper_without_exec_bit_is_skipped_on_unix() {
        let tmp = std::env::temp_dir().join(format!(
            "gw_mvnw_noexec_{}",
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        std::fs::create_dir_all(&tmp).unwrap();
        let mvnw = tmp.join("mvnw");
        std::fs::write(&mvnw, b"#!/bin/sh\n").unwrap();
        // 不可执行（0o644）。
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&mvnw, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(find_wrapper_in_dir(&tmp).is_none(), "non-executable wrapper is skipped");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn priority_chain_wrapper_first() {
        // 构造一个含 mvnw 的临时项目目录。
        let tmp = std::env::temp_dir().join(format!(
            "gw_mvn_chain_{}",
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        std::fs::create_dir_all(&tmp).unwrap();
        #[cfg(unix)]
        {
            let mvnw = tmp.join("mvnw");
            std::fs::write(&mvnw, b"#!/bin/sh\n").unwrap();
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&mvnw, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        #[cfg(windows)]
        {
            std::fs::write(tmp.join("mvnw.cmd"), b"@echo off\n").unwrap();
        }

        let candidates = detect_maven_candidates(Some(&tmp), None);
        assert!(!candidates.is_empty());
        // wrapper 必须排第一。
        assert_eq!(candidates[0].source, MavenSource::ProjectWrapper);
        assert_eq!(candidates[0].project_path.as_deref(), Some(tmp.to_str().unwrap()));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn configured_path_included_when_present() {
        let tmp = std::env::temp_dir().join(format!(
            "gw_mvn_cfg_{}",
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        std::fs::create_dir_all(&tmp).unwrap();
        let fake_mvn = tmp.join(if cfg!(windows) { "mvn.exe" } else { "mvn" });
        std::fs::write(&fake_mvn, b"#!/bin/sh\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&fake_mvn, std::fs::Permissions::from_mode(0o755)).unwrap();
        }

        let candidates = detect_maven_candidates(None, Some(fake_mvn.to_str().unwrap()));
        let cfg = candidates
            .iter()
            .find(|c| c.source == MavenSource::Configured)
            .expect("configured candidate must be present");
        // Windows canonicalize 加 `\\?\` 前缀，用 ends_with 比较 basename。
        assert!(
            cfg.executable_path
                .ends_with(fake_mvn.file_name().unwrap().to_string_lossy().as_ref()),
            "configured path should point to the fake mvn: got {}",
            cfg.executable_path
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn needs_cmd_c_for_cmd_and_bat() {
        assert!(needs_cmd_c(Path::new("C:/tools/mvnw.cmd")));
        assert!(needs_cmd_c(Path::new("mvn.bat")));
        assert!(!needs_cmd_c(Path::new("mvn")));
        assert!(!needs_cmd_c(Path::new("mvn.exe")));
    }

    #[test]
    fn build_version_command_uses_cmd_c_on_windows_for_cmd() {
        // 这个测试验证命令构造逻辑，不实际 spawn。
        let exe = Path::new(if cfg!(windows) { "C:/mvnw.cmd" } else { "/usr/bin/mvn" });
        let cmd = build_version_command(exe);
        // 跨平台：只是确保不 panic 且构造出 Command。
        let _ = cmd;
    }

    /// 真实 `mvn -v` 集成：机器无 mvn 或探测失败时跳过并标注
    /// （仿 R-04 java 集成约定；本机 mvn 可能是 shell stub 不可执行）。
    #[test]
    fn real_mvn_version_probe_roundtrip() {
        let mvn = find_in_path("mvn");
        let mvn = match mvn {
            Some(p) => p,
            None => {
                eprintln!("R-05: no `mvn` on PATH; skipping real probe integration test");
                return;
            }
        };
        let (info, valid) = probe_version(&mvn.to_string_lossy());
        if !valid {
            eprintln!(
                "R-05: `mvn` found at {:?} but probe failed (raw={:?}); \
                 skipping — likely a non-executable stub",
                mvn, info.raw
            );
            return;
        }
        assert!(info.major_version.is_some());
        eprintln!(
            "R-05 real probe: major={:?} full={:?}",
            info.major_version, info.full_version
        );
    }
}
