//! Maven `settings.xml` 解析（R-05，§18 本地仓库路径探测）。
//!
//! 探测默认本地仓库路径：默认 `~/.m2/repository` + 解析 `settings.xml` 的
//! `localRepository` 覆盖。`settings.xml` 位置（§18）：用户级
//! `~/.m2/settings.xml` 优先，全局级 `${maven.home}/conf/settings.xml` 回退。
//!
//! 解析为纯本地 XML 提取，不调用 Maven、不发起网络请求（全局约束 §10）。
//! 复杂 settings（profile / mirror / server）不在此解析——交给 `mvn` 自身
//! （全局约束 §1）；这里只取 `localRepository` 供依赖分析使用。

use std::path::{Path, PathBuf};

/// 默认本地仓库路径：`~/.m2/repository`（§18）。
///
/// 与 R-02 `resolver::default_local_repository` 一致，这里重新导出以保持
/// R-05 模块自洽（同一逻辑、单一来源语义）。
pub fn default_local_repository() -> PathBuf {
    crate::maven::resolver::default_local_repository()
}

/// 探测生效的本地仓库路径：先查 `settings.xml` 的 `localRepository`，没有则
/// 回退 `~/.m2/repository`。
///
/// `global_settings_path` 为全局 `settings.xml`（`${maven.home}/conf/settings.xml`），
/// 可选；用户级 `~/.m2/settings.xml` 优先于全局级。
pub fn resolve_local_repository(global_settings_path: Option<&Path>) -> PathBuf {
    // 用户级 settings.xml 优先。
    if let Some(user_settings) = user_settings_path() {
        if user_settings.is_file() {
            if let Some(local) = parse_local_repository(&user_settings) {
                return expand_path(&local);
            }
        }
    }
    // 全局级 settings.xml 回退。
    if let Some(global) = global_settings_path {
        if global.is_file() {
            if let Some(local) = parse_local_repository(global) {
                return expand_path(&local);
            }
        }
    }
    default_local_repository()
}

/// 用户级 `settings.xml` 路径：`~/.m2/settings.xml`。
pub fn user_settings_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".m2").join("settings.xml"))
}

/// 从 `settings.xml` 内容中提取 `<localRepository>...</localRepository>`。
///
/// 用朴素字符串/正则提取而非完整 XML 解析（避免引入 XML 依赖；Maven
/// `settings.xml` 的 `localRepository` 是简单文本元素，§18）。宽容空白与
/// 注释；找不到返回 `None`。
pub fn parse_local_repository(settings_path: &Path) -> Option<String> {
    let content = std::fs::read_to_string(settings_path).ok()?;
    extract_local_repository_text(&content)
}

/// 从 settings.xml 文本中提取 localRepository 值（纯函数，便于单测）。
fn extract_local_repository_text(content: &str) -> Option<String> {
    // 移除 XML 注释，避免注释中的 localRepository 被误取。
    let stripped = strip_xml_comments(content);
    let open = stripped.find("<localRepository>")?;
    let after_open = &stripped[open + "<localRepository>".len()..];
    let close = after_open.find("</localRepository>")?;
    let value = after_open[..close].trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

/// 移除 `<!-- ... -->` 注释。未闭合注释的剩余内容一并丢弃。
fn strip_xml_comments(content: &str) -> String {
    let mut out = String::with_capacity(content.len());
    let mut rest = content;
    while let Some(start) = rest.find("<!--") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 4..];
        match after.find("-->") {
            Some(end) => rest = &after[end + 3..],
            None => {
                // 未闭合注释：丢弃剩余内容。
                return out;
            }
        }
    }
    out.push_str(rest);
    out
}

/// 展开路径中的 `~` / `${user.home}` 为真实 home 目录。
fn expand_path(path: &str) -> PathBuf {
    let expanded = if let Some(rest) = path.strip_prefix("~/") {
        match dirs::home_dir() {
            Some(home) => home.join(rest),
            None => PathBuf::from(path),
        }
    } else if path == "~" {
        dirs::home_dir().unwrap_or_default()
    } else if let Some(rest) = path.strip_prefix("${user.home}/") {
        match dirs::home_dir() {
            Some(home) => home.join(rest),
            None => PathBuf::from(path),
        }
    } else {
        PathBuf::from(path)
    };
    expanded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_local_repository_plain() {
        let xml = r#"<settings>
            <localRepository>/custom/m2/repo</localRepository>
            <mirrors></mirrors>
        </settings>"#;
        assert_eq!(
            extract_local_repository_text(xml).as_deref(),
            Some("/custom/m2/repo")
        );
    }

    #[test]
    fn ignores_commented_local_repository() {
        let xml = r#"<settings>
            <!-- <localRepository>/should/not/use</localRepository> -->
            <localRepository>/real/repo</localRepository>
        </settings>"#;
        assert_eq!(
            extract_local_repository_text(xml).as_deref(),
            Some("/real/repo")
        );
    }

    #[test]
    fn returns_none_when_absent() {
        let xml = r#"<settings><mirrors></mirrors></settings>"#;
        assert!(extract_local_repository_text(xml).is_none());
    }

    #[test]
    fn returns_none_for_empty_value() {
        let xml = r#"<settings><localRepository>   </localRepository></settings>"#;
        assert!(extract_local_repository_text(xml).is_none());
    }

    #[test]
    fn expand_tilde_to_home() {
        let expanded = expand_path("~/custom");
        if let Some(home) = dirs::home_dir() {
            assert_eq!(expanded, home.join("custom"));
        }
    }

    #[test]
    fn expand_user_home_variable() {
        let expanded = expand_path("${user.home}/.m2/custom-repo");
        if let Some(home) = dirs::home_dir() {
            assert_eq!(expanded, home.join(".m2").join("custom-repo"));
        }
    }

    #[test]
    fn leaves_absolute_path_unchanged() {
        let expanded = expand_path("/opt/maven-repo");
        assert_eq!(expanded, PathBuf::from("/opt/maven-repo"));
    }

    #[test]
    fn strip_comments_handles_unclosed() {
        let content = "<!-- never closed";
        let stripped = strip_xml_comments(content);
        assert_eq!(stripped, "");
    }

    #[test]
    fn default_local_repository_is_home_m2() {
        let def = default_local_repository();
        if let Some(home) = dirs::home_dir() {
            assert_eq!(def, home.join(".m2").join("repository"));
        }
    }

    #[test]
    fn resolve_falls_back_to_default_without_settings() {
        // 临时目录下无 settings.xml -> 回退默认。
        let tmp = std::env::temp_dir().join(format!(
            "gw_mvn_settings_{}",
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        std::fs::create_dir_all(&tmp).unwrap();
        let resolved = resolve_local_repository(Some(&tmp.join("settings.xml")));
        assert_eq!(resolved, default_local_repository());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn resolve_uses_settings_when_present() {
        let tmp = std::env::temp_dir().join(format!(
            "gw_mvn_settings2_{}",
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        std::fs::create_dir_all(&tmp).unwrap();
        let settings = tmp.join("settings.xml");
        std::fs::write(
            &settings,
            r#"<settings><localRepository>/from/settings/repo</localRepository></settings>"#,
        )
        .unwrap();
        let resolved = resolve_local_repository(Some(&settings));
        assert_eq!(resolved, PathBuf::from("/from/settings/repo"));
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
