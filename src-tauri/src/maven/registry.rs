//! Maven 可执行体注册表持久化（R-05，§18）。
//!
//! 复用 T-03 SQLite 数据层：WAL / 单写者 / 版本化迁移 / 批量事务（全局约束 §7）。
//! 缓存 `mvn -v` 探测结果：按 `executable_path` + `project_path` 唯一键 upsert，
//! 探测失败的条目保留 `is_valid=false` 而非删除，便于用户排查后重检（与 R-04
//! JDK 注册表同策略）。

use rusqlite::{params, Connection};

use crate::error::AppResult;
use crate::maven::exec_model::{MavenExecutable, MavenSource};

/// 单条 Maven 可执行体 upsert：按 `executable_path` 唯一键冲突更新。返回行 id。
pub fn upsert_maven_executable(conn: &Connection, exe: &MavenExecutable) -> AppResult<i64> {
    let now = chrono::Utc::now().to_rfc3339();
    let project_path = exe.project_path.as_deref();
    conn.execute(
        "INSERT INTO maven_executables (
            executable_path, project_path, source, major_version, full_version,
            is_valid, last_checked, raw_version, created_at, updated_at
        ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?9)
        ON CONFLICT(executable_path) DO UPDATE SET
            project_path = excluded.project_path,
            source = excluded.source,
            major_version = excluded.major_version,
            full_version = excluded.full_version,
            is_valid = excluded.is_valid,
            last_checked = excluded.last_checked,
            raw_version = excluded.raw_version,
            updated_at = excluded.updated_at",
        params![
            exe.executable_path,
            project_path,
            exe.source.as_str(),
            exe.major_version.map(|v| v as i64),
            exe.full_version,
            exe.is_valid as i64,
            exe.last_checked,
            exe.raw_version,
            now,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// 全量读取注册表（按 source 优先级升序、is_valid 降序）。
pub fn list_maven_executables(conn: &Connection) -> AppResult<Vec<MavenExecutable>> {
    let mut stmt = conn.prepare(
        "SELECT id, executable_path, project_path, source, major_version, full_version,
                is_valid, last_checked, raw_version, created_at, updated_at
         FROM maven_executables
         ORDER BY is_valid DESC, source ASC, executable_path ASC",
    )?;
    let rows = stmt.query_map([], row_to_exe)?;
    let mut out: Vec<MavenExecutable> = rows.collect::<Result<_, _>>()?;
    out.sort_by(|a, b| {
        b.is_valid
            .cmp(&a.is_valid)
            .then_with(|| a.source.priority().cmp(&b.source.priority()))
            .then_with(|| a.executable_path.cmp(&b.executable_path))
    });
    Ok(out)
}

/// 按 id 取单条。
pub fn get_maven_executable(conn: &Connection, id: i64) -> AppResult<Option<MavenExecutable>> {
    let mut stmt = conn.prepare(
        "SELECT id, executable_path, project_path, source, major_version, full_version,
                is_valid, last_checked, raw_version, created_at, updated_at
         FROM maven_executables WHERE id = ?1",
    )?;
    let mut rows = stmt.query_map(params![id], row_to_exe)?;
    Ok(rows.next().transpose()?)
}

/// 按 id 删除。
pub fn remove_maven_executable(conn: &Connection, id: i64) -> AppResult<()> {
    conn.execute("DELETE FROM maven_executables WHERE id = ?1", params![id])?;
    Ok(())
}

/// 更新单条的有效性与校验时间。
pub fn mark_validity(conn: &Connection, id: i64, is_valid: bool, last_checked: &str) -> AppResult<()> {
    conn.execute(
        "UPDATE maven_executables SET is_valid = ?2, last_checked = ?3, updated_at = ?3 WHERE id = ?1",
        params![id, is_valid as i64, last_checked],
    )?;
    Ok(())
}

/// 用探测得到的版本信息更新单条全部字段（强制复检用）。
pub fn apply_version(conn: &Connection, id: i64, exe: &MavenExecutable, last_checked: &str) -> AppResult<()> {
    conn.execute(
        "UPDATE maven_executables SET
            major_version = ?2, full_version = ?3, is_valid = ?4,
            last_checked = ?5, raw_version = ?6, updated_at = ?5
         WHERE id = ?1",
        params![
            id,
            exe.major_version.map(|v| v as i64),
            exe.full_version,
            exe.is_valid as i64,
            last_checked,
            exe.raw_version,
        ],
    )?;
    Ok(())
}

/// 惰性校验：把 `executable_path` 已不存在的条目标记 `is_valid=false`。
/// 返回被标记失效的条数（与 R-04 `prune_invalid_homes` 同策略）。
pub fn prune_invalid_paths(conn: &mut Connection) -> AppResult<usize> {
    let now = chrono::Utc::now().to_rfc3339();
    let mut stmt = conn.prepare("SELECT id, executable_path FROM maven_executables WHERE is_valid = 1")?;
    let rows = stmt.query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)))?;
    let stale: Vec<i64> = rows
        .filter_map(Result::ok)
        .filter(|(_, path)| !std::path::Path::new(path).is_file())
        .map(|(id, _)| id)
        .collect();
    drop(stmt);
    if stale.is_empty() {
        return Ok(0);
    }
    let tx = conn.transaction()?;
    let mut changed = 0usize;
    {
        let mut upd = tx.prepare_cached(
            "UPDATE maven_executables SET is_valid = 0, last_checked = ?2, updated_at = ?2 WHERE id = ?1",
        )?;
        for id in &stale {
            changed += upd.execute(params![id, &now])?;
        }
    }
    tx.commit()?;
    Ok(changed)
}

