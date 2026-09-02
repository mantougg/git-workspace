//! 本机 Node 工具链扫描（N-10）。
//!
//! 枚举常见安装位置与版本管理器数据目录（nvm / nvm-windows / fnm / volta /
//! mise / asdf / n / scoop / homebrew）中的 node 与包管理器可执行，返回候选
//! 列表供用户勾选登记。**只读发现**：不写注册表（注册表条目优先于 PATH，
//! 自动登记会改变决策行为，必须用户确认——与 JDK `discover_jdks` 直接入库
//! 的有意差异，见 N-10 spec）。
//!
//! - 可执行定位一律复用 `java/detect.rs::find_executable_in_dirs`
//!   （`.exe → .cmd → .bat → 裸名`，Windows shim 硬规则）。
//! - 去重键 = 分隔符归一化（`\` → `/`）+ 小写化：集合语义按 AGENTS §1 边界
//!   采用小写化，覆盖 Windows/macOS 大小写不敏感 FS 上的 junction/symlink
//!   重复（nvm-windows 的 `Program Files\nodejs` junction、Homebrew `bin`
//!   符号链接与 `opt/node*/bin` 同物异径）。
//! - 版本探测复用 `detect.rs::probe_tool`，失败降级「未知版本」不阻断。

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::java::detect::find_executable_in_dirs;
use crate::node::detect::probe_tool;
use crate::node::model::{NodeExecutableKind, NodeScanCandidate, PackageManager};

/// companion 包管理器：node 安装目录（bin）内同住的 shim（npm 随 node 发行，
/// pnpm/yarn/bun 常由 corepack/手动装进版本目录）。
const COMPANION_MANAGERS: [PackageManager; 4] = [
    PackageManager::Npm,
    PackageManager::Pnpm,
    PackageManager::Yarn,
    PackageManager::Bun,
];

/// 路径归一化键：分隔符统一为 `/` 并小写化（去重与 registered 比对共用）。
pub(crate) fn normalize_path_key(path: &str) -> String {
    path.replace('\\', "/").to_ascii_lowercase()
}

/// 扫描根：`BinDir` 目录本身即可执行目录（系统 bin、volta shim、pnpm home）；
/// `VersionParent` 的子目录是各版本安装目录（nvm/fnm/mise/scoop/homebrew），
/// `name_prefix` 过滤子目录名（homebrew `opt` 下只看 `node*`）。
#[derive(Debug, Clone)]
pub(crate) enum ScanRoot {
    BinDir {
        source: &'static str,
        dir: PathBuf,
    },
    VersionParent {
        source: &'static str,
        dir: PathBuf,
        name_prefix: Option<&'static str>,
    },
}

/// 收集阶段产物：路径级候选（尚未探测版本）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RawCandidate {
    pub kind: NodeExecutableKind,
    pub package_manager: Option<PackageManager>,
    pub executable_path: PathBuf,
    pub source: &'static str,
}

/// 一个安装目录（或 bin 目录）中的 node + 同住包管理器。
fn candidates_from_install_dir(dir: &Path, source: &'static str, out: &mut Vec<RawCandidate>) {
    if let Some(node) = find_executable_in_dirs("node", std::slice::from_ref(&dir.to_path_buf())) {
        out.push(RawCandidate {
            kind: NodeExecutableKind::Node,
            package_manager: None,
            executable_path: node,
            source,
        });
    }
    for pm in COMPANION_MANAGERS {
        if let Some(exe) = find_executable_in_dirs(pm.executable_name(), &[dir.to_path_buf()]) {
            out.push(RawCandidate {
                kind: NodeExecutableKind::PackageManager,
                package_manager: Some(pm),
                executable_path: exe,
                source,
            });
        }
    }
}

/// 版本目录中的安装目录候选：`<dir>`（nvm-windows / mise-windows / volta
/// image 的 node.exe 在根）与 fnm 的 `<dir>/installation`；两者再各试 `bin`。
fn install_dirs_for_version_dir(version_dir: &Path) -> Vec<PathBuf> {
    let mut dirs = vec![version_dir.to_path_buf()];
    dirs.push(version_dir.join("installation"));
    dirs
}

