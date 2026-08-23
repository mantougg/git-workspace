//! Runtime 配置中的 JDK 指定字符串解析（R-09 Build 流水线的 Validate JDK 步骤）。
//!
//! Runtime 配置的 `jdk` 字段是用户可读字符串（如 `"21"` 或 JDK home 路径），
//! 这里把它解析为注册表中的 [`JdkInstallation`]。匹配顺序：
//!
//! 1. `home_path` 精确匹配；
//! 2. 前导数字按 `major_version` 匹配，取注册表排序后的第一个
//!    （`list_jdks` 已按「有效优先 + major 降序」排序，即最新的有效 JDK）；
//! 3. 都未命中 → `JdkNotFound` 可行动错误（列出当前可用 JDK 供用户修正）。

use rusqlite::Connection;

use crate::error::{AppError, AppResult};
use crate::java::model::JdkInstallation;
use crate::java::registry::list_jdks;

/// 把 Runtime 配置的 `jdk` 字符串解析为注册表中的 JDK 安装。
pub fn resolve_jdk_for_config(conn: &Connection, spec: &str) -> AppResult<JdkInstallation> {
    let spec = spec.trim();
    let jdks = list_jdks(conn)?;

    // 1. home_path 精确匹配（用户从 Settings 选择了具体 JDK 目录）。
    if let Some(jdk) = jdks.iter().find(|jdk| jdk.home_path == spec) {
        return Ok(jdk.clone());
    }

    // 2. 前导数字 → major_version（如 "21" / "17.0.12" 都取 21 / 17）。
    let major: Option<u32> = spec
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>()
        .parse()
        .ok();
    if let Some(major) = major {
        if let Some(jdk) = jdks
            .iter()
            .find(|jdk| jdk.is_valid && jdk.major_version == Some(major))
        {
            return Ok(jdk.clone());
        }
    }

    // 3. 可行动错误：列出可用 JDK，指向修正路径。
    let available = if jdks.is_empty() {
        "（注册表为空）".to_string()
    } else {
        jdks
            .iter()
            .map(|jdk| {
                format!(
                    "{} [{}]",
                    jdk.home_path,
                    jdk.major_version
                        .map(|v| v.to_string())
                        .unwrap_or_else(|| "未知版本".into())
                )
            })
            .collect::<Vec<_>>()
            .join("、")
    };
    Err(AppError::JdkNotFound(format!(
        "Runtime 配置指定的 JDK '{spec}' 未找到。当前可用 JDK：{available}。\
         请先在 Settings 中扫描/添加 JDK，或修正 Runtime 配置的 jdk 字段"
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::java::model::JdkDiscoverySource;

    fn open_db() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        crate::db::init_db(&mut conn).unwrap();
        conn
    }

    fn jdk(home: &str, major: u32, valid: bool) -> JdkInstallation {
        let mut jdk = JdkInstallation::new(home, JdkDiscoverySource::System);
        jdk.major_version = Some(major);
        jdk.is_valid = valid;
        jdk
    }

    #[test]
    fn resolves_by_exact_home_path() {
        let conn = open_db();
        crate::java::registry::upsert_jdk(&conn, &jdk("/jdk-17", 17, true)).unwrap();
        crate::java::registry::upsert_jdk(&conn, &jdk("/jdk-21", 21, true)).unwrap();

        let resolved = resolve_jdk_for_config(&conn, "/jdk-17").unwrap();
        assert_eq!(resolved.home_path, "/jdk-17");
    }

    #[test]
    fn resolves_by_leading_major_version() {
        let conn = open_db();
        crate::java::registry::upsert_jdk(&conn, &jdk("/jdk-17", 17, true)).unwrap();
        crate::java::registry::upsert_jdk(&conn, &jdk("/jdk-21", 21, true)).unwrap();

        let resolved = resolve_jdk_for_config(&conn, "21").unwrap();
        assert_eq!(resolved.home_path, "/jdk-21");
        // 带小数的完整版本串也按前导数字匹配。
        let resolved = resolve_jdk_for_config(&conn, "17.0.12").unwrap();
        assert_eq!(resolved.home_path, "/jdk-17");
    }

    #[test]
    fn invalid_jdk_is_not_matched_by_major() {
        let conn = open_db();
        crate::java::registry::upsert_jdk(&conn, &jdk("/jdk-broken", 21, false)).unwrap();

        let error = resolve_jdk_for_config(&conn, "21").unwrap_err();
        assert_eq!(error.code(), "JdkNotFound");
        assert!(error.to_string().contains("21"));
        assert!(error.to_string().contains("/jdk-broken"));
    }

    #[test]
    fn unknown_spec_reports_available_jdks() {
        let conn = open_db();
        crate::java::registry::upsert_jdk(&conn, &jdk("/jdk-17", 17, true)).unwrap();

        let error = resolve_jdk_for_config(&conn, "8").unwrap_err();
        assert_eq!(error.code(), "JdkNotFound");
        assert!(error.to_string().contains("/jdk-17"));
    }
}
