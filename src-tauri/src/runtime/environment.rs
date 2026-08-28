//! Multi-Service Runtime 与 Runtime Environment（R-15，§38/§39/§40/§82/§84）。
//!
//! 职责：
//! - [`RuntimeEnvironment`] 模型：环境名 + 服务列表；每个服务引用已有
//!   Runtime 配置，只存**覆盖项**（JDK / Profile / 环境变量 / 端口 / 外部
//!   服务备注），避免配置双份漂移（§82/§84）；
//! - 持久化：`.gitworkspace/environments/<name>.json`（可 Git 版本化、团队
//!   共享；用户项目只读护栏经 [`guard::assert_workspace_write_path`]）；
//! - 服务依赖（§39）：`depends_on` 声明，运行时拓扑排序分「波次」——
//!   同波无依赖关系的服务并行启动，波间严格串行；环依赖在保存与启动前
//!   都会被拒绝（`RuntimeConfig` 可行动错误）。
//!
//! 编排执行（Start/Stop Environment）在 [`crate::runtime::service`] 的
//! `exec_start_environment` / `exec_stop_environment`：本模块只提供纯模型
//! 与排序逻辑（单测友好），以及就绪等待的判定函数。

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};

pub const ENVIRONMENT_SCHEMA_VERSION: u32 = 1;
/// 环境配置目录（`.gitworkspace/environments/`）。
pub const ENVIRONMENTS_DIR: &str = "environments";

/// 一个多服务环境（§82 Runtime Environment）。名称对齐典型场景：
/// Local / Development / Test / Demo。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeEnvironment {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    /// 服务列表（顺序不影响启动顺序——拓扑排序运行时计算）。
    pub services: Vec<EnvironmentService>,
}

fn default_schema_version() -> u32 {
    ENVIRONMENT_SCHEMA_VERSION
}

/// 环境内的一个服务（§39 Service Dependency）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentService {
    /// 引用的 Runtime 配置名（必须存在于 workspace 的 Runtime 配置中）。
    pub runtime_name: String,
    /// 依赖的其他服务（环境内 runtime_name 列表；拓扑排序决定启动顺序）。
    #[serde(default)]
    pub depends_on: Vec<String>,
    // ---- 覆盖项（只存与 Runtime 配置不同的部分；None = 跟随配置）----
    #[serde(default)]
    pub jdk: Option<String>,
    #[serde(default)]
    pub profile: Option<String>,
    /// 追加到五层环境之上的环境变量（Application 层之后合并）。
    #[serde(default)]
    pub environment: BTreeMap<String, String>,
    /// 服务端口覆盖（注入 `--server.port=`；None 跟随配置）。
    #[serde(default)]
    pub port: Option<u16>,
    /// 外部服务备注（§82：数据库 / 中间件等非本工作区服务的说明）。
    #[serde(default)]
    pub external_notes: Option<String>,
    /// 就绪等待超时（秒）；缺省 [`DEFAULT_READY_TIMEOUT_SECS`]。
    #[serde(default)]
    pub ready_timeout_seconds: Option<u64>,
}

/// 就绪等待默认超时：60s（Healthy 门限未就绪按警告继续，见编排语义）。
pub const DEFAULT_READY_TIMEOUT_SECS: u64 = 60;

impl RuntimeEnvironment {
    pub fn validate(&self) -> AppResult<()> {
        let name = self.name.trim();
        if name.is_empty() || name != self.name || name.len() > 128 {
            return Err(AppError::RuntimeConfig(format!(
                "环境名称 '{}' 非法：不能为空、不含首尾空格且长度 ≤ 128",
                self.name
            )));
        }
        if self.services.is_empty() {
            return Err(AppError::RuntimeConfig(format!(
                "环境 '{name}' 至少需要一个服务"
            )));
        }
        let mut seen = BTreeSet::new();
        for service in &self.services {
            if service.runtime_name.trim().is_empty() {
                return Err(AppError::RuntimeConfig(format!(
                    "环境 '{name}' 存在空的 runtime_name"
                )));
            }
            if !seen.insert(service.runtime_name.as_str()) {
                return Err(AppError::RuntimeConfig(format!(
                    "环境 '{name}' 中服务 '{}' 重复",
                    service.runtime_name
                )));
            }
            if service.depends_on.contains(&service.runtime_name) {
                return Err(AppError::RuntimeConfig(format!(
                    "环境 '{name}' 中服务 '{}' 依赖自身（环依赖）",
                    service.runtime_name
                )));
            }
        }
        for service in &self.services {
            for dep in &service.depends_on {
                if !seen.contains(dep.as_str()) {
                    return Err(AppError::RuntimeConfig(format!(
                        "环境 '{name}' 中服务 '{}' 依赖了不存在的服务 '{}'；\
                         依赖目标必须是环境内的 runtime_name",
                        service.runtime_name, dep
                    )));
                }
            }
        }
        // 环检测（拓扑排序即可复用；validate 阶段提前报错）。
        topo_sort_services(self)?;
        Ok(())
    }
}

