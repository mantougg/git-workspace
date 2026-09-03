//! JDK 注册表持久化与惰性校验（R-04，§32）。
//!
//! 复用 T-03 SQLite 数据层：WAL / 单写者 / 版本化迁移 / 批量事务（全局约束 §7）。
//! JDK 条目是检测生成 + 用户手动添加的元数据；按 `home_path` 唯一键 upsert，
//! 探测失败的条目保留 `is_valid=false` 而非删除，便于用户排查后重检。

use rusqlite::{params, Connection};

use crate::error::AppResult;
use crate::java::model::{JdkDiscoverySource, JdkInstallation, JdkVendor};

/// 单条 JDK upsert：按 `home_path` 唯一键冲突更新。返回行 id。
pub fn upsert_jdk(conn: &Connection, jdk: &JdkInstallation) -> AppResult<i64> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO jdks (
            home_path, major_version, full_version, vendor, architecture,
            bitness, source, java_exec, javac_exec, is_valid, last_checked,
            raw_version, created_at, updated_at
        ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?13)
        ON CONFLICT(home_path) DO UPDATE SET
            major_version = excluded.major_version,
            full_version = excluded.full_version,
            vendor = excluded.vendor,
            architecture = excluded.architecture,
            bitness = excluded.bitness,
            source = excluded.source,
            java_exec = excluded.java_exec,
            javac_exec = excluded.javac_exec,
            is_valid = excluded.is_valid,
            last_checked = excluded.last_checked,
            raw_version = excluded.raw_version,
            updated_at = excluded.updated_at",
        params![
            jdk.home_path,
            jdk.major_version.map(|v| v as i64),
            jdk.full_version,
            jdk.vendor.map(|v| v.as_str()),
            jdk.architecture,
            jdk.bitness.map(|v| v as i64),
            jdk.source.as_str(),
            jdk.java_exec,
            jdk.javac_exec,
            jdk.is_valid as i64,
            jdk.last_checked,
            jdk.raw_version,
            now,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// 批量事务 upsert：一次事务写完全部候选，遵循单写者 + Prepared Statement。
pub fn upsert_jdks_batch(conn: &mut Connection, jdks: &[JdkInstallation]) -> AppResult<()> {
    let now = chrono::Utc::now().to_rfc3339();
    let tx = conn.transaction()?;
    {
        let mut stmt = tx.prepare_cached(
            "INSERT INTO jdks (
                home_path, major_version, full_version, vendor, architecture,
                bitness, source, java_exec, javac_exec, is_valid, last_checked,
                raw_version, created_at, updated_at
            ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?13)
            ON CONFLICT(home_path) DO UPDATE SET
                major_version = excluded.major_version,
                full_version = excluded.full_version,
                vendor = excluded.vendor,
                architecture = excluded.architecture,
                bitness = excluded.bitness,
                source = excluded.source,
                java_exec = excluded.java_exec,
                javac_exec = excluded.javac_exec,
                is_valid = excluded.is_valid,
                last_checked = excluded.last_checked,
                raw_version = excluded.raw_version,
                updated_at = excluded.updated_at",
        )?;
        for jdk in jdks {
            stmt.execute(params![
                jdk.home_path,
                jdk.major_version.map(|v| v as i64),
                jdk.full_version,
                jdk.vendor.map(|v| v.as_str()),
                jdk.architecture,
                jdk.bitness.map(|v| v as i64),
                jdk.source.as_str(),
                jdk.java_exec,
                jdk.javac_exec,
                jdk.is_valid as i64,
                jdk.last_checked,
                jdk.raw_version,
                now,
            ])?;
        }
    }
    tx.commit()?;
    Ok(())
}

/// 全量读取注册表（按 major version 降序、home_path 升序稳定排序）。
pub fn list_jdks(conn: &Connection) -> AppResult<Vec<JdkInstallation>> {
    let mut stmt = conn.prepare(
        "SELECT id, home_path, major_version, full_version, vendor, architecture,
                bitness, source, java_exec, javac_exec, is_valid, last_checked,
                raw_version, created_at, updated_at
         FROM jdks
         ORDER BY is_valid DESC, major_version DESC, home_path ASC",
    )?;
    let rows = stmt.query_map([], row_to_jdk)?;
    let mut out: Vec<JdkInstallation> = rows.collect::<Result<_, _>>()?;
    out.sort_by(|a, b| {
        // 有效的优先；同有效性下 major 降序；再按路径。
        b.is_valid
            .cmp(&a.is_valid)
            .then_with(|| b.major_version.cmp(&a.major_version))
            .then_with(|| a.home_path.cmp(&b.home_path))
    });
    Ok(out)
}

