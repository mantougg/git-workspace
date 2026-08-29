//! JDK 数据模型（R-04，源文档 §31 JVM 管理、§32 JDK Manager）。
//!
//! 这些结构是纯数据（serde 序列化），作为 JDK 检测、注册表持久化与 IPC/UI
//! 消费的基础。IPC 类型单一事实来源沿用全局约束 §6：Rust serde 结构为权威，
//! `models/ipc_golden/` 用 golden 快照守卫 TS 类型不漂移。

use serde::{Deserialize, Serialize};

/// JDK 发现来源（§31）。
///
/// `camelCase` 序列化：system / javaHome / path / mise / jenv / sdkman / manual。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum JdkDiscoverySource {
    /// 系统常见安装目录扫描（Program Files / /Library/Java/... / /usr/lib/jvm 等）。
    System,
    /// `JAVA_HOME` 环境变量指向的 JDK。
    JavaHome,
    /// `PATH` 中可执行 `java` 解析出的 JDK。
    Path,
    /// mise（原 rtx）管理的 JDK。
    Mise,
    /// jEnv 管理的 JDK。
    Jenv,
    /// SDKMAN! 管理的 JDK。
    Sdkman,
    /// 用户手动添加（Settings UI 选目录）。
    Manual,
}

impl JdkDiscoverySource {
    /// 持久化到 SQLite 的稳定字符串（与 serde 一致，便于直接存取）。
    pub fn as_str(self) -> &'static str {
        match self {
            JdkDiscoverySource::System => "system",
            JdkDiscoverySource::JavaHome => "javaHome",
            JdkDiscoverySource::Path => "path",
            JdkDiscoverySource::Mise => "mise",
            JdkDiscoverySource::Jenv => "jenv",
            JdkDiscoverySource::Sdkman => "sdkman",
            JdkDiscoverySource::Manual => "manual",
        }
    }

    /// 从持久化字符串还原；未识别值降级为 `Manual`，避免损坏条目阻断列举。
    pub fn parse(s: &str) -> Self {
        match s {
            "system" => JdkDiscoverySource::System,
            "javaHome" => JdkDiscoverySource::JavaHome,
            "path" => JdkDiscoverySource::Path,
            "mise" => JdkDiscoverySource::Mise,
            "jenv" => JdkDiscoverySource::Jenv,
            "sdkman" => JdkDiscoverySource::Sdkman,
            "manual" => JdkDiscoverySource::Manual,
            _ => JdkDiscoverySource::Manual,
        }
    }
}

/// JDK 厂商（§32）。由 `java -version` 输出关键字推断，宽容多 vendor。
///
/// `camelCase` 序列化：oracle / openJdk / temurin / corretto / graalVm / zulu /
/// liberica / other。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum JdkVendor {
    Oracle,
    OpenJdk,
    /// Eclipse Temurin / Adoptium / AdoptOpenJDK。
    Temurin,
    /// Amazon Corretto。
    Corretto,
    /// Oracle GraalVM / GraalVM Community。
    GraalVm,
    /// Azul Zulu。
    Zulu,
    /// BellSoft Liberica。
    Liberica,
    /// 未识别 vendor。
    Other,
}

impl JdkVendor {
    pub fn as_str(self) -> &'static str {
        match self {
            JdkVendor::Oracle => "oracle",
            JdkVendor::OpenJdk => "openJdk",
            JdkVendor::Temurin => "temurin",
            JdkVendor::Corretto => "corretto",
            JdkVendor::GraalVm => "graalVm",
            JdkVendor::Zulu => "zulu",
            JdkVendor::Liberica => "liberica",
            JdkVendor::Other => "other",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "oracle" => JdkVendor::Oracle,
            "openJdk" => JdkVendor::OpenJdk,
            "temurin" => JdkVendor::Temurin,
            "corretto" => JdkVendor::Corretto,
            "graalVm" => JdkVendor::GraalVm,
            "zulu" => JdkVendor::Zulu,
            "liberica" => JdkVendor::Liberica,
            _ => JdkVendor::Other,
        }
    }
}

/// 一个已发现 / 注册的 JDK 安装。
///
/// 既是 DB 行模型也是 IPC 载荷：`id` / `created_at` / `updated_at` 在新建时为
/// `None`，由注册表插入时赋值；版本 / vendor 字段在 `java -version` 探测失败
/// 时保持 `None`，`is_valid=false`。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JdkInstallation {
    /// DB 主键；未持久化时为 `None`。
    #[serde(default)]
    pub id: Option<i64>,
    /// JDK 根目录（`JAVA_HOME` 语义），规范化后存储，唯一键。
    pub home_path: String,
    /// Major version（8 / 11 / 17 / 21 / 25+）。
    #[serde(default)]
    pub major_version: Option<u32>,
    /// 完整版本串（如 `17.0.12` / `1.8.0_422`）。
    #[serde(default)]
    pub full_version: Option<String>,
    /// 厂商推断。
    #[serde(default)]
    pub vendor: Option<JdkVendor>,
    /// 架构（`x86_64` / `aarch64` / `x86`）；探测不到为 `None`。
    #[serde(default)]
    pub architecture: Option<String>,
    /// 位宽（64 / 32）。
    #[serde(default)]
    pub bitness: Option<u32>,
    pub source: JdkDiscoverySource,
    /// `java` 可执行绝对路径。
    #[serde(default)]
    pub java_exec: Option<String>,
    /// `javac` 可执行绝对路径（JDK 而非 JRE 才有）。
    #[serde(default)]
    pub javac_exec: Option<String>,
    /// 路径存在 + 可执行 `java -version` 时为 true。
    #[serde(default)]
    pub is_valid: bool,
    /// 最近一次校验时间（RFC3339）。
    #[serde(default)]
    pub last_checked: String,
    /// `java -version` 原始输出，便于排查 vendor 差异。
    #[serde(default)]
    pub raw_version: Option<String>,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
}

impl JdkInstallation {
    /// 构造一个最小字段的新候选（未持久化、未探测版本）。
    pub fn new(home_path: impl Into<String>, source: JdkDiscoverySource) -> Self {
        Self {
            id: None,
            home_path: home_path.into(),
            major_version: None,
            full_version: None,
            vendor: None,
            architecture: None,
            bitness: None,
            source,
            java_exec: None,
            javac_exec: None,
            is_valid: false,
            last_checked: String::new(),
            raw_version: None,
            created_at: None,
            updated_at: None,
        }
    }
}
