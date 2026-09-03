//! 用户项目只读护栏（R-14 §78）。
//!
//! 硬约束（全局约束 §2）：运行时生成物（Synthetic Reactor、配置、日志、
//! 缓存、索引）只允许写入 `workspace_root/.gitworkspace/`，绝不触碰用户
//! pom / 源码 / Git 状态。
//!
//! 本护栏是 Runtime 写路径的统一入口校验：生产路径返回 `Permission` 可行动
//! 错误；开发期 `debug_assert` 双保险（fail-fast 定位违规写）。护栏只加在
//! 写路径，不影响只读流程性能（§78「护栏断言只加在 Runtime 写路径」）。

use std::path::Path;

use crate::error::{AppError, AppResult};

/// 纯校验：运行时生成物写路径必须在 `workspace_root/.gitworkspace/` 下。
/// 不触发 debug 断言（供测试与内部复用直接调用）。
pub fn check_workspace_write_path(path: &Path, workspace_root: &Path, what: &str) -> AppResult<()> {
    let gitworkspace = workspace_root.join(".gitworkspace");
    if !path.starts_with(&gitworkspace) {
        return Err(AppError::Permission(format!(
            "{what} 试图写入 workspace 之外：{path:?}。\
             运行时生成物只允许落在 {gitworkspace:?} 下（用户项目只读，全局约束 §2）"
        )));
    }
    Ok(())
}

/// 校验运行时生成物的写路径必须在 `workspace_root/.gitworkspace/` 下。
/// `what` 用于错误文案（如「Synthetic Reactor 生成」「日志落盘」）。
/// 开发期 `debug_assert` fail-fast（违规写立即暴露），生产返回
/// `Permission` 可行动错误。
pub fn assert_workspace_write_path(path: &Path, workspace_root: &Path, what: &str) -> AppResult<()> {
    let allowed = path.starts_with(&workspace_root.join(".gitworkspace"));
    debug_assert!(
        allowed,
        "R-14 guard: {what} writes outside .gitworkspace: {path:?} (workspace {workspace_root:?})"
    );
    check_workspace_write_path(path, workspace_root, what)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_gitworkspace_paths() {
        let root = Path::new("/ws");
        for path in [
            "/ws/.gitworkspace/runtimes/app.json",
            "/ws/.gitworkspace/runtime/app/pom.xml",
            "/ws/.gitworkspace/logs/app/1.log",
            "/ws/.gitworkspace/runtime/app/classpath/cp.txt",
        ] {
            assert_workspace_write_path(Path::new(path), root, "测试")
                .unwrap_or_else(|e| panic!("{path} 应在护栏内: {e}"));
        }
    }

    #[test]
    fn rejects_outside_paths_with_permission_error() {
        let root = Path::new("/ws");
        for path in [
            "/ws/repo/pom.xml",
            "/ws/repo/src/main/java/App.java",
            "/ws/.git/config",
            "/etc/passwd",
            "/other/.gitworkspace/x.json",
        ] {
            let error = check_workspace_write_path(Path::new(path), root, "测试").unwrap_err();
            assert_eq!(error.code(), "PermissionError", "{path}");
        }
    }

    #[test]
    fn rejects_sibling_gitworkspace_of_submodule() {
        // 用户子仓库内的 .gitworkspace 也属于「用户项目区」——只在 workspace
        // 根下允许（子模块的 .gitworkspace 可能被提交/污染用户仓库）。
        let root = Path::new("/ws");
        let error = check_workspace_write_path(Path::new("/ws/repo/.gitworkspace/x.json"), root, "测试").unwrap_err();
        assert_eq!(error.code(), "PermissionError");
    }
}
