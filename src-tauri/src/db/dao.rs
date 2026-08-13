use chrono::Utc;
use rusqlite::{params, Connection};

use crate::error::{AppError, AppResult};
use crate::models::group::{CreateGroupRequest, RepoGroup};
use crate::models::repository::{Repository, ScannedRepo};
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
            r#"INSERT INTO repositories (workspace_id, path, name, relative_path, last_scanned, created_at, updated_at)
               VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)
               ON CONFLICT(path) DO UPDATE SET
                   workspace_id = ?1,
                   name = ?3,
                   relative_path = ?4,
                   last_scanned = ?5,
                   updated_at = ?6"#,
        )?;
        for repo in repos {
            stmt.execute(params![
                workspace_id,
                repo.path,
                repo.name,
                repo.relative_path,
                now,
                now
            ])?;
        }
    }
    tx.commit()?;
    Ok(())
}

/// List all repositories for a given workspace.
pub fn list_repositories_by_workspace(
    conn: &Connection,
    workspace_id: i64,
) -> AppResult<Vec<Repository>> {
    let mut stmt = conn.prepare(
        r#"SELECT id, workspace_id, path, name, relative_path, is_favorite, tags, group_id
           FROM repositories
           WHERE workspace_id = ?1
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

/// Delete repositories that are no longer found in the workspace directory.
pub fn cleanup_stale_repositories(
    conn: &Connection,
    workspace_id: i64,
    existing_paths: &[String],
) -> AppResult<()> {
    // Build placeholder string for IN clause
    if existing_paths.is_empty() {
        conn.execute(
            "DELETE FROM repositories WHERE workspace_id = ?1",
            params![workspace_id],
        )?;
    } else {
        let placeholders: Vec<String> = (0..existing_paths.len())
            .map(|i| format!("?{}", i + 2))
            .collect();
        let sql = format!(
            "DELETE FROM repositories WHERE workspace_id = ?1 AND path NOT IN ({})",
            placeholders.join(", ")
        );

        let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> =
            vec![Box::new(workspace_id)];
        for path in existing_paths {
            params_vec.push(Box::new(path.clone()));
        }

        let params_ref: Vec<&dyn rusqlite::ToSql> =
            params_vec.iter().map(|p| p.as_ref()).collect();

        conn.execute(&sql, params_ref.as_slice())?;
    }
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
// Task History DAO
// ---------------------------------------------------------------------------

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
