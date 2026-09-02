//! Automation Platform（T-32，Roadmap §73）：插件级脚本动作、定时任务、
//! 模板导入导出。
//!
//! - 脚本级动作（P3 边界，见任务文档）：用户注册命令字符串，跨平台经
//!   Windows `cmd /C` / Unix `sh -c` 执行（与 Runtime 用户脚本同语义），
//!   带超时与 CREATE_NO_WINDOW；运行本身即用户显式触发。
//! - Scheduled Tasks：独立 30s 轮询线程，到点提交——脚本动作另起线程执行、
//!   pipeline 复用 T-23 compile + T-24 submit_dag 执行内核，不阻塞交互。
//! - 模板导入导出：Pipeline 模板 JSON 落盘 / 读入（复用 T-23 存储）。

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::{DateTime, Datelike, Local, Timelike};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::core::pipeline::{Pipeline, RepoSelection};
use crate::error::{AppError, AppResult};

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const ACTION_TIMEOUT: Duration = Duration::from_secs(300);
/// 调度器轮询间隔。
const SCHEDULER_TICK: Duration = Duration::from_secs(30);

// ---------------------------------------------------------------------------
// 数据模型
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginAction {
    pub id: i64,
    pub name: String,
    /// 脚本命令字符串（跨平台 shell 语义）
    pub command: String,
    /// repo（cwd=仓库根）| workspace（cwd=工作区根）；执行参数由前端传 cwd
    pub scope: String,
    pub timeout_secs: u64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduledTask {
    pub id: i64,
    pub name: String,
    /// script_action | pipeline
    pub kind: String,
    /// plugin action id 或 pipeline template id
    pub target_id: String,
    /// interval | daily
    pub schedule_kind: String,
    pub interval_minutes: Option<i64>,
    /// "HH:MM"（本地时区）
    pub daily_time: Option<String>,
    /// 调度参数补充：pipeline 的仓库选择 JSON（Vec<RepoSelection>）
    pub payload: Option<String>,
    pub enabled: bool,
    pub last_run: Option<String>,
    pub next_run: String,
    pub created_at: String,
    pub updated_at: String,
}

// ---------------------------------------------------------------------------
// 调度计算（纯函数）
// ---------------------------------------------------------------------------

/// 计算下一次运行时间。interval = now + N 分钟；daily = 明日（或今日剩余
/// 时间晚于 HH:MM 时为今日）HH:MM 本地时间。
pub fn next_run_at(
    schedule_kind: &str,
    interval_minutes: Option<i64>,
    daily_time: Option<&str>,
    now: DateTime<Local>,
) -> AppResult<DateTime<Local>> {
    match schedule_kind {
        "interval" => {
            let minutes = interval_minutes
                .filter(|m| *m >= 1)
                .ok_or_else(|| AppError::Other("interval 调度需要 >=1 的间隔分钟数".to_string()))?;
            Ok(now + chrono::Duration::minutes(minutes))
        }
        "daily" => {
            let t =
                daily_time.ok_or_else(|| AppError::Other("daily 调度需要 HH:MM".to_string()))?;
            let (h, m) = t
                .split_once(':')
                .and_then(|(h, m)| Some((h.parse::<u32>().ok()?, m.parse::<u32>().ok()?)))
                .filter(|(h, m)| *h < 24 && *m < 60)
                .ok_or_else(|| AppError::Other(format!("daily_time 不合法：{t}")))?;
            let mut next = now
                .with_hour(h)
                .and_then(|d| d.with_minute(m))
                .and_then(|d| d.with_second(0))
                .and_then(|d| d.with_nanosecond(0))
                .ok_or_else(|| AppError::Other("daily_time 计算失败".to_string()))?;
            if next <= now {
                next = next + chrono::Duration::days(1);
            }
            let _ = (next.year(), next.month());
            Ok(next)
        }
        other => Err(AppError::Other(format!("未知调度类型：{other}"))),
    }
}

// ---------------------------------------------------------------------------
// 脚本动作执行（唯一系统调用入口）
// ---------------------------------------------------------------------------

/// 跨平台脚本执行器：Windows `cmd /C`、Unix `sh -c`（AGENTS 平台规范 §2）。
fn script_command(command: &str) -> Command {
    #[cfg(windows)]
    {
        let mut c = Command::new("cmd");
        c.args(["/C", command]);
        c
    }
    #[cfg(not(windows))]
    {
        let mut c = Command::new("sh");
        c.args(["-c", command]);
        c
    }
}

struct ProcOutput {
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

fn wait_with_streams(mut child: std::process::Child, timeout: Duration) -> AppResult<ProcOutput> {
    fn read_all<R: std::io::Read + Send + 'static>(
        stream: Option<R>,
    ) -> std::thread::JoinHandle<String> {
        std::thread::spawn(move || {
            let mut buf = String::new();
            if let Some(mut s) = stream {
                use std::io::Read;
                let mut raw = Vec::new();
                let _ = s.read_to_end(&mut raw);
                buf.push_str(&String::from_utf8_lossy(&raw));
            }
            buf
        })
    }
    let out_t = read_all(child.stdout.take());
    let err_t = read_all(child.stderr.take());
    let start = std::time::Instant::now();
    let code = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status.code(),
            Ok(None) => {
                if start.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(AppError::Other(format!(
                        "脚本超时（{}s），已终止",
                        timeout.as_secs()
                    )));
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(err) => return Err(AppError::Other(format!("等待脚本失败：{err}"))),
        }
    };
    let stdout = out_t.join().unwrap_or_default();
    let stderr = err_t.join().unwrap_or_default();
    Ok(ProcOutput {
        code,
        stdout,
        stderr,
    })
}

