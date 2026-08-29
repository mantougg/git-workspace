//! Queries over the operation log: paged listing, per-log detail loading.
//!
//! Kept out of db/dao.rs while parallel task agents share the tree —
//! single-writer rules still apply: short transactions, prepared statement,
//! no per-row round trips.

use rusqlite::{params, Connection, OptionalExtension};

use crate::error::{AppError, AppResult};

use super::{OperationLogDetail, OperationLogItem, OperationLogPage, OperationLogSummary};

/// Query filters for the operation log list. `date_from`/`date_to` are
/// `YYYY-MM-DD` bounds compared against the UTC date part of `created_at`.
#[derive(Debug, Default)]
pub(crate) struct LogFilter<'a> {
    pub workspace_id: Option<i64>,
    pub repo_path: Option<&'a str>,
    pub op_type: Option<&'a str>,
    pub date_from: Option<&'a str>,
    pub date_to: Option<&'a str>,
}

const LOG_WHERE: &str = "WHERE (?1 IS NULL OR l.workspace_id = ?1)
       AND (?2 IS NULL OR l.op_type = ?2)
       AND (?3 IS NULL OR EXISTS (SELECT 1 FROM operation_log_items x
                                  WHERE x.log_id = l.id
                                    AND x.repo_path LIKE '%' || ?3 || '%'))
       AND (?4 IS NULL OR substr(l.created_at, 1, 10) >= ?4)
       AND (?5 IS NULL OR substr(l.created_at, 1, 10) <= ?5)";

/// Query one page of operation logs (newest first) plus the total count.
pub(crate) fn query_operation_logs(
    conn: &Connection,
    filter: &LogFilter,
    limit: i64,
    offset: i64,
) -> AppResult<OperationLogPage> {
    let where_params = params![
        filter.workspace_id,
        filter.op_type,
        filter.repo_path,
        filter.date_from,
        filter.date_to
    ];
    let total: i64 = conn.query_row(
        &format!("SELECT COUNT(*) FROM operation_logs l {LOG_WHERE}"),
        where_params,
        |r| r.get(0),
    )?;

    let mut stmt = conn.prepare(&format!(
        "SELECT l.id, l.workspace_id, l.op_type, l.summary, l.created_at, l.undone_at,
                (SELECT COUNT(*) FROM operation_log_items i WHERE i.log_id = l.id),
                (SELECT COUNT(*) FROM operation_log_items i WHERE i.log_id = l.id AND i.undone_at IS NOT NULL)
         FROM operation_logs l {LOG_WHERE}
         ORDER BY l.id DESC LIMIT ?6 OFFSET ?7"
    ))?;
    let logs = stmt
        .query_map(
            params![
                filter.workspace_id,
                filter.op_type,
                filter.repo_path,
                filter.date_from,
                filter.date_to,
                limit,
                offset
            ],
            |row| {
                Ok(OperationLogSummary {
                    id: row.get(0)?,
                    workspace_id: row.get(1)?,
                    op_type: row.get(2)?,
                    summary: row.get(3)?,
                    created_at: row.get(4)?,
                    undone_at: row.get(5)?,
                    repo_count: row.get(6)?,
                    undone_count: row.get(7)?,
                })
            },
        )?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(OperationLogPage { total, logs })
}

/// Load one operation log with all its per-repo items.
pub(crate) fn get_operation_log(conn: &Connection, log_id: i64) -> AppResult<OperationLogDetail> {
    let (id, workspace_id, op_type, summary, created_at, undone_at) = conn
        .query_row(
            "SELECT id, workspace_id, op_type, summary, created_at, undone_at
             FROM operation_logs WHERE id = ?1",
            params![log_id],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, Option<i64>>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, Option<String>>(5)?,
                ))
            },
        )
        .optional()?
        .ok_or_else(|| AppError::NotFound(format!("operation log {} not found", log_id)))?;

    let mut stmt = conn.prepare(
        "SELECT id, log_id, repo_path, ref_name, before_oid, after_oid, detail, undone_at
         FROM operation_log_items WHERE log_id = ?1 ORDER BY id",
    )?;
    let items = stmt
        .query_map(params![log_id], |row| {
            Ok(OperationLogItem {
                id: row.get(0)?,
                log_id: row.get(1)?,
                repo_path: row.get(2)?,
                ref_name: row.get(3)?,
                before_oid: row.get(4)?,
                after_oid: row.get(5)?,
                detail: row.get(6)?,
                undone_at: row.get(7)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(OperationLogDetail {
        id,
        workspace_id,
        op_type,
        summary,
        created_at,
        undone_at,
        items,
    })
}
