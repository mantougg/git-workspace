//! JDK 多来源发现与 `java -version` 探测（R-04，§31 / §32）。
//!
//! 发现来源（§31）：`JAVA_HOME`、`PATH` 中的 `java`、系统常见安装目录、
//! mise / jEnv / SDKMAN 管理目录。按规范化 `home_path` 去重，每个候选 fork
//! `java -version`（只读探测，非 shell 脚本；全局约束 §3 的自动执行禁令不适用，
//! build/run 进程的确认流留给 R-09/R-10）。探测失败标 `is_valid=false`，不硬错误。
//!
//! 全程本地完成、无网络请求（全局约束 §10）；检测惰性由 R-04 注册表的
//! `prune_invalid_homes` 与显式 `validate_jdk` 承担，这里只负责「找到并探测」。

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::java::model::{JdkDiscoverySource, JdkInstallation};
use crate::java::version::{parse_java_version, JdkVersionInfo};

/// 发现本机全部 JDK：收集候选 home -> 去重 -> fork `java -version` 探测。
///
/// 返回可直接批量 upsert 的 `JdkInstallation` 列表（无 id / 时间戳，由注册表填）。
pub fn discover_jdks() -> Vec<JdkInstallation> {
    let candidates = collect_candidate_homes();
    let mut seen: HashSet<PathBuf> = HashSet::new();
    let mut out = Vec::new();
    for (home, source) in candidates {
        let canon = std::fs::canonicalize(&home).unwrap_or_else(|_| home.clone());
        if !seen.insert(canon.clone()) {
            continue;
        }
        if let Some(inst) = probe_into_installation(&canon, source) {
            out.push(inst);
        }
    }
    out.sort_by(|a, b| a.home_path.cmp(&b.home_path));
    out
}

/// 把一个 home 探测为 `JdkInstallation`：找 `java` 可执行 -> fork `java -version`。
///
/// 找不到 `java` 可执行 -> 视为非 JDK 目录，返回 `None`（不录入注册表）。
/// 找到但探测失败 -> 录入 `is_valid=false` + raw 原文，便于用户排查。
fn probe_into_installation(
    home: &Path,
    source: JdkDiscoverySource,
) -> Option<JdkInstallation> {
    let java_exec = java_exec_for_home(home)?;
    let javac_exec = javac_exec_for_home(home);
    let (info, is_valid) = probe_java_version(&java_exec);
    let mut jdk = JdkInstallation::new(home.to_string_lossy().to_string(), source);
    jdk.major_version = info.major_version;
    jdk.full_version = info.full_version;
    jdk.vendor = info.vendor;
    jdk.architecture = info.architecture;
    jdk.bitness = info.bitness;
    jdk.java_exec = Some(java_exec.to_string_lossy().to_string());
    jdk.javac_exec = javac_exec.map(|p| p.to_string_lossy().to_string());
    jdk.is_valid = is_valid;
    jdk.last_checked = chrono::Utc::now().to_rfc3339();
    jdk.raw_version = if info.raw.is_empty() {
        None
    } else {
        Some(info.raw)
    };
    Some(jdk)
}

/// Fork `java -version` 并解析。返回 `(info, is_valid)`。
///
/// `java -version` 的版本信息打到 stderr（Java 8 及 9+ 一致）；`--version`（9+）
/// 打到 stdout。这里合并 stderr + stdout 后解析。`java -version` 正常退出码为 0；
/// 即便非零也尝试解析输出，解析不到版本串即 `is_valid=false`。
fn probe_java_version(java_exec: &Path) -> (JdkVersionInfo, bool) {
    let output = Command::new(java_exec).arg("-version").output();
    let combined = match output {
        Ok(o) => {
            let mut s = String::new();
            s.push_str(&String::from_utf8_lossy(&o.stderr));
            s.push('\n');
            s.push_str(&String::from_utf8_lossy(&o.stdout));
            s
        }
        Err(err) => {
            log::warn!("JDK probe {:?} failed: {}", java_exec, err);
            let mut info = JdkVersionInfo::default();
            info.raw = format!("probe error: {err}");
            return (info, false);
        }
    };
    let info = parse_java_version(&combined);
    let valid = info.major_version.is_some();
    (info, valid)
}

