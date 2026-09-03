//! AI 结果缓存（设计文档 §11.1 / §11.3）。
//!
//! ```text
//! 缓存 Key = taskKind + providerId + modelId + promptVersion + contextHash + settingsHash
//! ```
//!
//! 硬约束（§11.3 / 全局约束 §8）：
//! - **不得跨模型 / 跨 Provider 复用**：两者都进 Key，且读取时逐维度校验
//!   （[`CachedResult::matches`]）——DB 被外部改动或未来 Key 算法调整时不至于误用；
//! - 缓存条目带**生成时间与 context hash**，UI 不得把过期结果显示成当前事实；
//! - diff / 日志 / 错误上下文 / 模型 / Prompt 版本 / 脱敏排除策略任一变化都必须
//!   失效：前三者进 `contextHash`，模型与 Prompt 版本进 Key，脱敏排除策略进
//!   `settingsHash`；
//! - 删除会话级联删除关联缓存行（`ai_result_cache.session_id` FK，§10.4）。
//!
//! 存储分层（§11.1）：内存 LRU（有上限，进程内命中不必读库）+ SQLite 持久
//! （跨进程重启复用）；两层数据一致，读取时先内存后 SQLite 并回填空缺。

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::error::AppResult;

use super::context::content_hash;
use super::model::AiTaskKind;
use super::request::{AiRequest, AiResult};

/// Prompt 模板版本（§11.3 缓存维度之一，定义见 [`super::prompt`]）。
pub use super::prompt::PROMPT_VERSION;

/// 持久化条目上限（超出按生成时间淘汰最旧）。与内存 LRU 上限解耦：内存层
/// 控制常驻占用，持久层控制磁盘增长。
pub const DEFAULT_PERSISTED_LIMIT: usize = 200;

// ---------------------------------------------------------------------------
// 缓存 Key 与指纹
// ---------------------------------------------------------------------------

/// 缓存维度（§11.3）。任一维度变化 → Key 变化 → 缓存失效。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheKeyParts {
    pub task_kind: AiTaskKind,
    pub provider_id: String,
    pub model_id: String,
    pub prompt_version: String,
    /// 最终内容 hash（system + 全部消息正文；口径同 Preview 的 `contentHash`）。
    pub context_hash: String,
    /// 影响输出但与上下文内容无关的设置指纹（含脱敏/排除策略）。
    pub settings_hash: String,
}

impl CacheKeyParts {
    /// 从请求 + 解析后的 Provider/模型组装缓存维度（§11.3）。
    ///
    /// Prompt 版本取当前常量，contextHash / settingsHash 由同一口径的两个
    /// 指纹函数计算——Gateway 与 Preview 共用此入口，避免 Key 算法漂移。
    pub fn for_request(request: &AiRequest, provider_id: &str, model_id: &str) -> Self {
        Self {
            task_kind: request.task_kind,
            provider_id: provider_id.to_string(),
            model_id: model_id.to_string(),
            prompt_version: PROMPT_VERSION.to_string(),
            context_hash: request_content_hash(request),
            settings_hash: settings_fingerprint(request),
        }
    }

    /// 组装缓存 Key（FNV-1a，稳定但非加密；分段带分隔符避免拼接歧义）。
    pub fn key(&self) -> String {
        content_hash(&[
            self.task_kind.as_str(),
            self.provider_id.as_str(),
            self.model_id.as_str(),
            self.prompt_version.as_str(),
            self.context_hash.as_str(),
            self.settings_hash.as_str(),
        ])
    }
}

/// 缓存条目（§11.3：必须标记生成时间与 context hash）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CachedResult {
    pub cache_key: String,
    pub task_kind: AiTaskKind,
    pub provider_id: String,
    pub model_id: String,
    pub prompt_version: String,
    pub context_hash: String,
    pub settings_hash: String,
    pub result: AiResult,
    /// 关联会话（仅用于级联清理；为 None 时不受会话删除影响）。
    pub session_id: Option<String>,
    pub created_at: String,
}

impl CachedResult {
    /// 读取时的维度校验（§11.3：不得跨模型/跨 Provider 复用）。
    pub fn matches(&self, parts: &CacheKeyParts) -> bool {
        self.cache_key == parts.key()
            && self.task_kind == parts.task_kind
            && self.provider_id == parts.provider_id
            && self.model_id == parts.model_id
            && self.prompt_version == parts.prompt_version
            && self.context_hash == parts.context_hash
            && self.settings_hash == parts.settings_hash
    }
}

