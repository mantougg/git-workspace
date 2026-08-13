use std::collections::HashMap;

use chrono::Utc;
use rusqlite::{params, Connection};

use crate::error::{AppError, AppResult};
use crate::models::group::{CreateGroupRequest, RepoGroup};
use crate::models::repository::{CommitRecord, Repository, ScannedRepo};
use crate::models::workspace::{CreateWorkspaceRequest, UpdateWorkspaceRequest, Workspace};

// ---------------------------------------------------------------------------
// Workspace DAO
// ---------------------------------------------------------------------------

/// Insert a new workspace and return the created record.
pub fn insert_workspace(
    conn: &Connection,
    req: &CreateWorkspaceRequest,
) -> AppResult<Workspace> {
    let now = Utc::now().to_rfc3339();
    let scan_depth = req.scan_depth.unwrap_or(5);

    conn.execute(
        "INSERT INTO workspaces (name, path, scan_depth, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![req.name, req.path, scan_depth, now, now],
    )?;

    let id = conn.last_insert_rowid();
    Ok(Workspace {
        id,
        name: req.name.clone(),
        path: req.path.clone(),
        scan_depth,
        created_at: now.clone(),
        updated_at: now,
    })
}

/// List all workspaces, ordered by name.
pub fn list_workspaces(conn: &Connection) -> AppResult<Vec<Workspace>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, path, scan_depth, created_at, updated_at FROM workspaces ORDER BY name",
    )?;

    let workspaces = stmt
        .query_map([], |row| {
            Ok(Workspace {
                id: row.get(0)?,
                name: row.get(1)?,
                path: row.get(2)?,
                scan_depth: row.get(3)?,
                created_at: row.get(4)?,
                updated_at: row.get(5)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(workspaces)
}

/// Get a single workspace by ID.
pub fn get_workspace(conn: &Connection, id: i64) -> AppResult<Workspace> {
    conn.query_row(
        "SELECT id, name, path, scan_depth, created_at, updated_at FROM workspaces WHERE id = ?1",
        params![id],
        |row| {
            Ok(Workspace {
                id: row.get(0)?,
                name: row.get(1)?,
                path: row.get(2)?,
                scan_depth: row.get(3)?,
                created_at: row.get(4)?,
                updated_at: row.get(5)?,
            })
        },
    )
    .map_err(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => {
            AppError::NotFound(format!("Workspace {} not found", id))
        }
        other => AppError::Db(other),
    })
}

/// Delete a workspace by ID. Cascades to repositories and groups.
pub fn delete_workspace(conn: &Connection, id: i64) -> AppResult<()> {
    conn.execute("DELETE FROM workspaces WHERE id = ?1", params![id])?;
    Ok(())
}

/// Update workspace name and/or scan_depth.
pub fn update_workspace(
    conn: &Connection,
    id: i64,
    req: &UpdateWorkspaceRequest,
) -> AppResult<Workspace> {
    let current = get_workspace(conn, id)?;
    let now = Utc::now().to_rfc3339();
    let name = req.name.as_ref().unwrap_or(&current.name);
    let scan_depth = req.scan_depth.unwrap_or(current.scan_depth);

    conn.execute(
        "UPDATE workspaces SET name = ?1, scan_depth = ?2, updated_at = ?3 WHERE id = ?4",
        params![name, scan_depth, now, id],
    )?;

    Ok(Workspace {
        id,
        name: name.clone(),
        path: current.path.clone(),
        scan_depth,
        created_at: current.created_at,
        updated_at: now,
    })
}

// ---------------------------------------------------------------------------
// Repository DAO
// ---------------------------------------------------------------------------

/// Insert or update a batch of repositories (upsert by path) in a single
/// transaction. Avoids the per-row implicit-transaction overhead of calling
/// `upsert_repository` in a loop.
pub fn upsert_repositories_batch(
    conn: &mut Connection,
    workspace_id: i64,
    repos: &[ScannedRepo],
) -> AppResult<()> {
    if repos.is_empty() {
        return Ok(());
    }
    let now = Utc::now().to_rfc3339();
    let tx = conn.transaction()?;
    {
        let mut stmt = tx.prepare(
            r#"INSERT INTO repositories (workspace_id, path, name, relative_path, is_deleted, git_dir_mtime, last_scanned, created_at, updated_at)
               VALUES (?1, ?2, ?3, ?4, 0, ?5, ?6, ?7, ?7)
               ON CONFLICT(path) DO UPDATE SET
                   workspace_id = ?1,
                   name = ?3,
                   relative_path = ?4,
                   is_deleted = 0,
                   git_dir_mtime = ?5,
                   last_scanned = ?6,
                   updated_at = ?7"#,
        )?;
        for repo in repos {
            stmt.execute(params![
                workspace_id,
                repo.path,
                repo.name,
                repo.relative_path,
                repo.git_dir_mtime,
                now,
                now
            ])?;
        }
    }
    tx.commit()?;
    Ok(())
}

/// Load the known repository paths (with their last recorded `.git` mtime) for
/// a workspace. This is the incremental-scan cache: the scanner skips libgit2
/// validation for a repo whose path is present and whose mtime is unchanged.
/// Soft-deleted repositories are excluded.
pub fn list_repository_paths(
    conn: &Connection,
    workspace_id: i64,
) -> AppResult<HashMap<String, Option<i64>>> {
    let mut stmt = conn.prepare(
        "SELECT path, git_dir_mtime FROM repositories WHERE workspace_id = ?1 AND is_deleted = 0",
    )?;
    let rows = stmt.query_map(params![workspace_id], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, Option<i64>>(1)?))
    })?;

    let mut map = HashMap::new();
    for row in rows {
        let (path, mtime) = row?;
        map.insert(path, mtime);
    }
    Ok(map)
}

