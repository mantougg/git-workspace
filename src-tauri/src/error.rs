use serde::Serialize;

/// Structured error payload returned to the UI over IPC.
/// Mirrors Roadmap §44: code / message / repository / operation / details / recoverable.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorResponse {
    pub code: &'static str,
    pub message: String,
    pub repository: Option<String>,
    pub operation: Option<String>,
    pub details: Option<String>,
    pub recoverable: bool,
}

/// Unified error type for all GitWorkspace operations.
/// Implements Serialize as a structured `ErrorResponse` (not a bare string)
/// so the UI can render a readable message plus a recoverable hint.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("Database error: {0}")]
    Db(#[from] rusqlite::Error),

    #[error("Git error: {0}")]
    Git(#[from] git2::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("File watcher error: {0}")]
    Watcher(#[from] notify::Error),

    #[error("Scanner error: {0}")]
    Scanner(String),

    #[error("SSH error: {0}")]
    Ssh(String),

    #[error("Task error: {0}")]
    Task(String),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Project not found: {0}")]
    ProjectNotFound(String),

    #[error("JDK not found: {0}")]
    JdkNotFound(String),

    #[error("Maven not found: {0}")]
    MavenNotFound(String),

    /// N-01（§4.7 显式扩展）：PATH/配置均无 node。
    /// details 携带 suggestedActions（安装 Node / 加入 PATH）。
    #[error("Node.js not found: {0}")]
    NodeNotFound(String),

    /// N-01（§4.7 显式扩展）：决策链选中的包管理器不可执行
    /// （如 `pnpm-lock.yaml` 存在但没装 pnpm；bun 只识别不执行）。
    /// details 携带 suggestedActions（安装该 pm / 改选 npm）。
    #[error("package manager not found or not executable: {0}")]
    PackageManagerNotFound(String),

    /// N-03: Node Runtime 配置引用了缺失的 npm script。
    /// F-54：Display 不泄漏 `Some(...)` Debug 包装（用户面向消息）。
    #[error("script '{}' not found in Node project {project}", .script.as_deref().unwrap_or("<未指定>"))]
    ScriptNotFound {
        project: String,
        script: Option<String>,
        available: Vec<String>,
    },

    #[error("Invalid pom at {path}: {reason}")]
    InvalidPom { path: String, reason: String },

    #[error("port {port} is occupied by {process_name:?} (pid {pid:?})")]
    PortOccupied {
        port: u16,
        pid: Option<u32>,
        process_name: Option<String>,
    },

    #[error("health check failed for {runtime}: {reason}")]
    HealthCheckFailed { runtime: String, reason: String },

    #[error("Runtime configuration error: {0}")]
    RuntimeConfig(String),

    #[error("Network error: {0}")]
    Network(String),

    #[error("Conflict: {0}")]
    Conflict(String),

    #[error("Index error: {0}")]
    Index(String),

    #[error("Dependency resolution failed: {0}")]
    DependencyResolve(String),

    #[error("Source mapping failed: {0}")]
    SourceMapping(String),

    #[error("build failed in module {module}: maven exited with code {exit_code:?}")]
    BuildFailed {
        module: String,
        exit_code: Option<i32>,
        log_tail: String,
    },

    #[error("failed to start runtime process for {runtime}: {reason}")]
    ProcessStartFailed { runtime: String, reason: String },

    #[error("runtime process for {runtime} crashed (pid {pid:?}, exit code {exit_code:?})")]
    ProcessCrashed {
        runtime: String,
        pid: Option<u32>,
        exit_code: Option<i32>,
    },

    /// R-14 Command Safety（§75）：Pre/Post Build Script 未获用户确认。
    /// 结构化字段供 UI 弹出确认对话框（脚本预览 + 不再询问）。
    #[error("pre-build script for '{runtime_name}' requires user confirmation")]
    ScriptConfirmationRequired {
        workspace_id: i64,
        runtime_name: String,
        /// `"pre"` / `"post"`。
        script_type: String,
        /// 脚本内容哈希：内容变更后需重新确认。
        script_hash: String,
        /// 脚本内容预览（首行 + 截断），供 UI 展示。
        preview: String,
    },

    /// N-08: dependency installation is an explicit network action.
    #[error("node dependency installation for '{project_dir}' requires user confirmation")]
    NodeInstallConfirmationRequired {
        project_dir: String,
        package_manager: String,
        command_preview: String,
    },

    #[error("script `{script_type}` for '{runtime}' failed with exit code {exit_code:?}")]
    ScriptFailed {
        script_type: String,
        runtime: String,
        exit_code: Option<i32>,
        log_tail: String,
    },

    /// AI 错误（设计文档 §17）：结构化 code（AiNotConfigured 等）+ details
    /// （含 suggestedActions）。严禁携带 API Key 或 Secret 原文。
    /// Display 直接透传 AiError 的用户可读 message，不再加前缀。
    #[error("{0}")]
    Ai(#[from] crate::ai::AiError),

    #[error("Permission error: {0}")]
    Permission(String),

    /// LAN Chat 错误：面向用户的中文提示，禁止携带 secret / 明文等敏感内容。
    #[error("{0}")]
    LanChat(String),

    #[error("{0}")]
    Other(String),

    /// GF-08：平台访问令牌缺失/失效（创建 PR / CI 查询）。details 携带
    /// suggestedActions（去配置 token），**严禁携带 token 原文**——
    /// message 只含平台与 host，不含凭据内容。
    #[error("未配置或无权使用 {platform} 平台的访问令牌（{host}）")]
    RemoteAuth { platform: String, host: String },
}

impl AppError {
    /// Stable machine-readable error category (Roadmap §44).
    pub fn code(&self) -> &'static str {
        match self {
            AppError::Db(_) => "DatabaseError",
            AppError::Git(_) | AppError::Ssh(_) => "GitError",
            AppError::Io(_) | AppError::Watcher(_) => "IOError",
            AppError::Json(_) => "DataError",
            AppError::Scanner(_) | AppError::NotFound(_) => "RepositoryError",
            AppError::ProjectNotFound(_) => "ProjectNotFound",
            AppError::JdkNotFound(_) => "JdkNotFound",
            AppError::MavenNotFound(_) => "MavenNotFound",
            AppError::NodeNotFound(_) => "NodeNotFound",
            AppError::PackageManagerNotFound(_) => "PackageManagerNotFound",
            AppError::ScriptNotFound { .. } => "ScriptNotFound",
            AppError::InvalidPom { .. } => "InvalidPom",
            AppError::PortOccupied { .. } => "PortOccupied",
            AppError::HealthCheckFailed { .. } => "HealthCheckFailed",
            AppError::ScriptConfirmationRequired { .. } => "ScriptConfirmationRequired",
            AppError::NodeInstallConfirmationRequired { .. } => "NodeInstallConfirmationRequired",
            AppError::ScriptFailed { .. } => "ScriptFailed",
            AppError::RuntimeConfig(_) => "RuntimeConfigError",
            AppError::Task(_) => "TaskError",
            AppError::Network(_) => "NetworkError",
            AppError::Conflict(_) => "ConflictError",
            AppError::Index(_) => "IndexError",
            AppError::DependencyResolve(_) => "DependencyResolveFailed",
            AppError::SourceMapping(_) => "SourceMappingFailed",
            AppError::BuildFailed { .. } => "BuildFailed",
            AppError::ProcessStartFailed { .. } => "ProcessStartFailed",
            AppError::ProcessCrashed { .. } => "ProcessCrashed",
            AppError::Ai(e) => e.code(),
            AppError::Permission(_) => "PermissionError",
            AppError::LanChat(_) => "LanChatError",
            AppError::Other(_) => "Other",
            AppError::RemoteAuth { .. } => "RemoteAuthRequired",
        }
    }

    /// Whether the error is recoverable by retry or user action.
    pub fn recoverable(&self) -> bool {
        match self {
            AppError::Ai(e) => e.recoverable(),
            _ => !matches!(
                self,
                AppError::NotFound(_)
                    | AppError::Permission(_)
                    | AppError::Other(_)
                    // InvalidPom 需用户修复 pom 后重新解析，非自动可恢复。
                    | AppError::InvalidPom { .. }
            ),
        }
    }
}

impl From<String> for AppError {
    fn from(s: String) -> Self {
        AppError::Other(s)
    }
}

impl From<&str> for AppError {
    fn from(s: &str) -> Self {
        AppError::Other(s.to_string())
    }
}

impl Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let details = match self {
            // BuildFailed carries the structured context R-14 needs for
            // actionable hints: failing module, Maven exit code and log tail.
            AppError::BuildFailed {
                module,
                exit_code,
                log_tail,
            } => Some(
                serde_json::json!({
                    "module": module,
                    "exitCode": exit_code,
                    "logTail": log_tail,
                })
                .to_string(),
            ),
            // R-10 进程错误同样带结构化上下文（§79/§80 可行动提示）。
            AppError::ProcessStartFailed { runtime, reason } => Some(
                serde_json::json!({
                    "runtime": runtime,
                    "reason": reason,
                })
                .to_string(),
            ),
            AppError::ProcessCrashed {
                runtime,
                pid,
                exit_code,
            } => Some(
                serde_json::json!({
                    "runtime": runtime,
                    "pid": pid,
                    "exitCode": exit_code,
                })
                .to_string(),
            ),
            // R-14（§79/§80）：InvalidPom / PortOccupied / HealthCheckFailed /
            // 脚本确认与失败同样携带结构化上下文，供 UI 渲染可行动提示。
            AppError::InvalidPom { path, reason } => Some(
                serde_json::json!({
                    "path": path,
                    "reason": reason,
                })
                .to_string(),
            ),
            AppError::PortOccupied {
                port,
                pid,
                process_name,
            } => Some(
                serde_json::json!({
                    "port": port,
                    "pid": pid,
                    "processName": process_name,
                })
                .to_string(),
            ),
            AppError::HealthCheckFailed { runtime, reason } => Some(
                serde_json::json!({
                    "runtime": runtime,
                    "reason": reason,
                })
                .to_string(),
            ),
            AppError::ScriptConfirmationRequired {
                workspace_id,
                runtime_name,
                script_type,
                script_hash,
                preview,
            } => Some(
                serde_json::json!({
                    "workspaceId": workspace_id,
                    "runtimeName": runtime_name,
                    "scriptType": script_type,
                    "scriptHash": script_hash,
                    "preview": preview,
                })
                .to_string(),
            ),
            AppError::ScriptFailed {
                script_type,
                runtime,
                exit_code,
                log_tail,
            } => Some(
                serde_json::json!({
                    "scriptType": script_type,
                    "runtime": runtime,
                    "exitCode": exit_code,
                    "logTail": log_tail,
                })
                .to_string(),
            ),
            AppError::NodeInstallConfirmationRequired {
                project_dir,
                package_manager,
                command_preview,
            } => Some(
                serde_json::json!({
                    "projectDir": project_dir,
                    "packageManager": package_manager,
                    "commandPreview": command_preview,
                    "suggestedActions": ["确认后再次执行 node_install", "检查依赖源与网络设置"],
                })
                .to_string(),
            ),
            // N-01（§4.7 扩展，§80 可行动错误）：Node 工具链错误携带
            // Suggested Actions，供 UI 直接渲染下一步操作。
            AppError::NodeNotFound(_) => Some(
                serde_json::json!({
                    "suggestedActions": [
                        "安装 Node.js LTS（https://nodejs.org）并把 node 加入 PATH",
                        "安装后重启 GitWorkspace 使 PATH 生效",
                    ],
                })
                .to_string(),
            ),
            AppError::PackageManagerNotFound(_) => Some(
                serde_json::json!({
                    "suggestedActions": [
                        "安装决策链选中的包管理器（如 npm i -g pnpm / corepack enable）",
                        "或在 Runtime 配置中显式改选 npm",
                    ],
                })
                .to_string(),
            ),
            AppError::ScriptNotFound {
                project,
                script,
                available,
            } => Some(
                serde_json::json!({
                    "project": project,
                    "script": script,
                    "availableScripts": available,
                    "suggestedActions": [
                        "从 package.json 的 scripts 中选择一个脚本",
                        "运行 Node 项目发现以刷新脚本列表",
                    ],
                })
                .to_string(),
            ),
            // AI（§17）：details 携带非敏感上下文 + suggestedActions。
            AppError::Ai(e) => Some(e.details_json()),
            // GF-08：Git 错误分类。`AppError::Git` 覆盖系统 git CLI stderr
            // 尾部（经 `task::console::readable_error` 还原）与 libgit2
            // 错误；`AppError::Ssh` 覆盖 SSH 子进程错误。未分类时
            // details 保持 None——非认证类错误展示行为不劣化。
            AppError::Git(e) => git_error_details(e.message(), Some(e.class())),
            AppError::Ssh(msg) => git_error_details(msg, None),
            // GF-08：平台 token 缺失/失效（创建 PR 401/403）。suggestedActions
            // 引导「去配置 token」；平台与 host 非敏感，token 原文严禁入内。
            AppError::RemoteAuth { platform, host } => Some(
                serde_json::json!({
                    "platform": platform,
                    "host": host,
                    "reason": "平台访问令牌缺失或已失效：无法以你的身份调用平台 API",
                    "suggestedActions": [
                        "在分支页「创建 PR」面板中填写平台 token 并保存到 OS 凭据库",
                        "或在系统 Git 凭据管理器中重新登录该平台",
                    ],
                })
                .to_string(),
            ),
            _ => None,
        };
        ErrorResponse {
            code: self.code(),
            message: self.to_string(),
            repository: None,
            operation: None,
            details,
            recoverable: self.recoverable(),
        }
        .serialize(serializer)
    }
}