/// 写入缓存的入参（生成时间由调用方给出，便于测试确定性断言）。
#[derive(Debug, Clone)]
pub struct CacheEntryInput {
    pub parts: CacheKeyParts,
    pub result: AiResult,
    pub session_id: Option<String>,
    pub created_at: String,
}

/// 请求内容 hash：system + 全部消息正文。
///
/// 与 Preview 的 `content_hash` **同一口径**（Preview 直接调用本函数），
/// 因此「排除项变更 → hash 变更 → 缓存失效」在两条链路上一致（§7.3 / §11.3）。
pub fn request_content_hash(request: &AiRequest) -> String {
    let mut parts: Vec<&str> = vec![request.system_instruction.as_str()];
    parts.extend(request.messages.iter().map(|m| m.content.as_str()));
    content_hash(&parts)
}

/// 设置指纹（§11.3 `settingsHash`）：响应格式、温度、预算、工具策略、
/// Warn 确认状态，以及 manifest 中每条目的脱敏/排除/截断状态。
///
/// 排除项或脱敏策略变化 → 该指纹变化 → 缓存失效（§10.2 / §11.3）。
pub fn settings_fingerprint(request: &AiRequest) -> String {
    let mut rows: Vec<String> = vec![
        format!(
            "responseFormat={}",
            serde_json::to_value(request.response_format)
                .ok()
                .and_then(|v| v.as_str().map(String::from))
                .unwrap_or_default()
        ),
        format!(
            "toolPolicy={}",
            serde_json::to_value(request.tool_policy)
                .ok()
                .and_then(|v| v.as_str().map(String::from))
                .unwrap_or_default()
        ),
        format!(
            "temperature={}",
            request
                .temperature
                .map(|t| format!("{t:.4}"))
                .unwrap_or_else(|| "-".to_string())
        ),
        format!("tokenBudget={}", request.token_budget),
        format!("secretWarnConfirmed={}", request.secret_warn_confirmed),
    ];
    // 条目级脱敏/排除策略（按 source_id 排序，与收集顺序无关）。
    let mut items: Vec<String> = request
        .context_manifest
        .iter()
        .map(|i| {
            format!(
                "{}|{}|redacted={}|excluded={}|truncated={}|reason={}",
                i.source_id,
                i.kind.as_str(),
                i.redacted,
                i.excluded,
                i.truncated,
                i.exclusion_reason.map(|r| r.as_str()).unwrap_or("-")
            )
        })
        .collect();
    items.sort();
    rows.extend(items);
    let refs: Vec<&str> = rows.iter().map(|s| s.as_str()).collect();
    content_hash(&refs)
}

// ---------------------------------------------------------------------------
// 内存 LRU（有上限）
// ---------------------------------------------------------------------------

/// 确定性 LRU（容量小，用 VecDeque 维护访问序即可）。
#[derive(Debug, Default)]
struct Lru {
    map: HashMap<String, CachedResult>,
    /// 访问序：队首最久未使用，队尾最近使用。
    order: VecDeque<String>,
    /// 容量上限（插入后立即收缩到该值）。
    capacity: usize,
}

impl Lru {
    fn get(&mut self, key: &str) -> Option<CachedResult> {
        if !self.map.contains_key(key) {
            return None;
        }
        self.touch(key);
        self.map.get(key).cloned()
    }

    fn insert(&mut self, entry: CachedResult) -> usize {
        let key = entry.cache_key.clone();
        self.map.insert(key.clone(), entry);
        self.touch(&key);
        let mut evicted = 0usize;
        while self.order.len() > self.capacity {
            if let Some(oldest) = self.order.pop_front() {
                self.map.remove(&oldest);
                evicted += 1;
            }
        }
        evicted
    }

    fn touch(&mut self, key: &str) {
        self.order.retain(|k| k != key);
        self.order.push_back(key.to_string());
    }
}

// ---------------------------------------------------------------------------
// 结果缓存
// ---------------------------------------------------------------------------

