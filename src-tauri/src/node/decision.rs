//! 包管理器决策链（纯函数；N-01，设计文档 §4.1）。
//!
//! 优先级（从高到低）：
//! 1. Runtime 配置显式指定的 `packageManager`（用户覆盖）；
//! 2. `package.json` 的 `packageManager` 字段（Corepack 标准，如 `pnpm@9.1.0`）；
//! 3. lockfile 推断（多 lockfile 并存时固定顺序：pnpm > yarn > npm > bun）；
//! 4. 回退 PATH 上的 `npm`。
//!
//! 各层取到未知名（如配置写 `deno`）时记 `log::warn` 并落到下一层，
//! 决策函数保持全函数（total）：任何输入都有一个确定的决策结果。
//! bun 会作为决策结果被识别（`bun.lockb` / `bun@x.y`），是否可执行由
//! `detect::resolve_package_manager` 判定（MVP 报可行动错误引导改选）。

use std::path::Path;

use crate::node::model::PackageManager;

/// 决策来源（可观测性：错误与日志中说明「为什么选中这个 pm」）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecisionSource {
    /// Runtime 配置显式指定（用户覆盖）。
    Configured,
    /// `package.json` 的 `packageManager` 字段（Corepack 标准）。
    PackageJsonField,
    /// lockfile 推断。
    Lockfile,
    /// 回退 PATH 上的 `npm`。
    PathFallback,
}

impl DecisionSource {
    pub fn as_str(self) -> &'static str {
        match self {
            DecisionSource::Configured => "configured",
            DecisionSource::PackageJsonField => "packageJsonField",
            DecisionSource::Lockfile => "lockfile",
            DecisionSource::PathFallback => "pathFallback",
        }
    }
}

/// lockfile 存在性快照（决策链输入；`scan` 是文件系统调用唯一入口）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LockfileSnapshot {
    /// `pnpm-lock.yaml`。
    pub pnpm_lock: bool,
    /// `yarn.lock`。
    pub yarn_lock: bool,
    /// `package-lock.json`。
    pub npm_package_lock: bool,
    /// `npm-shrinkwrap.json`。
    pub npm_shrinkwrap: bool,
    /// `bun.lockb`（只识别不执行，引导改选）。
    pub bun_lockb: bool,
}

impl LockfileSnapshot {
    /// 采样项目目录（`package.json` 所在目录）的 lockfile 存在性。
    pub fn scan(project_dir: &Path) -> Self {
        Self {
            pnpm_lock: project_dir.join("pnpm-lock.yaml").is_file(),
            yarn_lock: project_dir.join("yarn.lock").is_file(),
            npm_package_lock: project_dir.join("package-lock.json").is_file(),
            npm_shrinkwrap: project_dir.join("npm-shrinkwrap.json").is_file(),
            bun_lockb: project_dir.join("bun.lockb").is_file(),
        }
    }

    /// 固定优先级推断：pnpm > yarn > npm（package-lock 先于 shrinkwrap）> bun；
    /// 无任何 lockfile 返回 `None`。
    pub fn infer(&self) -> Option<(PackageManager, &'static str)> {
        if self.pnpm_lock {
            Some((PackageManager::Pnpm, "pnpm-lock.yaml"))
        } else if self.yarn_lock {
            Some((PackageManager::Yarn, "yarn.lock"))
        } else if self.npm_package_lock {
            Some((PackageManager::Npm, "package-lock.json"))
        } else if self.npm_shrinkwrap {
            Some((PackageManager::Npm, "npm-shrinkwrap.json"))
        } else if self.bun_lockb {
            Some((PackageManager::Bun, "bun.lockb"))
        } else {
            None
        }
    }
}

/// 决策链输入（全部纯数据，可单测）。
#[derive(Debug, Clone, Default)]
pub struct DecisionInput {
    /// Runtime 配置显式指定的包管理器（用户覆盖；`node_package_manager`）。
    pub configured: Option<String>,
    /// `package.json` 的 `packageManager` 字段原文（如 `pnpm@9.1.0`）。
    pub package_json_field: Option<String>,
    /// lockfile 存在性快照。
    pub lockfiles: LockfileSnapshot,
}

/// 决策链结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageManagerDecision {
    pub manager: PackageManager,
    pub source: DecisionSource,
    /// 人类可读的选中原因（进入错误消息与日志，保证决策链可观测）。
    pub reason: String,
}

