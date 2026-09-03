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
    #[error("script {script:?} not found in Node project {project}")]
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

    #[error("{0}")]
    Other(String),
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
            AppError::Other(_) => "Other",
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
}
