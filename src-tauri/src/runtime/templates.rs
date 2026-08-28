//! Runtime Templates（R-19，§83）：配置模板降低重复配置成本。
//!
//! - 模板模型与 R-07 配置同构（复用 [`RuntimeApplicationConfig`]）+ 模板
//!   元信息（名称 / 描述 / 适用类型）；模板只做「创建时预填」，不与已创建
//!   配置保持联动（避免模板改动级联影响存量配置）；
//! - 内置模板随代码版本提供（`Spring Boot Development`，§83）；用户自定义
//!   模板存 `.gitworkspace/templates/<name>.json`（可 Git 版本化、团队共享，
//!   同 R-07 约定）；
//! - **同名遮蔽**：用户自定义模板与内置模板同名时，列表与应用均以用户文件
//!   为准——内置模板升级不会覆盖用户同名自定义模板（验收标准 4）。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use crate::runtime::config::RuntimeApplicationConfig;

pub const TEMPLATE_SCHEMA_VERSION: u32 = 1;
pub const TEMPLATES_DIR: &str = "templates";

/// 一个 Runtime 配置模板（§83）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeTemplate {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    /// 模板名（workspace 内唯一；与应用配置名无关）。
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    /// 适用类型（如 `spring-boot` / `maven-jar`）；展示用。
    #[serde(default)]
    pub applies_to: Option<String>,
    /// true = 代码内置模板（无用户文件时列出；不可删除）。
    #[serde(default)]
    pub builtin: bool,
    /// 预填配置（name / project 通常留空，应用时由用户填写）。
    pub config: RuntimeApplicationConfig,
}

fn default_schema_version() -> u32 {
    TEMPLATE_SCHEMA_VERSION
}

