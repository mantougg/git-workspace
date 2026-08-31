//! 包管理器 `run <script>` 命令形状（纯函数；N-08，设计文档 §7 P2）。
//!
//! MVP（N-04）对所有包管理器都用 npm 形状 `run <script> -- <args>`。N-08 补齐
//! pnpm / yarn / bun 的**参数透传差异**——这是不同 CLI 的真实行为差异，不是我们
//! 重新实现 script 语义（对齐 00 约束 §1：只拼命令，不解读 script 文本）。
//!
//! 形状差异（取自各工具官方文档与真实 `--help` 输出）：
//! - **npm**：`npm run <script> -- <args>`。npm 会吞掉脚本名之后的 flag，
//!   必须用 `--` 分隔才会把参数透传给底层脚本（npm 7+ 仍如此）。
//! - **pnpm**：`pnpm run <script> <args>`。pnpm 把脚本名之后的一切直接转发给脚本，
//!   无需 `--`（给了也会被剥掉一个前导 `--`）。
//! - **yarn**（v1 classic 与 berry 一致）：`yarn run <script> <args>`，参数直接透传。
//! - **bun**：`bun run <script> <args>`，参数直接透传（bun 执行性由 `resolve_package_manager`
//!   拦截，这里只为函数保持全函数性而覆盖）。
//!
//! 统一保留 `run` 子命令（不用 pnpm/yarn 允许的「省略 run」简写）：显式 `run`
//! 可消除脚本名与包管理器内置子命令（`install` / `test` / `start` …）的歧义，
//! 是各工具都支持的最稳妥形状。

use crate::node::model::PackageManager;

impl PackageManager {
    /// 该包管理器把额外参数透传给脚本时是否需要 `--` 分隔符。
    ///
    /// 只有 npm 需要；pnpm / yarn / bun 直接把脚本名之后的参数转发给脚本。
    pub fn needs_arg_separator(self) -> bool {
        matches!(self, PackageManager::Npm)
    }
}

/// 按包管理器拼 `run <script> [args]` 的参数向量（不含可执行文件本身）。
///
/// 纯函数：输入包管理器 + 脚本名 + 透传参数，输出稳定的参数序列，可单测。
/// `program_args` 为空时不追加分隔符与参数（`npm run dev` 而非 `npm run dev --`）。
pub fn build_run_args(pm: PackageManager, script: &str, program_args: &[String]) -> Vec<String> {
    let mut args = vec!["run".to_string(), script.to_string()];
    if !program_args.is_empty() {
        if pm.needs_arg_separator() {
            args.push("--".to_string());
        }
        args.extend(program_args.iter().cloned());
    }
    args
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn no_program_args_omits_separator_for_all_managers() {
        for pm in [
            PackageManager::Npm,
            PackageManager::Pnpm,
            PackageManager::Yarn,
            PackageManager::Bun,
        ] {
            assert_eq!(
                build_run_args(pm, "dev", &[]),
                args(&["run", "dev"]),
                "{pm:?} 无参数时不应追加分隔符或空参数",
            );
        }
    }

    #[test]
    fn npm_uses_double_dash_separator() {
        assert_eq!(
            build_run_args(PackageManager::Npm, "dev", &args(&["--port", "3000"])),
            args(&["run", "dev", "--", "--port", "3000"]),
        );
        assert!(PackageManager::Npm.needs_arg_separator());
    }

    #[test]
    fn pnpm_passes_args_directly_without_separator() {
        assert_eq!(
            build_run_args(PackageManager::Pnpm, "dev", &args(&["--port", "3000"])),
            args(&["run", "dev", "--port", "3000"]),
        );
        assert!(!PackageManager::Pnpm.needs_arg_separator());
    }

    #[test]
    fn yarn_passes_args_directly_without_separator() {
        assert_eq!(
            build_run_args(PackageManager::Yarn, "build", &args(&["--mode", "prod"])),
            args(&["run", "build", "--mode", "prod"]),
        );
        assert!(!PackageManager::Yarn.needs_arg_separator());
    }

    #[test]
    fn bun_passes_args_directly_without_separator() {
        // bun 执行性由 resolve 拦截；此处仅验证函数对全部枚举保持全函数性。
        assert_eq!(
            build_run_args(PackageManager::Bun, "start", &args(&["--watch"])),
            args(&["run", "start", "--watch"]),
        );
        assert!(!PackageManager::Bun.needs_arg_separator());
    }

    #[test]
    fn script_name_is_preserved_verbatim() {
        // 不解读脚本名（00 约束 §1）：含冒号的 monorepo 常见命名原样保留。
        assert_eq!(
            build_run_args(PackageManager::Pnpm, "dev:web", &[]),
            args(&["run", "dev:web"]),
        );
    }
}
