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

    #[error("AI error: {0}")]
    Ai(String),

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
            AppError::Task(_) => "TaskError",
            AppError::Network(_) => "NetworkError",
            AppError::Conflict(_) => "ConflictError",
            AppError::Index(_) => "IndexError",
            AppError::DependencyResolve(_) => "DependencyResolveFailed",
            AppError::SourceMapping(_) => "SourceMappingFailed",
            AppError::Ai(_) => "AIError",
            AppError::Permission(_) => "PermissionError",
            AppError::Other(_) => "Other",
        }
    }

    /// Whether the error is recoverable by retry or user action.
    pub fn recoverable(&self) -> bool {
        !matches!(
            self,
            AppError::NotFound(_) | AppError::Permission(_) | AppError::Other(_)
        )
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
        ErrorResponse {
            code: self.code(),
            message: self.to_string(),
            repository: None,
            operation: None,
            details: None,
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
}