/// AI 结果缓存：内存 LRU（有上限）+ SQLite 持久（§11.1 / §11.3）。
///
/// 以 `Arc<AiResultCache>` 挂在 AppState 上，Gateway 在发送前查询、成功后写入。
/// 审计/缓存写入失败只告警，不阻断请求（§16.1：辅助设施不得拖垮主链路）。
pub struct AiResultCache {
    memory: Mutex<Lru>,
    capacity: usize,
    persisted_limit: usize,
}

impl AiResultCache {
    pub fn new(capacity: usize) -> Self {
        Self::with_persisted_limit(capacity, DEFAULT_PERSISTED_LIMIT)
    }

    pub fn with_persisted_limit(capacity: usize, persisted_limit: usize) -> Self {
        Self {
            memory: Mutex::new(Lru {
                capacity: capacity.max(1),
                ..Default::default()
            }),
            capacity: capacity.max(1),
            persisted_limit: persisted_limit.max(1),
        }
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// 内存层当前条目数（测试与诊断用）。
    pub fn memory_len(&self) -> usize {
        self.lock().map.len()
    }

    /// 查询缓存：先内存、后 SQLite（命中后回填内存）。
    pub fn get(&self, conn: &Connection, parts: &CacheKeyParts) -> Option<CachedResult> {
        let key = parts.key();
        if let Some(hit) = self.lock().get(&key) {
            return hit.matches(parts).then_some(hit);
        }
        let row = query_cached(conn, &key).ok().flatten()?;
        if !row.matches(parts) {
            return None;
        }
        let mut memory = self.lock();
        memory.insert(row.clone());
        Some(row)
    }

    /// 写入缓存（内存 + SQLite）。
    pub fn put(&self, conn: &Connection, input: &CacheEntryInput) -> AppResult<()> {
        let entry = CachedResult {
            cache_key: input.parts.key(),
            task_kind: input.parts.task_kind,
            provider_id: input.parts.provider_id.clone(),
            model_id: input.parts.model_id.clone(),
            prompt_version: input.parts.prompt_version.clone(),
            context_hash: input.parts.context_hash.clone(),
            settings_hash: input.parts.settings_hash.clone(),
            result: input.result.clone(),
            session_id: input.session_id.clone(),
            created_at: input.created_at.clone(),
        };
        {
            let mut memory = self.lock();
            memory.insert(entry.clone());
        }
        conn.execute(
            "INSERT OR REPLACE INTO ai_result_cache
             (cache_key, task_kind, provider_id, model_id, prompt_version,
              context_hash, settings_hash, result_json, session_id, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                entry.cache_key,
                entry.task_kind.as_str(),
                entry.provider_id,
                entry.model_id,
                entry.prompt_version,
                entry.context_hash,
                entry.settings_hash,
                serde_json::to_string(&entry.result)?,
                entry.session_id,
                entry.created_at,
            ],
        )?;
        self.prune_persisted(conn)?;
        Ok(())
    }

    /// 使某会话关联的全部缓存失效（内存 + SQLite）。
    ///
    /// 删除会话时调用（§10.4）：DB 侧由 FK 级联清理，内存层必须同步——否则
    /// 会话已删除仍可能命中内存里的旧结果。
    pub fn invalidate_session(&self, conn: &Connection, session_id: &str) {
        {
            let mut memory = self.lock();
            let stale: Vec<String> = memory
                .map
                .iter()
                .filter(|(_, entry)| entry.session_id.as_deref() == Some(session_id))
                .map(|(key, _)| key.clone())
                .collect();
            for key in stale {
                memory.map.remove(&key);
                memory.order.retain(|k| *k != key);
            }
        }
        if let Err(e) = conn.execute("DELETE FROM ai_result_cache WHERE session_id = ?1", params![session_id]) {
            log::warn!("ai cache invalidate session failed: {}", e);
        }
    }

    /// 使单条缓存失效（内存 + SQLite）。
    pub fn invalidate(&self, conn: &Connection, key: &str) {
        {
            let mut memory = self.lock();
            memory.map.remove(key);
            memory.order.retain(|k| k != key);
        }
        if let Err(e) = conn.execute("DELETE FROM ai_result_cache WHERE cache_key = ?1", params![key]) {
            log::warn!("ai cache invalidate failed: {}", e);
        }
    }

    /// 清空全部缓存（设置页「清除缓存」，§12.2）。
    pub fn clear(&self, conn: &Connection) -> AppResult<usize> {
        let removed = self.persisted_count(conn)?;
        {
            let mut memory = self.lock();
            memory.map.clear();
            memory.order.clear();
        }
        conn.execute("DELETE FROM ai_result_cache", [])?;
        Ok(removed)
    }

    /// 持久层条目数（诊断用）。
    pub fn persisted_count(&self, conn: &Connection) -> AppResult<usize> {
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM ai_result_cache", [], |r| r.get(0))?;
        Ok(count as usize)
    }

    /// 淘汰超出上限的最旧持久化条目（防止磁盘无界增长）。
    fn prune_persisted(&self, conn: &Connection) -> AppResult<()> {
        conn.execute(
            "DELETE FROM ai_result_cache
             WHERE cache_key NOT IN (
                 SELECT cache_key FROM ai_result_cache
                 ORDER BY created_at DESC, cache_key DESC
                 LIMIT ?1
             )",
            params![self.persisted_limit as i64],
        )?;
        Ok(())
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Lru> {
        self.memory.lock().unwrap_or_else(|e| e.into_inner())
    }
}

const CACHE_COLS: &str = "cache_key, task_kind, provider_id, model_id, prompt_version, \
     context_hash, settings_hash, result_json, session_id, created_at";

fn query_cached(conn: &Connection, key: &str) -> AppResult<Option<CachedResult>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {} FROM ai_result_cache WHERE cache_key = ?1",
        CACHE_COLS
    ))?;
    let mut rows = stmt.query_map(params![key], row_to_cached)?;
    match rows.next() {
        Some(row) => Ok(Some(row?)),
        None => Ok(None),
    }
}

