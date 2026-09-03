//! LAN Encrypted Mesh Chat（docs/局域网 P2P 加密聊天ao小工具需求与技术设计.md）。
//!
//! 无中心、纯 P2P、端到端加密（XChaCha20-Poly1305 + Argon2id KDF）、
//! 消息只驻留内存，离开房间即销毁（§26-§29）。

pub mod manager;
pub mod message;
pub mod peer;
pub mod protocol;

use std::sync::{Arc, Mutex};

use serde::Serialize;

use crate::discovery::mdns::{NearbyRoom, RoomBrowser};
use crate::error::{AppError, AppResult};
use crate::chat::manager::ChatManager;
use crate::chat::message::ChatMessage;
use crate::chat::peer::Member;

// 事件名（F-15 教训：不能含 `.`，一律 snake_case）。
pub const EVENT_ROOM_STATE: &str = "lan_chat_room_state";
pub const EVENT_MESSAGE: &str = "lan_chat_message";
pub const EVENT_ROOMS: &str = "lan_chat_rooms";
pub const EVENT_ERROR: &str = "lan_chat_error";

/// 推送给前端的房间状态全量快照（事件 `lan_chat_room_state`）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoomState {
    pub room_id: String,
    pub room_name: String,
    pub nickname: String,
    pub peer_id: String,
    /// 本节点 QUIC 监听端口（跨子网 Manual Bootstrap 时告诉其他用户）。
    pub port: u16,
    pub connected_peers: usize,
    pub members: Vec<Member>,
}

/// `lan_chat_generate_secret` 输出（§39）：32 随机字节的三种编码。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SecretOutput {
    pub hex: String,
    pub base64: String,
    pub base64_url: String,
}

/// `lan_chat_error` 事件 payload：面向用户的中文错误提示（不泄露加密内部细节，§50）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatErrorPayload {
    pub message: String,
}

/// 事件出口抽象：生产环境是 Tauri AppHandle，集成测试用内存 sink。
pub trait ChatEventSink: Send + Sync {
    fn emit_room_state(&self, state: &RoomState);
    fn emit_message(&self, message: &ChatMessage);
    fn emit_rooms(&self, rooms: &[NearbyRoom]);
    fn emit_error(&self, message: &str);
}

/// 生产环境事件出口：Tauri `AppHandle.emit`。
pub struct TauriChatEventSink {
    app: tauri::AppHandle,
}

impl TauriChatEventSink {
    pub fn new(app: tauri::AppHandle) -> Self {
        Self { app }
    }
}

impl ChatEventSink for TauriChatEventSink {
    fn emit_room_state(&self, state: &RoomState) {
        use tauri::Emitter;
        let _ = self.app.emit(EVENT_ROOM_STATE, state);
    }

    fn emit_message(&self, message: &ChatMessage) {
        use tauri::Emitter;
        let _ = self.app.emit(EVENT_MESSAGE, message);
    }

    fn emit_rooms(&self, rooms: &[NearbyRoom]) {
        use tauri::Emitter;
        let _ = self.app.emit(EVENT_ROOMS, rooms);
    }

    fn emit_error(&self, message: &str) {
        use tauri::Emitter;
        let _ = self.app.emit(
            EVENT_ERROR,
            ChatErrorPayload {
                message: message.to_string(),
            },
        );
    }
}

/// 全局聊天状态（`app.manage` 注册）：当前房间 + 未进房时的附近房间浏览器。
pub struct LanChatState {
    manager: Mutex<Option<Arc<ChatManager>>>,
    browser: Mutex<Option<RoomBrowser>>,
    /// 当前进房的 peer_id，用于 browse 时过滤本节点自己的广播。
    self_peer_id: Arc<Mutex<Option<String>>>,
}

impl LanChatState {
    pub fn new() -> Self {
        Self {
            manager: Mutex::new(None),
            browser: Mutex::new(None),
            self_peer_id: Arc::new(Mutex::new(None)),
        }
    }

    pub fn manager(&self) -> Option<Arc<ChatManager>> {
        self.manager.lock().ok()?.clone()
    }

    pub fn set_manager(&self, manager: Option<Arc<ChatManager>>) {
        let peer_id = manager.as_ref().map(|m| m.peer_id().to_string());
        if let Ok(mut g) = self.self_peer_id.lock() {
            *g = peer_id;
        }
        if let Ok(mut g) = self.manager.lock() {
            *g = manager;
        }
    }

    pub fn take_manager(&self) -> Option<Arc<ChatManager>> {
        if let Ok(mut g) = self.self_peer_id.lock() {
            *g = None;
        }
        self.manager.lock().ok()?.take()
    }

    /// 未进房时浏览附近房间（§15）。重复调用为幂等 no-op。
    pub fn start_discovery(&self, sink: Arc<dyn ChatEventSink>) -> AppResult<()> {
        let mut guard = self
            .browser
            .lock()
            .map_err(|_| AppError::LanChat("内部状态错误".into()))?;
        if guard.is_some() {
            return Ok(());
        }
        let browser = RoomBrowser::start(Arc::clone(&self.self_peer_id), move |rooms| {
            sink.emit_rooms(&rooms);
        })?;
        *guard = Some(browser);
        Ok(())
    }

    pub fn stop_discovery(&self) {
        if let Ok(mut g) = self.browser.lock() {
            // Drop 内部会 stop_browse + shutdown + join 线程。
            g.take();
        }
    }

    /// App 退出钩子（§29）：尽力而为地清理——停 mDNS、关连接、清内存、清零密钥。
    pub fn shutdown_on_exit(&self) {
        let manager = self.take_manager();
        if let Some(m) = manager {
            // 退出阶段事件循环已接近结束，尽力广播一次 leave（不阻塞退出）。
            let _ = tauri::async_runtime::block_on(async move {
                let _ = tokio::time::timeout(std::time::Duration::from_secs(2), m.leave(true)).await;
            });
        }
        self.stop_discovery();
    }
}