/// 收集所有来源的候选 (home, source)，不去重（去重在 `discover_jdks` 做）。
fn collect_candidate_homes() -> Vec<(PathBuf, JdkDiscoverySource)> {
    let mut out = Vec::new();
    if let Some(home) = env_home("JAVA_HOME") {
        out.push((home, JdkDiscoverySource::JavaHome));
    }
    if let Some(home) = path_java_home() {
        out.push((home, JdkDiscoverySource::Path));
    }
    for home in system_install_dirs() {
        out.push((home, JdkDiscoverySource::System));
    }
    for home in mise_homes() {
        out.push((home, JdkDiscoverySource::Mise));
    }
    for home in jenv_homes() {
        out.push((home, JdkDiscoverySource::Jenv));
    }
    for home in sdkman_homes() {
        out.push((home, JdkDiscoverySource::Sdkman));
    }
    out
}

fn env_home(var: &str) -> Option<PathBuf> {
    let raw = std::env::var_os(var)?;
    let home = PathBuf::from(raw);
    if home.is_dir() {
        Some(home)
    } else {
        None
    }
}

/// 从 `PATH` 中找 `java` 可执行，反推 JDK home（`java` 所在 `bin` 的父目录）。
fn path_java_home() -> Option<PathBuf> {
    let exe = find_in_path("java")?;
    // exe 应位于 <home>/bin/java(.exe)。
    let bin = exe.parent()?;
    let home = bin.parent()?;
    if java_exec_for_home(home).is_some() {
        Some(home.to_path_buf())
    } else {
        None
    }
}

/// 在 `PATH` 各目录中查找可执行（Windows 自动加 `.exe`）。
pub(crate) fn find_in_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    let sep = if cfg!(windows) { ';' } else { ':' };
    for dir in path.to_string_lossy().split(sep) {
        if dir.is_empty() {
            continue;
        }
        if cfg!(windows) {
            // PATHEXT 语义：目录内优先可执行扩展名。mise 等工具会把 Unix
            // shell 脚本（无扩展名，Windows 不可执行，error 193）与
            // `mvn.cmd` 放在同一 bin 目录——必须先命中 `.cmd`（R-14 修复）。
            for extension in [".exe", ".cmd", ".bat"] {
                let exe = Path::new(dir).join(format!("{name}{extension}"));
                if exe.is_file() {
                    return Some(exe);
                }
            }
        }
        let candidate = Path::new(dir).join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// JDK home 下的 `java` 可执行（`<home>/bin/java` 或 Windows `java.exe`）。
pub(crate) fn java_exec_for_home(home: &Path) -> Option<PathBuf> {
    let bin = home.join("bin");
    if cfg!(windows) {
        let exe = bin.join("java.exe");
        if exe.is_file() {
            return Some(exe);
        }
    }
    let java = bin.join("java");
    if java.is_file() {
        return Some(java);
    }
    None
}

/// JDK home 下的 `javac`（仅 JDK 有，JRE 无）。
pub(crate) fn javac_exec_for_home(home: &Path) -> Option<PathBuf> {
    let bin = home.join("bin");
    if cfg!(windows) {
        let exe = bin.join("javac.exe");
        if exe.is_file() {
            return Some(exe);
        }
    }
    let javac = bin.join("javac");
    if javac.is_file() {
        return Some(javac);
    }
    None
}

/// 在一组父目录下扫描 JDK home（子目录含 `bin/java` 或 `bin/java.exe`）。
/// 供系统目录与版本管理器目录复用。
fn scan_dirs_for_jdks(parents: &[PathBuf]) -> Vec<PathBuf> {
    let mut homes = Vec::new();
    for parent in parents {
        if !parent.is_dir() {
            continue;
        }
        let entries = match std::fs::read_dir(parent) {
            Ok(e) => e,
            Err(err) => {
                log::debug!("JDK scan skipped {:?}: {}", parent, err);
                continue;
            }
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() && java_exec_for_home(&path).is_some() {
                homes.push(path);
            }
        }
    }
    homes
}

/// 平台常见 JDK 安装目录扫描。
fn system_install_dirs() -> Vec<PathBuf> {
    let mut parents: Vec<PathBuf> = Vec::new();
    if cfg!(target_os = "windows") {
        for base in ["C:\\Program Files\\Java", "C:\\Program Files\\Eclipse Adoptium", "C:\\Program Files\\Microsoft"] {
            parents.push(PathBuf::from(base));
        }
        // 用户级 %LOCALAPPDATA%\Programs\Eclipse Adoptium 等。
        if let Some(local) = dirs::data_local_dir() {
            parents.push(local.join("Programs").join("Eclipse Adoptium"));
        }
    } else if cfg!(target_os = "macos") {
        parents.push(PathBuf::from("/Library/Java/JavaVirtualMachines"));
        if let Some(home) = dirs::home_dir() {
            parents.push(home.join("Library").join("Java").join("JavaVirtualMachines"));
        }
        // Homebrew formula 符号链接目录。
        parents.push(PathBuf::from("/opt/homebrew/opt"));
        parents.push(PathBuf::from("/usr/local/opt"));
    } else {
        // Linux / 其他 Unix。
        parents.push(PathBuf::from("/usr/lib/jvm"));
        parents.push(PathBuf::from("/usr/java"));
        parents.push(PathBuf::from("/opt/java"));
    }

    let mut homes = scan_dirs_for_jdks(&parents);
    // macOS JDK home 是 `*.jdk/Contents/Home`，需再下钻一层。
    if cfg!(target_os = "macos") {
        let mut mac_homes = Vec::new();
        for h in &homes {
            let candidate = h.join("Contents").join("Home");
            if java_exec_for_home(&candidate).is_some() {
                mac_homes.push(candidate);
            } else if java_exec_for_home(h).is_some() {
                mac_homes.push(h.clone());
            }
        }
        if !mac_homes.is_empty() {
            homes = mac_homes;
        }
    }
    homes
}

fn mise_homes() -> Vec<PathBuf> {
    let mut parents = Vec::new();
    if let Some(dir) = std::env::var_os("MISE_DATA_DIR").map(PathBuf::from) {
        parents.push(dir.join("installs").join("java"));
    }
    // F-03：mise 默认数据目录——Windows 是 %LOCALAPPDATA%\mise（原来漏了，
    // 用户实测 temurin 全系漏检）；Unix 是 $XDG_DATA_HOME/mise 或
    // ~/.local/share/mise。dirs::data_local_dir 在 Linux 上就是
    // ~/.local/share（与下面条目重复无妨，discover 阶段会去重）。
    if let Some(dir) = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from) {
        parents.push(dir.join("mise").join("installs").join("java"));
    }
    if let Some(local) = dirs::data_local_dir() {
        parents.push(local.join("mise").join("installs").join("java"));
    }
    if let Some(home) = dirs::home_dir() {
        parents.push(home.join(".mise").join("installs").join("java"));
        parents.push(home.join(".local").join("share").join("mise").join("installs").join("java"));
        // 旧名 asdf。
        parents.push(home.join(".asdf").join("installs").join("java"));
    }
    scan_dirs_for_jdks(&parents)
}

