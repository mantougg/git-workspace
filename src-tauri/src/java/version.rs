//! `java -version` 输出解析器（R-04，§32）。
//!
//! `java -version` 把版本信息打到 **stderr**（Java 8 及以前；Java 9+ 同样走
//! stderr，而 `--version` 走 stdout）。调用方应合并 stderr 为主、stdout 为
//! 回退后传入。本解析器尽力提取 major version / full version / vendor /
//! 架构 / 位宽，并保留 raw 原文以便排查。解析为纯函数、宽容多 vendor，覆盖
//! Oracle / OpenJDK / Temurin / Corretto / GraalVM / Zulu / Liberica 的常见
//! 输出形态；复杂或无法识别的字段保持 `None`，不阻断检测。
//!
//! 解析全程本地完成、无网络请求（全局约束 §10）。

use crate::java::model::JdkVendor;

/// `java -version` 解析结果（纯数据）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct JdkVersionInfo {
    pub major_version: Option<u32>,
    pub full_version: Option<String>,
    pub vendor: Option<JdkVendor>,
    pub architecture: Option<String>,
    pub bitness: Option<u32>,
    pub raw: String,
}

/// 解析 `java -version` 合并输出。
///
/// 不假设输入来自哪个 vendor；找不到版本串时返回全 `None`（`raw` 仍保留）。
pub fn parse_java_version(output: &str) -> JdkVersionInfo {
    let raw = output.to_string();
    let full = extract_version_string(output);
    let major = full.as_deref().and_then(major_from_full);
    let vendor = infer_vendor(output);
    let (arch, bitness) = infer_arch_bitness(output);

    JdkVersionInfo {
        major_version: major,
        full_version: full,
        vendor,
        architecture: arch,
        bitness,
        raw,
    }
}

/// 抽取 `version "X.Y.Z[_w]"` 中的 `X.Y.Z[_w]`。
///
/// 兼容三种常见形态：
/// - `openjdk version "17.0.12" 2024-07-16`（Java 9+）
/// - `java version "1.8.0_422"`（Java 8 及以前）
/// - `openjdk version "21" 2023-09-19`（裸 major）
fn extract_version_string(output: &str) -> Option<String> {
    let key = "version \"";
    for line in output.lines() {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix(key).or_else(|| {
            // `java` / `openjdk` / `Picked up JAVA_TOOL_OPTIONS` 等噪声行之外，
            // 偶见 `Runtime Environment version "..."` 形态；按 `version "` 定位即可。
            trimmed.find(key).map(|i| &trimmed[i + key.len()..])
        }) {
            if let Some(end) = rest.find('"') {
                let v = &rest[..end];
                if !v.is_empty() {
                    return Some(v.to_string());
                }
            }
        }
    }
    None
}

/// 由完整版本串推 major version。
///
/// - `1.8.0_422` / `1.8.0` -> 8（Java 8 及以前用 `1.X` 编码）。
/// - `17.0.12` / `21` / `25` -> 取第一个点号前的数字。
fn major_from_full(full: &str) -> Option<u32> {
    let head = full.split('.').next().filter(|s| !s.is_empty())?;
    // 处理 `1.8` 形态：取第二段为 major。
    if head == "1" {
        let second = full.split('.').nth(1)?;
        return second.split('_').next().and_then(|s| s.parse().ok());
    }
    head.parse().ok()
}

/// 由输出关键字推断 vendor。顺序敏感：发行版关键字优先于通用 `OpenJDK`。
fn infer_vendor(output: &str) -> Option<JdkVendor> {
    let lower = output.to_ascii_lowercase();
    if lower.contains("temurin") || lower.contains("adoptium") || lower.contains("adoptopenjdk") {
        return Some(JdkVendor::Temurin);
    }
    if lower.contains("corretto") {
        return Some(JdkVendor::Corretto);
    }
    if lower.contains("graalvm") || lower.contains("graal vm") {
        return Some(JdkVendor::GraalVm);
    }
    if lower.contains("zulu") {
        return Some(JdkVendor::Zulu);
    }
    if lower.contains("liberica") {
        return Some(JdkVendor::Liberica);
    }
    if lower.contains("java(tm)") || lower.contains("java(tm) se") || lower.contains("oracle") {
        return Some(JdkVendor::Oracle);
    }
    if lower.contains("openjdk") {
        return Some(JdkVendor::OpenJdk);
    }
    None
}

