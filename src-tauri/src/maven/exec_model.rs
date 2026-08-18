//! Maven 执行策略数据模型（R-05，源文档 §18 Maven 执行策略、§19 Maven Wrapper）。
//!
//! 这些结构是纯数据（serde 序列化），作为 Maven 检测、版本探测缓存与 IPC/UI
//! 消费的基础。IPC 类型单一事实来源沿用全局约束 §6：Rust serde 结构为权威，
//! `models/ipc_golden_tests.rs` 用 golden 快照守卫 TS 类型不漂移。
//!
//! 本模块只描述「用哪个 Maven 可执行体 + 它的版本信息」，不描述 POM 内容
//! （POM 模型在 `model.rs`）。

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Maven 可执行体来源（§18 优先级链）。
///
/// `camelCase` 序列化：projectWrapper / configured / system。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MavenSource {
    /// 项目内 Maven Wrapper（`mvnw` / `mvnw.cmd`）。
    ProjectWrapper,
    /// 用户配置的 Maven（Settings UI 指定路径）。
    Configured,
    /// 系统 `PATH` 中的 `mvn`。
    System,
}

impl MavenSource {
    /// 持久化到 SQLite 的稳定字符串（与 serde 一致，便于直接存取）。
    pub fn as_str(self) -> &'static str {
        match self {
            MavenSource::ProjectWrapper => "projectWrapper",
            MavenSource::Configured => "configured",
            MavenSource::System => "system",
        }
    }

    /// 从持久化字符串还原；未识别值降级为 `System`，避免损坏条目阻断列举。
    pub fn parse(s: &str) -> Self {
        match s {
            "projectWrapper" => MavenSource::ProjectWrapper,
            "configured" => MavenSource::Configured,
            "system" => MavenSource::System,
            _ => MavenSource::System,
        }
    }

    /// 优先级链顺序：wrapper > configured > system（§18）。
    pub fn priority(self) -> u8 {
        match self {
            MavenSource::ProjectWrapper => 0,
            MavenSource::Configured => 1,
            MavenSource::System => 2,
        }
    }
}

/// `mvn -v` 解析出的版本信息（§18）。
///
/// Maven 版本串形如 `3.9.6`；major 即首段数字。低于最低支持版本（3.x）
/// 时检测层返回版本但由调用方判定是否可用。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MavenVersionInfo {
    /// Major version（3 / 4）。
    #[serde(default)]
    pub major_version: Option<u32>,
    /// 完整版本串（如 `3.9.6`）。
    #[serde(default)]
    pub full_version: Option<String>,
    /// `mvn -v` 原始输出的首行版本串，便于排查。
    #[serde(default)]
    pub raw: String,
}

impl MavenVersionInfo {
    /// 是否解析出了 major version（3+）。
    pub fn is_valid(&self) -> bool {
        self.major_version.is_some()
    }
}

/// 一个已检测的 Maven 可执行体（§18 / §19）。
///
/// 既是内存模型也是 IPC 载荷：`id` / `created_at` / `updated_at` 在新建时为
/// `None`，由注册表插入时赋值；版本字段在 `mvn -v` 探测失败时保持 `None`，
/// `is_valid=false`。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MavenExecutable {
    /// DB 主键；未持久化时为 `None`。
    #[serde(default)]
    pub id: Option<i64>,
    /// 可执行体绝对路径（wrapper 为项目内 `mvnw` / `mvnw.cmd`，system 为
    /// `PATH` 解析出的 `mvn`，configured 为用户指定路径）。
    pub executable_path: String,
    /// 来源。
    pub source: MavenSource,
    /// 若为 wrapper，所属项目路径（`mvnw` 所在目录）；非 wrapper 为 `None`。
    #[serde(default)]
    pub project_path: Option<String>,
    /// Major version（3 / 4）。
    #[serde(default)]
    pub major_version: Option<u32>,
    /// 完整版本串。
    #[serde(default)]
    pub full_version: Option<String>,
    /// 路径存在 + 可执行 `mvn -v` 时为 true。
    #[serde(default)]
    pub is_valid: bool,
    /// 最近一次校验时间（RFC3339）。
    #[serde(default)]
    pub last_checked: String,
    /// `mvn -v` 原始输出首行。
    #[serde(default)]
    pub raw_version: Option<String>,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
}

impl MavenExecutable {
    /// 构造一个最小字段的新候选（未持久化、未探测版本）。
    pub fn new(
        executable_path: impl Into<String>,
        source: MavenSource,
        project_path: Option<String>,
    ) -> Self {
        Self {
            id: None,
            executable_path: executable_path.into(),
            source,
            project_path,
            major_version: None,
            full_version: None,
            is_valid: false,
            last_checked: String::new(),
            raw_version: None,
            created_at: None,
            updated_at: None,
        }
    }
}

/// 为某个项目解析出的最终生效 Maven（§18 优先级链结果）。
///
/// 这是 `resolve_maven_for_project` 的返回值：要么是一个已选定的可执行体，
/// 要么是 `MavenNotFound` 可行动错误（由调用方转成 `AppError`）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedMaven {
    /// 选中的可执行体。
    pub executable: MavenExecutable,
    /// 本地仓库路径（`~/.m2/repository` 或 settings.xml 覆盖）。
    pub local_repository: PathBuf,
    /// 该项目是否使用 wrapper。
    pub uses_wrapper: bool,
}

/// Maven 执行请求（供 Build Engine R-09 构造命令用）。
///
/// 描述「在哪个目录、用哪个 Maven、跑什么 goals」。实际进程启动与确认流
/// 留给 R-09 / R-10；本结构只做命令预览与参数构造。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MavenExecutionRequest {
    /// 工作目录（项目根，即 `pom.xml` 所在目录）。
    pub working_dir: PathBuf,
    /// 选中的 Maven 可执行体路径。
    pub executable: String,
    /// Maven goals / phases（如 `["clean", "install"]`）。
    pub goals: Vec<String>,
    /// 额外参数（如 `-DskipTests`、`-Pprod`）。
    #[serde(default)]
    pub extra_args: Vec<String>,
    /// 是否经 `cmd /c` 调用（Windows 下 wrapper `.cmd` 需要）。
    #[serde(default)]
    pub via_cmd_c: bool,
    /// 本地仓库路径（注入 `-Dmaven.repo.local`，可选覆盖）。
    #[serde(default)]
    pub local_repository: Option<PathBuf>,
}