fn jenv_homes() -> Vec<PathBuf> {
    let mut parents = Vec::new();
    if let Some(home) = dirs::home_dir() {
        parents.push(home.join(".jenv").join("versions"));
    }
    // jEnv 符号链接指向真实 home；canonicalize 在 discover_jdks 统一处理。
    scan_dirs_for_jdks(&parents)
}

fn sdkman_homes() -> Vec<PathBuf> {
    let mut parents = Vec::new();
    if let Some(dir) = std::env::var_os("SDKMAN_DIR").map(PathBuf::from) {
        parents.push(dir.join("candidates").join("java"));
    }
    if let Some(home) = dirs::home_dir() {
        parents.push(home.join(".sdkman").join("candidates").join("java"));
    }
    scan_dirs_for_jdks(&parents)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::java::model::JdkDiscoverySource;
    use std::fs;

    fn stamp(path: &Path) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        // 空文件即可（探测逻辑只检存在，不执行）。
        fs::write(path, b"").unwrap();
    }

    #[test]
    fn finds_java_exec_in_home_bin() {
        let tmp = std::env::temp_dir().join(format!(
            "gw_jdk_home_{}",
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        let exe = if cfg!(windows) {
            tmp.join("bin").join("java.exe")
        } else {
            tmp.join("bin").join("java")
        };
        stamp(&exe);
        assert_eq!(
            java_exec_for_home(&tmp).map(|p| p.file_name().unwrap().to_string_lossy().to_string()),
            Some(exe.file_name().unwrap().to_string_lossy().to_string())
        );
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn javac_absent_for_jre_layout() {
        let tmp = std::env::temp_dir().join(format!(
            "gw_jdk_jre_{}",
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        stamp(&tmp.join(if cfg!(windows) { "bin/java.exe" } else { "bin/java" }));
        assert!(javac_exec_for_home(&tmp).is_none(), "JRE layout has no javac");
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn scan_dirs_finds_jdk_subdirs() {
        let parent = std::env::temp_dir().join(format!(
            "gw_jvm_{}",
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        // 两个 JDK home + 一个非 JDK 目录。
        stamp(&parent.join("jdk17/bin/java"));
        stamp(&parent.join("jdk21/bin/java"));
        fs::create_dir_all(parent.join("not-a-jdk")).unwrap();
        let homes = scan_dirs_for_jdks(&[parent.clone()]);
        assert_eq!(homes.len(), 2, "only dirs with bin/java are JDK homes");
        assert!(homes.iter().any(|h| h.ends_with("jdk17")));
        assert!(homes.iter().any(|h| h.ends_with("jdk21")));
        let _ = fs::remove_dir_all(&parent);
    }

    #[test]
    fn discover_dedupes_and_skips_non_jdk() {
        // 构造一个真实布局的 JDK home（java 存在但不可执行 -> 探测失败 ->
        // is_valid=false，但仍录入以便用户重检）。
        let home = std::env::temp_dir().join(format!(
            "gw_jdk_disc_{}",
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        stamp(&home.join(if cfg!(windows) { "bin/java.exe" } else { "bin/java" }));
        // 直接调用 probe_into_installation 验证：有 java 可执行 -> 录入；
        // fork 会失败（空文件非可执行 ELF/PE）-> is_valid=false。
        let inst = probe_into_installation(&home, JdkDiscoverySource::System);
        assert!(inst.is_some(), "a home with bin/java is recorded");
        let inst = inst.unwrap();
        assert_eq!(inst.source, JdkDiscoverySource::System);
        assert!(inst.java_exec.is_some());
        // 空文件 fork 不会产出版本 -> is_valid=false，但不阻断录入。
        assert!(!inst.is_valid, "non-executable java probe is invalid but recorded");

        let _ = fs::remove_dir_all(&home);
    }

    /// F-03：mise 默认数据目录覆盖——Windows 上是 %LOCALAPPDATA%\mise
    /// （F-03 修复前漏掉，导致 mise 装的 JDK 全漏检）。探测不到 mise
    /// 安装目录就 skip 并打印原因（AGENTS.md 平台规范 §4）。
    #[test]
    fn mise_data_local_dir_is_scanned() {
        let local = match dirs::data_local_dir() {
            Some(dir) => dir,
            None => {
                eprintln!("F-03: no data_local_dir on this platform; skipping");
                return;
            }
        };
        let parent = local.join("mise").join("installs").join("java");
        if !parent.is_dir() {
            eprintln!("F-03: {parent:?} not present (mise 未安装或未装 JDK); skipping");
            return;
        }
        let homes = mise_homes();
        assert!(
            !homes.is_empty(),
            "mise java installs exist at {parent:?} but none were discovered"
        );
        // 至少一个发现的 home 来自该目录（ mise 的别名目录如 temurin-17
        // 与真实版本目录并列，均被接受）。
        assert!(
            homes.iter().any(|h| h.starts_with(&parent)),
            "expected a discovered home under {parent:?}, got {homes:?}"
        );
        eprintln!("F-03 mise homes: {homes:?}");
    }

    /// 真实 `java -version` 集成：机器无 java 时跳过并标注（仿 R-03 mvn 集成约定）。
    #[test]
    fn real_java_version_probe_roundtrip() {
        // PATH 兜底 JAVA_HOME，再无则跳过。
        let java = find_in_path("java")
            .or_else(|| env_home("JAVA_HOME").and_then(|h| java_exec_for_home(&h)));
        let java = match java {
            Some(p) => p,
            None => {
                eprintln!("R-04: no `java` on PATH / JAVA_HOME; skipping real probe integration test");
                return;
            }
        };
        let (info, valid) = probe_java_version(&java);
        assert!(valid, "real java -version must parse a major version");
        assert!(info.major_version.is_some());
        assert!(!info.raw.is_empty());
        eprintln!(
            "R-04 real probe: major={:?} vendor={:?} full={:?}",
            info.major_version, info.vendor, info.full_version
        );
    }
}
