//! AI 会话与消息（设计文档 §10.4 / §11.1 / §11.2 / §16.1）。
//!
//! - `ai_sessions` 存会话元数据（标题 / 角色 / 作用域 / 时间 / 归档）；
//! - `ai_messages` 存会话消息；**只有用户开启持久化时才写入正文**
//!   （§10.4：默认不保存完整 Prompt 中的敏感原文，关闭时仅保留 `ai_requests`
//!   审计元数据）；
//! - 删除会话级联删除消息与关联缓存（§10.4 / 全局约束 §8）：FK
//!   `ON DELETE CASCADE` 是主保障，删除函数内再显式清理一次——不依赖连接
//!   的 `PRAGMA foreign_keys` 实况；
//! - 会话列表分页、消息按需加载（`beforeSequence` 游标），避免长会话拖慢
//!   Drawer（§16.1）。

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};

use super::error::AiError;
use super::request::MessageRole;

/// 会话持久化开关在 `ai_settings` 中的键名。
pub const PERSIST_SESSIONS_KEY: &str = "persistSessions";

/// 会话角色（§12.3 Drawer 顶部「当前角色」）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AiSessionRole {
    /// 通用应用助手（默认）。
    Assistant,
    /// Runtime 排障专家（AI-06）。
    RuntimeDiagnostician,
    /// Git 助手（AI-07 ~ AI-09）。
    GitAssistant,
}

impl AiSessionRole {
    pub fn as_str(&self) -> &'static str {
        match self {
            AiSessionRole::Assistant => "assistant",
            AiSessionRole::RuntimeDiagnostician => "runtimeDiagnostician",
            AiSessionRole::GitAssistant => "gitAssistant",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "assistant" => Some(AiSessionRole::Assistant),
            "runtimeDiagnostician" => Some(AiSessionRole::RuntimeDiagnostician),
            "gitAssistant" => Some(AiSessionRole::GitAssistant),
            _ => None,
        }
    }
}

/// 会话（§11.2 `ai_sessions`）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiSession {
    pub id: String,
    pub title: String,
    pub role: AiSessionRole,
    pub workspace_id: Option<i64>,
    /// 作用域内的仓库路径清单（归一化正斜杠）。
    pub repository_scope: Vec<String>,
    /// Runtime 作用域（runtime 名 / 进程 id 等，按场景自定）。
    pub runtime_scope: serde_json::Value,
    pub created_at: String,
    pub updated_at: String,
    /// 归档时间；None = 未归档。
    pub archived_at: Option<String>,
    /// 消息条数（列表用，非表字段）。
    pub message_count: i64,
}

/// 新建会话入参。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateAiSessionRequest {
    pub title: String,
    #[serde(default)]
    pub role: Option<AiSessionRole>,
    pub workspace_id: Option<i64>,
    #[serde(default)]
    pub repository_scope: Vec<String>,
    #[serde(default)]
    pub runtime_scope: Option<serde_json::Value>,
}

/// 会话列表查询（分页 + 归档过滤）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiSessionListQuery {
    pub workspace_id: Option<i64>,
    /// 是否包含已归档会话（默认 false）。
    #[serde(default)]
    pub include_archived: bool,
    /// 每页条数（默认 20，上限 100）。
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

/// 会话列表（分页结果）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiSessionList {
    pub items: Vec<AiSession>,
    /// 满足过滤条件的总条数（用于分页 UI）。
    pub total: i64,
}

/// 会话消息（§11.2 `ai_messages`）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiSessionMessage {
    pub id: i64,
    pub session_id: String,
    pub role: MessageRole,
    /// 结构化内容（展示所需内容；Secret 原文永不入库，§10.4）。
    pub content: serde_json::Value,
    pub sequence: i64,
    pub created_at: String,
}

/// 会话详情（会话 + 按需加载的消息窗口）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiSessionDetail {
    pub session: AiSession,
    pub messages: Vec<AiSessionMessage>,
    /// 会话消息总条数（大于 `messages.len()` 时表示还有更早的历史）。
    pub total_messages: i64,
}

