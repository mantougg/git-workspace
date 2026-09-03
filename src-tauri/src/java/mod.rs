//! JDK 检测与 JDK Manager（R-04，源文档 §31 JVM 管理、§32 JDK Manager）。
//!
//! 本模块负责本机 JDK 的多来源发现、`java -version` 解析、注册表持久化与
//! 惰性校验，为 Launcher（R-10）提供可靠的 `java` 可执行路径。
//!
//! - 复用 T-03 SQLite 数据层（WAL / 单写者 / 版本化迁移 / 批量事务），不另起
//!   存储（全局约束 §7）。
//! - 检测只在「找 java + 探测版本」阶段 fork `java -version`（只读探测，非
//!   shell 脚本）；build/run 进程的确认流留给 R-09/R-10。
//! - 全程本地完成、无网络请求（全局约束 §10）。
//! - 检测惰性：`prune_invalid_homes` 只校验路径存在性、显式 `validate_jdk`
//!   才 fork 复检；禁止每次启动全量重扫（§性能）。
//!
//! 项目级 JDK 绑定（`runtime_projects.jdk`）的配置层 UI 随 R-07 落地，
//! R-10 启动实际使用该 JDK 的联调验证在 R-10 完成；R-04 只交付注册表与 API。

pub mod detect;
pub mod model;
pub mod registry;
pub mod resolve;
pub mod version;

pub use detect::discover_jdks;
pub use model::{JdkDiscoverySource, JdkInstallation, JdkVendor};
pub use registry::{
    apply_version, get_jdk as get_jdk_row, list_jdks as list_jdk_rows, mark_validity, prune_invalid_homes,
    remove_jdk as remove_jdk_row, upsert_jdk, upsert_jdks_batch,
};
pub use resolve::resolve_jdk_for_config;
pub use version::{parse_java_version, JdkVersionInfo};