/// 服务依赖拓扑排序（§39 → §40 Parallel Start）。
///
/// 返回「波次」：`waves[0]` 是无依赖服务（并行启动），`waves[k]` 的服务
/// 只依赖 `< k` 波的服务（波间严格串行）。环依赖返回 `RuntimeConfig`
/// 可行动错误（附带成环服务）。
pub fn topo_sort_services(environment: &RuntimeEnvironment) -> AppResult<Vec<Vec<String>>> {
    let mut indegree: BTreeMap<&str, usize> = BTreeMap::new();
    let mut dependents: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for service in &environment.services {
        indegree.entry(service.runtime_name.as_str()).or_insert(0);
        for dep in &service.depends_on {
            // validate 已保证 dep 存在；编排路径直接跑也防一手。
            let Some(target) = environment
                .services
                .iter()
                .find(|s| s.runtime_name == *dep)
                .map(|s| s.runtime_name.as_str())
            else {
                return Err(AppError::RuntimeConfig(format!(
                    "服务 '{}' 依赖了不存在的服务 '{}'",
                    service.runtime_name, dep
                )));
            };
            *indegree.entry(service.runtime_name.as_str()).or_insert(0) += 1;
            dependents.entry(target).or_default().push(service.runtime_name.as_str());
        }
    }

    let mut waves = Vec::new();
    let mut ready: VecDeque<&str> = indegree
        .iter()
        .filter(|(_, &d)| d == 0)
        .map(|(&name, _)| name)
        .collect();
    let mut resolved = 0usize;
    while !ready.is_empty() {
        let mut wave = Vec::new();
        let mut next = VecDeque::new();
        while let Some(name) = ready.pop_front() {
            wave.push(name.to_string());
            resolved += 1;
            if let Some(children) = dependents.get(name) {
                for child in children {
                    let degree = indegree.get_mut(child).expect("child known");
                    *degree -= 1;
                    if *degree == 0 {
                        next.push_back(*child);
                    }
                }
            }
        }
        waves.push(wave);
        ready = next;
    }
    if resolved != environment.services.len() {
        let stuck: Vec<String> = indegree
            .iter()
            .filter(|(_, &d)| d > 0)
            .map(|(&name, _)| name.to_string())
            .collect();
        return Err(AppError::RuntimeConfig(format!(
            "环境 '{}' 存在环依赖，无法确定启动顺序；涉及服务：{}",
            environment.name,
            stuck.join(", ")
        )));
    }
    Ok(waves)
}

// ---------------------------------------------------------------------------
// 持久化（.gitworkspace/environments/<name>.json，可 Git 版本化）
// ---------------------------------------------------------------------------

pub fn environments_dir(workspace_root: &Path) -> PathBuf {
    workspace_root
        .join(".gitworkspace")
        .join(ENVIRONMENTS_DIR)
}

fn environment_path(workspace_root: &Path, name: &str) -> AppResult<PathBuf> {
    validate_environment_name(name)?;
    let dir = environments_dir(workspace_root);
    Ok(dir.join(format!("{name}.json")))
}

/// 环境名校验（同 Runtime 名称口径：禁路径分隔符与 Windows 保留字符）。
fn validate_environment_name(name: &str) -> AppResult<()> {
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
            "环境名称 '{}' 不能用作配置文件名；请移除首尾空格、路径分隔符或 Windows 保留字符",
            name
        )));
    }
    Ok(())
}