// ---------------------------------------------------------------------------
// GF-08：Git 错误分类与可行动引导
//
// 认证失败无可行动引导的修复核心。网络 Git 操作（fetch/pull/push/clone）的
// 失败消息来自两条路径：
//   1. 系统 git CLI 的 stderr 尾部——`task::console::readable_error` 用尾部
//      还原出的文本构造 `AppError::Git`（worker 队列与单仓命令共用）；
//   2. libgit2 错误——本地/回退路径（如 smart_pull 的 merge 阶段）。
// 分类器对**文本**做模式匹配，并用 libgit2 error class 兜底，两条路径的
// 错误都能吃到分类。
// ---------------------------------------------------------------------------

/// GF-08：Git 失败的可行动分类。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitErrorCategory {
    /// 认证失败：HTTPS 凭据被拒 / SSH key 无权限 / 无可用凭据。
    Authentication,
    /// 网络不可达：DNS / 连接拒绝 / 超时 / TLS 证书 / 代理。
    Network,
    /// 仓库锁：`index.lock` 等被其它 git 进程持有。
    Lock,
    /// 工作区脏：本地改动 / 未跟踪文件会被覆盖。
    DirtyTree,
    /// 非快进拒绝：远端有新提交需先同步，或远端 hook 拒绝推送。
    Rejected,
}