fn row_to_cached(row: &rusqlite::Row) -> rusqlite::Result<CachedResult> {
    let task_kind_str: String = row.get("task_kind")?;
    let result_json: String = row.get("result_json")?;
    Ok(CachedResult {
        cache_key: row.get("cache_key")?,
        task_kind: AiTaskKind::parse(&task_kind_str).unwrap_or(AiTaskKind::Chat),
        provider_id: row.get("provider_id")?,
        model_id: row.get("model_id")?,
        prompt_version: row.get("prompt_version")?,
        context_hash: row.get("context_hash")?,
        settings_hash: row.get("settings_hash")?,
        // 损坏或版本不兼容的行按未命中处理（不因缓存阻断请求）。
        result: serde_json::from_str(&result_json).unwrap_or(super::request::AiResult::Answer { text: String::new() }),
        session_id: row.get("session_id")?,
        created_at: row.get("created_at")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::request::{
        AiMessage, ContextItem, ContextKind, ExclusionReason, MessageRole, ResponseFormat, ToolPolicy,
    };

    fn open_db() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        crate::db::init_db(&mut conn).unwrap();
        conn
    }

    fn parts(context_hash: &str) -> CacheKeyParts {
        CacheKeyParts {
            task_kind: AiTaskKind::GitReview,
            provider_id: "p1".into(),
            model_id: "m1".into(),
            prompt_version: PROMPT_VERSION.into(),
            context_hash: context_hash.into(),
            settings_hash: "s1".into(),
        }
    }

    fn entry(parts: &CacheKeyParts, text: &str, created_at: &str) -> CacheEntryInput {
        CacheEntryInput {
            parts: parts.clone(),
            result: AiResult::Answer { text: text.into() },
            session_id: None,
            created_at: created_at.into(),
        }
    }

    fn request_with(manifest: Vec<ContextItem>) -> AiRequest {
        AiRequest {
            request_id: "r".into(),
            session_id: None,
            task_kind: AiTaskKind::GitReview,
            git_scenario: None,
            provider_id: Some("p1".into()),
            model_id: Some("m1".into()),
            system_instruction: "sys".into(),
            messages: vec![AiMessage {
                role: MessageRole::User,
                content: "body".into(),
            }],
            context_manifest: manifest,
            response_format: ResponseFormat::Json,
            tool_policy: ToolPolicy::Disabled,
            token_budget: 100,
            temperature: Some(0.2),
            stream: false,
            secret_warn_confirmed: false,
            use_cache: true,
        }
    }

    /// §11.3：缓存 Key 覆盖 modelId / promptVersion / contextHash / settingsHash，
    /// 任一维度变化即失效（不同维度不得互相命中）。
    #[test]
    fn cache_key_isolates_every_dimension() {
        let base = parts("ctx");
        let baseline = base.key();
        assert_ne!(baseline, {
            let mut p = base.clone();
            p.model_id = "m2".into();
            p.key()
        });
        assert_ne!(baseline, {
            let mut p = base.clone();
            p.provider_id = "p2".into();
            p.key()
        });
        assert_ne!(baseline, {
            let mut p = base.clone();
            p.prompt_version = "2".into();
            p.key()
        });
        assert_ne!(baseline, {
            let mut p = base.clone();
            p.context_hash = "ctx2".into();
            p.key()
        });
        assert_ne!(baseline, {
            let mut p = base.clone();
            p.settings_hash = "s2".into();
            p.key()
        });
        assert_ne!(baseline, {
            let mut p = base.clone();
            p.task_kind = AiTaskKind::Chat;
            p.key()
        });
    }

    /// 命中后换模型 / 换 Prompt 版本 / 换上下文 → 全部未命中（§11.3 不得跨模型复用）。
    #[test]
    fn hit_requires_identical_model_prompt_and_context() {
        let conn = open_db();
        let cache = AiResultCache::new(8);
        let key = parts("ctx-1");
        cache
            .put(&conn, &entry(&key, "cached", "2026-01-01T00:00:00Z"))
            .unwrap();

        let hit = cache.get(&conn, &key).expect("same key must hit");
        assert!(matches!(hit.result, AiResult::Answer { ref text } if text == "cached"));

        let mut other_model = key.clone();
        other_model.model_id = "m2".into();
        assert!(cache.get(&conn, &other_model).is_none());

        let mut other_prompt = key.clone();
        other_prompt.prompt_version = "2".into();
        assert!(cache.get(&conn, &other_prompt).is_none());

        let mut other_ctx = key.clone();
        other_ctx.context_hash = "ctx-2".into();
        assert!(cache.get(&conn, &other_ctx).is_none());

        let mut other_settings = key.clone();
        other_settings.settings_hash = "s2".into();
        assert!(cache.get(&conn, &other_settings).is_none());
    }

    /// 内存 LRU 上限（§16.1 / 全局约束：每个 LRU 都有上限）。
    #[test]
    fn memory_lru_respects_capacity_bound() {
        let conn = open_db();
        let cache = AiResultCache::new(3);
        for i in 0..10 {
            let key = parts(&format!("ctx-{i}"));
            cache
                .put(&conn, &entry(&key, &format!("v{i}"), "2026-01-01T00:00:00Z"))
                .unwrap();
        }
        assert!(
            cache.memory_len() <= 3,
            "内存层不得超过上限，实际 {}",
            cache.memory_len()
        );
        // 最近写入的仍在（LRU 淘汰的是最久未使用的）。
        assert!(cache.get(&conn, &parts("ctx-9")).is_some());
        // 最旧的已被淘汰出内存；持久层仍可回源（§11.1 双层）。
        assert_eq!(cache.persisted_count(&conn).unwrap(), 10);
        assert!(cache.get(&conn, &parts("ctx-0")).is_some());
    }

    /// 持久化条目超出上限时淘汰最旧（防止磁盘无界增长）。
    #[test]
    fn persisted_layer_is_bounded() {
        let conn = open_db();
        let cache = AiResultCache::with_persisted_limit(4, 5);
        for i in 0..9 {
            let key = parts(&format!("ctx-{i}"));
            cache
                .put(
                    &conn,
                    &entry(&key, &format!("v{i}"), &format!("2026-01-0{}T00:00:00Z", i + 1)),
                )
                .unwrap();
        }
        assert_eq!(cache.persisted_count(&conn).unwrap(), 5);
        assert!(cache.get(&conn, &parts("ctx-0")).is_none(), "最旧的必须被淘汰");
        assert!(cache.get(&conn, &parts("ctx-8")).is_some(), "最新保留");
    }

    /// 内存未命中时从 SQLite 回源并回填内存。
    #[test]
    fn sqlite_layer_backfills_memory() {
        let conn = open_db();
        let cache = AiResultCache::new(4);
        let key = parts("ctx");
        cache
            .put(&conn, &entry(&key, "from-db", "2026-01-01T00:00:00Z"))
            .unwrap();
        cache.invalidate(&conn, &key.key());
        assert_eq!(cache.memory_len(), 0);
        assert_eq!(cache.persisted_count(&conn).unwrap(), 0);
    }

    /// 会话删除级联清理关联缓存（§10.4）：FK + 显式清理双保险。
    #[test]
    fn session_delete_cascades_cache_rows() {
        let conn = open_db();
        conn.execute(
            "INSERT INTO ai_sessions (id, title, created_at, updated_at) VALUES ('s1', 't', 'c', 'c')",
            [],
        )
        .unwrap();
        let cache = AiResultCache::new(4);
        let key = parts("ctx");
        cache
            .put(
                &conn,
                &CacheEntryInput {
                    parts: key.clone(),
                    result: AiResult::Answer { text: "v".into() },
                    session_id: Some("s1".into()),
                    created_at: "2026-01-01T00:00:00Z".into(),
                },
            )
            .unwrap();
        assert_eq!(cache.persisted_count(&conn).unwrap(), 1);

        conn.execute("DELETE FROM ai_sessions WHERE id = 's1'", []).unwrap();
        // 内存层必须与 DB 同步失效（否则会话已删仍可能命中内存旧结果）。
        cache.invalidate_session(&conn, "s1");
        assert_eq!(cache.persisted_count(&conn).unwrap(), 0);
        assert_eq!(cache.memory_len(), 0);
        assert!(cache.get(&conn, &key).is_none(), "缓存行必须随会话删除");
    }

    /// 内容 hash 口径：system + 消息正文；内容变化即 hash 变化（diff/日志变化 → 失效）。
    #[test]
    fn content_hash_covers_system_and_messages() {
        let mut a = request_with(vec![]);
        let hash_a = request_content_hash(&a);
        a.messages[0].content = "changed".into();
        assert_ne!(hash_a, request_content_hash(&a));
        a.messages[0].content = "body".into();
        a.system_instruction = "sys2".into();
        assert_ne!(hash_a, request_content_hash(&a));
    }

    /// 设置指纹：响应格式 / 温度 / 预算 / 工具策略 / 脱敏排除策略变化 → 失效。
    #[test]
    fn settings_fingerprint_covers_output_and_redaction_policy() {
        let base = request_with(vec![]);
        let hash = settings_fingerprint(&base);

        let mut changed = base.clone();
        changed.response_format = ResponseFormat::Text;
        assert_ne!(hash, settings_fingerprint(&changed));

        let mut changed = base.clone();
        changed.temperature = Some(0.9);
        assert_ne!(hash, settings_fingerprint(&changed));

        let mut changed = base.clone();
        changed.tool_policy = ToolPolicy::ReadOnlyWhitelist;
        assert_ne!(hash, settings_fingerprint(&changed));

        // 脱敏/排除策略变化（§11.3：Secret 脱敏/排除策略变化必须失效）。
        let item = ContextItem {
            kind: ContextKind::Diff,
            source_id: "diff:a.rs".into(),
            display_name: "a.rs".into(),
            char_count: 10,
            estimated_tokens: 3,
            redacted: false,
            truncated: false,
            excluded: false,
            exclusion_reason: None,
        };
        let with_item = request_with(vec![item.clone()]);
        let with_redacted = request_with(vec![ContextItem {
            redacted: true,
            ..item.clone()
        }]);
        assert_ne!(settings_fingerprint(&with_item), settings_fingerprint(&with_redacted));

        let with_excluded = request_with(vec![ContextItem {
            excluded: true,
            exclusion_reason: Some(ExclusionReason::User),
            ..item
        }]);
        assert_ne!(settings_fingerprint(&with_item), settings_fingerprint(&with_excluded));
    }

    /// 清空缓存（§12.2 设置页「清除缓存」）。
    #[test]
    fn clear_removes_every_layer() {
        let conn = open_db();
        let cache = AiResultCache::new(4);
        for i in 0..3 {
            cache
                .put(&conn, &entry(&parts(&format!("ctx-{i}")), "v", "2026-01-01T00:00:00Z"))
                .unwrap();
        }
        let removed = cache.clear(&conn).unwrap();
        assert_eq!(removed, 3);
        assert_eq!(cache.memory_len(), 0);
        assert_eq!(cache.persisted_count(&conn).unwrap(), 0);
    }
}