fn candidates_from_version_parent(
    parent: &Path,
    source: &'static str,
    name_prefix: Option<&'static str>,
    out: &mut Vec<RawCandidate>,
) {
    let entries = match std::fs::read_dir(parent) {
        Ok(e) => e,
        Err(err) => {
            log::debug!("node scan skipped {:?}: {}", parent, err);
            return;
        }
    };
    for entry in entries.flatten() {
        let version_dir = entry.path();
        if !version_dir.is_dir() {
            continue;
        }
        if let Some(prefix) = name_prefix {
            if !entry
                .file_name()
                .to_string_lossy()
                .to_ascii_lowercase()
                .starts_with(prefix)
            {
                continue;
            }
        }
        for install in install_dirs_for_version_dir(&version_dir) {
            // node 在 `<install>` 或 `<install>/bin`；companion 与 node 同目录。
            let bin_dirs = [install.clone(), install.join("bin")];
            if let Some(node) = find_executable_in_dirs("node", &bin_dirs) {
                let node_bin = node.parent().map(|bin| bin.to_path_buf());
                out.push(RawCandidate {
                    kind: NodeExecutableKind::Node,
                    package_manager: None,
                    executable_path: node,
                    source,
                });
                if let Some(bin) = node_bin {
                    for pm in COMPANION_MANAGERS {
                        if let Some(exe) = find_executable_in_dirs(
                            pm.executable_name(),
                            std::slice::from_ref(&bin),
                        ) {
                            out.push(RawCandidate {
                                kind: NodeExecutableKind::PackageManager,
                                package_manager: Some(pm),
                                executable_path: exe,
                                source,
                            });
                        }
                    }
                }
            } else {
                // 无 node 的安装目录仍可能住着独立安装的包管理器。
                for bin in &bin_dirs {
                    for pm in COMPANION_MANAGERS {
                        if let Some(exe) =
                            find_executable_in_dirs(pm.executable_name(), std::slice::from_ref(bin))
                        {
                            out.push(RawCandidate {
                                kind: NodeExecutableKind::PackageManager,
                                package_manager: Some(pm),
                                executable_path: exe,
                                source,
                            });
                        }
                    }
                }
            }
        }
    }
}

/// 收集所有扫描根的路径级候选（只读文件系统，不 spawn 进程）。
pub(crate) fn collect_candidates(roots: &[ScanRoot]) -> Vec<RawCandidate> {
    let mut out = Vec::new();
    for root in roots {
        match root {
            ScanRoot::BinDir { source, dir } => {
                if dir.is_dir() {
                    candidates_from_install_dir(dir, source, &mut out);
                }
            }
            ScanRoot::VersionParent {
                source,
                dir,
                name_prefix,
            } => {
                candidates_from_version_parent(dir, source, *name_prefix, &mut out);
            }
        }
    }
    out
}

/// 候选去重：canonicalize 折叠 junction/symlink，再按归一化键取首个来源。
fn dedupe(raw: Vec<RawCandidate>) -> Vec<RawCandidate> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut out = Vec::new();
    for candidate in raw {
        let path = std::fs::canonicalize(&candidate.executable_path)
            .unwrap_or_else(|_| candidate.executable_path.clone());
        let key = normalize_path_key(&path.to_string_lossy());
        if seen.insert(key) {
            out.push(RawCandidate {
                executable_path: path,
                ..candidate
            });
        }
    }
    out.sort_by(|a, b| {
        a.kind
            .as_str()
            .cmp(b.kind.as_str())
            .then_with(|| a.source.cmp(b.source))
            .then_with(|| {
                normalize_path_key(&a.executable_path.to_string_lossy())
                    .cmp(&normalize_path_key(&b.executable_path.to_string_lossy()))
            })
    });
    out
}