fn ensure_environments_dir(workspace_root: &Path) -> AppResult<PathBuf> {
    let dir = environments_dir(workspace_root);
    crate::runtime::guard::assert_workspace_write_path(&dir, workspace_root, "Environment 配置目录")?;
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// 列出 workspace 全部环境（读目录，按名称排序；解析失败的文件跳过并告警，
/// 不阻塞列表）。
pub fn list_environments(workspace_root: &Path) -> Vec<RuntimeEnvironment> {
    let dir = environments_dir(workspace_root);
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut environments = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        match std::fs::read_to_string(&path)
            .map_err(AppError::from)
            .and_then(|text| serde_json::from_str::<RuntimeEnvironment>(&text).map_err(Into::into))
        {
            Ok(environment) => environments.push(environment),
            Err(e) => log::warn!(
                "R-15: skipping invalid environment file {}: {e}",
                path.display()
            ),
        }
    }
    environments.sort_by(|a, b| a.name.cmp(&b.name));
    environments
}

/// 读取单个环境；不存在返回 NotFound。
pub fn get_environment(workspace_root: &Path, name: &str) -> AppResult<RuntimeEnvironment> {
    validate_environment_name(name)?;
    let path = environments_dir(workspace_root).join(format!("{name}.json"));
    let text = std::fs::read_to_string(&path).map_err(|e| {
        AppError::NotFound(format!(
            "环境 '{name}' 不存在（{}：{e}）",
            path.display()
        ))
    })?;
    let environment: RuntimeEnvironment = serde_json::from_str(&text)?;
    Ok(environment)
}

/// 保存（创建或覆盖）环境。先校验模型与依赖图，再原子写盘。
pub fn save_environment(
    workspace_root: &Path,
    environment: &RuntimeEnvironment,
) -> AppResult<RuntimeEnvironment> {
    environment.validate()?;
    let path = environment_path(workspace_root, &environment.name)?;
    ensure_environments_dir(workspace_root)?;
    crate::runtime::config::write_json_atomic(&path, &environment)?;
    log::info!("R-15: environment '{}' saved to {}", environment.name, path.display());
    Ok(environment.clone())
}

/// 删除环境；不存在返回 NotFound。
pub fn delete_environment(workspace_root: &Path, name: &str) -> AppResult<()> {
    let path = environment_path(workspace_root, name)?;
    std::fs::remove_file(&path).map_err(|e| {
        AppError::NotFound(format!("环境 '{name}' 不存在（{}：{e}）", path.display()))
    })?;
    log::info!("R-15: environment '{name}' deleted");
    Ok(())
}