impl RuntimeTemplate {
    /// 模板载荷校验：与 R-07 配置加载同一套规则（环境变量 / 健康检查 /
    /// schema 版本），但允许 name / project 为空（应用时填写）。
    pub fn validate(&self) -> AppResult<()> {
        let name = self.name.trim();
        if name.is_empty() || name != self.name {
            return Err(AppError::RuntimeConfig(format!(
                "模板名称 '{}' 非法：不能为空且不含首尾空格",
                self.name
            )));
        }
        if self.schema_version > TEMPLATE_SCHEMA_VERSION {
            return Err(AppError::RuntimeConfig(format!(
                "模板 {} 使用了不受支持的 schemaVersion={}（当前支持到 {}）",
                self.name, self.schema_version, TEMPLATE_SCHEMA_VERSION
            )));
        }
        // 环境变量 key 合法性与敏感 key 无需在此拦截（R-07 保存配置时校验；
        // 模板只预填，应用时走 create_config 全量校验）。这里校验健康检查
        // 配置这类结构化字段。
        if let Some(health) = &self.config.health_check {
            health.validate()?;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 内置模板（§83）
// ---------------------------------------------------------------------------

/// 内置模板集合。随版本升级可调整；被用户同名文件遮蔽。
pub fn builtin_templates() -> Vec<RuntimeTemplate> {
    vec![RuntimeTemplate {
        schema_version: TEMPLATE_SCHEMA_VERSION,
        name: "Spring Boot Development".into(),
        description: Some(
            "Spring Boot 开发默认：JDK 21 / dev profile / 堆内存 512m-2048m / DevTools 重启开启".into(),
        ),
        applies_to: Some("spring-boot".into()),
        builtin: true,
        config: RuntimeApplicationConfig {
            schema_version: TEMPLATE_SCHEMA_VERSION,
            name: String::new(),
            project: String::new(),
            main_class: None,
            jdk: Some("21".into()),
            profile: Some("dev".into()),
            vm_options: vec![
                "-Xms512m".into(),
                "-Xmx2048m".into(),
                // DevTools（若类路径存在）自动重启开关（§83 模板项）。
                "-Dspring.devtools.restart.enabled=true".into(),
            ],
            program_arguments: vec![],
            environment: Default::default(),
            runtime_environment: Default::default(),
            build_engine: Some("maven".into()),
            scope: crate::maven::RuntimeScope::Auto,
            pre_build_script: None,
            post_build_script: None,
            health_check: None,
            auto_restart: None,
        },
    }]
}

// ---------------------------------------------------------------------------
// 存储（.gitworkspace/templates/<name>.json，用户自定义）
// ---------------------------------------------------------------------------

pub fn templates_dir(workspace_root: &Path) -> PathBuf {
    workspace_root.join(".gitworkspace").join(TEMPLATES_DIR)
}

fn template_path(workspace_root: &Path, name: &str) -> AppResult<PathBuf> {
    validate_template_name(name)?;
    Ok(templates_dir(workspace_root).join(format!("{name}.json")))
}

/// 模板名校验（同 Runtime 名称口径：禁路径分隔符与 Windows 保留字符）。
fn validate_template_name(name: &str) -> AppResult<()> {
    let trimmed = name.trim();
    if trimmed.is_empty()
        || trimmed == "."
        || trimmed == ".."
        || trimmed != name
        || name.len() > 128
        || name
            .chars()
            .any(|c| c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'))
    {
        return Err(AppError::RuntimeConfig(format!(
            "模板名称 '{}' 不能用作配置文件名；请移除首尾空格、路径分隔符或 Windows 保留字符",
            name
        )));
    }
    Ok(())
}

/// 列出全部模板：用户文件 + 未被遮蔽的内置模板（按名称排序）。
pub fn list_templates(workspace_root: &Path) -> Vec<RuntimeTemplate> {
    let mut templates: Vec<RuntimeTemplate> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(templates_dir(workspace_root)) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            match std::fs::read_to_string(&path)
                .map_err(AppError::from)
                .and_then(|text| serde_json::from_str::<RuntimeTemplate>(&text).map_err(Into::into))
            {
                Ok(mut template) => {
                    // 文件里 builtin 恒为 false（用户文件；save 时强制）。
                    template.builtin = false;
                    templates.push(template);
                }
                Err(e) => {
                    log::warn!("R-19: skipping invalid template file {}: {e}", path.display())
                }
            }
        }
    }
    let user_names: std::collections::BTreeSet<String> =
        templates.iter().map(|t| t.name.clone()).collect();
    for builtin in builtin_templates() {
        if !user_names.contains(&builtin.name) {
            templates.push(builtin);
        }
    }
    templates.sort_by(|a, b| a.name.cmp(&b.name));
    templates
}

/// 读取一个模板：先用户文件，回落内置；都不存在返回 NotFound。
pub fn get_template(workspace_root: &Path, name: &str) -> AppResult<RuntimeTemplate> {
    validate_template_name(name)?;
    let path = templates_dir(workspace_root).join(format!("{name}.json"));
    if let Ok(text) = std::fs::read_to_string(&path) {
        let mut template: RuntimeTemplate = serde_json::from_str(&text)?;
        template.builtin = false;
        return Ok(template);
    }
    builtin_templates()
        .into_iter()
        .find(|t| t.name == name)
        .ok_or_else(|| AppError::NotFound(format!("模板 '{name}' 不存在")))
}

/// 保存用户模板（创建或覆盖）。IPC 传入的 `builtin` 标记被忽略——写盘的
/// 都是用户模板（内置模板由代码提供）。
pub fn save_template(workspace_root: &Path, template: &RuntimeTemplate) -> AppResult<RuntimeTemplate> {
    let mut template = template.clone();
    template.builtin = false;
    template.validate()?;
    let path = template_path(workspace_root, &template.name)?;
    let dir = templates_dir(workspace_root);
    crate::runtime::guard::assert_workspace_write_path(&dir, workspace_root, "Template 配置目录")?;
    std::fs::create_dir_all(&dir)?;
    crate::runtime::config::write_json_atomic(&path, &template)?;
    log::info!("R-19: template '{}' saved to {}", template.name, path.display());
    Ok(template)
}

/// 删除用户模板；内置模板（无文件）返回可行动错误。
pub fn delete_template(workspace_root: &Path, name: &str) -> AppResult<()> {
    let path = template_path(workspace_root, name)?;
    if !path.exists() {
        let is_builtin = builtin_templates().iter().any(|t| t.name == name);
        return Err(AppError::NotFound(if is_builtin {
            format!("模板 '{name}' 是内置模板，不能删除；如需同名版本请新建用户模板（会自动遮蔽内置）")
        } else {
            format!("模板 '{name}' 不存在")
        }));
    }
    std::fs::remove_file(&path)?;
    log::info!("R-19: template '{name}' deleted");
    Ok(())
}

/// R-19「另存为模板」：把现有 Runtime 配置存为模板。配置由调用方经
/// `load_config_unredacted` 读取（与构建/启动同源）；模板文件与 Runtime
/// 配置同目录约定（`.gitworkspace/`），是否提交 Git 由用户决定。
pub fn save_config_as_template(
    workspace_root: &Path,
    config: RuntimeApplicationConfig,
    template_name: &str,
    description: Option<String>,
) -> AppResult<RuntimeTemplate> {
    let mut payload = config;
    // 模板载荷不绑定具体应用名/项目——应用时由用户填写。
    payload.name = String::new();
    payload.project = String::new();
    let template = RuntimeTemplate {
        schema_version: TEMPLATE_SCHEMA_VERSION,
        name: template_name.to_string(),
        description,
        applies_to: Some("spring-boot".into()),
        builtin: false,
        config: payload,
    };
    save_template(workspace_root, &template)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "gw_r19_{}",
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn template(name: &str) -> RuntimeTemplate {
        RuntimeTemplate {
            schema_version: TEMPLATE_SCHEMA_VERSION,
            name: name.into(),
            description: None,
            applies_to: Some("spring-boot".into()),
            builtin: false,
            config: RuntimeApplicationConfig {
                jdk: Some("17".into()),
                vm_options: vec!["-Xmx1g".into()],
                ..Default::default()
            },
        }
    }

    #[test]
    fn builtin_templates_exist_and_validate() {
        let builtins = builtin_templates();
        assert!(builtins.iter().any(|t| t.name == "Spring Boot Development"));
        for t in &builtins {
            t.validate().unwrap();
        }
        let boot = builtin_templates()
            .into_iter()
            .find(|t| t.name == "Spring Boot Development")
            .unwrap();
        // §83：JDK 21 / dev profile / -Xms512m -Xmx2048m / DevTools。
        assert_eq!(boot.config.jdk.as_deref(), Some("21"));
        assert_eq!(boot.config.profile.as_deref(), Some("dev"));
        assert!(boot.config.vm_options.contains(&"-Xms512m".into()));
        assert!(boot.config.vm_options.contains(&"-Xmx2048m".into()));
        assert!(boot
            .config
            .vm_options
            .contains(&"-Dspring.devtools.restart.enabled=true".into()));
    }

    #[test]
    fn user_template_shadows_builtin_and_survives_upgrade() {
        let ws = root();
        // 内置先在列表里。
        assert!(list_templates(&ws).iter().any(|t| t.name == "Spring Boot Development"));
        // 用户创建同名模板（自定义内容）→ 遮蔽内置。
        let mut user = template("Spring Boot Development");
        user.config.jdk = Some("17".into());
        save_template(&ws, &user).unwrap();
        let listed = list_templates(&ws);
        assert_eq!(
            listed.iter().filter(|t| t.name == "Spring Boot Development").count(),
            1,
            "user file must shadow the builtin, not duplicate it"
        );
        let loaded = get_template(&ws, "Spring Boot Development").unwrap();
        assert_eq!(loaded.config.jdk.as_deref(), Some("17"));
        assert!(!loaded.builtin, "user file always lists as non-builtin");
        // 「内置升级」= builtin_templates() 变化；用户文件不被触碰（上面已验证）。
        let _ = std::fs::remove_dir_all(ws);
    }

    #[test]
    fn template_crud_roundtrip_and_builtin_delete_guard() {
        let ws = root();
        save_template(&ws, &template("my-template")).unwrap();
        assert_eq!(get_template(&ws, "my-template").unwrap().name, "my-template");
        assert!(list_templates(&ws).iter().any(|t| t.name == "my-template"));

        delete_template(&ws, "my-template").unwrap();
        assert!(get_template(&ws, "my-template").is_err());

        // 内置模板不可删除。
        let error = delete_template(&ws, "Spring Boot Development").unwrap_err();
        assert_eq!(error.code(), "RepositoryError");
        assert!(error.to_string().contains("内置"));
        let _ = std::fs::remove_dir_all(ws);
    }

    #[test]
    fn template_name_rejects_path_traversal_and_ipc_cannot_write_builtin() {
        let ws = root();
        assert_eq!(
            get_template(&ws, "../evil").unwrap_err().code(),
            "RuntimeConfigError"
        );
        // IPC 传入 builtin=true 也强制落为用户模板（builtin 只由代码提供）。
        let mut forced = template("forced-builtin");
        forced.builtin = true;
        let saved = save_template(&ws, &forced).unwrap();
        assert!(!saved.builtin);
        let _ = std::fs::remove_dir_all(ws);
    }

    #[test]
    fn save_as_template_strips_identity_and_reapplies() {
        let ws = root();
        let mut config = RuntimeApplicationConfig::default();
        config.name = "boot".into();
        config.project = "/ws/repo-boot/pom.xml".into();
        config.jdk = Some("21".into());
        config.profile = Some("dev".into());
        config.vm_options = vec!["-Xmx1024m".into()];

        let template = save_config_as_template(&ws, config, "from-boot", Some("另存".into())).unwrap();
        // 身份字段剥离：模板不绑定应用名 / 项目。
        assert_eq!(template.config.name, "");
        assert_eq!(template.config.project, "");
        assert_eq!(template.config.jdk.as_deref(), Some("21"));

        // 应用：填名 + 项目后即是一份合法的新配置。
        let mut applied = template.config.clone();
        applied.name = "boot2".into();
        applied.project = "/ws/repo-other/pom.xml".into();
        assert_eq!(applied.jdk.as_deref(), Some("21"));
        assert!(applied.vm_options.contains(&"-Xmx1024m".into()));
        let _ = std::fs::remove_dir_all(ws);
    }
}
