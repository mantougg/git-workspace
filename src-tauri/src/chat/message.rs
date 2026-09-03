//! 聊天消息类型与 Gossip 去重缓存（设计文档 §22/§25）。

use std::collections::HashMap;
use std::time::{Duration, Instant};

use serde::Serialize;

/// SeenMessageCache TTL：10 分钟（§22/§25）。
pub const SEEN_TTL: Duration = Duration::from_secs(10 * 60);
/// 缓存容量上限，超出时淘汰最旧条目（LRU 语义近似）。
pub const SEEN_CAPACITY: usize = 4096;

/// 推送给前端的聊天消息（事件 `lan_chat_message`）。
/// 只推送可成功解密的消息。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessage {
    pub message_id: String,
    pub sender_name: String,
    pub content: String,
    /// Unix 秒。
    pub timestamp: i64,
    pub mine: bool,
}

/// Gossip 去重缓存：message_id → 首次见到的时间。
/// 重复 message_id 直接丢弃（§22），同时承担 replay 防护（§25）。
pub struct SeenMessageCache {
    seen: HashMap<String, Instant>,
}

impl SeenMessageCache {
    pub fn new() -> Self {
        Self {
            seen: HashMap::new(),
        }
    }

    /// 首次见到返回 `true`（并标记）；重复返回 `false`（调用方应丢弃）。
    pub fn check_and_mark(&mut self, message_id: &str) -> bool {
        if self.seen.contains_key(message_id) {
            return false;
        }
        if self.seen.len() >= SEEN_CAPACITY {
            self.evict_oldest();
        }
        self.seen.insert(message_id.to_string(), Instant::now());
        true
    }

    /// 清理过期条目（由周期任务调用）。
    pub fn prune_expired(&mut self) {
        self.seen.retain(|_, ts| ts.elapsed() < SEEN_TTL);
    }

    /// 清空缓存（离房时调用，§28）。
    pub fn clear(&mut self) {
        self.seen.clear();
    }

    fn evict_oldest(&mut self) {
        if let Some(oldest) = self
            .seen
            .iter()
            .max_by_key(|(_, ts)| ts.elapsed())
            .map(|(id, _)| id.clone())
        {
            self.seen.remove(&oldest);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicate_message_id_is_dropped() {
        let mut cache = SeenMessageCache::new();
        assert!(cache.check_and_mark("msg-1"));
        // 同一 message_id 第二次处理被丢弃（§22/§61 Replay）。
        assert!(!cache.check_and_mark("msg-1"));
        assert!(!cache.check_and_mark("msg-1"));
        assert!(cache.check_and_mark("msg-2"));
    }

    #[test]
    fn capacity_eviction_keeps_cache_bounded() {
        let mut cache = SeenMessageCache::new();
        for i in 0..(SEEN_CAPACITY + 10) {
            cache.check_and_mark(&format!("msg-{i}"));
        }
        assert!(cache.seen.len() <= SEEN_CAPACITY);
    }
}
