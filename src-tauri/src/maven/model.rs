//! Maven POM 数据模型（§52 / §53 / §54）。
//!
//! 这些结构是纯数据（serde 序列化），作为 R-02 依赖图、R-03 Closure 的数据基础。
//! 字段覆盖 §52 列出的全集：`groupId / artifactId / version / packaging /
//! parent / modules / dependencies / dependencyManagement / profiles / properties /
//! plugins`。

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Maven 依赖 scope（§54）。使用小写字符串以兼容 Maven 自定义 scope。
#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DependencyScope {
    #[default]
    Compile,
    Provided,
    Runtime,
    Test,
    System,
    Import,
}

impl DependencyScope {
    /// 从 POM 文本解析；未识别值降级为 `Compile`（Maven 默认），保持解析不中断。
    pub fn parse(s: &str) -> Self {
        match s.trim() {
            "compile" => DependencyScope::Compile,
            "provided" => DependencyScope::Provided,
            "runtime" => DependencyScope::Runtime,
            "test" => DependencyScope::Test,
            "system" => DependencyScope::System,
            "import" => DependencyScope::Import,
            _ => DependencyScope::Compile,
        }
    }
}

/// Maven GAV 坐标。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PomCoordinates {
    pub group_id: String,
    pub artifact_id: String,
    pub version: String,
}

impl PomCoordinates {
    pub fn gav(&self) -> String {
        format!("{}:{}:{}", self.group_id, self.artifact_id, self.version)
    }
}

/// Parent POM 引用（§52）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MavenParent {
    pub group_id: String,
    pub artifact_id: String,
    pub version: String,
    /// `relativePath`，缺省默认 `../pom.xml`。
    #[serde(default)]
    pub relative_path: Option<String>,
}

/// 一个 `<module>` 声明（§10）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MavenModule {
    /// 模块路径（相对 parent pom 所在目录）。
    pub path: String,
}

/// 一个已解析的 Maven Reactor 关系。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MavenReactor {
    pub parent_path: PathBuf,
    pub parent: PomCoordinates,
    pub modules: Vec<MavenReactorModule>,
}

/// Reactor 中由 parent `<modules>` 声明的一条模块边。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MavenReactorModule {
    pub declared_path: String,
    pub pom_path: PathBuf,
    /// 模块 POM 已被发现时携带其坐标；缺失模块保持 `None`，供上层给出诊断。
    pub project: Option<PomCoordinates>,
}

/// Maven 依赖（§54）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MavenDependency {
    pub group_id: String,
    pub artifact_id: String,
    /// 版本可能为空（由 `dependencyManagement` 或 parent 提供）。
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub scope: DependencyScope,
    #[serde(default)]
    pub optional: bool,
    /// `<type>`，缺省 `jar`。
    #[serde(default = "default_type")]
    pub dep_type: String,
    /// `<classifier>`，缺省空。
    #[serde(default)]
    pub classifier: Option<String>,
    /// 排除列表 `<exclusions>` 的 GAV 坐标。
    #[serde(default)]
    pub exclusions: Vec<PomCoordinates>,
}

fn default_type() -> String {
    "jar".to_string()
}

/// `<dependencyManagement>` 中的一个受管理依赖项。
/// 与 `MavenDependency` 同结构，但语义上只提供版本/scope 默认值。
pub type ManagedDependency = MavenDependency;

/// Maven Profile（§52）。只解析字段，不激活——profile 激活交给 `mvn`（全局约束 §1）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MavenProfile {
    pub id: String,
    #[serde(default)]
    pub properties: BTreeMap<String, String>,
    #[serde(default)]
    pub dependencies: Vec<MavenDependency>,
}

/// `<plugin>` 声明（§52）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MavenPlugin {
    pub group_id: String,
    pub artifact_id: String,
    #[serde(default)]
    pub version: Option<String>,
}

/// 解析后的原始 POM 模型（XML 第一层 + properties/dependencyManagement 等）。
/// effective model 由 [`crate::maven::effective::build_effective`] 在此基础上构建。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MavenProject {
    /// `pom.xml` 文件绝对路径。
    pub path: PathBuf,
    pub group_id: String,
    pub artifact_id: String,
    pub version: String,
    #[serde(default = "default_packaging")]
    pub packaging: String,
    #[serde(default)]
    pub parent: Option<MavenParent>,
    #[serde(default)]
    pub modules: Vec<MavenModule>,
    #[serde(default)]
    pub dependencies: Vec<MavenDependency>,
    #[serde(default)]
    pub dependency_management: Vec<ManagedDependency>,
    #[serde(default)]
    pub profiles: Vec<MavenProfile>,
    #[serde(default)]
    pub properties: BTreeMap<String, String>,
    #[serde(default)]
    pub plugins: Vec<MavenPlugin>,
    /// POM 文件内容的哈希，用于 POM Cache（path + hash）。
    pub file_hash: String,
}

fn default_packaging() -> String {
    "jar".to_string()
}

/// 项目类型识别（§9.2）。Spring Boot 判定归 R-06。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MavenProjectType {
    /// 独立 Maven 项目。
    #[default]
    Standalone,
    /// Parent Project（packaging=pom，含 modules）。
    Parent,
    /// 多模块项目中的某个 module（有 parent 且 parent 在本 workspace 内）。
    MultiModule,
    /// 库项目（packaging=jar 且被其它模块依赖）。
    Library,
}

impl MavenProject {
    /// 该 POM 的 GAV 坐标。
    pub fn coordinates(&self) -> PomCoordinates {
        PomCoordinates {
            group_id: self.group_id.clone(),
            artifact_id: self.artifact_id.clone(),
            version: self.version.clone(),
        }
    }

    /// 推断项目类型（不含 Spring Boot 判定，归 R-06）。
    /// `has_workspace_parent`：parent 是否可在本 workspace 内解析（由 discovery 提供）。
    pub fn project_type(&self, has_workspace_parent: bool) -> MavenProjectType {
        if self.packaging == "pom" {
            return MavenProjectType::Parent;
        }
        if has_workspace_parent || !self.modules.is_empty() {
            return MavenProjectType::MultiModule;
        }
        // Library 需要 workspace 依赖关系才能判定，由 discovery 在 effective
        // dependencies 建好后补充；此处保持保守的 Standalone。
        MavenProjectType::Standalone
    }
}