/// List all repositories for a given workspace (excluding soft-deleted ones).
pub fn list_repositories_by_workspace(
    conn: &Connection,
    workspace_id: i64,
) -> AppResult<Vec<Repository>> {
    let mut stmt = conn.prepare(
        r#"SELECT id, workspace_id, path, name, relative_path, is_favorite, tags, group_id
           FROM repositories
           WHERE workspace_id = ?1 AND is_deleted = 0
           ORDER BY name"#,
    )?;

    let repos = stmt
        .query_map(params![workspace_id], |row| {
            let tags_str: String = row.get(6).unwrap_or_else(|_| "[]".to_string());
            let tags: Vec<String> =
                serde_json::from_str(&tags_str).unwrap_or_default();
            Ok(Repository {
                id: Some(row.get(0)?),
                workspace_id: row.get(1)?,
                path: row.get(2)?,
                name: row.get(3)?,
                relative_path: row.get(4)?,
                is_favorite: row.get::<_, i64>(5)? != 0,
                tags,
                group_id: row.get(7)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(repos)
}

/// Soft-delete repositories that are no longer found in the workspace directory.
///
/// Rows are marked `is_deleted = 1` instead of being hard-deleted, so tags,
/// groups, favorites and cached metadata survive a temporary move/removal and
/// are restored on the next successful scan (see `upsert_repositories_batch`).
pub fn cleanup_stale_repositories(
    conn: &Connection,
    workspace_id: i64,
    existing_paths: &[String],
) -> AppResult<()> {
    let now = Utc::now().to_rfc3339();
    // Build placeholder string for the IN clause. The first two bound params
    // are workspace_id and now, so the IN placeholders start at ?3.
    if existing_paths.is_empty() {
        conn.execute(
            "UPDATE repositories SET is_deleted = 1, updated_at = ?2 WHERE workspace_id = ?1 AND is_deleted = 0",
            params![workspace_id, now],
        )?;
    } else {
        let placeholders: Vec<String> = (0..existing_paths.len())
            .map(|i| format!("?{}", i + 3))
            .collect();
        let sql = format!(
            "UPDATE repositories SET is_deleted = 1, updated_at = ?2 WHERE workspace_id = ?1 AND is_deleted = 0 AND path NOT IN ({})",
            placeholders.join(", ")
        );

        let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> =
            vec![Box::new(workspace_id), Box::new(now)];
        for path in existing_paths {
            params_vec.push(Box::new(path.clone()));
        }

        let params_ref: Vec<&dyn rusqlite::ToSql> =
            params_vec.iter().map(|p| p.as_ref()).collect();

        conn.execute(&sql, params_ref.as_slice())?;
    }
    Ok(())
}

/// Soft-delete a specific set of repositories (used by Scan Selected / subtree
/// scans so repositories outside the scanned subtree are never touched).
pub fn soft_delete_repositories(
    conn: &Connection,
    workspace_id: i64,
    paths: &[String],
) -> AppResult<()> {
    if paths.is_empty() {
        return Ok(());
    }
    let now = Utc::now().to_rfc3339();
    let placeholders: Vec<String> = (0..paths.len())
        .map(|i| format!("?{}", i + 3))
        .collect();
    let sql = format!(
        "UPDATE repositories SET is_deleted = 1, updated_at = ?2 WHERE workspace_id = ?1 AND is_deleted = 0 AND path IN ({})",
        placeholders.join(", ")
    );

    let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> =
        vec![Box::new(workspace_id), Box::new(now)];
    for path in paths {
        params_vec.push(Box::new(path.clone()));
    }

    let params_ref: Vec<&dyn rusqlite::ToSql> =
        params_vec.iter().map(|p| p.as_ref()).collect();

    conn.execute(&sql, params_ref.as_slice())?;
    Ok(())
}

/// Toggle favorite status of a repository.
pub fn toggle_favorite(conn: &Connection, repo_id: i64) -> AppResult<()> {
    conn.execute(
        "UPDATE repositories SET is_favorite = CASE WHEN is_favorite = 0 THEN 1 ELSE 0 END WHERE id = ?1",
        params![repo_id],
    )?;
    Ok(())
}

/// Update tags for a repository.
pub fn update_tags(conn: &Connection, repo_id: i64, tags: &[String]) -> AppResult<()> {
    let tags_json = serde_json::to_string(tags)?;
    conn.execute(
        "UPDATE repositories SET tags = ?1 WHERE id = ?2",
        params![tags_json, repo_id],
    )?;
    Ok(())
}

/// Assign a repository to a group.
pub fn assign_group(conn: &Connection, repo_id: i64, group_id: Option<i64>) -> AppResult<()> {
    conn.execute(
        "UPDATE repositories SET group_id = ?1 WHERE id = ?2",
        params![group_id, repo_id],
    )?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Repo Groups DAO
// ---------------------------------------------------------------------------

/// Create a new repository group.
pub fn create_group(conn: &Connection, req: &CreateGroupRequest) -> AppResult<RepoGroup> {
    let sort_order: i32 = conn
        .query_row(
            "SELECT COALESCE(MAX(sort_order), 0) + 1 FROM repo_groups WHERE workspace_id = ?1 AND parent_id IS ?2",
            params![req.workspace_id, req.parent_id],
            |row| row.get(0),
        )
        .unwrap_or(0);

    conn.execute(
        "INSERT INTO repo_groups (workspace_id, name, parent_id, sort_order) VALUES (?1, ?2, ?3, ?4)",
        params![req.workspace_id, req.name, req.parent_id, sort_order],
    )?;

    let id = conn.last_insert_rowid();
    Ok(RepoGroup {
        id,
        workspace_id: req.workspace_id,
        name: req.name.clone(),
        parent_id: req.parent_id,
        sort_order,
    })
}

/// List all groups for a workspace.
pub fn list_groups(conn: &Connection, workspace_id: i64) -> AppResult<Vec<RepoGroup>> {
    let mut stmt = conn.prepare(
        "SELECT id, workspace_id, name, parent_id, sort_order FROM repo_groups WHERE workspace_id = ?1 ORDER BY sort_order",
    )?;

    let groups = stmt
        .query_map(params![workspace_id], |row| {
            let parent_id: Option<i64> = row.get(3)?;
            Ok(RepoGroup {
                id: row.get(0)?,
                workspace_id: row.get(1)?,
                name: row.get(2)?,
                parent_id,
                sort_order: row.get(4)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(groups)
}

/// Delete a group by ID. Cascades to subgroups.
pub fn delete_group(conn: &Connection, id: i64) -> AppResult<()> {
    // Clear group_id for repos in this group
    conn.execute(
        "UPDATE repositories SET group_id = NULL WHERE group_id = ?1",
        params![id],
    )?;

    conn.execute("DELETE FROM repo_groups WHERE id = ?1", params![id])?;
    Ok(())
}

/// Assign a repository to a group by repo path.
pub fn assign_group_by_path(
    conn: &Connection,
    repo_path: &str,
    group_id: Option<i64>,
) -> AppResult<()> {
    conn.execute(
        "UPDATE repositories SET group_id = ?1 WHERE path = ?2",
        params![group_id, repo_path],
    )?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Commit metadata DAO (Graph data cache, T-04)
// ---------------------------------------------------------------------------

/// Look up a repository's ID by its absolute path (excluding soft-deleted rows).
pub fn get_repository_id_by_path(conn: &Connection, path: &str) -> AppResult<Option<i64>> {
    use rusqlite::OptionalExtension;
    conn.query_row(
        "SELECT id FROM repositories WHERE path = ?1 AND is_deleted = 0",
        params![path],
        |row| row.get(0),
    )
    .optional()
    .map_err(Into::into)
}

/// Upsert a batch of commit metadata plus parent edges in one transaction.
/// Existing commits (by `repo_id` + `oid`) are refreshed in place.
pub fn upsert_commits_batch(
    conn: &mut Connection,
    repo_id: i64,
    commits: &[CommitRecord],
) -> AppResult<()> {
    if commits.is_empty() {
        return Ok(());
    }
    let tx = conn.transaction()?;
    {
        let mut commit_stmt = tx.prepare(
            r#"INSERT INTO commits (repo_id, oid, message, author, committer, authored_at, committed_at, author_offset)
               VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
               ON CONFLICT(repo_id, oid) DO UPDATE SET
                   message = ?3,
                   author = ?4,
                   committer = ?5,
                   authored_at = ?6,
                   committed_at = ?7,
                   author_offset = ?8"#,
        )?;
        let mut parent_stmt = tx.prepare(
            r#"INSERT OR IGNORE INTO commit_parents (commit_id, parent_oid)
               SELECT id, ?1 FROM commits WHERE repo_id = ?2 AND oid = ?3"#,
        )?;

        for c in commits {
            commit_stmt.execute(params![
                repo_id,
                c.oid,
                c.message,
                c.author,
                c.committer,
                c.authored_at.to_string(),
                c.committed_at.to_string(),
                c.offset_minutes
            ])?;
            for parent in &c.parents {
                parent_stmt.execute(params![parent, repo_id, c.oid])?;
            }
        }
    }
    tx.commit()?;
    Ok(())
}

/// Read a single cached commit record (metadata + parents) for a repository.
pub fn get_commit_record(
    conn: &Connection,
    repo_id: i64,
    oid: &str,
) -> AppResult<Option<CommitRecord>> {
    use rusqlite::OptionalExtension;

    let meta = conn
        .query_row(
            "SELECT message, author, committer, authored_at, committed_at, author_offset FROM commits WHERE repo_id = ?1 AND oid = ?2",
            params![repo_id, oid],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, i64>(5)?,
                ))
            },
        )
        .optional()?;

    let Some((message, author, committer, authored_at, committed_at, offset)) = meta else {
        return Ok(None);
    };

    let mut parent_stmt = conn.prepare(
        r#"SELECT cp.parent_oid
           FROM commit_parents cp
           JOIN commits c ON cp.commit_id = c.id
           WHERE c.repo_id = ?1 AND c.oid = ?2
           ORDER BY cp.rowid"#,
    )?;
    let parents: Vec<String> = parent_stmt
        .query_map(params![repo_id, oid], |row| row.get(0))?
        .collect::<Result<_, _>>()?;

    Ok(Some(CommitRecord {
        oid: oid.to_string(),
        message,
        author,
        committer,
        authored_at: authored_at.parse().unwrap_or(0),
        committed_at: committed_at.parse().unwrap_or(0),
        offset_minutes: offset as i32,
        parents,
    }))
}

// ---------------------------------------------------------------------------
// Task History DAO
// ---------------------------------------------------------------------------

/// Insert a task record into the `tasks` table and return its DB id.
pub fn insert_task_record(
    conn: &Connection,
    task_uuid: &str,
    task_type: &str,
    status: &str,
    params_json: &str,
    created_at: &str,
) -> AppResult<i64> {
    conn.execute(
        "INSERT INTO tasks (task_uuid, task_type, status, params_json, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![task_uuid, task_type, status, params_json, created_at],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Update a task's status (and optionally finished_at) by its UUID.
pub fn update_task_status(
    conn: &Connection,
    task_uuid: &str,
    status: &str,
    finished_at: Option<&str>,
) -> AppResult<()> {
    conn.execute(
        "UPDATE tasks SET status = ?1, finished_at = ?2 WHERE task_uuid = ?3",
        params![status, finished_at, task_uuid],
    )?;
    Ok(())
}

/// Mark any unfinished (queued/running) tasks as interrupted after a restart.
/// Returns the number of tasks marked.
pub fn mark_interrupted_tasks(conn: &Connection, now: &str) -> AppResult<usize> {
    let n = conn.execute(
        "UPDATE tasks SET status = 'interrupted', finished_at = ?1 WHERE finished_at IS NULL AND status IN ('queued', 'running')",
        params![now],
    )?;
    Ok(n)
}

/// Insert a task history record.
#[allow(dead_code)]
pub fn insert_task_history(
    conn: &Connection,
    task_type: &str,
    repo_path: &str,
    status: &str,
    message: Option<&str>,
    started_at: &str,
    finished_at: Option<&str>,
) -> AppResult<()> {
    conn.execute(
        "INSERT INTO task_history (task_type, repo_path, status, message, started_at, finished_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![task_type, repo_path, status, message, started_at, finished_at],
    )?;
    Ok(())
}

/// List recent task history records.
#[allow(dead_code)]
pub fn list_task_history(conn: &Connection, limit: i64) -> AppResult<Vec<(i64, String, String, String, Option<String>, String, Option<String>)>> {
    let mut stmt = conn.prepare(
        "SELECT id, task_type, repo_path, status, message, started_at, finished_at FROM task_history ORDER BY started_at DESC LIMIT ?1",
    )?;

    let records = stmt
        .query_map(params![limit], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
                row.get(6)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(records)
}
