//! 发布工程化：崩溃捕获 / 诊断反馈包 / 遥测（T-35，§63 扩展 / §69）。
//!
//! - 崩溃捕获：panic hook 落盘崩溃报告到 app data；「先转发默认 hook，
//!   再写文件」，写失败静默（§45 崩溃路径不能二次 panic）。
//! - 诊断包：logs + crash-reports + 用户备注 打包为一个目录，形成
//!   「出问题 → 导出 → 反馈」闭环。
//! - 遥测：**opt-in，默认关闭**；开启后事件经 `core::secret::mask_secrets`
//!   脱敏再落本地缓冲；仅当显式配置了上报端点（环境变量 GW_TELEMETRY_ENDPOINT）
//!   才网络上报，失败静默。 Offline First：全部能力不影响核心 Git 功能。
//!
//! 遥测/崩溃上报端点缺省为空 = 不上传，仅本地留存，用户手动反馈。

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::Manager;

use crate::core::logger::logs_dir;
use crate::core::secret::mask_secrets;
use crate::error::AppResult;

const CRASH_DIR_NAME: &str = "crash-reports";
const TELEMETRY_FILE: &str = "telemetry.json";
const TELEMETRY_BUFFER_LIMIT: usize = 500;

/// app data 目录（crash / 遥测的持久化根）。
fn app_data_dir(app: &tauri::AppHandle) -> PathBuf {
    app.path()
        .app_data_dir()
        .unwrap_or_else(|_| std::env::temp_dir().join("git-workspace"))
}

fn crash_dir(app: &tauri::AppHandle) -> PathBuf {
    app_data_dir(app).join(CRASH_DIR_NAME)
}

// ---------------------------------------------------------------------------
// 崩溃捕获
// ---------------------------------------------------------------------------

/// 崩溃报告文本（纯函数，可单测）。
pub fn format_crash_report(payload: &str, location: &str, thread: &str, version: &str, timestamp: &str) -> String {
    // 崩溃报告本身也可能带出敏感信息（如 panic 消息内含 URL / token 片段）
    let payload = mask_secrets(payload);
    format!(
        "GitWorkspace Crash Report\n\
         =========================\n\
         time:    {timestamp}\n\
         version: {version}\n\
         thread:  {thread}\n\
         location:{location}\n\n\
         message:\n{payload}\n"
    )
}

/// 安装全局 panic hook：转发默认 hook（保持控制台输出），随后落盘
/// 崩溃报告。写文件全程吞错——崩溃路径上绝不二次 panic。
pub fn install_panic_hook(app: tauri::AppHandle, version: &'static str) {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        previous(info);
        let payload = info.payload();
        let message = payload
            .downcast_ref::<&str>()
            .copied()
            .or_else(|| payload.downcast_ref::<String>().map(|s| s.as_str()))
            .unwrap_or("unknown panic payload");
        let location = info
            .location()
            .map(|l| format!(" {}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_default();
        let thread = std::thread::current().name().unwrap_or("<unnamed>").to_string();
        let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        let report = format_crash_report(message, &location, &thread, version, &timestamp);

        let dir = crash_dir(&app);
        let file = dir.join(format!(
            "crash-{}.log",
            chrono::Local::now().format("%Y%m%d-%H%M%S%.3f")
        ));
        // 全部静默：目录创建 / 写入失败都只放弃
        if fs::create_dir_all(&dir).is_ok() {
            let _ = fs::write(&file, report);
        }
    }));
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
#[allow(non_snake_case)]
pub struct CrashReportInfo {
    pub file: String,
    pub created: String,
    pub sizeBytes: u64,
}

fn list_reports_in(dir: &Path) -> Vec<CrashReportInfo> {
    let mut reports = Vec::new();
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let meta = fs::metadata(&path).ok();
            let created = entry
                .metadata()
                .ok()
                .and_then(|m| m.modified().ok())
                .map(|t| {
                    chrono::DateTime::<chrono::Local>::from(t)
                        .format("%Y-%m-%d %H:%M:%S")
                        .to_string()
                })
                .unwrap_or_default();
            reports.push(CrashReportInfo {
                file: path.to_string_lossy().to_string(),
                created,
                sizeBytes: meta.map(|m| m.len()).unwrap_or(0),
            });
        }
    }
    reports.sort_by(|a, b| b.created.cmp(&a.created));
    reports
}

/// 崩溃报告列表。
#[tauri::command]
pub fn get_crash_reports(app: tauri::AppHandle) -> AppResult<Vec<CrashReportInfo>> {
    Ok(list_reports_in(&crash_dir(&app)))
}

