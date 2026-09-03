//! Peer 连接管理（设计文档 §43）：连接句柄与 Peer 表。

use std::net::SocketAddr;

use serde::Serialize;
use tokio::sync::mpsc;

use super::protocol::Envelope;

/// Partial Mesh 目标连接数（§2.2/§56）：每节点 4～8 个活跃连接。
pub const MIN_PEERS: usize = 4;
pub const MAX_PEERS: usize = 8;

/// Peer Exchange 一次最多交换的地址数。
pub const MAX_EXCHANGED_ADDRS: usize = 32;

/// 成员列表条目（事件 `lan_chat_room_state` 内嵌）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Member {
    pub peer_id: String,
    pub nickname: String,
    pub is_self: bool,
}

/// 成员内部状态：昵称 + 最近一次收到其（可解密的）Presence 的时间。
/// 成员列表只由可成功解密的 Presence 维护（§12.2）。
#[derive(Debug, Clone)]
pub struct MemberInfo {
    pub nickname: String,
    pub last_seen: std::time::Instant,
}

/// 一条到远端 peer 的活跃连接。
pub struct PeerHandle {
    pub peer_id: String,
    /// 对端连接地址（QUIC remote addr）。
    pub addr: SocketAddr,
    /// 对端自报的监听端口（来自 Handshake，供 Peer Exchange 拼可拨地址）。
    pub listen_port: u16,
    /// 出站发送通道（writer task 消费）。
    pub tx: mpsc::UnboundedSender<Envelope>,
    /// 是否为本节点主动拨出（断线重连只由拨出方发起）。
    pub outbound: bool,
    /// quinn 连接（用于稳定 ID 比对与主动关闭）。
    pub conn: quinn::Connection,
}

impl PeerHandle {
    /// 该 peer 的可拨地址（remote IP + 其监听端口）。
    pub fn dial_addr(&self) -> SocketAddr {
        SocketAddr::new(self.addr.ip(), self.listen_port)
    }
}
