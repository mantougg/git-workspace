//! LAN Chat Tauri 命令（docs/局域网 P2P 加密聊天ao小工具需求与技术设计.md §47）。
//!
//! 业务逻辑在 `chat` / `crypto` / `discovery` / `transport` 模块，
//! 本文件只做参数校验与转发。错误信息面向用户、使用中文，
//! 不泄露加密内部细节（§50）。

use std::sync::Arc;

use base64::engine::general_purpose::{STANDARD as B64, URL_SAFE_NO_PAD as B64URL};
use base64::Engine;
use rand::RngCore;
use tauri::{AppHandle, Emitter, State};
use zeroize::{Zeroize, Zeroizing};

use crate::chat::manager::ChatManager;
use crate::chat::{
    ChatEventSink, LanChatState, RoomState, SecretOutput, TauriChatEventSink, EVENT_ROOMS,
    EVENT_ROOM_STATE,
};
use crate::discovery::mdns::NearbyRoom;
use crate::error::{AppError, AppResult};
use crate::transport::quic;

/// 房间名 / 昵称长度上限（字符数）。
const MAX_NAME_LEN: usize = 32;
/// 单条消息长度上限（字节），远低于协议帧上限 1MB。
const MAX_MESSAGE_LEN: usize = 8192;

fn validate_name(field: &str, value: &str) -> AppResult<()> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(AppError::LanChat(format!("{field}不能为空")));
    }
    if trimmed.chars().count() > MAX_NAME_LEN {
        return Err(AppError::LanChat(format!("{field}过长（最多 {MAX_NAME_LEN} 个字符）")));
    }
    Ok(())
}

fn validate_secret(secret: &str) -> AppResult<()> {
    if secret.trim().is_empty() {
        return Err(AppError::LanChat("加密 Secret 不能为空".into()));
    }
    Ok(())
}

/// 生成 256-bit 随机 Secret（§39），输出 hex / base64 / base64url 三种格式。
#[tauri::command]
pub fn lan_chat_generate_secret() -> SecretOutput {
    let mut bytes = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    let output = SecretOutput {
        hex: bytes.iter().map(|b| format!("{b:02x}")).collect(),
        base64: B64.encode(bytes),
        base64_url: B64URL.encode(bytes),
    };
    bytes.zeroize();
    output
}

/// 创建房间（§12.1）。
#[tauri::command]
pub async fn lan_chat_create_room(
    app: AppHandle,
    state: State<'_, LanChatState>,
    room_name: String,
    secret: String,
    nickname: String,
) -> AppResult<RoomState> {
    validate_name("房间名", &room_name)?;
    validate_name("昵称", &nickname)?;
    validate_secret(&secret)?;
    if state.manager().is_some() {
        return Err(AppError::LanChat("已在房间中，请先离开当前房间".into()));
    }
    let sink: Arc<dyn ChatEventSink> = Arc::new(TauriChatEventSink::new(app));
    let secret = Zeroizing::new(secret);
    let manager = ChatManager::create(
        sink,
        room_name.trim().to_string(),
        &secret,
        nickname.trim().to_string(),
        true,
    )
    .await?;
    let snapshot = manager.snapshot();
    state.set_manager(Some(manager));
    Ok(snapshot)
}

/// 加入房间（§12.2）。`bootstrap` 为可选的跨子网手动引导地址（"ip:port"，§16）。
#[tauri::command]
pub async fn lan_chat_join_room(
    app: AppHandle,
    state: State<'_, LanChatState>,
    room_id: String,
    secret: String,
    nickname: String,
    bootstrap: Option<String>,
) -> AppResult<RoomState> {
    validate_name("Room ID", &room_id)?;
    validate_name("昵称", &nickname)?;
    validate_secret(&secret)?;
    if state.manager().is_some() {
        return Err(AppError::LanChat("已在房间中，请先离开当前房间".into()));
    }
    let bootstrap = match bootstrap {
        Some(raw) if !raw.trim().is_empty() => Some(quic::parse_addr(&raw)?),
        _ => None,
    };
    let sink: Arc<dyn ChatEventSink> = Arc::new(TauriChatEventSink::new(app));
    let secret = Zeroizing::new(secret);
    let manager = ChatManager::join(
        sink,
        room_id.trim().to_string(),
        &secret,
        nickname.trim().to_string(),
        bootstrap,
        true,
    )
    .await?;
    let snapshot = manager.snapshot();
    state.set_manager(Some(manager));
    Ok(snapshot)
}

/// 离开房间（§26-§28）：leave 与 close 对本节点语义相同——销毁本地房间状态。
#[tauri::command]
pub async fn lan_chat_leave_room(app: AppHandle, state: State<'_, LanChatState>) -> AppResult<()> {
    if let Some(manager) = state.take_manager() {
        manager.leave(true).await;
    }
    // 推送 null 快照让前端清空房间状态。
    let _ = app.emit(EVENT_ROOM_STATE, Option::<RoomState>::None);
    Ok(())
}

/// 发送文本消息。
#[tauri::command]
pub async fn lan_chat_send_message(state: State<'_, LanChatState>, text: String) -> AppResult<()> {
    if text.trim().is_empty() {
        return Err(AppError::LanChat("消息不能为空".into()));
    }
    if text.len() > MAX_MESSAGE_LEN {
        return Err(AppError::LanChat(format!("消息过长（最多 {MAX_MESSAGE_LEN} 字节）")));
    }
    let manager = state
        .manager()
        .ok_or_else(|| AppError::LanChat("当前不在任何房间中".into()))?;
    manager.send_message(&text).await
}