/// 会话持久化设置（§10.4）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiSessionPersistence {
    /// 是否保存完整会话（false = 只保留 `ai_requests` 审计元数据）。
    pub persist_sessions: bool,
    /// 当前已持久化的会话数（诊断展示用）。
    pub session_count: i64,
}

fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339()
}

fn not_found(id: &str) -> AppError {
    AppError::Ai(AiError::NotConfigured {
        message: format!("会话不存在: {}", id),
    })
}

// ---------------------------------------------------------------------------
// 会话 CRUD
// ---------------------------------------------------------------------------

/// 创建会话。
pub fn create_session(conn: &Connection, input: &CreateAiSessionRequest) -> AppResult<AiSession> {
    let title = input.title.trim();
    if title.is_empty() {
        return Err(AppError::Ai(AiError::NotConfigured {
            message: "会话标题不能为空".to_string(),
        }));
    }
    let id = uuid::Uuid::new_v4().to_string();
    let now = now_rfc3339();
    let role = input.role.unwrap_or(AiSessionRole::Assistant);
    let repository_scope = serde_json::to_string(&input.repository_scope)?;
    let runtime_scope = input
        .runtime_scope
        .clone()
        .unwrap_or_else(|| serde_json::json!({}));
    conn.execute(
        "INSERT INTO ai_sessions
         (id, title, role, workspace_id, repository_scope_json, runtime_scope_json, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)",
        params![
            id,
            title,
            role.as_str(),
            input.workspace_id,
            repository_scope,
            runtime_scope.to_string(),
            now
        ],
    )?;
    Ok(get_session(conn, &id)?.expect("just inserted"))
}

const SESSION_COLS: &str = "id, title, role, workspace_id, repository_scope_json, runtime_scope_json, created_at, updated_at, archived_at";

fn row_to_session(row: &rusqlite::Row) -> rusqlite::Result<AiSession> {
    let role_str: String = row.get("role")?;
    let scope_json: String = row.get("repository_scope_json")?;
    let runtime_json: String = row.get("runtime_scope_json")?;
    Ok(AiSession {
        id: row.get("id")?,
        title: row.get("title")?,
        role: AiSessionRole::parse(&role_str).unwrap_or(AiSessionRole::Assistant),
        workspace_id: row.get("workspace_id")?,
        repository_scope: serde_json::from_str(&scope_json).unwrap_or_default(),
        runtime_scope: serde_json::from_str(&runtime_json).unwrap_or_else(|_| serde_json::json!({})),
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
        archived_at: row.get("archived_at")?,
        message_count: 0,
    })
}

pub fn get_session(conn: &Connection, id: &str) -> AppResult<Option<AiSession>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {}, (SELECT COUNT(*) FROM ai_messages m WHERE m.session_id = s.id) AS message_count
         FROM ai_sessions s WHERE s.id = ?1",
        SESSION_COLS
            .split(", ")
            .map(|c| format!("s.{}", c))
            .collect::<Vec<_>>()
            .join(", ")
    ))?;
    let mut rows = stmt.query_map(params![id], |row| {
        let mut session = row_to_session(row)?;
        session.message_count = row.get("message_count")?;
        Ok(session)
    })?;
    match rows.next() {
        Some(row) => Ok(Some(row?)),
        None => Ok(None),
    }
}