/// 按 id 取单条。
pub fn get_jdk(conn: &Connection, id: i64) -> AppResult<Option<JdkInstallation>> {
    let mut stmt = conn.prepare(
        "SELECT id, home_path, major_version, full_version, vendor, architecture,
                bitness, source, java_exec, javac_exec, is_valid, last_checked,
                raw_version, created_at, updated_at
         FROM jdks WHERE id = ?1",
    )?;
    let mut rows = stmt.query_map(params![id], row_to_jdk)?;
    Ok(rows.next().transpose()?)
}

/// 按 id 删除。
pub fn remove_jdk(conn: &Connection, id: i64) -> AppResult<()> {
    conn.execute("DELETE FROM jdks WHERE id = ?1", params![id])?;
    Ok(())
}

/// 更新单条的有效性与校验时间（强制复检后调用）。
pub fn mark_validity(conn: &Connection, id: i64, is_valid: bool, last_checked: &str) -> AppResult<()> {
    conn.execute(
        "UPDATE jdks SET is_valid = ?2, last_checked = ?3, updated_at = ?3 WHERE id = ?1",
        params![id, is_valid as i64, last_checked],
    )?;
    Ok(())
}

/// 用一次探测得到的版本信息更新单条的全部版本字段（强制复检用）。
pub fn apply_version(conn: &Connection, id: i64, jdk: &JdkInstallation, last_checked: &str) -> AppResult<()> {
    conn.execute(
        "UPDATE jdks SET
            major_version = ?2, full_version = ?3, vendor = ?4, architecture = ?5,
            bitness = ?6, java_exec = ?7, javac_exec = ?8, is_valid = ?9,
            last_checked = ?10, raw_version = ?11, updated_at = ?10
         WHERE id = ?1",
        params![
            id,
            jdk.major_version.map(|v| v as i64),
            jdk.full_version,
            jdk.vendor.map(|v| v.as_str()),
            jdk.architecture,
            jdk.bitness.map(|v| v as i64),
            jdk.java_exec,
            jdk.javac_exec,
            jdk.is_valid as i64,
            last_checked,
            jdk.raw_version,
        ],
    )?;
    Ok(())
}

/// 惰性校验：把 `home_path` 已不存在的条目标记 `is_valid=false`。
///
/// 遵循全局约束性能原则：只检查路径存在性（不 fork `java -version`），
/// 失效条目保留以便用户重检，而非误用或静默删除。返回被标记失效的条数。
pub fn prune_invalid_homes(conn: &mut Connection) -> AppResult<usize> {
    let now = chrono::Utc::now().to_rfc3339();
    let mut stmt = conn.prepare("SELECT id, home_path FROM jdks WHERE is_valid = 1")?;
    let rows = stmt.query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)))?;
    let mut changed = 0usize;
    let stale: Vec<i64> = rows
        .filter_map(Result::ok)
        .filter(|(_, home)| !std::path::Path::new(home).is_dir())
        .map(|(id, _)| id)
        .collect();
    drop(stmt);
    if stale.is_empty() {
        return Ok(0);
    }
    let tx = conn.transaction()?;
    {
        let mut upd =
            tx.prepare_cached("UPDATE jdks SET is_valid = 0, last_checked = ?2, updated_at = ?2 WHERE id = ?1")?;
        for id in &stale {
            changed += upd.execute(params![id, &now])?;
        }
    }
    tx.commit()?;
    Ok(changed)
}

