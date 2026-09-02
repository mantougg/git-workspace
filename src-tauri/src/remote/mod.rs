//! Remote Platform 集成（T-29，Roadmap §27/§28）。
//!
//! - `platform`：origin URL 解析（HTTPS / SSH）、各平台 Open 仓库 / Issue /
//!   PR / CI / New PR 的 URL 构造（纯函数）
//! - `api`：GitHub / GitLab / Gitea / Gitee / Bitbucket REST——Create PR 与
//!   CI 状态查询（reqwest，单次调用不做轮询，速率限制友好）
//!
//! 凭据优先级（§69）：调用方显式 token → OS Credential Store（AI-01
//! keyring，ref=`remote:{platform}:{host}`）→ 系统 `git credential fill`。
//! 本模块不落盘任何明文 token。

pub mod api;
pub mod platform;