/// 列表（分页，默认排除已归档；按更新时间倒序）。
pub fn list_sessions(conn: &Connection, query: &AiSessionListQuery) -> AppResult<AiSessionList> {
    let limit = query.limit.unwrap_or(20).clamp(1, 100);
    let offset = query.offset.unwrap_or(0).max(0);

    let mut where_clause = String::from("WHERE 1 = 1");
    if !query.include_archived {
        where_clause.push_str(" AND s.archived_at IS NULL");
    }
    if query.workspace_id.is_some() {
        where_clause.push_str(" AND s.workspace_id = ?1");
    }

    let total: i64 = {
        let sql = format!(
            "SELECT COUNT(*) FROM ai_sessions s {}",
            where_clause
        );
        let mut stmt = conn.prepare(&sql)?;
        match query.workspace_id {
            Some(ws) => stmt.query_row(params![ws], |r| r.get(0)),
            None => stmt.query_row([], |r| r.get(0)),
        }?
    };

    let sql = format!(
        "SELECT {}, (SELECT COUNT(*) FROM ai_messages m WHERE m.session_id = s.id) AS message_count
         FROM ai_sessions s {}
         ORDER BY s.updated_at DESC, s.id DESC
         LIMIT ?{} OFFSET ?{}",
        SESSION_COLS
            .split(", ")
            .map(|c| format!("s.{}", c))
            .collect::<Vec<_>>()
            .join(", "),
        where_clause,
        // ?1 可能被 workspace_id 占用
        if query.workspace_id.is_some() { 2 } else { 1 },
        if query.workspace_id.is_some() { 3 } else { 2 },
    );
    let mut stmt = conn.prepare(&sql)?;
    let mut collect = |params: &[&dyn rusqlite::ToSql]| -> AppResult<Vec<AiSession>> {
        let rows = stmt.query_map(params, session_from_list_row)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    };
    let items = match query.workspace_id {
        Some(ws) => collect(params![ws, limit, offset])?,
        None => collect(params![limit, offset])?,
    };
    Ok(AiSessionList { items, total })
}

fn session_from_list_row(row: &rusqlite::Row) -> rusqlite::Result<AiSession> {
    let mut session = row_to_session(row)?;
    session.message_count = row.get("message_count")?;
    Ok(session)
}

/// 重命名会话。
pub fn rename_session(conn: &Connection, id: &str, title: &str) -> AppResult<AiSession> {
    let title = title.trim();
    if title.is_empty() {
        return Err(AppError::Ai(AiError::NotConfigured {
            message: "会话标题不能为空".to_string(),
        }));
    }
    let updated = conn.execute(
        "UPDATE ai_sessions SET title = ?2, updated_at = ?3 WHERE id = ?1",
        params![id, title, now_rfc3339()],
    )?;
    if updated == 0 {
        return Err(not_found(id));
    }
    Ok(get_session(conn, id)?.expect("just updated"))
}

/// 归档 / 取消归档。
pub fn set_archived(conn: &Connection, id: &str, archived: bool) -> AppResult<AiSession> {
    let now = now_rfc3339();
    let updated = conn.execute(
        "UPDATE ai_sessions SET archived_at = ?2, updated_at = ?3 WHERE id = ?1",
        params![
            id,
            if archived { Some(now.clone()) } else { None },
            now
        ],
    )?;
    if updated == 0 {
        return Err(not_found(id));
    }
    Ok(get_session(conn, id)?.expect("just updated"))
}

/// 删除会话：级联删除消息与关联缓存（§10.4）。
///
/// FK `ON DELETE CASCADE` 是主保障；这里再显式删除一次，确保即使连接未开启
/// `PRAGMA foreign_keys` 也不残留完整 Prompt。返回是否确实删除了会话。
pub fn delete_session(conn: &Connection, id: &str) -> AppResult<bool> {
    conn.execute("DELETE FROM ai_messages WHERE session_id = ?1", params![id])?;
    conn.execute(
        "DELETE FROM ai_result_cache WHERE session_id = ?1",
        params![id],
    )?;
    // ai_requests 保留审计（session_id 经 FK 置空）：审计只含 hash 与计量，
    // 不含 Prompt 原文（§10.4）。
    let removed = conn.execute("DELETE FROM ai_sessions WHERE id = ?1", params![id])?;
    Ok(removed > 0)
}

// ---------------------------------------------------------------------------
// 消息（按需加载）
// ---------------------------------------------------------------------------

