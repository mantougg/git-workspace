//! 应用层协议（设计文档 §9-§11、§24、§58、§60）。
//!
//! 帧格式：u32 LE 长度前缀 + postcard 序列化的 [`Envelope`]，帧上限 1MB。
//!
//! 明文 metadata：version / room_id / message_id / sender_id / hop_limit / kind / nonce。
//! Chat / Presence 的 `payload` 是 XChaCha20-Poly1305 密文（明文结构含
//! sender_name / timestamp / type / content，见 §10）；Handshake / PeerExchange
//! 是明文控制面，只携带网络元数据，不含 secret / 聊天内容。

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};

/// 协议版本（§58）。不一致的连接在 Handshake 阶段即关闭。
pub const PROTOCOL_VERSION: u8 = 1;
/// 单帧上限 1MB。
pub const MAX_FRAME_SIZE: usize = 1024 * 1024;
/// Gossip 默认 hop limit（§23）。
pub const DEFAULT_HOP_LIMIT: u8 = 8;
/// 消息时间戳容忍窗口（秒），超出视为重放/时钟异常（§25）。
pub const TIMESTAMP_TOLERANCE_SECS: i64 = 10 * 60;

/// 网络消息信封。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Envelope {
    pub version: u8,
    pub room_id: String,
    pub message_id: String,
    pub sender_id: String,
    pub hop_limit: u8,
    pub kind: MessageKind,
    /// 加密消息的 24 字节 nonce；控制面消息为空。
    pub nonce: Vec<u8>,
    /// Chat/Presence 为密文；Handshake/PeerExchange 为明文 postcard 控制数据。
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MessageKind {
    /// 连接建立后的握手（明文控制面）：版本 / peer_id / room_id / listen_port。
    Handshake,
    /// 聊天消息（密文 payload → [`ChatPayload`]）。
    Chat,
    /// 成员状态（密文 payload → [`PresencePayload`]）：join / heartbeat / leave。
    Presence,
    /// 已知 peer 地址交换（明文控制面，§16/§17）。
    PeerExchange,
}

/// 握手（明文控制面）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandshakePayload {
    pub version: u8,
    pub peer_id: String,
    pub room_id: String,
    /// 对端的 QUIC 监听端口（Peer Exchange 时与来源 IP 拼成可拨地址）。
    pub listen_port: u16,
}

/// Peer Exchange（明文控制面）：已知 peer 的 `ip:port` 列表。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerExchangePayload {
    pub peers: Vec<String>,
}

/// 聊天消息明文（加密前，§10）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatPayload {
    pub sender_name: String,
    /// Unix 秒。
    pub timestamp: i64,
    /// 消息类型（§11）：V1 只有 "text"，协议层预留扩展。
    pub msg_type: String,
    pub content: String,
}

/// Presence 明文（加密前）：成员列表只由可成功解密的 Presence 维护（§12.2）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PresencePayload {
    pub sender_name: String,
    pub timestamp: i64,
    pub status: PresenceStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PresenceStatus {
    Join,
    Heartbeat,
    Leave,
}

/// 计算转发后的 hop limit；`None` 表示不再转发（§23：hop 到 0 停止）。
pub fn next_hop_limit(hop_limit: u8) -> Option<u8> {
    if hop_limit > 1 {
        Some(hop_limit - 1)
    } else {
        None
    }
}

/// 时间戳是否在容忍窗口内（§25，纯函数便于测试）。
pub fn timestamp_within_window(timestamp: i64, now: i64) -> bool {
    (now - timestamp).abs() <= TIMESTAMP_TOLERANCE_SECS
}

pub fn encode_payload<T: Serialize>(value: &T) -> AppResult<Vec<u8>> {
    postcard::to_allocvec(value).map_err(|e| AppError::LanChat(format!("协议序列化失败: {e}")))
}

pub fn decode_payload<T: for<'de> Deserialize<'de>>(bytes: &[u8]) -> AppResult<T> {
    postcard::from_bytes(bytes).map_err(|_| AppError::LanChat("协议数据格式无效".into()))
}

/// 序列化 Envelope 并加 u32 LE 长度前缀（帧上限 1MB）。
pub fn encode_frame(envelope: &Envelope) -> AppResult<Vec<u8>> {
    let body = encode_payload(envelope)?;
    if body.len() > MAX_FRAME_SIZE {
        return Err(AppError::LanChat("消息超出大小限制".into()));
    }
    let len = (body.len() as u32).to_le_bytes();
    let mut frame = Vec::with_capacity(4 + body.len());
    frame.extend_from_slice(&len);
    frame.extend_from_slice(&body);
    Ok(frame)
}