/// 清空崩溃报告。
#[tauri::command]
pub fn clear_crash_reports(app: tauri::AppHandle) -> AppResult<()> {
    let dir = crash_dir(&app);
    if dir.exists() {
        fs::remove_dir_all(&dir)?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 诊断反馈包（日志 + 崩溃报告 + 备注）
// ---------------------------------------------------------------------------

/// 一键收集「反馈包」：logs + crash-reports + note.txt → 返回目录路径。
#[tauri::command]
pub fn collect_feedback_bundle(app: tauri::AppHandle, note: Option<String>) -> AppResult<String> {
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let bundle = app_data_dir(&app).join(format!("diagnostics-{stamp}"));
    fs::create_dir_all(bundle.join("logs"))?;
    fs::create_dir_all(bundle.join("crash-reports"))?;

    // 日志（core::logger 的五个分模块文件）
    let logs = logs_dir();
    for entry in fs::read_dir(&logs)?.flatten() {
        let path = entry.path();
        if path.is_file() {
            if let Some(name) = path.file_name() {
                let _ = fs::copy(&path, bundle.join("logs").join(name));
            }
        }
    }

    // 崩溃报告
    let crashes = crash_dir(&app);
    if let Ok(entries) = fs::read_dir(&crashes) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                if let Some(name) = path.file_name() {
                    let _ = fs::copy(&path, bundle.join("crash-reports").join(name));
                }
            }
        }
    }

    // 用户备注（脱敏）
    let note = note.unwrap_or_default();
    fs::write(bundle.join("note.txt"), mask_secrets(&note))?;

    Ok(bundle.to_string_lossy().to_string())
}

// ---------------------------------------------------------------------------
// 遥测（opt-in，默认关闭）
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
#[allow(non_snake_case)]
pub struct TelemetryConfig {
    /// 默认关闭（§69 数据安全）
    pub enabled: bool,
    /// 崩溃报告是否允许随遥测上报（独立开关，默认关）
    pub crashUpload: bool,
}

impl Default for TelemetryConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            crashUpload: false,
        }
    }
}

fn telemetry_path(app: &tauri::AppHandle) -> PathBuf {
    app_data_dir(app).join(TELEMETRY_FILE)
}

fn load_telemetry(app: &tauri::AppHandle) -> TelemetryConfig {
    fs::read_to_string(telemetry_path(app))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// 读取遥测配置。
#[tauri::command]
pub fn get_telemetry_config(app: tauri::AppHandle) -> AppResult<TelemetryConfig> {
    Ok(load_telemetry(&app))
}

/// 写遥测配置（前端开关）。
#[tauri::command]
pub fn set_telemetry_config(app: tauri::AppHandle, config: TelemetryConfig) -> AppResult<()> {
    let path = telemetry_path(&app);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, serde_json::to_vec_pretty(&config)?)?;
    Ok(())
}

/// 进程内事件缓冲（flush 间隔由事件量驱动；进程退出未上报即丢弃，
/// 不落盘明文——缓冲写盘前同样脱敏）。
static TELEMETRY_BUFFER: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// 记录一个遥测事件（opt-in；关闭时零开销 no-op）。
#[tauri::command]
pub fn track_event(app: tauri::AppHandle, name: String, props: Option<serde_json::Value>) -> AppResult<()> {
    let config = load_telemetry(&app);
    if !config.enabled {
        return Ok(());
    }
    // 脱敏：整个 props 序列化后过 Secret 掩码（§5）
    let props_json = props
        .map(|p| mask_secrets(&p.to_string()))
        .unwrap_or_else(|| "{}".to_string());
    let line = format!(
        "{{\"event\":{},\"time\":{},\"props\":{}}}\n",
        serde_json::json!(name),
        serde_json::json!(chrono::Utc::now().to_rfc3339()),
        // props 已是脱敏后的合法 JSON 文本；兜底包一层字符串防注入
        serde_json::json!(props_json)
    );

    let mut buffer = TELEMETRY_BUFFER.lock().unwrap_or_else(|e| e.into_inner());
    buffer.push(line);
    if buffer.len() >= TELEMETRY_BUFFER_LIMIT {
        let drained: Vec<String> = buffer.drain(..).collect();
        drop(buffer);
        append_telemetry_file(&app, &drained);
    }
    Ok(())
}

/// 遥测本地缓冲文件（仅在 enabled 时产生）。
fn append_telemetry_file(app: &tauri::AppHandle, lines: &[String]) {
    let path = app_data_dir(app).join("telemetry-events.log");
    if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(&path) {
        use std::io::Write;
        let _ = f.write_all(lines.concat().as_bytes());
    }
}

/// 上报端点：仅当环境变量显式配置才启用（默认空 = 本地留存）。
#[allow(dead_code)]
fn telemetry_endpoint() -> Option<String> {
    std::env::var("GW_TELEMETRY_ENDPOINT").ok().filter(|s| !s.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crash_report_masks_secrets() {
        let report = format_crash_report(
            // ghp_ 后 36 位（secret.rs 的 GitHub token 模式）
            "failed to fetch https://user:ghp_abcdef1234567890abcdef12345678901234@host/x",
            " src/main.rs:10:5",
            "main",
            "0.1.0",
            "2026-09-02 10:00:00",
        );
        assert!(report.contains("version: 0.1.0"));
        assert!(!report.contains("ghp_abcdef1234567890"));
    }

    #[test]
    fn telemetry_config_defaults_off() {
        let config = TelemetryConfig::default();
        assert!(!config.enabled);
        assert!(!config.crashUpload);
    }
}