impl GitErrorCategory {
    /// details JSON 里的稳定分类标识（camelCase，与既有 details 契约一致）。
    pub fn code(&self) -> &'static str {
        match self {
            GitErrorCategory::Authentication => "authentication",
            GitErrorCategory::Network => "network",
            GitErrorCategory::Lock => "lock",
            GitErrorCategory::DirtyTree => "dirtyTree",
            GitErrorCategory::Rejected => "rejected",
        }
    }

    /// 分类原因（中文人话）。**只允许固定文案**——不回填错误原文，防止
    /// URL 内嵌的用户名 / 令牌等敏感信息经 details 泄漏到 UI 与日志。
    pub fn reason(&self) -> &'static str {
        match self {
            GitErrorCategory::Authentication => "认证失败：平台拒绝了当前凭据（HTTPS 令牌/密码或 SSH key）",
            GitErrorCategory::Network => "网络不可达：无法连接到远程平台（DNS / 连接 / 超时 / 证书）",
            GitErrorCategory::Lock => "仓库被锁：另一个 Git 进程可能正在使用该仓库",
            GitErrorCategory::DirtyTree => "工作区有未提交改动：本地修改会被本次操作覆盖",
            GitErrorCategory::Rejected => "推送被拒：远端包含你没有的提交（非快进）",
        }
    }

    /// Suggested Actions：UI 直接渲染为动作入口。同样只含固定文案，
    /// 严禁携带任何凭据内容。
    pub fn suggested_actions(&self) -> &'static [&'static str] {
        match self {
            GitErrorCategory::Authentication => &[
                "打开系统凭据管理器，更新该平台的 Git 凭据",
                "检查 SSH key 配置（~/.ssh 下的私钥与 ssh-agent）",
                "确认平台访问令牌是否已过期或被撤销",
            ],
            GitErrorCategory::Network => &[
                "检查网络连接与代理设置后重试",
                "确认平台地址可达（防火墙 / VPN / 证书链）",
            ],
            GitErrorCategory::Lock => &[
                "关闭其它 Git 客户端或等待其结束后重试",
                "确认无残留 git 进程后，删除仓库 .git 目录下的锁文件",
            ],
            GitErrorCategory::DirtyTree => &[
                "先提交或 stash 本地改动，再执行远程操作",
                "在变更页确认需要保留的工作区改动",
            ],
            GitErrorCategory::Rejected => &[
                "先 pull（或 rebase）同步远端最新提交后再推送",
                "确需覆盖远端历史时使用 --force-with-lease，并先确认影响范围",
            ],
        }
    }
}