/// 从 QUIC 流读一个完整帧；返回 `Ok(None)` 表示对端正常关闭。
pub async fn read_frame(recv: &mut quinn::RecvStream) -> AppResult<Option<Envelope>> {
    let mut len_buf = [0u8; 4];
    match recv.read_exact(&mut len_buf).await {
        Ok(()) => {}
        Err(quinn::ReadExactError::FinishedEarly(_)) => return Ok(None),
        Err(quinn::ReadExactError::ReadError(e)) => {
            return Err(AppError::LanChat(format!("连接读取失败: {e}")))
        }
    }
    let len = u32::from_le_bytes(len_buf) as usize;
    if len == 0 || len > MAX_FRAME_SIZE {
        return Err(AppError::LanChat("收到非法协议帧".into()));
    }
    let mut body = vec![0u8; len];
    match recv.read_exact(&mut body).await {
        Ok(()) => {}
        Err(quinn::ReadExactError::FinishedEarly(_)) => {
            return Err(AppError::LanChat("协议帧被截断".into()))
        }
        Err(quinn::ReadExactError::ReadError(e)) => {
            return Err(AppError::LanChat(format!("连接读取失败: {e}")))
        }
    }
    Ok(Some(decode_payload(&body)?))
}

/// 向 QUIC 流写一个完整帧。
pub async fn write_frame(send: &mut quinn::SendStream, envelope: &Envelope) -> AppResult<()> {
    let frame = encode_frame(envelope)?;
    send.write_all(&frame)
        .await
        .map_err(|e| AppError::LanChat(format!("连接写入失败: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_envelope() -> Envelope {
        Envelope {
            version: PROTOCOL_VERSION,
            room_id: "room-1".into(),
            message_id: "msg-1".into(),
            sender_id: "peer-a1b2c3".into(),
            hop_limit: DEFAULT_HOP_LIMIT,
            kind: MessageKind::Chat,
            nonce: vec![9u8; 24],
            payload: vec![1, 2, 3, 4],
        }
    }

    #[test]
    fn envelope_frame_roundtrip() {
        let env = sample_envelope();
        let frame = encode_frame(&env).unwrap();
        let len = u32::from_le_bytes(frame[..4].try_into().unwrap()) as usize;
        assert_eq!(len, frame.len() - 4);
        let decoded: Envelope = decode_payload(&frame[4..]).unwrap();
        assert_eq!(decoded.message_id, "msg-1");
        assert_eq!(decoded.kind, MessageKind::Chat);
        assert_eq!(decoded.nonce, vec![9u8; 24]);
    }

    #[test]
    fn hop_limit_decrements_and_stops_at_zero() {
        assert_eq!(next_hop_limit(8), Some(7));
        assert_eq!(next_hop_limit(2), Some(1));
        // hop_limit=1 再减就是 0 → 不转发（§23）。
        assert_eq!(next_hop_limit(1), None);
        assert_eq!(next_hop_limit(0), None);
    }

    #[test]
    fn timestamp_window_check() {
        let now = 1_000_000;
        assert!(timestamp_within_window(now, now));
        assert!(timestamp_within_window(now - 600, now));
        assert!(timestamp_within_window(now + 600, now));
        assert!(!timestamp_within_window(now - 601, now));
        assert!(!timestamp_within_window(now + 601, now));
    }

    #[test]
    fn control_payloads_roundtrip() {
        let hs = HandshakePayload {
            version: PROTOCOL_VERSION,
            peer_id: "peer-deadbeef".into(),
            room_id: "room-x".into(),
            listen_port: 45678,
        };
        let bytes = encode_payload(&hs).unwrap();
        let back: HandshakePayload = decode_payload(&bytes).unwrap();
        assert_eq!(back.peer_id, "peer-deadbeef");
        assert_eq!(back.listen_port, 45678);

        let px = PeerExchangePayload {
            peers: vec!["192.168.1.5:45678".into()],
        };
        let bytes = encode_payload(&px).unwrap();
        let back: PeerExchangePayload = decode_payload(&bytes).unwrap();
        assert_eq!(back.peers, vec!["192.168.1.5:45678"]);
    }
}