/// 运行脚本动作（cwd 由调用方按 scope 传入）。
#[tauri::command]
pub fn run_plugin_action(cwd: String, action: PluginAction) -> AppResult<String> {
    let timeout = Duration::from_secs(action.timeout_secs.max(1));
    let mut command = script_command(&action.command);
    command
        .current_dir(&cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let child = command
        .spawn()
        .map_err(|e| AppError::Other(format!("脚本启动失败：{e}")))?;
    let output = wait_with_streams(child, timeout)?;
    let mut text = output.stdout;
    if !output.stderr.is_empty() {
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(&output.stderr);
    }
    if output.code != Some(0) {
        return Err(AppError::Other(format!(
            "脚本退出码 {:?}：{}",
            output.code,
            text.chars()
                .rev()
                .take(1000)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect::<String>()
        )));
    }
    Ok(text
        .chars()
        .rev()
        .take(8000)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect())
}

// ---------------------------------------------------------------------------
// Plugin Action CRUD
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn list_plugin_actions(
    state: tauri::State<'_, crate::state::AppState>,
) -> AppResult<Vec<PluginAction>> {
    let conn = state.db.lock().unwrap_or_else(|e| e.into_inner());
    let mut stmt = conn.prepare(
        "SELECT id, name, command, scope, timeout_secs, created_at, updated_at \
         FROM plugin_actions ORDER BY name",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(PluginAction {
            id: row.get(0)?,
            name: row.get(1)?,
            command: row.get(2)?,
            scope: row.get(3)?,
            timeout_secs: row.get::<_, i64>(4)? as u64,
            created_at: row.get(5)?,
            updated_at: row.get(6)?,
        })
    })?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

#[tauri::command]
pub fn save_plugin_action(
    mut action: PluginAction,
    state: tauri::State<'_, crate::state::AppState>,
) -> AppResult<PluginAction> {
    if action.name.trim().is_empty() || action.command.trim().is_empty() {
        return Err(AppError::Other("动作名称与命令不能为空".to_string()));
    }
    if action.scope != "repo" && action.scope != "workspace" {
        return Err(AppError::Other(
            "scope 只能为 repo 或 workspace".to_string(),
        ));
    }
    if action.timeout_secs == 0 {
        action.timeout_secs = 120;
    }
    let conn = state.db.lock().unwrap_or_else(|e| e.into_inner());
    let now = chrono::Utc::now().to_rfc3339();
    if action.id == 0 {
        conn.execute(
            "INSERT INTO plugin_actions (name, command, scope, timeout_secs, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![action.name, action.command, action.scope, action.timeout_secs as i64, now, now],
        )?;
        action.id = conn.last_insert_rowid();
    } else {
        let changed = conn.execute(
            "UPDATE plugin_actions SET name = ?2, command = ?3, scope = ?4, timeout_secs = ?5, updated_at = ?6 WHERE id = ?1",
            params![action.id, action.name, action.command, action.scope, action.timeout_secs as i64, now],
        )?;
        if changed == 0 {
            return Err(AppError::NotFound(format!("动作 {} 不存在", action.id)));
        }
    }
    action.created_at = if action.created_at.is_empty() {
        now.clone()
    } else {
        action.created_at
    };
    action.updated_at = now;
    Ok(action)
}

#[tauri::command]
pub fn delete_plugin_action(
    action_id: i64,
    state: tauri::State<'_, crate::state::AppState>,
) -> AppResult<()> {
    let conn = state.db.lock().unwrap_or_else(|e| e.into_inner());
    // 引用检查：定时任务挂在动作上时拒绝删除
    let refs: i64 = conn.query_row(
        "SELECT COUNT(*) FROM scheduled_tasks WHERE kind = 'script_action' AND target_id = ?1",
        params![action_id.to_string()],
        |row| row.get(0),
    )?;
    if refs > 0 {
        return Err(AppError::Other(format!(
            "该动作被 {refs} 个定时任务引用，请先删除对应定时任务"
        )));
    }
    conn.execute(
        "DELETE FROM plugin_actions WHERE id = ?1",
        params![action_id],
    )?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Scheduled Tasks CRUD
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn list_scheduled_tasks(
    state: tauri::State<'_, crate::state::AppState>,
) -> AppResult<Vec<ScheduledTask>> {
    let conn = state.db.lock().unwrap_or_else(|e| e.into_inner());
    query_scheduled(&conn)
}

fn query_scheduled(conn: &Connection) -> AppResult<Vec<ScheduledTask>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, kind, target_id, schedule_kind, interval_minutes, daily_time, payload, \
                enabled, last_run, next_run, created_at, updated_at \
         FROM scheduled_tasks ORDER BY name",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(ScheduledTask {
            id: row.get(0)?,
            name: row.get(1)?,
            kind: row.get(2)?,
            target_id: row.get(3)?,
            schedule_kind: row.get(4)?,
            interval_minutes: row.get(5)?,
            daily_time: row.get(6)?,
            payload: row.get(7)?,
            enabled: row.get::<_, i64>(8)? != 0,
            last_run: row.get(9)?,
            next_run: row.get(10)?,
            created_at: row.get(11)?,
            updated_at: row.get(12)?,
        })
    })?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