/// 解析 `package.json` 的 `packageManager` 字段：取 `@` 前的名字段
///（`pnpm@9.1.0` → pnpm；`yarn@4.1.1+sha.abc` → yarn）。
/// 空串、scoped 形式（`@...`）、未知名均返回 `None`（落到下一层）。
pub fn parse_package_manager_field(raw: &str) -> Option<PackageManager> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.starts_with('@') {
        return None;
    }
    let name = trimmed.split('@').next()?.trim();
    PackageManager::parse(name)
}

/// 包管理器决策链（纯函数）：配置 → packageManager 字段 → lockfile → PATH npm。
pub fn decide_package_manager(input: &DecisionInput) -> PackageManagerDecision {
    // 1. 配置显式指定（用户覆盖，最高优先级）。
    if let Some(configured) = non_empty(input.configured.as_deref()) {
        if let Some(pm) = PackageManager::parse(configured) {
            return decision(pm, DecisionSource::Configured, format!("配置显式指定 {}", pm.name()));
        }
        log::warn!("unknown configured package manager {configured:?}; falling through decision chain");
    }

    // 2. package.json 的 packageManager 字段（Corepack 标准）。
    if let Some(field) = non_empty(input.package_json_field.as_deref()) {
        if let Some(pm) = parse_package_manager_field(field) {
            return decision(
                pm,
                DecisionSource::PackageJsonField,
                format!("package.json packageManager 字段 {field:?}"),
            );
        }
        log::warn!("unrecognized packageManager field {field:?}; falling through decision chain");
    }

    // 3. lockfile 推断（固定顺序 pnpm > yarn > npm > bun）。
    if let Some((pm, lockfile)) = input.lockfiles.infer() {
        return decision(pm, DecisionSource::Lockfile, format!("lockfile 推断：{lockfile}"));
    }

    // 4. 回退 PATH 上的 npm（node 自带，MVP 保底）。
    decision(
        PackageManager::Npm,
        DecisionSource::PathFallback,
        "无配置 / packageManager 字段 / lockfile，回退 PATH npm".to_string(),
    )
}

fn decision(pm: PackageManager, source: DecisionSource, reason: String) -> PackageManagerDecision {
    PackageManagerDecision {
        manager: pm,
        source,
        reason,
    }
}

