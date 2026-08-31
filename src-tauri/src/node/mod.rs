//! Node.js 工具链检测（N-01，设计文档 §4.1 / §4.7）。
//!
//! 本模块检测 `node` 与包管理器（npm / pnpm / yarn）可执行文件与版本，
//! 并实现包管理器决策链，为后续启动链路（N-04 LaunchPlan::Script）提供
//! 「解析出可执行绝对路径」的能力。
//!
//! 边界（tasks-node 00 约束）：
//! - 一律走 `java/detect.rs::find_in_path`（`.exe → .cmd → .bat → 裸名`）；
//!   Windows 上 npm/pnpm/yarn 实体是 `.cmd` shim，禁止裸名兜底。
//! - 决策链是纯函数（输入：配置值 + package.json 摘要 + lockfile 快照；
//!   输出：枚举 + 来源 + 原因），系统调用只留检测入口。
//! - bun 只识别不执行（MVP 不支持，属 N-09）：选中即报可行动错误引导改选。
//! - 版本探测失败降级为「未知版本」，不报错；检测不到可执行才报
//!   `NodeNotFound` / `PackageManagerNotFound` 可行动错误（§4.7）。
//! - MVP 不做注册表（自定义路径登记属 N-08）。

pub mod decision;
pub mod detect;
pub mod discovery;
pub mod model;

pub use decision::{
    decide_package_manager, parse_package_manager_field, DecisionInput, DecisionSource,
    LockfileSnapshot, PackageManagerDecision,
};
pub use detect::{detect_node, detect_package_manager, extract_version, resolve_package_manager};
pub use discovery::{
    discover_package_jsons, global_package_cache, list_node_projects, sync_node_projects,
    NodeDiscoveryResult, NodePackageCache,
};
pub use model::{NodeProjectNode, PackageManager, ToolDetection};