/// 全量扫描：收集 → 去重 → 逐个 `-v` 探测（失败降级「未知版本」）。
pub fn scan_node_toolchain() -> Vec<NodeScanCandidate> {
    let roots = scan_roots();
    dedupe(collect_candidates(&roots))
        .into_iter()
        .map(|raw| {
            let detection = probe_tool(&raw.executable_path);
            NodeScanCandidate {
                kind: raw.kind,
                package_manager: raw.package_manager,
                executable_path: raw.executable_path.to_string_lossy().into_owned(),
                version: detection.version,
                probe_ok: detection.probe_ok,
                source: raw.source.to_string(),
                registered: false,
            }
        })
        .collect()
}

/// 平台扫描根枚举。目录不存在时由收集阶段跳过，这里不判存在。
fn scan_roots() -> Vec<ScanRoot> {
    let mut roots = Vec::new();
    let home = dirs::home_dir();
    if cfg!(target_os = "windows") {
        // 系统安装（含 nvm-windows 的当前版本 junction 与 choco 默认落点）。
        roots.push(ScanRoot::BinDir {
            source: "system",
            dir: PathBuf::from(r"C:\Program Files\nodejs"),
        });
        // scoop：apps/nodejs{,-lts} 下每个子目录是一个版本（含 current junction）。
        if let Some(home) = &home {
            for name in ["nodejs", "nodejs-lts"] {
                roots.push(ScanRoot::VersionParent {
                    source: "scoop",
                    dir: home.join("scoop").join("apps").join(name),
                    name_prefix: None,
                });
            }
        }
        // nvm-windows：NVM_HOME 或 %APPDATA%\nvm，子目录 `<version>\node.exe`。
        let nvm = std::env::var_os("NVM_HOME")
            .map(PathBuf::from)
            .or_else(|| dirs::data_dir().map(|d| d.join("nvm")));
        if let Some(dir) = nvm {
            roots.push(ScanRoot::VersionParent {
                source: "nvm-windows",
                dir,
                name_prefix: None,
            });
        }
        // fnm：FNM_DIR 或 %APPDATA%\fnm\node-versions。
        let fnm = std::env::var_os("FNM_DIR")
            .map(PathBuf::from)
            .or_else(|| dirs::data_dir().map(|d| d.join("fnm")));
        if let Some(dir) = fnm {
            roots.push(ScanRoot::VersionParent {
                source: "fnm",
                dir: dir.join("node-versions"),
                name_prefix: None,
            });
        }
        // volta：只扫 shim 目录（即 PATH 实体），不下钻 tools/image。
        let volta = std::env::var_os("VOLTA_HOME")
            .map(PathBuf::from)
            .or_else(|| home.as_ref().map(|h| h.join(".volta")));
        if let Some(dir) = volta {
            roots.push(ScanRoot::BinDir {
                source: "volta",
                dir: dir.join("bin"),
            });
        }
        // mise：MISE_DATA_DIR 或 %LOCALAPPDATA%\mise（F-03 同款默认目录）。
        let mise = std::env::var_os("MISE_DATA_DIR")
            .map(PathBuf::from)
            .or_else(|| dirs::data_local_dir().map(|d| d.join("mise")));
        if let Some(dir) = mise {
            roots.push(ScanRoot::VersionParent {
                source: "mise",
                dir: dir.join("installs").join("node"),
                name_prefix: None,
            });
        }
    } else {
        // Unix 系统位置（发行版包 / 手动安装）。
        for dir in ["/usr/local/bin", "/usr/bin"] {
            roots.push(ScanRoot::BinDir {
                source: "system",
                dir: PathBuf::from(dir),
            });
        }
        if cfg!(target_os = "macos") {
            for bin in ["/opt/homebrew/bin", "/usr/local/bin"] {
                roots.push(ScanRoot::BinDir {
                    source: "system",
                    dir: PathBuf::from(bin),
                });
            }
            // Homebrew 版本化 keg：opt 下 node / node@XX。
            for opt in ["/opt/homebrew/opt", "/usr/local/opt"] {
                roots.push(ScanRoot::VersionParent {
                    source: "homebrew",
                    dir: PathBuf::from(opt),
                    name_prefix: Some("node"),
                });
            }
        }
        // nvm（Unix 布局 versions/node/<ver>/bin/node）。
        if let Some(home) = &home {
            roots.push(ScanRoot::VersionParent {
                source: "nvm",
                dir: home.join(".nvm").join("versions").join("node"),
                name_prefix: None,
            });
        }
        // fnm：FNM_DIR / XDG / ~/.local/share/fnm / 旧版 ~/.fnm。
        let fnm = std::env::var_os("FNM_DIR")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("XDG_DATA_HOME").map(|d| PathBuf::from(d).join("fnm")))
            .or_else(|| home.as_ref().map(|h| h.join(".local/share/fnm")))
            .or_else(|| home.as_ref().map(|h| h.join(".fnm")));
        if let Some(dir) = fnm {
            roots.push(ScanRoot::VersionParent {
                source: "fnm",
                dir: dir.join("node-versions"),
                name_prefix: None,
            });
        }
        // volta shim。
        let volta = std::env::var_os("VOLTA_HOME")
            .map(PathBuf::from)
            .or_else(|| home.as_ref().map(|h| h.join(".volta")));
        if let Some(dir) = volta {
            roots.push(ScanRoot::BinDir {
                source: "volta",
                dir: dir.join("bin"),
            });
        }
        // mise：MISE_DATA_DIR / XDG_DATA_HOME/mise / ~/.local/share/mise / 旧名 ~/.mise。
        let mise = std::env::var_os("MISE_DATA_DIR")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("XDG_DATA_HOME").map(|d| PathBuf::from(d).join("mise")))
            .or_else(|| home.as_ref().map(|h| h.join(".local/share/mise")))
            .or_else(|| home.as_ref().map(|h| h.join(".mise")));
        if let Some(dir) = mise {
            roots.push(ScanRoot::VersionParent {
                source: "mise",
                dir: dir.join("installs").join("node"),
                name_prefix: None,
            });
        }
        // asdf（nodejs 插件）。
        if let Some(home) = &home {
            roots.push(ScanRoot::VersionParent {
                source: "asdf",
                dir: home.join(".asdf").join("installs").join("nodejs"),
                name_prefix: None,
            });
        }
        // tj/n：N_PREFIX 或 /usr/local，布局 `<prefix>/n/versions/node/<ver>/bin/node`。
        let n_prefix = std::env::var_os("N_PREFIX")
            .map(PathBuf::from)
            .or_else(|| Some(PathBuf::from("/usr/local")));
        if let Some(prefix) = n_prefix {
            roots.push(ScanRoot::VersionParent {
                source: "n",
                dir: prefix.join("n").join("versions").join("node"),
                name_prefix: None,
            });
        }
    }
    // 跨平台：pnpm 独立安装目录与 yarn 全局 bin。
    let pnpm_home = std::env::var_os("PNPM_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            if cfg!(target_os = "windows") {
                dirs::data_local_dir().map(|d| d.join("pnpm"))
            } else {
                home.as_ref().map(|h| h.join(".local/share/pnpm"))
            }
        });
    if let Some(dir) = pnpm_home {
        roots.push(ScanRoot::BinDir {
            source: "pnpm-home",
            dir,
        });
    }
    if let Some(home) = &home {
        roots.push(ScanRoot::BinDir {
            source: "yarn-home",
            dir: home.join(".yarn").join("bin"),
        });
    }
    roots
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::model::PackageManager;
    use std::fs;

    fn tmp_dir(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "gw_node_scan_{tag}_{}",
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ))
    }

    fn stamp(path: &Path) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, b"").unwrap();
    }

    fn node_file(dir: &Path) -> PathBuf {
        // Unix shim 与 Windows node.exe 都可能是「裸名或 .exe」；
        // find_executable_in_dirs 按平台候选序命中。
        if cfg!(windows) {
            dir.join("node.exe")
        } else {
            dir.join("node")
        }
    }

    #[test]
    fn finds_node_in_plain_and_bin_layouts() {
        let root = tmp_dir("layout");
        // 布局 A：node 在版本目录根（nvm-windows / mise-windows / volta image）。
        let a = root.join("v22.14.0");
        stamp(&node_file(&a));
        // 布局 B：node 在 <dir>/bin（nvm / mise / asdf Unix）。
        let b = root.join("v20.11.0");
        stamp(&b.join("bin").join("node"));
        let mut out = Vec::new();
        candidates_from_version_parent(&root, "test", None, &mut out);
        let nodes: Vec<_> = out
            .iter()
            .filter(|c| c.kind == NodeExecutableKind::Node)
            .collect();
        assert_eq!(nodes.len(), 2, "both layouts must be found: {out:?}");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn finds_fnm_installation_layout() {
        let root = tmp_dir("fnm");
        let version = root.join("node-versions").join("v22.14.0");
        if cfg!(windows) {
            stamp(&version.join("installation").join("node.exe"));
        } else {
            stamp(&version.join("installation").join("bin").join("node"));
        }
        let mut out = Vec::new();
        candidates_from_version_parent(&root.join("node-versions"), "fnm", None, &mut out);
        assert!(
            out.iter().any(|c| c.kind == NodeExecutableKind::Node),
            "fnm installation layout must be found: {out:?}"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn companions_are_found_next_to_node() {
        let bin = tmp_dir("companion");
        stamp(&node_file(&bin));
        // npm shim：Windows 是 .cmd，Unix 裸名（与真实发行布局一致）。
        stamp(&bin.join(if cfg!(windows) { "npm.cmd" } else { "npm" }));
        let mut out = Vec::new();
        candidates_from_install_dir(&bin, "test", &mut out);
        assert!(out.iter().any(|c| c.kind == NodeExecutableKind::Node));
        let npm = out
            .iter()
            .find(|c| c.package_manager == Some(PackageManager::Npm));
        assert!(npm.is_some(), "npm companion must be found: {out:?}");
        let _ = fs::remove_dir_all(&bin);
    }

    #[test]
    fn version_parent_prefix_filters_subdirs() {
        let root = tmp_dir("prefix");
        stamp(&node_file(&root.join("node@22").join("bin")));
        stamp(&node_file(&root.join("someother").join("bin")));
        let mut out = Vec::new();
        candidates_from_version_parent(&root, "homebrew", Some("node"), &mut out);
        let nodes: Vec<_> = out
            .iter()
            .filter(|c| c.kind == NodeExecutableKind::Node)
            .collect();
        assert_eq!(
            nodes.len(),
            1,
            "prefix filter must skip non-node subdirs: {out:?}"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn dedupe_collapses_same_path_and_keeps_first_source() {
        let dir = tmp_dir("dedupe");
        stamp(&node_file(&dir));
        let raw = vec![
            RawCandidate {
                kind: NodeExecutableKind::Node,
                package_manager: None,
                executable_path: dir.join(if cfg!(windows) { "node.exe" } else { "node" }),
                source: "system",
            },
            RawCandidate {
                kind: NodeExecutableKind::Node,
                package_manager: None,
                executable_path: dir.join(if cfg!(windows) { "node.exe" } else { "node" }),
                source: "homebrew",
            },
        ];
        let deduped = dedupe(raw);
        assert_eq!(deduped.len(), 1, "same path must collapse to one candidate");
        assert_eq!(deduped[0].source, "system", "first source wins");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn normalize_key_unifies_separators_and_case() {
        assert_eq!(
            normalize_path_key("C:\\Users\\Me\\node.exe"),
            normalize_path_key("c:/users/me/node.exe")
        );
    }

    /// 真实环境冒烟：本机扫描至少命中一个可探测版本的 node（无则 skip）。
    #[test]
    fn scan_finds_node_on_real_machine() {
        let candidates = scan_node_toolchain();
        eprintln!("N-10 scan candidates: {candidates:?}");
        let valid_nodes: Vec<_> = candidates
            .iter()
            .filter(|c| c.kind == NodeExecutableKind::Node && c.probe_ok)
            .collect();
        if valid_nodes.is_empty() {
            eprintln!("N-10: no node found by scan on this machine; skipping real smoke");
            return;
        }
        assert!(
            valid_nodes.iter().all(|c| c.version.is_some()),
            "probe_ok candidates must carry a version"
        );
    }
}