#[tauri::command]
pub fn save_scheduled_task(
    mut task: ScheduledTask,
    state: tauri::State<'_, crate::state::AppState>,
) -> AppResult<ScheduledTask> {
    if task.name.trim().is_empty() {
        return Err(AppError::Other("任务名称不能为空".to_string()));
    }
    if task.kind != "script_action" && task.kind != "pipeline" {
        return Err(AppError::Other(
            "kind 只能为 script_action 或 pipeline".to_string(),
        ));
    }
    let now_local = Local::now();
    let next = next_run_at(
        &task.schedule_kind,
        task.interval_minutes,
        task.daily_time.as_deref(),
        now_local,
    )?;
    let next_str = next.to_rfc3339();
    let conn = state.db.lock().unwrap_or_else(|e| e.into_inner());
    let now = chrono::Utc::now().to_rfc3339();
    if task.id == 0 {
        conn.execute(
            "INSERT INTO scheduled_tasks (name, kind, target_id, schedule_kind, interval_minutes, daily_time, \
                                         payload, enabled, next_run, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                task.name, task.kind, task.target_id, task.schedule_kind,
                task.interval_minutes, task.daily_time, task.payload,
                task.enabled as i64, next_str, now, now
            ],
        )?;
        task.id = conn.last_insert_rowid();
    } else {
        let changed = conn.execute(
            "UPDATE scheduled_tasks SET name = ?2, kind = ?3, target_id = ?4, schedule_kind = ?5, \
                                        interval_minutes = ?6, daily_time = ?7, payload = ?8, \
                                        enabled = ?9, next_run = ?10, updated_at = ?11 WHERE id = ?1",
            params![
                task.id, task.name, task.kind, task.target_id, task.schedule_kind,
                task.interval_minutes, task.daily_time, task.payload,
                task.enabled as i64, next_str, now
            ],
        )?;
        if changed == 0 {
            return Err(AppError::NotFound(format!("定时任务 {} 不存在", task.id)));
        }
    }
    task.next_run = next_str;
    task.created_at = if task.created_at.is_empty() {
        now.clone()
    } else {
        task.created_at
    };
    task.updated_at = now;
    Ok(task)
}