/// 校验环境引用的 Runtime 配置都存在（保存与启动前各跑一次；启动路径
/// 由编排器调用，保存路径供 UI 即时反馈）。
pub fn validate_environment_configs(
    conn: &Connection,
    workspace_id: i64,
    environment: &RuntimeEnvironment,
) -> AppResult<()> {
    let configs = crate::runtime::config::list_configs(conn, workspace_id)?;
    let known: BTreeSet<String> = configs.into_iter().map(|c| c.name).collect();
    for service in &environment.services {
        if !known.contains(&service.runtime_name) {
            return Err(AppError::RuntimeConfig(format!(
                "环境 '{}' 的服务 '{}' 没有对应的 Runtime 配置；请先在 Runtime 总览创建",
                environment.name, service.runtime_name
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(name: &str, services: Vec<EnvironmentService>) -> RuntimeEnvironment {
        RuntimeEnvironment {
            schema_version: ENVIRONMENT_SCHEMA_VERSION,
            name: name.into(),
            description: None,
            services,
        }
    }

    fn service(name: &str, deps: &[&str]) -> EnvironmentService {
        EnvironmentService {
            runtime_name: name.into(),
            depends_on: deps.iter().map(|s| s.to_string()).collect(),
            jdk: None,
            profile: None,
            environment: BTreeMap::new(),
            port: None,
            external_notes: None,
            ready_timeout_seconds: None,
        }
    }

    #[test]
    fn topo_sort_layers_by_dependency() {
        // gateway → auth → system → common（验收链）+ 独立 file 服务。
        let environment = env(
            "demo",
            vec![
                service("gateway", &["auth"]),
                service("auth", &["system"]),
                service("system", &["common"]),
                service("common", &[]),
                service("file", &[]),
            ],
        );
        let waves = topo_sort_services(&environment).unwrap();
        assert_eq!(waves.len(), 4);
        // 第 1 波：无依赖（common/file），其余波各一个（按拓扑序）。
        let mut wave0 = waves[0].clone();
        wave0.sort();
        assert_eq!(wave0, vec!["common", "file"]);
        assert_eq!(waves[1], vec!["system"]);
        assert_eq!(waves[2], vec!["auth"]);
        assert_eq!(waves[3], vec!["gateway"]);
    }

    #[test]
    fn topo_sort_rejects_cycles_actionably() {
        let environment = env(
            "loop",
            vec![service("a", &["b"]), service("b", &["a"]), service("c", &[])],
        );
        let error = topo_sort_services(&environment).unwrap_err();
        assert_eq!(error.code(), "RuntimeConfigError");
        assert!(error.to_string().contains("环依赖"));
        assert!(error.to_string().contains('a') && error.to_string().contains('b'));
    }

    #[test]
    fn validate_rejects_unknown_dependency_and_duplicates() {
        let unknown = env("x", vec![service("a", &["missing"])]);
        assert_eq!(unknown.validate().unwrap_err().code(), "RuntimeConfigError");

        let duplicate = env("x", vec![service("a", &[]), service("a", &[])]);
        assert_eq!(duplicate.validate().unwrap_err().code(), "RuntimeConfigError");

        let self_dep = env("x", vec![service("a", &["a"])]);
        assert_eq!(self_dep.validate().unwrap_err().code(), "RuntimeConfigError");

        let empty = env("x", vec![]);
        assert_eq!(empty.validate().unwrap_err().code(), "RuntimeConfigError");
    }

    #[test]
    fn environment_roundtrips_json_with_defaults() {
        let environment = env(
            "Development",
            vec![
                EnvironmentService {
                    runtime_name: "gateway".into(),
                    depends_on: vec!["auth".into()],
                    jdk: Some("21".into()),
                    profile: Some("dev".into()),
                    environment: BTreeMap::from([("GATEWAY_URL".into(), "http://localhost:8080".into())]),
                    port: Some(8080),
                    external_notes: Some("依赖外部 MySQL（见 README）".into()),
                    ready_timeout_seconds: Some(90),
                },
                service("auth", &[]),
            ],
        );
        let text = serde_json::to_string(&environment).unwrap();
        let back: RuntimeEnvironment = serde_json::from_str(&text).unwrap();
        assert_eq!(environment, back);

        // 缺省字段（schemaVersion / 覆盖项）向后兼容。
        let minimal: RuntimeEnvironment =
            serde_json::from_str(r#"{"name":"Local","services":[{"runtimeName":"app"}]}"#).unwrap();
        assert_eq!(minimal.schema_version, ENVIRONMENT_SCHEMA_VERSION);
        assert_eq!(minimal.services[0].ready_timeout_seconds, None);
    }

    #[test]
    fn persistence_crud_roundtrip() {
        let root = std::env::temp_dir().join(format!(
            "gw_r15_env_{}",
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let environment = env("Test", vec![service("app", &[]), service("lib", &["app"])]);

        assert!(list_environments(&root).is_empty());
        save_environment(&root, &environment).unwrap();

        let loaded = get_environment(&root, "Test").unwrap();
        assert_eq!(loaded, environment);
        assert_eq!(list_environments(&root).len(), 1);

        delete_environment(&root, "Test").unwrap();
        assert!(get_environment(&root, "Test").is_err());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn environment_name_rejects_path_traversal() {
        let root = std::env::temp_dir();
        assert_eq!(
            get_environment(&root, "../evil").unwrap_err().code(),
            "RuntimeConfigError"
        );
        let bad = env("../evil", vec![service("a", &[])]);
        assert_eq!(save_environment(&root, &bad).unwrap_err().code(), "RuntimeConfigError");
    }
}