/// 推断架构与位宽。`java -version` 不总是直接给出 arch，尽力而为。
///
/// 命中 `aarch64`/`arm64` -> aarch64/64；`amd64`/`x86_64` -> x86_64/64；
/// `i386`/`x86` 且非 64 -> 32。`64-Bit`/`64 bit` 只能定 64 位，arch 留空。
fn infer_arch_bitness(output: &str) -> (Option<String>, Option<u32>) {
    let lower = output.to_ascii_lowercase();
    if lower.contains("aarch64") || lower.contains("arm64") {
        return (Some("aarch64".into()), Some(64));
    }
    if lower.contains("amd64") || lower.contains("x86_64") || lower.contains("x86-64") {
        return (Some("x86_64".into()), Some(64));
    }
    if lower.contains("64-bit") || lower.contains("64 bit") || lower.contains("64bit") {
        return (None, Some(64));
    }
    if lower.contains("i386") || lower.contains("x86") {
        return (Some("x86".into()), Some(32));
    }
    (None, None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::java::model::JdkVendor;

    #[test]
    fn parses_oracle_java_8() {
        // Java 8 `java -version` 输出形态。
        let out = "java version \"1.8.0_422\"\nJava(TM) SE Runtime Environment (build 1.8.0_422-b05)\nJava HotSpot(TM) 64-Bit Server VM (build 25.422-b05, mixed mode)";
        let info = parse_java_version(out);
        assert_eq!(info.full_version.as_deref(), Some("1.8.0_422"));
        assert_eq!(info.major_version, Some(8));
        assert_eq!(info.vendor, Some(JdkVendor::Oracle));
        assert_eq!(info.bitness, Some(64));
        assert!(info.raw.contains("Java(TM)"));
    }

    #[test]
    fn parses_temurin_17() {
        let out = "openjdk version \"17.0.12\" 2024-07-16\nOpenJDK Runtime Environment Temurin-17.0.12+7 (build 17.0.12+7)\nOpenJDK 64-Bit Server VM Temurin-17.0.12+7 (build 17.0.12+7, mixed mode, sharing)";
        let info = parse_java_version(out);
        assert_eq!(info.full_version.as_deref(), Some("17.0.12"));
        assert_eq!(info.major_version, Some(17));
        assert_eq!(info.vendor, Some(JdkVendor::Temurin));
        assert_eq!(info.bitness, Some(64));
    }

    #[test]
    fn parses_graalvm_21() {
        let out = "openjdk version \"21.0.2\" 2023-10-17\nOpenJDK Runtime Environment GraalVM 21.0.2+13.1 (build 21.0.2+13.1)\nOpenJDK 64-Bit Server VM GraalVM (build 21.0.2+13.1, mixed mode, sharing)";
        let info = parse_java_version(out);
        assert_eq!(info.major_version, Some(21));
        // GraalVM 关键字优先于 OpenJDK。
        assert_eq!(info.vendor, Some(JdkVendor::GraalVm));
    }

    #[test]
    fn parses_bare_major() {
        let out = "openjdk version \"21\" 2023-09-19\nOpenJDK Runtime Environment (build 21)";
        let info = parse_java_version(out);
        assert_eq!(info.full_version.as_deref(), Some("21"));
        assert_eq!(info.major_version, Some(21));
        assert_eq!(info.vendor, Some(JdkVendor::OpenJdk));
    }

    #[test]
    fn parses_corretto_and_zulu() {
        let corretto = "openjdk version \"17.0.8.8.1\" 2023-08-22\nOpenJDK Runtime Environment Corretto-17.0.8.8.1 (build 17.0.8.8.1)";
        assert_eq!(parse_java_version(corretto).vendor, Some(JdkVendor::Corretto));
        let zulu = "openjdk version \"17.0.10\" 2024-01-16\nOpenJDK Runtime Environment Zulu17.50+19 (build 17.0.10+7)";
        assert_eq!(parse_java_version(zulu).vendor, Some(JdkVendor::Zulu));
    }

    #[test]
    fn parses_aarch64_arch() {
        let out = "openjdk version \"17.0.12\" 2024-07-16\nOpenJDK 64-Bit Server VM (build 17.0.12+7, mixed mode), OS_ARCH=aarch64";
        let info = parse_java_version(out);
        assert_eq!(info.architecture.as_deref(), Some("aarch64"));
        assert_eq!(info.bitness, Some(64));
    }

    #[test]
    fn broken_output_yields_none_without_panicking() {
        let info = parse_java_version("not a version output at all\nrandom text");
        assert!(info.full_version.is_none());
        assert!(info.major_version.is_none());
        assert!(info.vendor.is_none());
        assert!(!info.raw.is_empty());
    }
}