/// 暂停 / 恢复（enabled 开关）。
#[tauri::command]
pub fn set_scheduled_task_enabled(
    task_id: i64,
    enabled: bool,
    state: tauri::State<'_, crate::state::AppState>,
) -> AppResult<()> {
    let conn = state.db.lock().unwrap_or_else(|e| e.into_inner());
    let changed = conn.execute(
        "UPDATE scheduled_tasks SET enabled = ?2, updated_at = ?3 WHERE id = ?1",
        params![task_id, enabled as i64, chrono::Utc::now().to_rfc3339()],
    )?;
    if changed == 0 {
        return Err(AppError::NotFound(format!("定时任务 {task_id} 不存在")));
    }
    Ok(())
}

#[tauri::command]
pub fn delete_scheduled_task(
    task_id: i64,
    state: tauri::State<'_, crate::state::AppState>,
) -> AppResult<()> {
    let conn = state.db.lock().unwrap_or_else(|e| e.into_inner());
    conn.execute(
        "DELETE FROM scheduled_tasks WHERE id = ?1",
        params![task_id],
    )?;
    Ok(())
}

// ---------------------------------------------------------------------------
// 调度器（独立线程，不阻塞交互）
// ---------------------------------------------------------------------------

/// 启动调度线程（lib.rs setup 调用）；每 tick 检查到期任务。
pub fn spawn_scheduler(
    db: Arc<Mutex<Connection>>,
    task_manager: Arc<crate::task::manager::TaskManager>,
) {
    std::thread::Builder::new()
        .name("scheduled-tasks".into())
        .spawn(move || loop {
            std::thread::sleep(SCHEDULER_TICK);
            if let Err(e) = tick(&db, &task_manager) {
                log::warn!("scheduled-tasks tick failed: {e}");
            }
        })
        .expect("failed to spawn scheduled-tasks thread");
}

fn tick(
    db: &Arc<Mutex<Connection>>,
    task_manager: &Arc<crate::task::manager::TaskManager>,
) -> AppResult<()> {
    let now = Local::now();
    let due: Vec<ScheduledTask> = {
        let conn = db.lock().unwrap_or_else(|e| e.into_inner());
        let tasks = query_scheduled(&conn)?;
        tasks
            .into_iter()
            .filter(|t| {
                if !t.enabled {
                    return false;
                }
                DateTime::parse_from_rfc3339(&t.next_run)
                    .map(|next| next.with_timezone(&Local) <= now)
                    .unwrap_or(false)
            })
            .collect()
    };
    for task in due {
        // 运行在派生线程：pipeline 提交与脚本执行都不阻塞调度循环
        let db = Arc::clone(db);
        let tm = Arc::clone(task_manager);
        let task = task.clone();
        std::thread::Builder::new()
            .name(format!("scheduled-{}", task.id))
            .spawn(move || {
                let result = execute_scheduled(&db, &tm, &task);
                // 记录 last_run 并推进 next_run；执行失败也推进（避免风暴）
                let next = next_run_at(
                    &task.schedule_kind,
                    task.interval_minutes,
                    task.daily_time.as_deref(),
                    Local::now(),
                )
                .map(|t| t.to_rfc3339())
                .unwrap_or_else(|_| (Local::now() + chrono::Duration::hours(1)).to_rfc3339());
                if let Ok(conn) = db.lock() {
                    let _ = conn.execute(
                        "UPDATE scheduled_tasks SET last_run = ?2, next_run = ?3 WHERE id = ?1",
                        params![task.id, chrono::Utc::now().to_rfc3339(), next],
                    );
                }
                match result {
                    Ok(detail) => log::info!("scheduled task {} ran: {detail}", task.name),
                    Err(e) => log::warn!("scheduled task {} failed: {e}", task.name),
                }
            })
            .map_err(|e| AppError::Other(format!("调度线程创建失败：{e}")))?;
    }
    Ok(())
}

