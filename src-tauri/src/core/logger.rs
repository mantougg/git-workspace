use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use log::{Log, Metadata, Record};

use crate::core::secret::mask_secrets;

#[derive(Clone, Copy)]
enum Bucket {
    App,
    Git,
    Task,
    Ai,
    Performance,
}

impl Bucket {
    /// Route a log target (module path) to its bucket.
    fn from_target(target: &str) -> Bucket {
        if target.contains("performance") || target.contains("::perf") {
            Bucket::Performance
        } else if target.contains("::task") {
            Bucket::Task
        } else if target.contains("::ai") {
            Bucket::Ai
        } else if target.contains("::core::git_ops")
            || target.contains("::core::git_status")
            || target.contains("::core::graph")
            || target.contains("::core::diff")
        {
            Bucket::Git
        } else {
            Bucket::App
        }
    }
}

struct LogFiles {
    app: File,
    git: File,
    task: File,
    ai: File,
    performance: File,
}

/// A logger that writes one file per module bucket and redacts secrets.
pub struct AppLogger {
    files: Mutex<LogFiles>,
}

impl AppLogger {
    pub fn new(dir: &Path) -> std::io::Result<Self> {
        fs::create_dir_all(dir)?;
        Ok(AppLogger {
            files: Mutex::new(LogFiles {
                app: open(dir, "app.log")?,
                git: open(dir, "git.log")?,
                task: open(dir, "task.log")?,
                ai: open(dir, "ai.log")?,
                performance: open(dir, "performance.log")?,
            }),
        })
    }
}

fn open(dir: &Path, name: &str) -> std::io::Result<File> {
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join(name))
}

impl Log for AppLogger {
    fn enabled(&self, _: &Metadata) -> bool {
        true
    }

    fn log(&self, record: &Record) {
        let bucket = Bucket::from_target(record.target());
        let ts = chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
        let line = format!(
            "{} [{}] {}: {}\n",
            ts,
            record.level(),
            record.target(),
            mask_secrets(&record.args().to_string())
        );

        let mut files = self.files.lock().unwrap();
        let f = match bucket {
            Bucket::App => &mut files.app,
            Bucket::Git => &mut files.git,
            Bucket::Task => &mut files.task,
            Bucket::Ai => &mut files.ai,
            Bucket::Performance => &mut files.performance,
        };
        let _ = f.write_all(line.as_bytes());
    }

    fn flush(&self) {}
}

/// Initialize the global logger to write into `dir`.
/// Must be called before any log statement is emitted.
pub fn init_logger(dir: &Path) -> std::io::Result<()> {
    let logger = AppLogger::new(dir)?;
    // Leak the logger so it lives for the whole process (set_logger needs 'static).
    let logger: &'static AppLogger = Box::leak(Box::new(logger));
    log::set_logger(logger)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;
    log::set_max_level(log::LevelFilter::Info);
    Ok(())
}

/// The five per-module log file names, in a stable order.
pub const LOG_FILES: [&str; 5] = ["app.log", "git.log", "task.log", "ai.log", "performance.log"];

/// Directory where log files live (matches the app data dir used at startup).
pub fn logs_dir() -> PathBuf {
    let base = if let Some(dir) = dirs::config_dir() {
        dir.join("com.gitworkspace.app")
    } else if let Some(dir) = dirs::home_dir() {
        dir.join(".gitworkspace")
    } else {
        PathBuf::from(".gitworkspace")
    };
    base.join("logs")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_files_cover_five_module_buckets() {
        assert_eq!(LOG_FILES.len(), 5);
        assert_eq!(LOG_FILES[0], "app.log");
        assert_eq!(LOG_FILES[1], "git.log");
        assert_eq!(LOG_FILES[2], "task.log");
        assert_eq!(LOG_FILES[3], "ai.log");
        assert_eq!(LOG_FILES[4], "performance.log");
    }

    #[test]
    fn logs_dir_ends_with_logs() {
        assert!(logs_dir().ends_with("logs"));
    }
}
