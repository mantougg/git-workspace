//! `runtime_processes` 表 DAO（R-10，§33）。
//!
//! 每启动一个 Runtime 应用写入一行；状态迁移、指标、端口、退出码都落在
//! 这一行上。表只是 OS 进程状态的**缓存**——权威状态核对走
//! [`crate::process::process_alive`]（manager 的 reconcile / Stop 路径）。

use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};

use crate::error::{AppError, AppResult};
use crate::runtime::build::RunStrategy;
use crate::runtime::launch::{LifecycleStatus, RuntimeProcessInfo};

/// `runtime_processes` 行的内存镜像。
#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeProcessRow {
    pub id: i64,
    pub workspace_id: i64,
    pub runtime_name: String,
    pub pid: Option<u32>,
    pub pid_start_time: Option<u64>,
    pub status: LifecycleStatus,
    pub run_strategy: Option<RunStrategy>,
    pub command_preview: Option<String>,
    pub working_dir: Option<String>,
    pub ports: Vec<u16>,
    pub exit_code: Option<i32>,
    pub cpu_percent: Option<f32>,
    pub memory_bytes: Option<u64>,
    pub adopted: bool,
    pub started_at: String,
    pub stopped_at: Option<String>,
    pub last_seen_at: Option<String>,
    pub updated_at: String,
}

const COLUMNS: &str = "id, workspace_id, runtime_name, pid, pid_start_time, status,
     run_strategy, command_preview, working_dir, ports_json, exit_code,
     cpu_percent, memory_bytes, adopted, started_at, stopped_at, last_seen_at, updated_at";

fn now() -> String {
    Utc::now().to_rfc3339()
}

/// 新建一行（状态 `Created`）；返回行 id（`MARKER_PROCESS_ID` 的值）。
pub fn insert_process(conn: &Connection, workspace_id: i64, runtime_name: &str) -> AppResult<i64> {
    let now = now();
    conn.execute(
        "INSERT INTO runtime_processes (workspace_id, runtime_name, status, started_at, updated_at)
         VALUES (?1, ?2, 'created', ?3, ?3)",
        params![workspace_id, runtime_name, now],
    )?;
    Ok(conn.last_insert_rowid())
}

/// 生命周期迁移的 SQL 落点：读-校验-写在调用方持有的同一连接锁内完成
/// （单写者连接 + 互斥锁序列化并发迁移）。
///
/// - 终态迁移自动写 `stopped_at`；`exit_code` 传 `Some(Some(code))` 一并落列。
/// - 非法迁移（[`LifecycleStatus::can_transition`]）返回错误，不改库。
pub fn transition_status(
    conn: &Connection,
    id: i64,
    to: LifecycleStatus,
    exit_code: Option<Option<i32>>,
) -> AppResult<(LifecycleStatus, LifecycleStatus)> {
    let row = get_process(conn, id)?
        .ok_or_else(|| AppError::NotFound(format!("runtime_processes 行 {id} 不存在")))?;
    let from = row.status;
    from.transition(to)?;
    let now = now();
    let stopped_at = if to.is_terminal() { Some(now.clone()) } else { None };
    conn.execute(
        "UPDATE runtime_processes
         SET status = ?2,
             exit_code = COALESCE(?3, exit_code),
             stopped_at = COALESCE(?4, stopped_at),
             updated_at = ?5
         WHERE id = ?1",
        params![id, to.as_str(), exit_code.flatten(), stopped_at, now],
    )?;
    Ok((from, to))
}

/// spawn 前落启动命令元数据（§3 可追溯）：即使 spawn 失败，预览也已留痕。
pub fn set_launched_meta(
    conn: &Connection,
    id: i64,
    strategy: RunStrategy,
    command_preview: &str,
    working_dir: &std::path::Path,
) -> AppResult<()> {
    conn.execute(
        "UPDATE runtime_processes
         SET run_strategy = ?2, command_preview = ?3, working_dir = ?4, updated_at = ?5
         WHERE id = ?1",
        params![
            id,
            strategy.as_str(),
            command_preview,
            working_dir.to_string_lossy().to_string(),
            now()
        ],
    )?;
    Ok(())
}

