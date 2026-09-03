//! Persistent Node.js and package-manager executable registry (N-08).

use rusqlite::{params, Connection};

use crate::error::AppResult;
use crate::node::model::{NodeExecutable, NodeExecutableKind, PackageManager};

pub fn upsert_node_executable(conn: &Connection, entry: &NodeExecutable) -> AppResult<i64> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO node_executables (
            kind, package_manager, executable_path, version, raw_output,
            is_valid, last_checked, created_at, updated_at
        ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?8)
        ON CONFLICT(executable_path) DO UPDATE SET
            kind = excluded.kind,
            package_manager = excluded.package_manager,
            version = excluded.version,
            raw_output = excluded.raw_output,
            is_valid = excluded.is_valid,
            last_checked = excluded.last_checked,
            updated_at = excluded.updated_at",
        params![
            entry.kind.as_str(),
            entry.package_manager.map(PackageManager::name),
            entry.executable_path,
            entry.version,
            entry.raw_output,
            entry.is_valid as i64,
            entry.last_checked,
            now,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn list_node_executables(conn: &Connection) -> AppResult<Vec<NodeExecutable>> {
    let mut stmt = conn.prepare(
        "SELECT id, kind, package_manager, executable_path, version, raw_output,
                is_valid, last_checked, created_at, updated_at
         FROM node_executables
         ORDER BY is_valid DESC, kind ASC, package_manager ASC, executable_path ASC",
    )?;
    let rows = stmt.query_map([], row_to_entry)?;
    let mut entries: Vec<_> = rows.collect::<Result<_, _>>()?;
    entries.sort_by(|a, b| {
        b.is_valid
            .cmp(&a.is_valid)
            .then_with(|| a.kind.as_str().cmp(b.kind.as_str()))
            .then_with(|| {
                a.package_manager
                    .map(PackageManager::name)
                    .cmp(&b.package_manager.map(PackageManager::name))
            })
            .then_with(|| a.executable_path.cmp(&b.executable_path))
    });
    Ok(entries)
}

pub fn get_node_executable(conn: &Connection, id: i64) -> AppResult<Option<NodeExecutable>> {
    let mut stmt = conn.prepare(
        "SELECT id, kind, package_manager, executable_path, version, raw_output,
                is_valid, last_checked, created_at, updated_at
         FROM node_executables WHERE id = ?1",
    )?;
    let mut rows = stmt.query_map(params![id], row_to_entry)?;
    Ok(rows.next().transpose()?)
}

pub fn remove_node_executable(conn: &Connection, id: i64) -> AppResult<()> {
    conn.execute("DELETE FROM node_executables WHERE id = ?1", params![id])?;
    Ok(())
}

pub fn apply_node_probe(conn: &Connection, id: i64, entry: &NodeExecutable, checked_at: &str) -> AppResult<()> {
    conn.execute(
        "UPDATE node_executables SET version = ?2, raw_output = ?3,
         is_valid = ?4, last_checked = ?5, updated_at = ?5 WHERE id = ?1",
        params![id, entry.version, entry.raw_output, entry.is_valid as i64, checked_at],
    )?;
    Ok(())
}

pub fn prune_invalid_paths(conn: &mut Connection) -> AppResult<usize> {
    let now = chrono::Utc::now().to_rfc3339();
    let mut stmt = conn.prepare("SELECT id, executable_path FROM node_executables WHERE is_valid = 1")?;
    let rows = stmt.query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)))?;
    let stale: Vec<i64> = rows
        .filter_map(Result::ok)
        .filter(|(_, path)| !std::path::Path::new(path).is_file())
        .map(|(id, _)| id)
        .collect();
    drop(stmt);
    let tx = conn.transaction()?;
    let mut changed = 0;
    {
        let mut update = tx.prepare_cached(
            "UPDATE node_executables SET is_valid = 0, last_checked = ?2, updated_at = ?2 WHERE id = ?1",
        )?;
        for id in stale {
            changed += update.execute(params![id, &now])?;
        }
    }
    tx.commit()?;
    Ok(changed)
}

pub fn find_valid_node(conn: &Connection) -> AppResult<Option<NodeExecutable>> {
    find_valid(conn, NodeExecutableKind::Node, None)
}

pub fn find_valid_package_manager(conn: &Connection, manager: PackageManager) -> AppResult<Option<NodeExecutable>> {
    find_valid(conn, NodeExecutableKind::PackageManager, Some(manager))
}

fn find_valid(
    conn: &Connection,
    kind: NodeExecutableKind,
    manager: Option<PackageManager>,
) -> AppResult<Option<NodeExecutable>> {
    let entries = list_node_executables(conn)?;
    Ok(entries.into_iter().find(|entry| {
        entry.kind == kind
            && entry.is_valid
            && std::path::Path::new(&entry.executable_path).is_file()
            && entry.package_manager == manager
    }))
}

fn row_to_entry(row: &rusqlite::Row<'_>) -> rusqlite::Result<NodeExecutable> {
    Ok(NodeExecutable {
        id: Some(row.get("id")?),
        kind: NodeExecutableKind::parse(&row.get::<_, String>("kind")?),
        package_manager: row
            .get::<_, Option<String>>("package_manager")?
            .and_then(|value| PackageManager::parse(&value)),
        executable_path: row.get("executable_path")?,
        version: row.get("version")?,
        raw_output: row.get("raw_output")?,
        is_valid: row.get::<_, i64>("is_valid")? != 0,
        last_checked: row.get("last_checked")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        crate::db::init_db(&mut conn).unwrap();
        conn
    }

    fn entry(path: &str, kind: NodeExecutableKind, pm: Option<PackageManager>) -> NodeExecutable {
        let mut value = NodeExecutable::new(kind, pm, path);
        value.version = Some("22.14.0".into());
        value.raw_output = "v22.14.0\n".into();
        value.is_valid = true;
        value.last_checked = "2026-01-01T00:00:00Z".into();
        value
    }

    #[test]
    fn upsert_list_find_and_remove() {
        let conn = db();
        let path = std::env::temp_dir().join(format!("gw-node-reg-{}", uuid::Uuid::new_v4()));
        std::fs::write(&path, b"node").unwrap();
        let mut value = entry(path.to_str().unwrap(), NodeExecutableKind::Node, None);
        let id = upsert_node_executable(&conn, &value).unwrap();
        value.id = Some(id);
        assert_eq!(list_node_executables(&conn).unwrap().len(), 1);
        assert_eq!(find_valid_node(&conn).unwrap().unwrap().id, Some(id));
        remove_node_executable(&conn, id).unwrap();
        assert!(find_valid_node(&conn).unwrap().is_none());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn manager_entries_are_scoped_by_manager() {
        let conn = db();
        let path = std::env::temp_dir().join(format!("gw-pnpm-reg-{}", uuid::Uuid::new_v4()));
        std::fs::write(&path, b"pnpm").unwrap();
        let value = entry(
            path.to_str().unwrap(),
            NodeExecutableKind::PackageManager,
            Some(PackageManager::Pnpm),
        );
        upsert_node_executable(&conn, &value).unwrap();
        assert!(find_valid_package_manager(&conn, PackageManager::Pnpm)
            .unwrap()
            .is_some());
        assert!(find_valid_package_manager(&conn, PackageManager::Yarn)
            .unwrap()
            .is_none());
        let _ = std::fs::remove_file(path);
    }
}
