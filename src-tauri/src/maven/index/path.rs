//! Maven Index 路径归一化（R-02，B-04 拆分）。
//!
//! DB 中存储的路径统一为正斜杠（`path_key`）；Windows verbatim 前缀
//! （`\\?\` / `\\?\UNC\`）在展示与比较前清理。所有路径比较必须经过
//! 归一化，禁止裸 `==`（平台规范 §1）。verbatim 剥离的公共实现在
//! [`crate::pathutil`]。

use std::path::{Path, PathBuf};

pub(super) fn path_key(path: &Path) -> String {
    let normalized = std::fs::canonicalize(path).unwrap_or_else(|_| lexical_normalize(path));
    crate::pathutil::strip_windows_verbatim_prefix(&normalized.to_string_lossy()).replace('\\', "/")
}

fn lexical_normalize(path: &Path) -> PathBuf {
    use std::path::Component;

    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() && !path.has_root() {
                    normalized.push("..");
                }
            }
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => {
                normalized.push(component.as_os_str());
            }
        }
    }
    normalized
}