fn row_to_exe(row: &rusqlite::Row) -> rusqlite::Result<MavenExecutable> {
    Ok(MavenExecutable {
        id: Some(row.get("id")?),
        executable_path: row.get("executable_path")?,
        project_path: row.get("project_path")?,
        source: MavenSource::parse(&row.get::<_, String>("source")?),
        major_version: row.get::<_, Option<i64>>("major_version")?.map(|v| v as u32),
        full_version: row.get("full_version")?,
        is_valid: row.get::<_, i64>("is_valid")? != 0,
        last_checked: row.get("last_checked")?,
        raw_version: row.get("raw_version")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open_db() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        crate::db::init_db(&mut conn).unwrap();
        conn
    }

    fn sample(path: &str, source: MavenSource) -> MavenExecutable {
        let mut exe = MavenExecutable::new(path, source, None);
        exe.major_version = Some(3);
        exe.full_version = Some("3.9.6".into());
        exe.is_valid = true;
        exe.last_checked = "2026-01-01T00:00:00Z".into();
        exe.raw_version = Some("Apache Maven 3.9.6".into());
        exe
    }

    #[test]
    fn upsert_inserts_then_updates_by_path() {
        let conn = open_db();
        let id = upsert_maven_executable(&conn, &sample("/mvn", MavenSource::System)).unwrap();
        assert!(id > 0);
        assert_eq!(list_maven_executables(&conn).unwrap().len(), 1);

        let mut updated = sample("/mvn", MavenSource::System);
        updated.full_version = Some("3.9.9".into());
        upsert_maven_executable(&conn, &updated).unwrap();
        let all = list_maven_executables(&conn).unwrap();
        assert_eq!(all.len(), 1, "upsert by path must not duplicate");
        assert_eq!(all[0].full_version.as_deref(), Some("3.9.9"));
    }

    #[test]
    fn wrapper_and_system_distinguished_by_project_path() {
        let conn = open_db();
        let mut wrapper = sample("/proj/mvnw", MavenSource::ProjectWrapper);
        wrapper.project_path = Some("/proj".into());
        upsert_maven_executable(&conn, &wrapper).unwrap();
        upsert_maven_executable(&conn, &sample("/usr/bin/mvn", MavenSource::System)).unwrap();
        let all = list_maven_executables(&conn).unwrap();
        assert_eq!(all.len(), 2, "wrapper and system are distinct rows");
    }

    #[test]
    fn list_orders_valid_and_source_priority() {
        let conn = open_db();
        upsert_maven_executable(&conn, &sample("/sys", MavenSource::System)).unwrap();
        upsert_maven_executable(&conn, &sample("/cfg", MavenSource::Configured)).unwrap();
        let all = list_maven_executables(&conn).unwrap();
        // 同有效时 configured (priority 1) 排在 system (priority 2) 前。
        assert_eq!(all[0].source, MavenSource::Configured);
        assert_eq!(all[1].source, MavenSource::System);
    }

    #[test]
    fn prune_marks_missing_path_invalid() {
        let mut conn = open_db();
        upsert_maven_executable(&conn, &sample("/gone", MavenSource::System)).unwrap();
        let tmp = std::env::temp_dir().join(format!(
            "gw_mvn_prune_{}",
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        std::fs::write(&tmp, b"mvn").unwrap();
        let real = sample(tmp.to_str().unwrap(), MavenSource::System);
        upsert_maven_executable(&conn, &real).unwrap();

        let changed = prune_invalid_paths(&mut conn).unwrap();
        assert!(changed >= 1, "missing path must be marked invalid");
        let all = list_maven_executables(&conn).unwrap();
        let gone = all.iter().find(|e| e.executable_path == "/gone").unwrap();
        assert!(!gone.is_valid);
        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn remove_deletes_row() {
        let conn = open_db();
        let id = upsert_maven_executable(&conn, &sample("/rm", MavenSource::System)).unwrap();
        assert_eq!(list_maven_executables(&conn).unwrap().len(), 1);
        remove_maven_executable(&conn, id).unwrap();
        assert!(get_maven_executable(&conn, id).unwrap().is_none());
    }
}