/// 执行一个到期任务。pipeline 复用 T-23 compile + T-24 submit_dag。
fn execute_scheduled(
    db: &Arc<Mutex<Connection>>,
    task_manager: &Arc<crate::task::manager::TaskManager>,
    task: &ScheduledTask,
) -> AppResult<String> {
    match task.kind.as_str() {
        "script_action" => {
            let action_id: i64 = task
                .target_id
                .parse()
                .map_err(|_| AppError::Other("定时任务引用的动作 id 不合法".to_string()))?;
            let action = {
                let conn = db.lock().unwrap_or_else(|e| e.into_inner());
                let mut stmt = conn.prepare(
                    "SELECT id, name, command, scope, timeout_secs, created_at, updated_at \
                         FROM plugin_actions WHERE id = ?1",
                )?;
                let mut rows = stmt.query_map(params![action_id], |row| {
                    Ok(PluginAction {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        command: row.get(2)?,
                        scope: row.get(3)?,
                        timeout_secs: row.get::<_, i64>(4)? as u64,
                        created_at: row.get(5)?,
                        updated_at: row.get(6)?,
                    })
                })?;
                let found = rows.next().transpose()?;
                found.ok_or_else(|| AppError::NotFound(format!("动作 {action_id} 不存在")))?
            };
            let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            let mut command = script_command(&action.command);
            command
                .current_dir(&cwd)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                command.creation_flags(CREATE_NO_WINDOW);
            }
            let child = command
                .spawn()
                .map_err(|e| AppError::Other(format!("脚本启动失败：{e}")))?;
            let output = wait_with_streams(child, Duration::from_secs(action.timeout_secs.max(1)))?;
            Ok(format!(
                "exit={:?} out_len={}",
                output.code,
                output.stdout.len()
            ))
        }
        "pipeline" => {
            let template = crate::core::pipeline::load_templates()
                .into_iter()
                .find(|p| p.id == task.target_id)
                .ok_or_else(|| {
                    AppError::NotFound(format!("Pipeline 模板 {} 不存在", task.target_id))
                })?;
            let repos: Vec<RepoSelection> = task
                .payload
                .as_deref()
                .filter(|s| !s.is_empty())
                .map(|s| serde_json::from_str(s))
                .transpose()
                .map_err(|e| AppError::Other(format!("payload 仓库选择解析失败：{e}")))?
                .unwrap_or_default();
            let request =
                crate::core::pipeline::compile_pipeline(&template, &repos, Default::default())
                    .map_err(AppError::Task)?;
            let run_id = task_manager.submit_dag(&request)?;
            Ok(format!("pipeline run {run_id}"))
        }
        other => Err(AppError::Other(format!("未知任务类型：{other}"))),
    }
}

// ---------------------------------------------------------------------------
// 模板导入 / 导出（T-23 Pipeline 模板）
// ---------------------------------------------------------------------------

/// 导出模板 JSON 到指定文件路径，返回路径。
#[tauri::command]
pub fn export_pipeline_template(template_id: String, file_path: String) -> AppResult<String> {
    let template = crate::core::pipeline::load_templates()
        .into_iter()
        .find(|p| p.id == template_id)
        .ok_or_else(|| AppError::NotFound(format!("模板 {template_id} 不存在")))?;
    let json = serde_json::to_vec_pretty(&template)?;
    if let Some(parent) = PathBuf::from(&file_path).parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&file_path, json)?;
    Ok(file_path)
}

/// 从 JSON 文件导入模板：校验 + 分配新 id 保存（避免覆盖已有模板）。
#[tauri::command]
pub fn import_pipeline_template(file_path: String) -> AppResult<Pipeline> {
    let content = std::fs::read_to_string(&file_path)?;
    let mut template: Pipeline = serde_json::from_str(&content)
        .map_err(|e| AppError::Other(format!("模板 JSON 解析失败：{e}")))?;
    crate::core::pipeline::validate_pipeline(&template).map_err(AppError::Task)?;
    let mut all = crate::core::pipeline::load_templates();
    let now = chrono::Utc::now().to_rfc3339();
    template.id = uuid::Uuid::new_v4().to_string();
    template.created_at = now.clone();
    template.updated_at = now;
    all.push(template.clone());
    crate::core::pipeline::save_templates(&all)?;
    Ok(template)
}

// ---------------------------------------------------------------------------
// tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interval_next_run() {
        let now = Local::now();
        let next = next_run_at("interval", Some(15), None, now).unwrap();
        assert!(next - now >= chrono::Duration::minutes(14));
    }

    #[test]
    fn daily_next_run_rolls_to_tomorrow() {
        let now = Local::now();
        let next = next_run_at("daily", None, Some("00:00"), now).unwrap();
        // HH:MM 已过（凌晨 0 点）→ 明天 0 点（按日历日断言，避免 DST 小时漂移）
        assert_eq!(next.hour(), 0);
        assert_eq!(next.minute(), 0);
        let diff_days = (next.date_naive() - now.date_naive()).num_days();
        assert_eq!(diff_days, 1);
    }

    #[test]
    fn invalid_schedule_rejected() {
        assert!(next_run_at("interval", Some(0), None, Local::now()).is_err());
        assert!(next_run_at("daily", None, Some("25:00"), Local::now()).is_err());
        assert!(next_run_at("cron", None, None, Local::now()).is_err());
    }
}
