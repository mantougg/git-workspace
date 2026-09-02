//! Tree-sitter Symbol Index（T-28，Roadmap §26）。
//!
//! - `lang`    语言配置（语法、符号/引用/调用查询），按扩展名识别
//! - `extract` 单文件符号与引用提取（纯函数，逐语法查询捕获）
//! - `index`   落库与查询：增量重建（内容 hash）、定义/引用/调用层级、过滤
//!
//! 性能约束（00-全局约束 §2）：解析走每仓库调用内限步；增量靠
//! `symbol_index_files` 内容 hash——单文件变更只重解析该文件；
//! 查询命中 `idx_symbols_*` / `idx_symbol_refs_*` 索引。

pub mod extract;
pub mod index;
pub mod lang;