/// spawn 成功后回填进程身份（pid + start_time，后者防 PID 复用）。
pub fn set_pid(
    conn: &Connection,
    id: i64,
    pid: u32,
    pid_start_time: Option<u64>,
) -> AppResult<()> {
    conn.execute(
        "UPDATE runtime_processes
         SET pid = ?2, pid_start_time = ?3, updated_at = ?4
         WHERE id = ?1",
        params![id, pid, pid_start_time.map(|t| t as i64), now()],
    )?;
    Ok(())
}

/// 指标采样落库（节流调用，非每次采样都写）。
pub fn set_metrics(
    conn: &Connection,
    id: i64,
    cpu_percent: f32,
    memory_bytes: u64,
) -> AppResult<()> {
    conn.execute(
        "UPDATE runtime_processes
         SET cpu_percent = ?2, memory_bytes = ?3, last_seen_at = ?4, updated_at = ?4
         WHERE id = ?1",
        params![id, cpu_percent, memory_bytes as i64, now()],
    )?;
    Ok(())
}

/// 启动日志探测到的端口（去重后整体覆盖）。
pub fn set_ports(conn: &Connection, id: i64, ports: &[u16]) -> AppResult<()> {
    let json = serde_json::to_string(ports)?;
    conn.execute(
        "UPDATE runtime_processes SET ports_json = ?2, updated_at = ?3 WHERE id = ?1",
        params![id, json, now()],
    )?;
    Ok(())
}

/// 标记该行为「重启后接管的孤儿进程」。
pub fn set_adopted(conn: &Connection, id: i64) -> AppResult<()> {
    conn.execute(
        "UPDATE runtime_processes SET adopted = 1, updated_at = ?2 WHERE id = ?1",
        params![id, now()],
    )?;
    Ok(())
}

pub fn get_process(conn: &Connection, id: i64) -> AppResult<Option<RuntimeProcessRow>> {
    conn.query_row(
        &format!("SELECT {COLUMNS} FROM runtime_processes WHERE id = ?1"),
        params![id],
        map_row,
    )
    .optional()
    .map_err(AppError::from)
}

/// 某 workspace 的全部进程记录（新的在前）。
pub fn list_processes(conn: &Connection, workspace_id: i64) -> AppResult<Vec<RuntimeProcessRow>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLUMNS} FROM runtime_processes WHERE workspace_id = ?1 ORDER BY id DESC"
    ))?;
    let rows = stmt.query_map(params![workspace_id], map_row)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(AppError::from)
}

/// 非终态行（reconcile 的候选集：GitWorkspace 上次退出时仍在生命周期内的）。
pub fn list_unfinished(conn: &Connection, workspace_id: i64) -> AppResult<Vec<RuntimeProcessRow>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLUMNS} FROM runtime_processes
         WHERE workspace_id = ?1 AND status NOT IN ('stopped', 'failed')
         ORDER BY id"
    ))?;
    let rows = stmt.query_map(params![workspace_id], map_row)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(AppError::from)
}

/// 某 Runtime 当前活跃（非终态）的最新一行；Start 的重复启动守卫用。
pub fn find_active(
    conn: &Connection,
    workspace_id: i64,
    runtime_name: &str,
) -> AppResult<Option<RuntimeProcessRow>> {
    conn.query_row(
        &format!(
            "SELECT {COLUMNS} FROM runtime_processes
             WHERE workspace_id = ?1 AND runtime_name = ?2
               AND status NOT IN ('stopped', 'failed')
             ORDER BY id DESC LIMIT 1"
        ),
        params![workspace_id, runtime_name],
        map_row,
    )
    .optional()
    .map_err(AppError::from)
}