/// libgit2 error class → 分类（文本分类的兜底信号）。
fn category_from_git2_class(class: git2::ErrorClass) -> Option<GitErrorCategory> {
    use git2::ErrorClass;
    match class {
        ErrorClass::Ssh => Some(GitErrorCategory::Authentication),
        ErrorClass::Net | ErrorClass::Http | ErrorClass::Ssl => Some(GitErrorCategory::Network),
        _ => None,
    }
}

/// GF-08：从错误**文本**提取分类（纯函数，可单测）。
///
/// 覆盖系统 git CLI 的典型 stderr（Windows Git / OpenSSH / curl 文案）与
/// libgit2 消息。返回 `None` 表示未分类——details 保持 `None`，非认证类
/// 错误的展示行为不劣化。
///
/// 匹配顺序：认证 → 网络 → 锁 → 脏工作区 → 非快进。认证先于网络：SSH 类
/// 失败（"could not read from remote repository"）多数是 key/权限问题，
/// 而 "connect to host ... timed out" 明确是网络。
pub fn classify_git_error(message: &str) -> Option<GitErrorCategory> {
    let m = message.to_ascii_lowercase();

    // —— 认证 ——
    const AUTH: &[&str] = &[
        "authentication failed",
        "authentication required",
        "failed to authenticate",
        "permission denied",
        "publickey",
        "access denied",
        "invalid username or password",
        "invalid credentials",
        "could not read username",
        "could not read from remote repository",
        "terminal prompts disabled",
        "unable to read askpass",
        "no password available",
        "too many authentication failures",
        "host key verification failed",
        "remote: repository not found",
        "repository not found",
        "403 forbidden",
        "401 unauthorized",
        "the requested url returned error: 401",
        "the requested url returned error: 403",
    ];
    if AUTH.iter().any(|p| m.contains(p)) {
        return Some(GitErrorCategory::Authentication);
    }

    // —— 网络 ——
    const NETWORK: &[&str] = &[
        "failed to connect",
        "could not connect",
        "couldn't connect",
        "could not resolve host",
        "could not resolve hostname",
        "couldn't resolve host",
        "connection refused",
        "connection reset",
        "connection timed out",
        "network is unreachable",
        "unable to access",
        "operation timed out",
        "ssl certificate problem",
        "certificate verify failed",
        "server certificate verification failed",
        "proxy connect",
        "the remote end hung up unexpectedly",
        "rpc failed",
        "http/2 stream",
        "connect to host",
        "early eof",
    ];
    if NETWORK.iter().any(|p| m.contains(p)) {
        return Some(GitErrorCategory::Network);
    }

    // —— 锁 ——
    const LOCK: &[&str] = &[
        "index.lock",
        "cannot lock ref",
        "failed to lock",
        "unable to lock",
        "lock exists",
        "another git process",
        ".lock",
    ];
    if LOCK.iter().any(|p| m.contains(p)) {
        return Some(GitErrorCategory::Lock);
    }

    // —— 脏工作区 ——
    const DIRTY: &[&str] = &[
        "your local changes",
        "would be overwritten by merge",
        "would be overwritten by checkout",
        "untracked working tree files",
        "please commit your changes or stash",
        "commit or stash",
    ];
    if DIRTY.iter().any(|p| m.contains(p)) {
        return Some(GitErrorCategory::DirtyTree);
    }

    // —— 非快进 / 远端拒绝 ——
    const REJECTED: &[&str] = &[
        "non-fast-forward",
        "fetch first",
        "updates were rejected",
        "! [rejected]",
        "[remote rejected]",
        "failed to push some refs",
        "tip of your current branch is behind",
    ];
    if REJECTED.iter().any(|p| m.contains(p)) {
        return Some(GitErrorCategory::Rejected);
    }

    None
}