/// 读取会话与消息窗口（§16.1：消息按需加载）。
///
/// `before_sequence` 为游标：只返回 `sequence < before_sequence` 的消息
/// （用于向上翻页加载更早历史）；默认返回**最后** `limit` 条。
pub fn get_session_detail(
    conn: &Connection,
    id: &str,
    limit: i64,
    before_sequence: Option<i64>,
) -> AppResult<Option<AiSessionDetail>> {
    let session = match get_session(conn, id)? {
        Some(s) => s,
        None => return Ok(None),
    };
    let limit = limit.clamp(1, 200);
    let total_messages = session.message_count;

    let messages = match before_sequence {
        Some(cursor) => {
            let mut stmt = conn.prepare(
                "SELECT id, session_id, role, content_json, sequence, created_at
                 FROM ai_messages WHERE session_id = ?1 AND sequence < ?2
                 ORDER BY sequence DESC LIMIT ?3",
            )?;
            let rows = stmt.query_map(params![id, cursor, limit], row_to_message)?;
            let mut messages: Vec<AiSessionMessage> =
                rows.collect::<rusqlite::Result<Vec<_>>>()?;
            messages.reverse();
            messages
        }
        None => {
            let mut stmt = conn.prepare(
                "SELECT id, session_id, role, content_json, sequence, created_at
                 FROM ai_messages WHERE session_id = ?1
                 ORDER BY sequence DESC LIMIT ?2",
            )?;
            let rows = stmt.query_map(params![id, limit], row_to_message)?;
            let mut messages: Vec<AiSessionMessage> =
                rows.collect::<rusqlite::Result<Vec<_>>>()?;
            messages.reverse();
            messages
        }
    };

    Ok(Some(AiSessionDetail {
        session,
        messages,
        total_messages,
    }))
}

fn row_to_message(row: &rusqlite::Row) -> rusqlite::Result<AiSessionMessage> {
    let role_str: String = row.get("role")?;
    let content_json: String = row.get("content_json")?;
    Ok(AiSessionMessage {
        id: row.get("id")?,
        session_id: row.get("session_id")?,
        role: MessageRole::parse(&role_str).unwrap_or(MessageRole::User),
        content: serde_json::from_str(&content_json)
            .unwrap_or_else(|_| serde_json::Value::String(String::new())),
        sequence: row.get("sequence")?,
        created_at: row.get("created_at")?,
    })
}

/// 追加一条消息。**持久化开关关闭时静默跳过**（§10.4：默认不保存完整会话）。
///
/// 返回写入的消息；跳过时返回 `None`（调用方据此不把消息内容当已落盘）。
pub fn append_message(
    conn: &Connection,
    session_id: &str,
    role: MessageRole,
    content: &serde_json::Value,
) -> AppResult<Option<AiSessionMessage>> {
    if !persistence_enabled(conn)? {
        return Ok(None);
    }
    if get_session(conn, session_id)?.is_none() {
        return Err(not_found(session_id));
    }
    append_message_unchecked(conn, session_id, role, content)
}

/// 追加消息但**不检查持久化开关**：仅用于开关已确认开启的写入路径
/// （Gateway 成功后落库，避免重复读设置）。
pub fn append_message_unchecked(
    conn: &Connection,
    session_id: &str,
    role: MessageRole,
    content: &serde_json::Value,
) -> AppResult<Option<AiSessionMessage>> {
    if get_session(conn, session_id)?.is_none() {
        return Err(not_found(session_id));
    }
    let now = now_rfc3339();
    let sequence: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(sequence), -1) + 1 FROM ai_messages WHERE session_id = ?1",
            params![session_id],
            |r| r.get(0),
        )
        .unwrap_or(0);
    conn.execute(
        "INSERT INTO ai_messages (session_id, role, content_json, sequence, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            session_id,
            role.as_str(),
            serde_json::to_string(content)?,
            sequence,
            now
        ],
    )?;
    conn.execute(
        "UPDATE ai_sessions SET updated_at = ?2 WHERE id = ?1",
        params![session_id, now],
    )?;
    Ok(Some(AiSessionMessage {
        id: conn.last_insert_rowid(),
        session_id: session_id.to_string(),
        role,
        content: content.clone(),
        sequence,
        created_at: now,
    }))
}