/// 行 → IPC-ready 信息快照（uptime 按当前时间计算；终态行按 stopped_at）。
pub fn row_to_info(row: &RuntimeProcessRow) -> RuntimeProcessInfo {
    let started = chrono::DateTime::parse_from_rfc3339(&row.started_at).ok();
    let uptime = started.map(|started| {
        let end = row
            .stopped_at
            .as_deref()
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(Utc::now);
        end.signed_duration_since(started.with_timezone(&Utc))
            .num_seconds()
            .max(0) as u64
    });
    RuntimeProcessInfo {
        process_id: row.id,
        workspace_id: row.workspace_id,
        runtime_name: row.runtime_name.clone(),
        pid: row.pid,
        status: row.status,
        run_strategy: row.run_strategy,
        command_preview: row.command_preview.clone(),
        working_dir: row.working_dir.clone(),
        ports: row.ports.clone(),
        exit_code: row.exit_code,
        adopted: row.adopted,
        started_at: row.started_at.clone(),
        stopped_at: row.stopped_at.clone(),
        uptime_seconds: uptime,
        cpu_percent: row.cpu_percent,
        memory_bytes: row.memory_bytes,
    }
}

fn map_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<RuntimeProcessRow> {
    let status_text: String = row.get(5)?;
    let strategy_text: Option<String> = row.get(6)?;
    let ports_json: String = row.get(9)?;
    Ok(RuntimeProcessRow {
        id: row.get(0)?,
        workspace_id: row.get(1)?,
        runtime_name: row.get(2)?,
        pid: row.get::<_, Option<i64>>(3)?.map(|pid| pid as u32),
        pid_start_time: row.get::<_, Option<i64>>(4)?.map(|t| t as u64),
        status: LifecycleStatus::parse(&status_text).ok_or_else(|| {
            rusqlite::Error::FromSqlConversionFailure(
                5,
                rusqlite::types::Type::Text,
                Box::new(AppError::RuntimeConfig(format!(
                    "runtime_processes.status 含未知值 '{status_text}'"
                ))),
            )
        })?,
        run_strategy: strategy_text.as_deref().and_then(RunStrategy::parse),
        command_preview: row.get(7)?,
        working_dir: row.get(8)?,
        ports: serde_json::from_str(&ports_json).unwrap_or_default(),
        exit_code: row.get(10)?,
        cpu_percent: row.get(11)?,
        memory_bytes: row.get::<_, Option<i64>>(12)?.map(|m| m as u64),
        adopted: row.get::<_, i64>(13)? != 0,
        started_at: row.get(14)?,
        stopped_at: row.get(15)?,
        last_seen_at: row.get(16)?,
        updated_at: row.get(17)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open_db() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        crate::db::init_db(&mut conn).unwrap();
        conn.execute(
            "INSERT INTO workspaces (name, path, created_at, updated_at) VALUES ('w', '/tmp/w', 't', 't')",
            [],
        )
        .unwrap();
        conn
    }

    #[test]
    fn insert_launch_transition_and_read_back() {
        let conn = open_db();
        let id = insert_process(&conn, 1, "app").unwrap();
        let row = get_process(&conn, id).unwrap().unwrap();
        assert_eq!(row.status, LifecycleStatus::Created);
        assert_eq!(row.runtime_name, "app");

        let (from, to) = transition_status(&conn, id, LifecycleStatus::Preparing, None).unwrap();
        assert_eq!((from, to), (LifecycleStatus::Created, LifecycleStatus::Preparing));

        set_launched_meta(
            &conn,
            id,
            RunStrategy::ClasspathRun,
            "java -cp ... com.example.App",
            std::path::Path::new("/ws/app"),
        )
        .unwrap();
        set_pid(&conn, id, 4321, Some(1_700_000_000)).unwrap();
        set_ports(&conn, id, &[8080, 9090]).unwrap();
        set_metrics(&conn, id, 12.5, 256 * 1024 * 1024).unwrap();

        let row = get_process(&conn, id).unwrap().unwrap();
        assert_eq!(row.pid, Some(4321));
        assert_eq!(row.pid_start_time, Some(1_700_000_000));
        assert_eq!(row.run_strategy, Some(RunStrategy::ClasspathRun));
        // ports 原样 round-trip（探测侧负责去重）。
        assert_eq!(row.ports, vec![8080, 9090]);
        assert_eq!(row.cpu_percent, Some(12.5));
        assert_eq!(row.memory_bytes, Some(256 * 1024 * 1024));

        // 走到 Running 再自然退出（Running → Stopped 自然终止边）。
        for status in [
            LifecycleStatus::Resolving,
            LifecycleStatus::Building,
            LifecycleStatus::Starting,
            LifecycleStatus::Running,
            LifecycleStatus::Stopped,
        ] {
            transition_status(&conn, id, status, None).unwrap();
        }
        let row = get_process(&conn, id).unwrap().unwrap();
        assert_eq!(row.status, LifecycleStatus::Stopped);
        assert!(row.stopped_at.is_some());

        // 终态后冻结。
        assert!(transition_status(&conn, id, LifecycleStatus::Running, None).is_err());
    }

    #[test]
    fn exit_code_lands_with_terminal_transition() {
        let conn = open_db();
        let id = insert_process(&conn, 1, "app").unwrap();
        transition_status(&conn, id, LifecycleStatus::Preparing, None).unwrap();
        let (_, to) = transition_status(&conn, id, LifecycleStatus::Failed, Some(Some(137))).unwrap();
        assert_eq!(to, LifecycleStatus::Failed);
        let row = get_process(&conn, id).unwrap().unwrap();
        assert_eq!(row.exit_code, Some(137));
        assert!(row.stopped_at.is_some());
    }

    #[test]
    fn find_active_and_unfinished_skip_terminal_rows() {
        let conn = open_db();
        let running = insert_process(&conn, 1, "app").unwrap();
        transition_status(&conn, running, LifecycleStatus::Preparing, None).unwrap();
        let stopped = insert_process(&conn, 1, "app").unwrap();
        transition_status(&conn, stopped, LifecycleStatus::Preparing, None).unwrap();
        transition_status(&conn, stopped, LifecycleStatus::Failed, None).unwrap();

        let active = find_active(&conn, 1, "app").unwrap().unwrap();
        assert_eq!(active.id, running);
        assert!(find_active(&conn, 1, "other").unwrap().is_none());

        let unfinished = list_unfinished(&conn, 1).unwrap();
        assert_eq!(unfinished.len(), 1);
        assert_eq!(unfinished[0].id, running);

        let all = list_processes(&conn, 1).unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].id, stopped, "newest first");
    }

    #[test]
    fn row_to_info_computes_uptime_and_carries_fields() {
        let conn = open_db();
        let id = insert_process(&conn, 1, "app").unwrap();
        set_launched_meta(
            &conn,
            id,
            RunStrategy::MavenRun,
            "mvn spring-boot:run",
            std::path::Path::new("/ws"),
        )
        .unwrap();
        set_pid(&conn, id, 1, None).unwrap();
        let row = get_process(&conn, id).unwrap().unwrap();
        let info = row_to_info(&row);
        assert_eq!(info.process_id, id);
        assert_eq!(info.runtime_name, "app");
        assert_eq!(info.status, LifecycleStatus::Created);
        assert_eq!(info.run_strategy, Some(RunStrategy::MavenRun));
        assert!(info.uptime_seconds.is_some());
        assert!(!info.adopted);

        // serde 形状：camelCase + status 小写串（R-12 IPC 契约预览）。
        let json = serde_json::to_value(&info).unwrap();
        assert_eq!(json["processId"], id);
        assert_eq!(json["runtimeName"], "app");
        assert_eq!(json["status"], "created");
        assert_eq!(json["runStrategy"], "mavenRun");
        assert!(json.get("runtime_name").is_none());
    }
}