fn row_to_jdk(row: &rusqlite::Row) -> rusqlite::Result<JdkInstallation> {
    Ok(JdkInstallation {
        id: Some(row.get("id")?),
        home_path: row.get("home_path")?,
        major_version: row.get::<_, Option<i64>>("major_version")?.map(|v| v as u32),
        full_version: row.get("full_version")?,
        vendor: row.get::<_, Option<String>>("vendor")?.map(|s| JdkVendor::parse(&s)),
        architecture: row.get("architecture")?,
        bitness: row.get::<_, Option<i64>>("bitness")?.map(|v| v as u32),
        source: JdkDiscoverySource::parse(&row.get::<_, String>("source")?),
        java_exec: row.get("java_exec")?,
        javac_exec: row.get("javac_exec")?,
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

    fn sample(home: &str, major: u32) -> JdkInstallation {
        let mut jdk = JdkInstallation::new(home, JdkDiscoverySource::System);
        jdk.major_version = Some(major);
        jdk.full_version = Some(format!("{major}.0.0"));
        jdk.vendor = Some(JdkVendor::Temurin);
        jdk.is_valid = true;
        jdk.last_checked = "2026-01-01T00:00:00Z".into();
        jdk.raw_version = Some("raw".into());
        jdk
    }

    #[test]
    fn upsert_inserts_then_updates_by_home_path() {
        let conn = open_db();
        let id = upsert_jdk(&conn, &sample("/jdk-17", 17)).unwrap();
        assert!(id > 0);
        assert_eq!(list_jdks(&conn).unwrap().len(), 1);

        // 同一 home_path 二次 upsert：更新而非新增。
        let mut updated = sample("/jdk-17", 17);
        updated.major_version = Some(21);
        updated.full_version = Some("21.0.0".into());
        upsert_jdk(&conn, &updated).unwrap();
        let all = list_jdks(&conn).unwrap();
        assert_eq!(all.len(), 1, "upsert by home_path must not duplicate");
        assert_eq!(all[0].major_version, Some(21));
    }

    #[test]
    fn batch_upsert_is_transactional_and_idempotent() {
        let mut conn = open_db();
        let batch = vec![sample("/j1", 17), sample("/j2", 21), sample("/j3", 8)];
        upsert_jdks_batch(&mut conn, &batch).unwrap();
        assert_eq!(list_jdks(&conn).unwrap().len(), 3);

        // 同一批再写一次：仍 3 行（按 home_path upsert）。
        upsert_jdks_batch(&mut conn, &batch).unwrap();
        assert_eq!(list_jdks(&conn).unwrap().len(), 3);
    }

    #[test]
    fn list_orders_valid_and_major_desc() {
        let conn = open_db();
        upsert_jdk(&conn, &sample("/j8", 8)).unwrap();
        upsert_jdk(&conn, &sample("/j21", 21)).unwrap();
        upsert_jdk(&conn, &sample("/j17", 17)).unwrap();
        let all = list_jdks(&conn).unwrap();
        // 全有效时按 major 降序。
        assert_eq!(all[0].major_version, Some(21));
        assert_eq!(all[1].major_version, Some(17));
        assert_eq!(all[2].major_version, Some(8));
    }

    #[test]
    fn prune_marks_missing_home_invalid_without_deleting() {
        let mut conn = open_db();
        upsert_jdk(&conn, &sample("/existent", 17)).unwrap();
        upsert_jdk(&conn, &sample("/gone", 21)).unwrap();
        // 真实存在与不存在的路径：用 temp 目录造一个存在、一个不存在。
        let tmp = std::env::temp_dir().join(format!(
            "gw_jdk_{}",
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        std::fs::create_dir_all(&tmp).unwrap();
        let mut real = sample(tmp.to_str().unwrap(), 17);
        real.home_path = tmp.to_string_lossy().to_string();
        upsert_jdk(&conn, &real).unwrap();

        // /gone 不存在 -> prune 后标 invalid，real 仍 valid。
        let changed = prune_invalid_homes(&mut conn).unwrap();
        assert!(changed >= 1, "missing home must be marked invalid");
        let all = list_jdks(&conn).unwrap();
        let gone = all.iter().find(|j| j.home_path == "/gone").unwrap();
        assert!(!gone.is_valid, "gone JDK must be invalid, not deleted");
        let real_row = all.iter().find(|j| j.home_path == tmp.to_string_lossy());
        assert!(real_row.is_some_and(|j| j.is_valid), "existing JDK stays valid");

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn remove_deletes_row() {
        let conn = open_db();
        let id = upsert_jdk(&conn, &sample("/rm", 17)).unwrap();
        assert_eq!(list_jdks(&conn).unwrap().len(), 1);
        remove_jdk(&conn, id).unwrap();
        assert!(get_jdk(&conn, id).unwrap().is_none());
        assert_eq!(list_jdks(&conn).unwrap().len(), 0);
    }

    #[test]
    fn apply_version_updates_fields_and_validity() {
        let conn = open_db();
        let id = upsert_jdk(&conn, &sample("/v", 17)).unwrap();
        let mut probed = sample("/v", 21);
        probed.full_version = Some("21.0.2".into());
        probed.vendor = Some(JdkVendor::GraalVm);
        apply_version(&conn, id, &probed, "2026-08-18T00:00:00Z").unwrap();
        let row = get_jdk(&conn, id).unwrap().unwrap();
        assert_eq!(row.major_version, Some(21));
        assert_eq!(row.full_version.as_deref(), Some("21.0.2"));
        assert_eq!(row.vendor, Some(JdkVendor::GraalVm));
    }
}
