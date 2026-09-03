//! Maven 项目发现与 POM 解析（R-01）+ Maven 执行策略（R-05）。
//!
//! 本模块负责 Workspace 级 `pom.xml` 扫描、XML 解析、Effective Model 构建、
//! 项目类型识别与多模块 Reactor 关系建立，以及 POM Cache（R-01）；以及
//! Maven 可执行体优先级链检测、版本探测、Executor 抽象与 settings.xml
//! 本地仓库探测（R-05）。
//!
//! - 复用 T-01 Scanner 的忽略规则（`.gitworkspaceignore` + 默认跳过目录），
//!   不另起一套目录遍历框架（全局约束 §7）。
//! - 解析全程本地完成，**禁止任何网络请求**（远程 parent POM 缺失时降级标记）。
//! - 解析模型为纯数据，不缓存文件句柄，解析完即释放。
//! - Effective model 只覆盖 Runtime 所需字段，不追求完整 Maven Model Builder；
//!   复杂 profile 激活、远程 parent 解析交给 `mvn` 自身（全局约束 §1）。
//! - R-05 不重新实现 Maven（全局约束 §1）：只做发现、选择、执行封装。

pub mod cache;
pub mod closure;
pub mod detect_exec;
pub mod discovery;
pub mod effective;
pub mod exec_model;
pub mod executor;
pub mod index;
pub mod model;
pub mod mvnd;
pub mod parser;
pub mod reactor;
pub mod registry;
pub mod resolver;
pub mod settings;

pub use cache::{PomCache, PomCacheStats};
pub use closure::{
    compute_runtime_closure, ClosureCacheLookup, RuntimeClosure, RuntimeClosureCache, RuntimeScope, RuntimeScopeMode,
};
pub use detect_exec::{
    candidate_is_usable, detect_maven_candidates, probe_version, resolve_maven_for_project, MIN_MAVEN_MAJOR_VERSION,
};
pub use discovery::{
    discover_poms, discover_poms_in_repos, MavenDiscoveryResult, PomDiscoveryError, RepoDiscoveryResult,
};
pub use effective::{build_effective, EffectiveProject};
pub use exec_model::{MavenExecutable, MavenExecutionRequest, MavenSource, MavenVersionInfo, ResolvedMaven};
pub use executor::{build_command, build_process, build_request, preview_command};
pub use index::{
    query_dependency_graph, query_project_dependencies, refresh_dependency_sources, sync_workspace_index,
    DependencyEdge, DependencyGraph, DependencyGraphCache, GraphCacheLookup, IndexSyncResult, MavenModuleLink,
    MavenProjectNode, SourceMapping,
};
pub use model::{
    MavenDependency, MavenModule, MavenParent, MavenProject, MavenProjectType, MavenReactor, MavenReactorModule,
    PomCoordinates,
};
pub use parser::{parse_pom, parse_pom_file, PomParseError};
pub use reactor::{ensure_gitworkspace_ignored, prepare_runtime_reactor, RuntimeReactorKind, RuntimeReactorPlan};
pub use registry::{
    apply_version as apply_maven_version, get_maven_executable, list_maven_executables,
    mark_validity as mark_maven_validity, prune_invalid_paths, remove_maven_executable, upsert_maven_executable,
};
pub use resolver::{
    default_local_repository, resolve_dependency, DependencySource, IndexedMavenProject, ResolutionReason,
    ResolvedDependency, WorkspaceMavenIndex,
};
pub use settings::{resolve_local_repository, user_settings_path};