fn non_empty(raw: Option<&str>) -> Option<&str> {
    raw.map(str::trim).filter(|s| !s.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(configured: Option<&str>, field: Option<&str>, lockfiles: LockfileSnapshot) -> DecisionInput {
        DecisionInput {
            configured: configured.map(str::to_string),
            package_json_field: field.map(str::to_string),
            lockfiles,
        }
    }

    #[test]
    fn parses_package_manager_field_name_part() {
        assert_eq!(parse_package_manager_field("pnpm@9.1.0"), Some(PackageManager::Pnpm));
        assert_eq!(parse_package_manager_field("npm@10.2.3"), Some(PackageManager::Npm));
        assert_eq!(
            parse_package_manager_field("yarn@4.1.1+sha.abc"),
            Some(PackageManager::Yarn)
        );
        assert_eq!(parse_package_manager_field("bun@1.1.0"), Some(PackageManager::Bun));
        // 容忍无版本与大小写。
        assert_eq!(parse_package_manager_field("PNPM"), Some(PackageManager::Pnpm));
        // 空串 / scoped / 未知名 → None（落下一层）。
        assert_eq!(parse_package_manager_field(""), None);
        assert_eq!(parse_package_manager_field("   "), None);
        assert_eq!(parse_package_manager_field("@pnpm/exe"), None);
        assert_eq!(parse_package_manager_field("deno@1.0"), None);
    }

    #[test]
    fn configured_overrides_everything() {
        let snapshot = LockfileSnapshot {
            pnpm_lock: true,
            yarn_lock: true,
            ..Default::default()
        };
        let d = decide_package_manager(&input(Some("npm"), Some("pnpm@9.1.0"), snapshot));
        assert_eq!(d.manager, PackageManager::Npm);
        assert_eq!(d.source, DecisionSource::Configured);
    }

    #[test]
    fn package_json_field_beats_lockfile() {
        let snapshot = LockfileSnapshot {
            yarn_lock: true,
            ..Default::default()
        };
        let d = decide_package_manager(&input(None, Some("pnpm@9.1.0"), snapshot));
        assert_eq!(d.manager, PackageManager::Pnpm);
        assert_eq!(d.source, DecisionSource::PackageJsonField);
        assert!(d.reason.contains("pnpm@9.1.0"));
    }

    #[test]
    fn lockfile_conflict_uses_fixed_order() {
        // pnpm 与 yarn 并存 → pnpm。
        let snapshot = LockfileSnapshot {
            pnpm_lock: true,
            yarn_lock: true,
            ..Default::default()
        };
        let d = decide_package_manager(&input(None, None, snapshot));
        assert_eq!((d.manager, d.source), (PackageManager::Pnpm, DecisionSource::Lockfile));

        // yarn 与 npm lock 并存 → yarn。
        let snapshot = LockfileSnapshot {
            yarn_lock: true,
            npm_package_lock: true,
            ..Default::default()
        };
        assert_eq!(
            decide_package_manager(&input(None, None, snapshot)).manager,
            PackageManager::Yarn
        );

        // package-lock 与 shrinkwrap 并存 → npm（均为 npm，顺序不影响结果）。
        let snapshot = LockfileSnapshot {
            npm_package_lock: true,
            npm_shrinkwrap: true,
            ..Default::default()
        };
        assert_eq!(
            decide_package_manager(&input(None, None, snapshot)).manager,
            PackageManager::Npm
        );

        // 仅 shrinkwrap → npm。
        let snapshot = LockfileSnapshot {
            npm_shrinkwrap: true,
            ..Default::default()
        };
        assert_eq!(
            decide_package_manager(&input(None, None, snapshot)).manager,
            PackageManager::Npm
        );

        // 仅 bun.lockb → bun（识别；执行性由 resolve 判定）。
        let snapshot = LockfileSnapshot {
            bun_lockb: true,
            ..Default::default()
        };
        let d = decide_package_manager(&input(None, None, snapshot));
        assert_eq!((d.manager, d.source), (PackageManager::Bun, DecisionSource::Lockfile));
        assert!(d.reason.contains("bun.lockb"));
    }

    #[test]
    fn falls_back_to_path_npm() {
        let d = decide_package_manager(&input(None, None, LockfileSnapshot::default()));
        assert_eq!(
            (d.manager, d.source),
            (PackageManager::Npm, DecisionSource::PathFallback)
        );
    }

    #[test]
    fn unknown_values_fall_through_layers() {
        // 配置写未知名 → 落到 packageManager 字段层。
        let d = decide_package_manager(&input(Some("deno"), Some("yarn@4.1.1"), LockfileSnapshot::default()));
        assert_eq!(
            (d.manager, d.source),
            (PackageManager::Yarn, DecisionSource::PackageJsonField)
        );

        // packageManager 字段也未识别 → 落到 lockfile 层。
        let snapshot = LockfileSnapshot {
            pnpm_lock: true,
            ..Default::default()
        };
        let d = decide_package_manager(&input(Some("deno"), Some("deno@1.0"), snapshot));
        assert_eq!((d.manager, d.source), (PackageManager::Pnpm, DecisionSource::Lockfile));

        // 全部未识别 → 回退 PATH npm。
        let d = decide_package_manager(&input(Some("deno"), Some(""), LockfileSnapshot::default()));
        assert_eq!(
            (d.manager, d.source),
            (PackageManager::Npm, DecisionSource::PathFallback)
        );
    }

    #[test]
    fn scans_lockfiles_from_project_dir() {
        let tmp = std::env::temp_dir().join(format!(
            "gw_node_lock_{}",
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(tmp.join("pnpm-lock.yaml"), b"lockfileVersion: '9.0'").unwrap();
        std::fs::write(tmp.join("yarn.lock"), b"# yarn lockfile v1").unwrap();
        let snapshot = LockfileSnapshot::scan(&tmp);
        assert!(snapshot.pnpm_lock && snapshot.yarn_lock);
        assert!(!snapshot.npm_package_lock && !snapshot.npm_shrinkwrap && !snapshot.bun_lockb);
        assert_eq!(snapshot.infer().map(|(pm, _)| pm), Some(PackageManager::Pnpm));
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