/// 当前房间状态（未进房返回 null）。
#[tauri::command]
pub fn lan_chat_room_state(state: State<'_, LanChatState>) -> Option<RoomState> {
    state.manager().map(|m| m.snapshot())
}

/// 开始浏览附近房间（未进房时使用，§15）。结果通过 `lan_chat_rooms` 事件推送。
#[tauri::command]
pub fn lan_chat_start_discovery(app: AppHandle, state: State<'_, LanChatState>) -> AppResult<()> {
    let sink: Arc<dyn ChatEventSink> = Arc::new(TauriChatEventSink::new(app));
    state.start_discovery(sink)
}

/// 停止浏览附近房间。
#[tauri::command]
pub fn lan_chat_stop_discovery(app: AppHandle, state: State<'_, LanChatState>) -> AppResult<()> {
    state.stop_discovery();
    // 清空前端附近房间列表。
    let _ = app.emit(EVENT_ROOMS, Vec::<NearbyRoom>::new());
    Ok(())
}

/// 本机局域网 IPv4 地址列表（房间头部分享监听地址用；多网卡时会有多个，
/// 前端逐一拼 `IP:Port` 展示）。失败时返回空列表，不报错——展示层降级即可。
#[tauri::command]
pub fn lan_chat_local_addrs() -> Vec<String> {
    let mut addrs: Vec<String> = if_addrs::get_if_addrs()
        .map(|ifs| {
            ifs.into_iter()
                .filter(|i| !i.is_loopback())
                .filter_map(|i| match i.addr.ip() {
                    std::net::IpAddr::V4(v4) => Some(v4.to_string()),
                    std::net::IpAddr::V6(_) => None,
                })
                .collect()
        })
        .unwrap_or_default();
    addrs.sort();
    addrs.dedup();
    addrs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_secret_outputs_three_formats() {
        let out = lan_chat_generate_secret();
        assert_eq!(out.hex.len(), 64);
        assert!(out.hex.chars().all(|c| c.is_ascii_hexdigit()));
        // 32 字节标准 base64（含 padding）为 44 字符。
        assert_eq!(out.base64.len(), 44);
        // base64url 无 padding 为 43 字符，且不含 + / =。
        assert_eq!(out.base64_url.len(), 43);
        assert!(!out.base64_url.contains(['+', '/', '=']));
        // 三种格式解码后应是同一批字节。
        let raw = B64.decode(&out.base64).unwrap();
        assert_eq!(raw.len(), 32);
        assert_eq!(B64URL.decode(&out.base64_url).unwrap(), raw);
        assert_eq!(out.hex, raw.iter().map(|b| format!("{b:02x}")).collect::<String>());
    }

    #[test]
    fn validation_rejects_empty_and_overlong() {
        assert!(validate_name("昵称", "").is_err());
        assert!(validate_name("昵称", "   ").is_err());
        assert!(validate_name("昵称", &"长".repeat(33)).is_err());
        assert!(validate_name("昵称", &"好".repeat(32)).is_ok());
        assert!(validate_secret("").is_err());
        assert!(validate_secret("  ").is_err());
        assert!(validate_secret("some-secret").is_ok());
    }
}

#[cfg(test)]
mod contract_tests {
    //! IPC 契约锁定：前端按 camelCase 字段名消费，改字段即破坏契约。
    use crate::chat::peer::Member;
    use crate::chat::{RoomState, SecretOutput};
    use crate::chat::message::ChatMessage;
    use crate::discovery::mdns::NearbyRoom;

    #[test]
    fn ipc_payloads_are_camel_case() {
        let state = RoomState {
            room_id: "r1".into(),
            room_name: "n".into(),
            nickname: "alice".into(),
            peer_id: "peer-x".into(),
            port: 45678,
            connected_peers: 2,
            members: vec![Member {
                peer_id: "peer-x".into(),
                nickname: "alice".into(),
                is_self: true,
            }],
        };
        let v = serde_json::to_value(&state).unwrap();
        for key in ["roomId", "roomName", "nickname", "peerId", "port", "connectedPeers", "members"] {
            assert!(v.get(key).is_some(), "RoomState missing {key}");
        }
        let m = &v["members"][0];
        for key in ["peerId", "nickname", "isSelf"] {
            assert!(m.get(key).is_some(), "Member missing {key}");
        }

        let msg = ChatMessage {
            message_id: "m1".into(),
            sender_name: "alice".into(),
            content: "hi".into(),
            timestamp: 1,
            mine: false,
        };
        let v = serde_json::to_value(&msg).unwrap();
        for key in ["messageId", "senderName", "content", "timestamp", "mine"] {
            assert!(v.get(key).is_some(), "ChatMessage missing {key}");
        }

        let room = NearbyRoom {
            room_id: "r1".into(),
            room_name: "n".into(),
            addr: "192.168.1.5".into(),
            port: 45678,
        };
        let v = serde_json::to_value(&room).unwrap();
        for key in ["roomId", "roomName", "addr", "port"] {
            assert!(v.get(key).is_some(), "NearbyRoom missing {key}");
        }

        let v = serde_json::to_value(SecretOutput {
            hex: "h".into(),
            base64: "b".into(),
            base64_url: "u".into(),
        })
        .unwrap();
        for key in ["hex", "base64", "base64Url"] {
            assert!(v.get(key).is_some(), "SecretOutput missing {key}");
        }
    }
}