// ---------------------------------------------------------------------------
// 持久化开关
// ---------------------------------------------------------------------------

/// 读取持久化开关（缺省 = 关闭，§10.4 保守取向）。
pub fn persistence_enabled(conn: &Connection) -> AppResult<bool> {
    let value: Option<String> = conn
        .query_row(
            "SELECT value FROM ai_settings WHERE key = ?1",
            params![PERSIST_SESSIONS_KEY],
            |r| r.get(0),
        )
        .ok();
    Ok(value.as_deref() == Some("true"))
}

pub fn set_persistence(conn: &Connection, persist: bool) -> AppResult<()> {
    conn.execute(
        "INSERT INTO ai_settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![PERSIST_SESSIONS_KEY, if persist { "true" } else { "false" }],
    )?;
    Ok(())
}

/// 持久化设置快照（含当前会话数，设置页展示用）。
pub fn persistence_settings(conn: &Connection) -> AppResult<AiSessionPersistence> {
    let session_count: i64 = conn.query_row("SELECT COUNT(*) FROM ai_sessions", [], |r| r.get(0))?;
    Ok(AiSessionPersistence {
        persist_sessions: persistence_enabled(conn)?,
        session_count,
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

    /// 建一个 Workspace（`ai_sessions.workspace_id` 有外键约束）。
    fn ensure_workspace(conn: &Connection, id: i64) {
        conn.execute(
            "INSERT OR IGNORE INTO workspaces (id, name, path, created_at, updated_at)
             VALUES (?1, 'ws', ?2, 't', 't')",
            rusqlite::params![id, format!("/ws/{id}")],
        )
        .unwrap();
    }

    fn create(conn: &Connection, title: &str, workspace_id: Option<i64>) -> AiSession {
        if let Some(id) = workspace_id {
            ensure_workspace(conn, id);
        }
        create_session(
            conn,
            &CreateAiSessionRequest {
                title: title.into(),
                role: None,
                workspace_id,
                repository_scope: vec![],
                runtime_scope: None,
            },
        )
        .unwrap()
    }

    fn text_message(conn: &Connection, session_id: &str, role: MessageRole, text: &str) {
        append_message_unchecked(conn, session_id, role, &serde_json::json!({ "text": text }))
            .unwrap();
    }

    #[test]
    fn session_crud_roundtrip() {
        let conn = open_db();
        let session = create(&conn, "排障会话", Some(1));
        assert!(!session.id.is_empty());
        assert_eq!(session.role, AiSessionRole::Assistant);
        assert_eq!(session.workspace_id, Some(1));
        assert!(session.archived_at.is_none());
        assert_eq!(session.message_count, 0);

        let renamed = rename_session(&conn, &session.id, "重命名后").unwrap();
        assert_eq!(renamed.title, "重命名后");
        assert_eq!(renamed.created_at, session.created_at);

        let listed = list_sessions(
            &conn,
            &AiSessionListQuery {
                workspace_id: None,
                include_archived: false,
                limit: None,
                offset: None,
            },
        )
        .unwrap();
        assert_eq!(listed.total, 1);
        assert_eq!(listed.items[0].title, "重命名后");
    }

    #[test]
    fn empty_title_is_rejected() {
        let conn = open_db();
        let err = create_session(
            &conn,
            &CreateAiSessionRequest {
                title: "  ".into(),
                role: None,
                workspace_id: None,
                repository_scope: vec![],
                runtime_scope: None,
            },
        )
        .unwrap_err();
        assert_eq!(err.code(), "AiNotConfigured");
    }

    #[test]
    fn archive_toggles_listing_visibility() {
        let conn = open_db();
        let session = create(&conn, "会话", None);
        let query_all = AiSessionListQuery {
            workspace_id: None,
            include_archived: true,
            limit: None,
            offset: None,
        };
        let query_active = AiSessionListQuery {
            include_archived: false,
            ..query_all.clone()
        };

        let archived = set_archived(&conn, &session.id, true).unwrap();
        assert!(archived.archived_at.is_some());
        assert_eq!(list_sessions(&conn, &query_active).unwrap().total, 0);
        assert_eq!(list_sessions(&conn, &query_all).unwrap().total, 1);

        let restored = set_archived(&conn, &session.id, false).unwrap();
        assert!(restored.archived_at.is_none());
        assert_eq!(list_sessions(&conn, &query_active).unwrap().total, 1);
    }

    /// 分页（§16.1）：分页窗口与总数正确，超出范围返回空列表。
    #[test]
    fn list_sessions_paginates_descending_by_update_time() {
        let conn = open_db();
        for i in 0..5 {
            create(&conn, &format!("s{i}"), None);
        }
        let page1 = list_sessions(
            &conn,
            &AiSessionListQuery {
                workspace_id: None,
                include_archived: false,
                limit: Some(2),
                offset: Some(0),
            },
        )
        .unwrap();
        assert_eq!(page1.total, 5);
        assert_eq!(page1.items.len(), 2);

        let page3 = list_sessions(
            &conn,
            &AiSessionListQuery {
                workspace_id: None,
                include_archived: false,
                limit: Some(2),
                offset: Some(4),
            },
        )
        .unwrap();
        assert_eq!(page3.items.len(), 1);

        let beyond = list_sessions(
            &conn,
            &AiSessionListQuery {
                workspace_id: None,
                include_archived: false,
                limit: Some(2),
                offset: Some(10),
            },
        )
        .unwrap();
        assert!(beyond.items.is_empty());
        assert_eq!(beyond.total, 5);
    }

    /// 按 Workspace 过滤。
    #[test]
    fn list_sessions_filters_by_workspace() {
        let conn = open_db();
        create(&conn, "ws1-a", Some(1));
        create(&conn, "ws2-a", Some(2));

        let for_ws1 = list_sessions(
            &conn,
            &AiSessionListQuery {
                workspace_id: Some(1),
                include_archived: false,
                limit: None,
                offset: None,
            },
        )
        .unwrap();
        assert_eq!(for_ws1.total, 1);
        assert_eq!(for_ws1.items[0].title, "ws1-a");
    }

    /// 消息按需加载（§16.1）：默认取最后 N 条，`beforeSequence` 游标向前翻页。
    #[test]
    fn messages_load_on_demand_with_cursor() {
        let conn = open_db();
        let session = create(&conn, "会话", None);
        for i in 0..5 {
            text_message(&conn, &session.id, MessageRole::User, &format!("m{i}"));
        }

        let latest = get_session_detail(&conn, &session.id, 2, None)
            .unwrap()
            .expect("session exists");
        assert_eq!(latest.total_messages, 5);
        assert_eq!(latest.messages.len(), 2);
        assert_eq!(latest.messages[0].content["text"], "m3");
        assert_eq!(latest.messages[1].content["text"], "m4");
        assert_eq!(latest.messages[1].sequence, 4);

        let older = get_session_detail(&conn, &session.id, 2, Some(3))
            .unwrap()
            .expect("session exists");
        assert_eq!(older.messages.len(), 2);
        assert_eq!(older.messages[0].content["text"], "m1");
        assert_eq!(older.messages[1].content["text"], "m2");
    }

    /// 消息顺序递增、角色与内容往返正确。
    #[test]
    fn messages_roundtrip_roles_and_content() {
        let conn = open_db();
        let session = create(&conn, "会话", None);
        text_message(&conn, &session.id, MessageRole::User, "问");
        text_message(&conn, &session.id, MessageRole::Assistant, "答");

        let detail = get_session_detail(&conn, &session.id, 10, None)
            .unwrap()
            .expect("session exists");
        assert_eq!(detail.messages.len(), 2);
        assert_eq!(detail.messages[0].role, MessageRole::User);
        assert_eq!(detail.messages[0].sequence, 0);
        assert_eq!(detail.messages[1].role, MessageRole::Assistant);
        assert_eq!(detail.messages[1].sequence, 1);
        assert_eq!(detail.messages[1].content["text"], "答");
        assert_eq!(detail.session.message_count, 2);
    }

    /// 删除会话级联删除消息与缓存（§10.4）：即便 FK 关闭也靠显式清理兜底。
    #[test]
    fn delete_session_cascades_messages_and_cache() {
        let conn = open_db();
        // 显式关闭外键，验证代码路径不依赖 PRAGMA 实况。
        conn.execute_batch("PRAGMA foreign_keys = OFF;").unwrap();
        let session = create(&conn, "会话", None);
        text_message(&conn, &session.id, MessageRole::User, "内容");
        conn.execute(
            "INSERT INTO ai_result_cache (cache_key, task_kind, provider_id, model_id, prompt_version, context_hash, settings_hash, result_json, session_id, created_at)
             VALUES ('k', 'chat', 'p', 'm', '1', 'c', 's', '{}', ?1, 't')",
            params![session.id],
        )
        .unwrap();

        assert!(delete_session(&conn, &session.id).unwrap());

        let messages: i64 = conn
            .query_row("SELECT COUNT(*) FROM ai_messages", [], |r| r.get(0))
            .unwrap();
        let cached: i64 = conn
            .query_row("SELECT COUNT(*) FROM ai_result_cache", [], |r| r.get(0))
            .unwrap();
        assert_eq!(messages, 0);
        assert_eq!(cached, 0);
        assert!(get_session(&conn, &session.id).unwrap().is_none());

        // 删除不存在的会话返回 false（幂等）。
        assert!(!delete_session(&conn, "missing").unwrap());
    }

    /// 持久化开关（§10.4）：默认关闭；关闭时 `append_message` 不落盘任何正文。
    #[test]
    fn persistence_switch_defaults_off_and_gates_writes() {
        let conn = open_db();
        assert!(!persistence_enabled(&conn).unwrap());

        let session = create(&conn, "会话", None);
        let written = append_message(
            &conn,
            &session.id,
            MessageRole::User,
            &serde_json::json!({"text": "敏感原文"}),
        )
        .unwrap();
        assert!(written.is_none(), "开关关闭时不得写入消息正文");

        let stored: i64 = conn
            .query_row("SELECT COUNT(*) FROM ai_messages", [], |r| r.get(0))
            .unwrap();
        assert_eq!(stored, 0);

        set_persistence(&conn, true).unwrap();
        assert!(persistence_enabled(&conn).unwrap());
        let written = append_message(
            &conn,
            &session.id,
            MessageRole::User,
            &serde_json::json!({"text": "开启后写入"}),
        )
        .unwrap();
        assert!(written.is_some());
        assert_eq!(
            persistence_settings(&conn).unwrap().session_count,
            1,
            "设置快照携带会话数"
        );

        set_persistence(&conn, false).unwrap();
        assert!(!persistence_enabled(&conn).unwrap());
    }

    /// 消息写入未知会话返回可行动错误（而非静默丢弃）。
    #[test]
    fn append_to_missing_session_errors() {
        let conn = open_db();
        set_persistence(&conn, true).unwrap();
        let err = append_message(
            &conn,
            "missing",
            MessageRole::User,
            &serde_json::json!({"text": "x"}),
        )
        .unwrap_err();
        assert_eq!(err.code(), "AiNotConfigured");
    }

    #[test]
    fn role_serde_names_match_design() {
        assert_eq!(
            serde_json::to_value(AiSessionRole::Assistant).unwrap(),
            "assistant"
        );
        assert_eq!(
            serde_json::to_value(AiSessionRole::RuntimeDiagnostician).unwrap(),
            "runtimeDiagnostician"
        );
        assert_eq!(
            serde_json::to_value(AiSessionRole::GitAssistant).unwrap(),
            "gitAssistant"
        );
    }
}
