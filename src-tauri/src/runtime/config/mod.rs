//! Runtime configuration model and persistence (R-07, B-06 拆分)。
//!
//! 文件布局（设计文档 §4.8）：
//! - `model`：RuntimeApplicationConfig、请求/摘要 DTO、schema 版本常量
//!   与脱敏占位符；
//! - `validation`：名称/路径/符号链接守卫、环境变量 key 校验、脱敏与
//!   占位符保留（完整秘密值不得重新暴露到 IPC）；
//! - `storage`：JSON 读写、原子写（临时文件 + rename + 备份回滚）、
//!   schema 默认值归一化；
//! - `repository`：SQLite 元数据索引与配置生命周期（创建/列表/读取/
//!   更新/删除）；
//! - `environment`：System/Global/Workspace/Runtime/Application 五层
//!   环境合并与 workspace 级环境文件。
//!
//! 持久化边界（不变）：SQLite 存元数据索引，`.gitworkspace/runtimes/*.json`
//! 存用户配置。文件先写（原子写），成功后才更新 SQLite 行；完整秘密值
//! 仅引擎内部（`load_config_unredacted`）可见，IPC 返回一律脱敏。

mod environment;
mod model;
mod repository;
mod storage;
mod validation;

pub use environment::{
    get_workspace_environment, merge_environment, resolve_environment, set_workspace_environment,
    EnvironmentLayers,
};
pub use model::{
    CreateRuntimeConfigRequest, RuntimeApplicationConfig, RuntimeConfigSummary, RuntimeKind,
    UpdateRuntimeConfigRequest, CURRENT_SCHEMA_VERSION, MASKED_VALUE,
};
pub use repository::{create_config, delete_config, get_config, list_configs, update_config};
pub(crate) use repository::{load_config_unredacted, workspace_root};
pub(crate) use storage::write_json_atomic;

#[cfg(test)]
mod tests;
