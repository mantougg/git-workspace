//! Node 工具链数据模型（N-01，设计文档 §4.1）。
//!
//! 纯数据定义：包管理器枚举与检测结果。serde 派生为后续 IPC
//!（`node_detect_toolchain`，设计文档 §4.8）预留，N-01 不暴露命令。

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// 支持的包管理器。
///
/// MVP（N-01 ~ N-07）只保 `npm run <script>` 执行链；pnpm / yarn 识别并
/// 检测，执行链在 N-08；bun 只识别不执行（N-09），选中即引导改选。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PackageManager {
    Npm,
    Pnpm,
    Yarn,
    Bun,
}

/// Node tool registered by the user. A package manager entry carries its
/// concrete manager name; the `Node` variant represents the Node.js runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NodeExecutableKind {
    Node,
    PackageManager,
}

impl NodeExecutableKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Node => "node",
            Self::PackageManager => "packageManager",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value {
            "packageManager" => Self::PackageManager,
            _ => Self::Node,
        }
    }
}

impl PackageManager {
    /// 稳定名字（package.json `packageManager` 字段与 CLI 名一致）。
    pub fn name(self) -> &'static str {
        match self {
            PackageManager::Npm => "npm",
            PackageManager::Pnpm => "pnpm",
            PackageManager::Yarn => "yarn",
            PackageManager::Bun => "bun",
        }
    }

    /// PATH 上查找的可执行名（Windows 上由 `find_in_path` 补 `.cmd` 等扩展名）。
    pub fn executable_name(self) -> &'static str {
        self.name()
    }

    /// 解析包管理器名（去空白、大小写不敏感）；未知名返回 `None`。
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "npm" => Some(PackageManager::Npm),
            "pnpm" => Some(PackageManager::Pnpm),
            "yarn" => Some(PackageManager::Yarn),
            "bun" => Some(PackageManager::Bun),
            _ => None,
        }
    }
}

/// 一个已检测的工具链可执行体（node 或包管理器）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolDetection {
    /// 可执行文件绝对路径（Windows 上包管理器通常是 `.cmd` shim）。
    pub executable: PathBuf,
    /// 探测到的版本串（node 去 `v` 前缀）；探测失败降级为 `None`（「未知版本」）。
    #[serde(default)]
    pub version: Option<String>,
    /// 版本探测原始输出（截断），排查用。
    #[serde(default)]
    pub raw_output: String,
    /// 探测进程成功退出并解析出版本。
    #[serde(default)]
    pub probe_ok: bool,
    /// Whether the executable came from the persistent registry or PATH.
    #[serde(default)]
    pub source: ToolDetectionSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum ToolDetectionSource {
    Registry,
    #[default]
    Path,
}

/// A user-registered Node.js or package-manager executable.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeExecutable {
    #[serde(default)]
    pub id: Option<i64>,
    pub kind: NodeExecutableKind,
    /// `None` for Node.js; the selected manager for package-manager entries.
    #[serde(default)]
    pub package_manager: Option<PackageManager>,
    pub executable_path: String,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub raw_output: String,
    #[serde(default)]
    pub is_valid: bool,
    #[serde(default)]
    pub last_checked: String,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
}

/// Request used by the registry IPC commands. The path is intentionally a
/// user-provided concrete executable, never a shell command or PATH token.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeExecutableRequest {
    pub kind: NodeExecutableKind,
    #[serde(default)]
    pub package_manager: Option<PackageManager>,
    pub executable_path: String,
}

impl NodeExecutable {
    pub fn new(
        kind: NodeExecutableKind,
        package_manager: Option<PackageManager>,
        executable_path: impl Into<String>,
    ) -> Self {
        Self {
            id: None,
            kind,
            package_manager,
            executable_path: executable_path.into(),
            version: None,
            raw_output: String::new(),
            is_valid: false,
            last_checked: String::new(),
            created_at: None,
            updated_at: None,
        }
    }
}

/// Persisted Node project metadata exposed by `node_list_projects`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeProjectNode {
    pub project_id: i64,
    pub repository_id: Option<i64>,
    /// Package directory (the directory containing `package.json`), stored as
    /// a normalized path key for cross-platform matching.
    pub path: PathBuf,
    pub name: String,
    pub version: String,
    pub package_manager: Option<String>,
    /// Original JSON object text. Keeping this as text preserves script order
    /// and lets the UI choose how to render values.
    pub scripts_json: String,
    /// N-09：workspace 根目录（npm/yarn `workspaces` 字段或 pnpm-workspace.yaml
    /// 命中）；独立工程为 `None`。列表查询时按目录向上推断。
    #[serde(default)]
    pub workspace_root: Option<String>,
    /// Content hash used by the index synchronizer; omitted from IPC payloads.
    #[serde(skip)]
    pub pkg_hash: String,
}