/// GF-08：Git 错误的 details JSON（分类 + 原因 + suggestedActions）。
///
/// 纯文本分类优先，libgit2 error class 兜底。输出**只含固定文案**
/// （category / reason / suggestedActions），不回填错误原文，确保
/// details 无 token/密码/私钥泄漏。
pub(crate) fn git_error_details(message: &str, class: Option<git2::ErrorClass>) -> Option<String> {
    let category = classify_git_error(message).or_else(|| class.and_then(category_from_git2_class))?;
    Some(
        serde_json::json!({
            "category": category.code(),
            "reason": category.reason(),
            "suggestedActions": category.suggested_actions(),
        })
        .to_string(),
    )
}

/// Convenience type alias for command return types.
pub type AppResult<T> = Result<T, AppError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_dependency_errors_keep_actionable_codes() {
        for (error, code) in [
            (
                AppError::DependencyResolve("missing effective model".into()),
                "DependencyResolveFailed",
            ),
            (
                AppError::SourceMapping("ambiguous source".into()),
                "SourceMappingFailed",
            ),
            (
                AppError::ProjectNotFound("missing Maven module".into()),
                "ProjectNotFound",
            ),
        ] {
            let payload = serde_json::to_value(error).unwrap();
            assert_eq!(payload["code"], code);
            assert_eq!(payload["recoverable"], true);
        }
    }

    #[test]
    fn script_not_found_message_hides_option_debug_wrapper() {
        let error = AppError::ScriptNotFound {
            project: "/ws/web".into(),
            script: Some("serve".into()),
            available: vec!["dev".into()],
        };
        let message = error.to_string();
        assert!(message.contains("script 'serve'"), "message: {message}");
        assert!(!message.contains("Some("), "message: {message}");

        let unnamed = AppError::ScriptNotFound {
            project: "/ws/web".into(),
            script: None,
            available: vec![],
        };
        assert!(!unnamed.to_string().contains("Some("));
    }

    #[test]
    fn build_failed_carries_structured_details() {
        let error = AppError::BuildFailed {
            module: "com.example:app".into(),
            exit_code: Some(1),
            log_tail: "[ERROR] COMPILATION ERROR".into(),
        };
        assert_eq!(error.code(), "BuildFailed");
        assert!(error.recoverable());
        let payload = serde_json::to_value(&error).unwrap();
        assert_eq!(payload["code"], "BuildFailed");
        assert_eq!(payload["recoverable"], true);
        assert!(payload["message"].as_str().unwrap().contains("com.example:app"));
        let details: serde_json::Value = serde_json::from_str(payload["details"].as_str().unwrap()).unwrap();
        assert_eq!(details["module"], "com.example:app");
        assert_eq!(details["exitCode"], 1);
        assert_eq!(details["logTail"], "[ERROR] COMPILATION ERROR");
    }

    #[test]
    fn process_errors_carry_structured_details() {
        let start = AppError::ProcessStartFailed {
            runtime: "app".into(),
            reason: "java 可执行文件不存在".into(),
        };
        assert_eq!(start.code(), "ProcessStartFailed");
        assert!(start.recoverable());
        let payload = serde_json::to_value(&start).unwrap();
        let details: serde_json::Value = serde_json::from_str(payload["details"].as_str().unwrap()).unwrap();
        assert_eq!(details["runtime"], "app");
        assert!(details["reason"].as_str().unwrap().contains("java"));

        let crash = AppError::ProcessCrashed {
            runtime: "app".into(),
            pid: Some(4321),
            exit_code: Some(137),
        };
        assert_eq!(crash.code(), "ProcessCrashed");
        let payload = serde_json::to_value(&crash).unwrap();
        let details: serde_json::Value = serde_json::from_str(payload["details"].as_str().unwrap()).unwrap();
        assert_eq!(details["pid"], 4321);
        assert_eq!(details["exitCode"], 137);
    }

    /// R-14 §79：错误分类全集——每类错误都有稳定 code、结构化 details 与
    /// recoverable 语义（含 R-14 新增的 InvalidPom / PortOccupied /
    /// HealthCheckFailed / 脚本确认与失败）。
    #[test]
    fn full_error_catalog_has_code_details_and_recoverability() {
        let cases: Vec<(AppError, &'static str, bool)> = vec![
            (
                AppError::ProjectNotFound("missing module".into()),
                "ProjectNotFound",
                true,
            ),
            (AppError::MavenNotFound("mvn 不在 PATH".into()), "MavenNotFound", true),
            (AppError::NodeNotFound("node 不在 PATH".into()), "NodeNotFound", true),
            (
                AppError::PackageManagerNotFound("pnpm 未安装".into()),
                "PackageManagerNotFound",
                true,
            ),
            (
                AppError::ScriptNotFound {
                    project: "/ws/web".into(),
                    script: Some("start".into()),
                    available: vec!["dev".into()],
                },
                "ScriptNotFound",
                true,
            ),
            (AppError::JdkNotFound("JDK 21 未安装".into()), "JdkNotFound", true),
            (
                AppError::InvalidPom {
                    path: "/ws/repo/pom.xml".into(),
                    reason: "missing artifactId".into(),
                },
                "InvalidPom",
                false,
            ),
            (
                AppError::DependencyResolve("effective model 缺失".into()),
                "DependencyResolveFailed",
                true,
            ),
            (AppError::SourceMapping("坐标歧义".into()), "SourceMappingFailed", true),
            (
                AppError::BuildFailed {
                    module: "com.example:app".into(),
                    exit_code: Some(1),
                    log_tail: "[ERROR]".into(),
                },
                "BuildFailed",
                true,
            ),
            (
                AppError::ProcessStartFailed {
                    runtime: "app".into(),
                    reason: "spawn 失败".into(),
                },
                "ProcessStartFailed",
                true,
            ),
            (
                AppError::PortOccupied {
                    port: 8080,
                    pid: Some(12345),
                    process_name: Some("java.exe".into()),
                },
                "PortOccupied",
                true,
            ),
            (
                AppError::HealthCheckFailed {
                    runtime: "app".into(),
                    reason: "HTTP 500".into(),
                },
                "HealthCheckFailed",
                true,
            ),
            (
                AppError::ProcessCrashed {
                    runtime: "app".into(),
                    pid: Some(7),
                    exit_code: Some(137),
                },
                "ProcessCrashed",
                true,
            ),
        ];
        for (error, expected_code, expected_recoverable) in cases {
            assert_eq!(error.code(), expected_code);
            assert_eq!(error.recoverable(), expected_recoverable, "code {}", expected_code);
            let payload = serde_json::to_value(&error).unwrap();
            assert_eq!(payload["code"], expected_code);
            assert_eq!(payload["recoverable"], expected_recoverable);
            assert!(
                payload["message"].as_str().unwrap().len() > 0,
                "{} must carry a readable message",
                expected_code
            );
        }

        // 结构化变体（携带上下文字段）必须有 details：R-14 新增的错误与
        // 既有进程/构建错误；N-01 的 Node 工具链错误虽为 String payload
        // （message 即完整信息），details 仍携带 suggestedActions（§4.7/§80）。
        let structured = [
            AppError::NodeNotFound("node 不在 PATH".into()),
            AppError::PackageManagerNotFound("pnpm 未安装".into()),
            AppError::ScriptNotFound {
                project: "/ws/web".into(),
                script: Some("start".into()),
                available: vec!["dev".into()],
            },
            AppError::InvalidPom {
                path: "/ws/pom.xml".into(),
                reason: "missing artifactId".into(),
            },
            AppError::PortOccupied {
                port: 8080,
                pid: Some(1),
                process_name: None,
            },
            AppError::HealthCheckFailed {
                runtime: "app".into(),
                reason: "timeout".into(),
            },
            AppError::ScriptConfirmationRequired {
                workspace_id: 1,
                runtime_name: "app".into(),
                script_type: "pre".into(),
                script_hash: "h".into(),
                preview: "echo".into(),
            },
            AppError::ScriptFailed {
                script_type: "pre".into(),
                runtime: "app".into(),
                exit_code: Some(1),
                log_tail: "err".into(),
            },
            AppError::BuildFailed {
                module: "m".into(),
                exit_code: Some(1),
                log_tail: "t".into(),
            },
            AppError::ProcessStartFailed {
                runtime: "app".into(),
                reason: "r".into(),
            },
            AppError::ProcessCrashed {
                runtime: "app".into(),
                pid: None,
                exit_code: None,
            },
        ];
        for error in structured {
            let payload = serde_json::to_value(&error).unwrap();
            assert!(
                payload["details"].is_string(),
                "{} must carry structured details",
                error.code()
            );
        }
    }

    #[test]
    fn port_occupied_details_include_occupier() {
        let error = AppError::PortOccupied {
            port: 8080,
            pid: Some(4242),
            process_name: Some("java.exe".into()),
        };
        let payload = serde_json::to_value(&error).unwrap();
        let details: serde_json::Value = serde_json::from_str(payload["details"].as_str().unwrap()).unwrap();
        assert_eq!(details["port"], 8080);
        assert_eq!(details["pid"], 4242);
        assert_eq!(details["processName"], "java.exe");
        assert!(payload["message"].as_str().unwrap().contains("8080"));
    }

    #[test]
    fn script_confirmation_error_carries_confirmation_fields() {
        let error = AppError::ScriptConfirmationRequired {
            workspace_id: 2,
            runtime_name: "app".into(),
            script_type: "pre".into(),
            script_hash: "abc123".into(),
            preview: "#!/bin/sh\necho hello".into(),
        };
        assert_eq!(error.code(), "ScriptConfirmationRequired");
        assert!(error.recoverable());
        let payload = serde_json::to_value(&error).unwrap();
        let details: serde_json::Value = serde_json::from_str(payload["details"].as_str().unwrap()).unwrap();
        assert_eq!(details["workspaceId"], 2);
        assert_eq!(details["runtimeName"], "app");
        assert_eq!(details["scriptType"], "pre");
        assert_eq!(details["scriptHash"], "abc123");
        assert_eq!(details["preview"], "#!/bin/sh\necho hello");

        let failed = AppError::ScriptFailed {
            script_type: "post".into(),
            runtime: "app".into(),
            exit_code: Some(2),
            log_tail: "boom".into(),
        };
        assert_eq!(failed.code(), "ScriptFailed");
        assert!(failed.recoverable());
        let payload = serde_json::to_value(&failed).unwrap();
        let details: serde_json::Value = serde_json::from_str(payload["details"].as_str().unwrap()).unwrap();
        assert_eq!(details["exitCode"], 2);
        assert_eq!(details["logTail"], "boom");
    }

    // ------------------------------------------------------------------
    // GF-08：Git 错误分类（纯函数）与 details 契约
    // ------------------------------------------------------------------

    /// 典型 git stderr 样本 → 分类。样本取自系统 git CLI（Windows Git /
    /// OpenSSH / curl）与 libgit2 的真实文案，含 `finish_streaming`
    /// 还原 stderr 尾部后进入 `AppError::Git` 的形态。
    #[test]
    fn classify_git_error_covers_typical_stderr_samples() {
        let cases: &[(&str, GitErrorCategory)] = &[
            (
                "fatal: Authentication failed for 'https://github.com/o/r.git/'",
                GitErrorCategory::Authentication,
            ),
            (
                "remote: Invalid username or password.\nfatal: Authentication failed for 'https://gitlab.com/g/r.git'",
                GitErrorCategory::Authentication,
            ),
            (
                "git@github.com: Permission denied (publickey).\r\nfatal: Could not read from remote repository.",
                GitErrorCategory::Authentication,
            ),
            (
                "fatal: could not read Username for 'https://github.com': terminal prompts disabled",
                GitErrorCategory::Authentication,
            ),
            (
                "remote: Repository not found.\nfatal: repository 'https://github.com/o/private-r.git' not found",
                GitErrorCategory::Authentication,
            ),
            (
                "fatal: unable to access 'https://github.com/o/r.git': Failed to connect to github.com port 443: Timed out",
                GitErrorCategory::Network,
            ),
            (
                "fatal: unable to access 'https://github.com/o/r.git': Could not resolve host: github.com",
                GitErrorCategory::Network,
            ),
            (
                "error: RPC failed; curl 56 GnuTLS recv error...\nsend-pack: unexpected disconnect",
                GitErrorCategory::Network,
            ),
            (
                "fatal: unable to access 'https://x/': server certificate verification failed. CAfile: none",
                GitErrorCategory::Network,
            ),
            (
                "fatal: Unable to create '/repo/.git/index.lock': File exists.",
                GitErrorCategory::Lock,
            ),
            (
                "error: cannot lock ref 'refs/heads/main': Unable to create '/repo/.git/refs/heads/main.lock': File exists.\nAnother git process seems to be running",
                GitErrorCategory::Lock,
            ),
            (
                "error: Your local changes to the following files would be overwritten by merge:\n\tsrc/main.rs",
                GitErrorCategory::DirtyTree,
            ),
            (
                "error: The following untracked working tree files would be overwritten by checkout:\n\tout.log",
                GitErrorCategory::DirtyTree,
            ),
            (
                " ! [rejected]          main -> main (non-fast-forward)\nerror: failed to push some refs",
                GitErrorCategory::Rejected,
            ),
            (
                "hint: Updates were rejected because the remote contains work that you do not have locally.",
                GitErrorCategory::Rejected,
            ),
        ];
        for (message, expected) in cases {
            assert_eq!(classify_git_error(message), Some(*expected), "message: {message}");
        }
    }

    /// 非分类文本必须返回 None（details 缺省，展示行为不劣化）。
    #[test]
    fn classify_git_error_returns_none_for_other_messages() {
        for message in [
            "fatal: bad object HEAD",
            "corrupt loose object 'x'",
            "fatal: not a git repository",
            "",
        ] {
            assert_eq!(classify_git_error(message), None, "message: {message}");
        }
    }

    /// libgit2 error class 兜底：文本无线索时按 class 分类。
    #[test]
    fn classify_git_error_falls_back_to_git2_error_class() {
        let ssh = git2::Error::new(git2::ErrorCode::GenericError, git2::ErrorClass::Ssh, "handshake failed");
        let err = AppError::Git(ssh);
        let payload = serde_json::to_value(&err).unwrap();
        assert_eq!(payload["code"], "GitError");
        let details: serde_json::Value = serde_json::from_str(payload["details"].as_str().unwrap()).unwrap();
        assert_eq!(details["category"], "authentication");

        let net = git2::Error::new(
            git2::ErrorCode::GenericError,
            git2::ErrorClass::Net,
            "could not connect",
        );
        let err = AppError::Git(net);
        let payload = serde_json::to_value(&err).unwrap();
        let details: serde_json::Value = serde_json::from_str(payload["details"].as_str().unwrap()).unwrap();
        assert_eq!(details["category"], "network");
    }

    /// 认证失败的 `AppError::Git` 序列化：code 不变、details 带
    /// category/reason/suggestedActions，message 保留原始 stderr 尾部。
    #[test]
    fn git_auth_error_carries_suggested_actions_in_details() {
        let message = "fatal: Authentication failed for 'https://user@github.com/o/r.git/'";
        let err = AppError::Git(git2::Error::from_str(message));
        let payload = serde_json::to_value(&err).unwrap();
        // 错误码契约不变（前端按 "GitError" 分支的逻辑不受影响）。
        assert_eq!(payload["code"], "GitError");
        assert_eq!(payload["recoverable"], true);
        assert!(payload["message"].as_str().unwrap().contains("Authentication failed"));

        let details: serde_json::Value = serde_json::from_str(payload["details"].as_str().unwrap()).unwrap();
        assert_eq!(details["category"], "authentication");
        assert!(details["reason"].as_str().unwrap().contains("认证失败"));
        let actions = details["suggestedActions"].as_array().unwrap();
        assert!(actions.len() >= 2, "至少两个可行动动作");
        assert!(actions.iter().any(|a| a.as_str().unwrap().contains("凭据")));
    }

    /// 敏感信息禁令：details 只含固定文案——即使原始错误文本里嵌了
    /// 用户名/令牌，序列化输出也不得夹带。
    #[test]
    fn git_error_details_never_leak_credentials() {
        let message = "fatal: Authentication failed for 'https://user:ghp_supersecrettoken123@github.com/o/r.git/'";
        let payload = serde_json::to_value(AppError::Git(git2::Error::from_str(message))).unwrap();
        let details = payload["details"].as_str().unwrap();
        for secret in ["ghp_supersecrettoken123", "user:", "supersecret"] {
            assert!(!details.contains(secret), "details 泄漏敏感信息: {secret}\n{details}");
        }
        // 非认证类错误 details 仍为 None（行为不劣化）。
        let other = serde_json::to_value(AppError::Git(git2::Error::from_str("fatal: bad object HEAD"))).unwrap();
        assert!(other["details"].is_null(), "未分类 Git 错误不应带 details");
    }

    /// Ssh 变体同样走分类（纯文本，无 libgit2 class）。
    #[test]
    fn ssh_error_also_gets_classification() {
        let err = AppError::Ssh("ssh: connect to host github.com port 22: Connection timed out".into());
        let payload = serde_json::to_value(&err).unwrap();
        assert_eq!(payload["code"], "GitError");
        let details: serde_json::Value = serde_json::from_str(payload["details"].as_str().unwrap()).unwrap();
        assert_eq!(details["category"], "network");
    }

    /// 平台 token 缺失：code / message / details 契约（无 token 原文）。
    #[test]
    fn remote_auth_error_carries_token_guidance() {
        let err = AppError::RemoteAuth {
            platform: "GitHub".into(),
            host: "github.com".into(),
        };
        assert_eq!(err.code(), "RemoteAuthRequired");
        assert!(err.recoverable());
        let payload = serde_json::to_value(&err).unwrap();
        assert_eq!(payload["code"], "RemoteAuthRequired");
        assert!(payload["message"].as_str().unwrap().contains("github.com"));
        let details: serde_json::Value = serde_json::from_str(payload["details"].as_str().unwrap()).unwrap();
        assert_eq!(details["platform"], "GitHub");
        let actions = details["suggestedActions"].as_array().unwrap();
        assert!(actions.len() >= 2);
        assert!(actions.iter().any(|a| a.as_str().unwrap().contains("token")));
        // 敏感信息禁令：序列化输出不得出现 token 字样之外的内容——
        // 本错误根本不携带 token，message/details 均不含凭据。
        let text = payload.to_string();
        assert!(!text.contains("ghp_"), "RemoteAuth details 不得含 token：{text}");
    }
}
