//! ChatManager（设计文档 §42）：房间生命周期 / Peer 管理 / Gossip / 发现 / 加密编排。
//!
//! 日志纪律（§49）：只记录 room_id / peer_id / message_id 等标识，
//! 绝不输出 secret / key / 明文 / 密文。

use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use tokio::sync::mpsc;
use zeroize::Zeroizing;

use crate::crypto::{cipher, kdf};
use crate::discovery::mdns as discovery;
use crate::error::{AppError, AppResult};
use crate::transport::quic;

use super::message::{ChatMessage, SeenMessageCache};
use super::peer::{Member, MemberInfo, PeerHandle, MAX_EXCHANGED_ADDRS, MAX_PEERS, MIN_PEERS};
use super::protocol::{
    self, ChatPayload, Envelope, HandshakePayload, MessageKind, PeerExchangePayload,
    PresencePayload, PresenceStatus, DEFAULT_HOP_LIMIT, PROTOCOL_VERSION,
};
use super::{ChatEventSink, RoomState};

/// Presence 周期广播间隔（§：加入一次 + 每 30 秒心跳 + 离开一次）。
const PRESENCE_INTERVAL: Duration = Duration::from_secs(30);
/// 维护任务间隔：seen cache 清理 / 成员超时剔除 / 连接数补拨。
const MAINTENANCE_INTERVAL: Duration = Duration::from_secs(15);
/// 超过 3 个心跳周期未收到 Presence 的成员视为离线。
const MEMBER_TIMEOUT: Duration = Duration::from_secs(90);
/// 断线重连指数退避（§52）：1s → 2s → … → 30s 封顶。
const RECONNECT_MIN: Duration = Duration::from_secs(1);
const RECONNECT_MAX: Duration = Duration::from_secs(30);
/// 握手超时，避免慢连接挂住 accept 任务。
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn new_peer_id() -> String {
    let raw = uuid::Uuid::new_v4().simple().to_string();
    format!("peer-{}", &raw[..8])
}

fn new_message_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// 一个房间的完整运行时。所有共享状态走 `Arc<ChatManager>`；
/// 离开房间时 `leave()` 停任务、关连接、清内存，Room Key 由
/// `Zeroizing` 在 drop 时清零（§28/§54）。
pub struct ChatManager {
    sink: Arc<dyn ChatEventSink>,
    room_id: String,
    room_name: String,
    nickname: String,
    peer_id: String,
    port: u16,
    key: Zeroizing<[u8; 32]>,
    endpoint: quinn::Endpoint,
    peers: Mutex<HashMap<String, PeerHandle>>,
    /// 正在拨出、尚未完成握手的地址（防止重复拨号）。
    connecting: Mutex<HashSet<SocketAddr>>,
    /// 已知可拨地址（Peer Exchange / mDNS 学习而来），用于补拨与重连。
    known_addrs: Mutex<HashSet<SocketAddr>>,
    seen: Mutex<SeenMessageCache>,
    members: Mutex<HashMap<String, MemberInfo>>,
    alive: Arc<AtomicBool>,
    tasks: Mutex<Vec<tokio::task::JoinHandle<()>>>,
    /// 收帧分发：各连接的 read_loop 只做 IO，帧统一由 dispatcher 任务处理
    /// （同时切断 read_loop → handle_frame → connect 的 async 递归环）。
    frame_tx: mpsc::UnboundedSender<(Envelope, String)>,
    mdns: Mutex<Option<(mdns_sd::ServiceDaemon, String)>>,
    mdns_browse: Mutex<Option<(mdns_sd::ServiceDaemon, std::thread::JoinHandle<()>)>>,
}

impl ChatManager {
    /// Create Room（§12.1）：生成 room_id，监听随机端口，广播 mDNS。
    pub async fn create(
        sink: Arc<dyn ChatEventSink>,
        room_name: String,
        secret: &Zeroizing<String>,
        nickname: String,
        enable_mdns: bool,
    ) -> AppResult<Arc<Self>> {
        let room_id = uuid::Uuid::new_v4().simple().to_string();
        Self::start(sink, room_id, room_name, secret, nickname, None, enable_mdns).await
    }

    /// Join Room（§12.2）：使用外部拿到的 room_id + 同一 Shared Secret。
    /// `bootstrap` 为跨子网手动引导地址（§16/§17）。
    pub async fn join(
        sink: Arc<dyn ChatEventSink>,
        room_id: String,
        secret: &Zeroizing<String>,
        nickname: String,
        bootstrap: Option<SocketAddr>,
        enable_mdns: bool,
    ) -> AppResult<Arc<Self>> {
        // Join 侧不知道房间名，mDNS 广播里用 room_id 占位。
        Self::start(sink, room_id, String::new(), secret, nickname, bootstrap, enable_mdns).await
    }

    async fn start(
        sink: Arc<dyn ChatEventSink>,
        room_id: String,
        room_name: String,
        secret: &Zeroizing<String>,
        nickname: String,
        bootstrap: Option<SocketAddr>,
        enable_mdns: bool,
    ) -> AppResult<Arc<Self>> {
        // Argon2id 是阻塞 CPU 任务，放 spawn_blocking；secret 随闭包 drop 后清零。
        let secret_z = Zeroizing::new(secret.as_str().to_string());
        let kdf_room = room_id.clone();
        let key = tokio::task::spawn_blocking(move || kdf::derive_room_key(&secret_z, &kdf_room))
            .await
            .map_err(|e| AppError::LanChat(format!("密钥派生任务失败: {e}")))??;

        let (endpoint, port) = quic::bind_endpoint()?;
        let peer_id = new_peer_id();
        let (frame_tx, frame_rx) = mpsc::unbounded_channel();

        let mut members = HashMap::new();
        members.insert(
            peer_id.clone(),
            MemberInfo {
                nickname: nickname.clone(),
                last_seen: Instant::now(),
            },
        );

        let manager = Arc::new(Self {
            sink,
            room_id,
            room_name,
            nickname,
            peer_id,
            port,
            key,
            endpoint,
            peers: Mutex::new(HashMap::new()),
            connecting: Mutex::new(HashSet::new()),
            known_addrs: Mutex::new(HashSet::new()),
            seen: Mutex::new(SeenMessageCache::new()),
            members: Mutex::new(members),
            alive: Arc::new(AtomicBool::new(true)),
            tasks: Mutex::new(Vec::new()),
            frame_tx,
            mdns: Mutex::new(None),
            mdns_browse: Mutex::new(None),
        });

        // 日志只带 room_id / peer_id（§49：禁止 secret / 明文）。
        log::info!("LAN chat room {} started, peer {}, port {}", manager.room_id, manager.peer_id, manager.port);

        manager.spawn_accept_loop();
        manager.spawn_dispatcher(frame_rx);
        manager.spawn_presence_loop();
        manager.spawn_maintenance_loop();
        if enable_mdns {
            manager.start_mdns();
        }

        // 加入即广播一次 Presence（§：加入时广播）。
        if let Ok(env) = manager.presence_envelope(PresenceStatus::Join) {
            manager.broadcast_except(&env, None);
        }

        if let Some(addr) = bootstrap {
            if let Err(e) = manager.connect(addr).await {
                manager.leave(false).await;
                return Err(e);
            }
        }

        manager.emit_state();
        Ok(manager)
    }

    // ------------------------------------------------------------------
    // 只读访问（commands / 测试用）
    // ------------------------------------------------------------------

    pub fn peer_id(&self) -> &str {
        &self.peer_id
    }

    pub fn room_id(&self) -> &str {
        &self.room_id
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn connected_count(&self) -> usize {
        self.peers.lock().map(|p| p.len()).unwrap_or(0)
    }

    pub fn member_count(&self) -> usize {
        self.members.lock().map(|m| m.len()).unwrap_or(0)
    }

    /// 全量房间状态快照（事件 `lan_chat_room_state` payload）。
    pub fn snapshot(&self) -> RoomState {
        let connected_peers = self.connected_count();
        let mut members: Vec<Member> = self
            .members
            .lock()
            .map(|m| {
                m.iter()
                    .map(|(id, info)| Member {
                        peer_id: id.clone(),
                        nickname: info.nickname.clone(),
                        is_self: id == &self.peer_id,
                    })
                    .collect()
            })
            .unwrap_or_default();
        members.sort_by(|a, b| a.peer_id.cmp(&b.peer_id));
        RoomState {
            room_id: self.room_id.clone(),
            room_name: self.room_name.clone(),
            nickname: self.nickname.clone(),
            peer_id: self.peer_id.clone(),
            port: self.port,
            connected_peers,
            members,
        }
    }

    fn emit_state(&self) {
        self.sink.emit_room_state(&self.snapshot());
    }

    // ------------------------------------------------------------------
    // 发送
    // ------------------------------------------------------------------

    /// 发送文本消息（§24）：本地 echo（mine=true）+ Gossip 广播。
    pub async fn send_message(&self, text: &str) -> AppResult<()> {
        if !self.alive.load(Ordering::SeqCst) {
            return Err(AppError::LanChat("房间已关闭".into()));
        }
        let payload = ChatPayload {
            sender_name: self.nickname.clone(),
            timestamp: unix_now(),
            msg_type: "text".into(),
            content: text.to_string(),
        };
        let env = self.encrypted_envelope(MessageKind::Chat, &payload)?;
        // 自己的消息先进 seen cache，防止 gossip 回环后重复投递（§22）。
        if let Ok(mut seen) = self.seen.lock() {
            seen.check_and_mark(&env.message_id);
        }
        self.sink.emit_message(&ChatMessage {
            message_id: env.message_id.clone(),
            sender_name: self.nickname.clone(),
            content: text.to_string(),
            timestamp: payload.timestamp,
            mine: true,
        });
        self.broadcast_except(&env, None);
        Ok(())
    }

    fn base_envelope(&self, kind: MessageKind, nonce: Vec<u8>, payload: Vec<u8>) -> Envelope {
        Envelope {
            version: PROTOCOL_VERSION,
            room_id: self.room_id.clone(),
            message_id: new_message_id(),
            sender_id: self.peer_id.clone(),
            hop_limit: DEFAULT_HOP_LIMIT,
            kind,
            nonce,
            payload,
        }
    }

    fn encrypted_envelope<T: serde::Serialize>(&self, kind: MessageKind, payload: &T) -> AppResult<Envelope> {
        let plain = protocol::encode_payload(payload)?;
        let (nonce, ciphertext) = cipher::encrypt(&self.key, &plain)?;
        Ok(self.base_envelope(kind, nonce, ciphertext))
    }

    fn presence_envelope(&self, status: PresenceStatus) -> AppResult<Envelope> {
        let payload = PresencePayload {
            sender_name: self.nickname.clone(),
            timestamp: unix_now(),
            status,
        };
        let env = self.encrypted_envelope(MessageKind::Presence, &payload)?;
        // 自己的 presence 也标记 seen。
        if let Ok(mut seen) = self.seen.lock() {
            seen.check_and_mark(&env.message_id);
        }
        Ok(env)
    }

    fn handshake_envelope(&self) -> AppResult<Envelope> {
        let payload = protocol::encode_payload(&HandshakePayload {
            version: PROTOCOL_VERSION,
            peer_id: self.peer_id.clone(),
            room_id: self.room_id.clone(),
            listen_port: self.port,
        })?;
        // 控制面消息：hop_limit=0（不参与 gossip）、nonce 为空。
        Ok(Envelope {
            hop_limit: 0,
            ..self.base_envelope(MessageKind::Handshake, Vec::new(), payload)
        })
    }

    /// Peer Exchange（§16/§17）：已知 peer 的可拨地址（明文控制面）。
    fn peer_exchange_envelope(&self, exclude_addr: Option<SocketAddr>) -> AppResult<Envelope> {
        let mut addrs: Vec<String> = self
            .peers
            .lock()
            .map(|peers| {
                peers
                    .values()
                    .map(|h| h.dial_addr().to_string())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        if let Ok(known) = self.known_addrs.lock() {
            addrs.extend(known.iter().map(|a| a.to_string()));
        }
        addrs.sort();
        addrs.dedup();
        if let Some(ex) = exclude_addr {
            addrs.retain(|a| a != &ex.to_string());
        }
        addrs.truncate(MAX_EXCHANGED_ADDRS);
        let payload = protocol::encode_payload(&PeerExchangePayload { peers: addrs })?;
        Ok(Envelope {
            hop_limit: 0,
            ..self.base_envelope(MessageKind::PeerExchange, Vec::new(), payload)
        })
    }

    /// 广播给除 `except_peer` 外的所有已连接 peer（Gossip，§57）。
    fn broadcast_except(&self, env: &Envelope, except_peer: Option<&str>) {
        let targets: Vec<mpsc::UnboundedSender<Envelope>> = self
            .peers
            .lock()
            .map(|peers| {
                peers
                    .iter()
                    .filter(|(id, _)| Some(id.as_str()) != except_peer)
                    .map(|(_, h)| h.tx.clone())
                    .collect()
            })
            .unwrap_or_default();
        for tx in targets {
            let _ = tx.send(env.clone());
        }
    }

    /// 只发给单个 peer（新连接建立后告知其我们的昵称）。
    fn send_to_peer(&self, peer_id: &str, env: &Envelope) {
        if let Ok(peers) = self.peers.lock() {
            if let Some(h) = peers.get(peer_id) {
                let _ = h.tx.send(env.clone());
            }
        }
    }

    // ------------------------------------------------------------------
    // 连接建立
    // ------------------------------------------------------------------

    /// 主动拨号（Manual Bootstrap / Peer Exchange / mDNS / 重连共用）。
    pub async fn connect(self: &Arc<Self>, addr: SocketAddr) -> AppResult<()> {
        if !self.alive.load(Ordering::SeqCst) {
            return Err(AppError::LanChat("房间已关闭".into()));
        }
        // 已连接 / 正在连接同一地址则跳过。
        {
            let peers = self.peers.lock().map_err(|_| AppError::LanChat("内部状态错误".into()))?;
            if peers.values().any(|h| h.addr == addr || h.dial_addr() == addr) {
                return Ok(());
            }
        }
        {
            let mut connecting = self.connecting.lock().map_err(|_| AppError::LanChat("内部状态错误".into()))?;
            if !connecting.insert(addr) {
                return Ok(());
            }
        }
        let result = self.connect_inner(addr).await;
        if let Ok(mut connecting) = self.connecting.lock() {
            connecting.remove(&addr);
        }
        result
    }

    async fn connect_inner(self: &Arc<Self>, addr: SocketAddr) -> AppResult<()> {
        let connect_err = || AppError::LanChat("无法连接到对端，请检查地址、端口与网络连通性".into());
        let connecting = self
            .endpoint
            .connect(addr, "localhost")
            .map_err(|_| connect_err())?;
        let conn = connecting.await.map_err(|_| connect_err())?;

        let (mut send, mut recv) = conn.open_bi().await.map_err(|_| connect_err())?;
        let handshake = self.handshake_envelope()?;
        protocol::write_frame(&mut send, &handshake).await.map_err(|_| connect_err())?;

        // 等对端握手回复（带超时）。
        let reply = tokio::time::timeout(HANDSHAKE_TIMEOUT, protocol::read_frame(&mut recv))
            .await
            .map_err(|_| AppError::LanChat("连接对端超时".into()))?
            .map_err(|_| connect_err())?
            .ok_or_else(connect_err)?;
        let remote = self.validate_handshake(&reply)?;
        if remote.peer_id == self.peer_id {
            conn.close(0u32.into(), b"self");
            return Ok(());
        }

        if !self.finish_registration(conn.clone(), send, recv, remote, addr, true).await {
            conn.close(0u32.into(), b"duplicate");
        }
        Ok(())
    }

    /// accept 循环：接受入站 QUIC 连接。
    fn spawn_accept_loop(self: &Arc<Self>) {
        let this = Arc::clone(self);
        let handle = tokio::spawn(async move {
            while this.alive.load(Ordering::SeqCst) {
                let Some(incoming) = this.endpoint.accept().await else { break };
                let that = Arc::clone(&this);
                tokio::spawn(async move {
                    if let Err(e) = that.handle_incoming(incoming).await {
                        log::debug!("LAN chat inbound connection ended: {e}");
                    }
                });
            }
        });
        self.tasks.lock().unwrap_or_else(|e| e.into_inner()).push(handle);
    }

    async fn handle_incoming(self: &Arc<Self>, incoming: quinn::Incoming) -> AppResult<()> {
        let conn = incoming
            .await
            .map_err(|e| AppError::LanChat(format!("入站连接失败: {e}")))?;
        let remote_addr = conn.remote_address();
        let (mut send, mut recv) = conn
            .accept_bi()
            .await
            .map_err(|e| AppError::LanChat(format!("入站流建立失败: {e}")))?;

        // 首帧必须是握手（带超时，§连接流程）。
        let first = tokio::time::timeout(HANDSHAKE_TIMEOUT, protocol::read_frame(&mut recv))
            .await
            .map_err(|_| AppError::LanChat("等待对端握手超时".into()))??;
        let Some(first) = first else {
            conn.close(0u32.into(), b"closed");
            return Ok(());
        };
        let remote = match self.validate_handshake(&first) {
            Ok(h) => h,
            Err(e) => {
                // 版本 / room 不匹配：立即关闭（§连接流程）。
                conn.close(0u32.into(), b"handshake rejected");
                return Err(e);
            }
        };
        if remote.peer_id == self.peer_id {
            conn.close(0u32.into(), b"self");
            return Ok(());
        }

        // 回握手 + Peer Exchange。
        let reply = self.handshake_envelope()?;
        if protocol::write_frame(&mut send, &reply).await.is_err() {
            conn.close(0u32.into(), b"closed");
            return Ok(());
        }
        let listen_addr = SocketAddr::new(remote_addr.ip(), remote.listen_port);
        if let Ok(px) = self.peer_exchange_envelope(Some(listen_addr)) {
            let _ = protocol::write_frame(&mut send, &px).await;
        }

        if !self.finish_registration(conn.clone(), send, recv, remote, remote_addr, false).await {
            conn.close(0u32.into(), b"duplicate");
        }
        Ok(())
    }

    /// 校验握手帧：kind=Handshake、version==1、room_id 一致。
    fn validate_handshake(&self, env: &Envelope) -> AppResult<HandshakePayload> {
        if env.kind != MessageKind::Handshake || env.version != PROTOCOL_VERSION {
            return Err(AppError::LanChat("对端协议版本不兼容".into()));
        }
        let hs: HandshakePayload = protocol::decode_payload(&env.payload)?;
        if hs.version != PROTOCOL_VERSION {
            return Err(AppError::LanChat("对端协议版本不兼容".into()));
        }
        if hs.room_id != self.room_id {
            return Err(AppError::LanChat("目标节点不属于该房间".into()));
        }
        Ok(hs)
    }

    /// 握手后注册连接：去重（tie-break）→ 建 writer → 登记 → 互认 → 读循环。
    /// 返回 false 表示因重复连接被拒绝（调用方负责关闭）。
    async fn finish_registration(
        self: &Arc<Self>,
        conn: quinn::Connection,
        send: quinn::SendStream,
        recv: quinn::RecvStream,
        remote: HandshakePayload,
        remote_addr: SocketAddr,
        outbound: bool,
    ) -> bool {
        let peer_id = remote.peer_id.clone();
        let tx = self.spawn_writer(send);
        let handle = PeerHandle {
            peer_id: peer_id.clone(),
            addr: remote_addr,
            listen_port: remote.listen_port,
            tx,
            outbound,
            conn: conn.clone(),
        };
        if !self.register_peer(handle) {
            return false;
        }
        log::info!("LAN chat peer connected: {} ({})", peer_id, remote_addr);

        // 告知新 peer 我们的昵称（等值于定向 Presence）。
        if let Ok(env) = self.presence_envelope(PresenceStatus::Join) {
            self.send_to_peer(&peer_id, &env);
        }
        // 出站方补发一次 Peer Exchange（入站方已在握手阶段发过）。
        if outbound {
            let dial = SocketAddr::new(remote_addr.ip(), remote.listen_port);
            if let Ok(px) = self.peer_exchange_envelope(Some(dial)) {
                self.send_to_peer(&peer_id, &px);
            }
        }
        self.emit_state();

        let this = Arc::clone(self);
        let dial_addr = SocketAddr::new(remote_addr.ip(), remote.listen_port);
        let read_task = tokio::spawn(async move {
            this.read_loop(conn, recv, peer_id, outbound, dial_addr).await;
        });
        self.tasks.lock().unwrap_or_else(|e| e.into_inner()).push(read_task);
        true
    }

    /// 同一 peer_id 重复连接去重（§连接管理）：
    /// 约定由 peer_id 字典序较小的一方主动拨出的连接优先保留，
    /// 双向同时互拨时收敛到一条连接。
    fn register_peer(&self, handle: PeerHandle) -> bool {
        let mut peers = self.peers.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(existing) = peers.get(&handle.peer_id) {
            let new_dialer_is_smaller = if handle.outbound {
                self.peer_id < handle.peer_id
            } else {
                handle.peer_id < self.peer_id
            };
            let existing_dialer_is_smaller = if existing.outbound {
                self.peer_id < existing.peer_id
            } else {
                existing.peer_id < self.peer_id
            };
            if !(new_dialer_is_smaller && !existing_dialer_is_smaller) {
                return false;
            }
            if let Some(old) = peers.remove(&handle.peer_id) {
                old.conn.close(0u32.into(), b"duplicate");
            }
        }
        if let Ok(mut known) = self.known_addrs.lock() {
            known.insert(handle.dial_addr());
        }
        peers.insert(handle.peer_id.clone(), handle);
        true
    }

    fn spawn_writer(&self, mut send: quinn::SendStream) -> mpsc::UnboundedSender<Envelope> {
        let (tx, mut rx) = mpsc::unbounded_channel::<Envelope>();
        let alive = Arc::clone(&self.alive);
        let handle = tokio::spawn(async move {
            while let Some(env) = rx.recv().await {
                if !alive.load(Ordering::SeqCst) {
                    break;
                }
                if protocol::write_frame(&mut send, &env).await.is_err() {
                    break;
                }
            }
            let _ = send.finish();
        });
        self.tasks.lock().unwrap_or_else(|e| e.into_inner()).push(handle);
        tx
    }

    // ------------------------------------------------------------------
    // 收包与 Gossip（§21-§25、§57）
    // ------------------------------------------------------------------

    async fn read_loop(
        self: &Arc<Self>,
        conn: quinn::Connection,
        mut recv: quinn::RecvStream,
        peer_id: String,
        outbound: bool,
        dial_addr: SocketAddr,
    ) {
        while self.alive.load(Ordering::SeqCst) {
            match protocol::read_frame(&mut recv).await {
                // 帧处理统一交给 dispatcher 任务（见 frame_tx 注释）。
                Ok(Some(env)) => {
                    if self.frame_tx.send((env, peer_id.clone())).is_err() {
                        break;
                    }
                }
                Ok(None) => break,
                Err(e) => {
                    log::debug!("LAN chat peer {} read error: {}", peer_id, e);
                    break;
                }
            }
        }
        // 清理：仅当登记的还是这条连接时才移除（避免重复连接去重误删新连接）。
        let removed = self
            .peers
            .lock()
            .map(|mut peers| match peers.get(&peer_id) {
                Some(h) if h.conn.stable_id() == conn.stable_id() => {
                    peers.remove(&peer_id);
                    true
                }
                _ => false,
            })
            .unwrap_or(false);
        if removed {
            log::info!("LAN chat peer disconnected: {}", peer_id);
            self.emit_state();
        }
        // 断线重连（§52）：只由原本的拨出方发起，指数退避 1s→30s 封顶。
        if removed && outbound && self.alive.load(Ordering::SeqCst) {
            self.schedule_reconnect(dial_addr, peer_id);
        }
    }

    fn schedule_reconnect(self: &Arc<Self>, addr: SocketAddr, peer_id: String) {
        let this = Arc::clone(self);
        let handle = tokio::spawn(async move {
            let mut delay = RECONNECT_MIN;
            while this.alive.load(Ordering::SeqCst) {
                tokio::time::sleep(delay).await;
                if !this.alive.load(Ordering::SeqCst) {
                    break;
                }
                // 已通过对端回拨 / Peer Exchange 恢复连接。
                let already = this.peers.lock().map(|p| p.contains_key(&peer_id)).unwrap_or(false);
                if already {
                    break;
                }
                log::info!("LAN chat reconnecting to peer {}", peer_id);
                if this.connect(addr).await.is_ok() {
                    break;
                }
                delay = (delay * 2).min(RECONNECT_MAX);
            }
        });
        self.tasks.lock().unwrap_or_else(|e| e.into_inner()).push(handle);
    }

    /// 帧分发任务：所有连接收到的帧在此串行处理（gossip 顺序稳定，实现简单）。
    fn spawn_dispatcher(self: &Arc<Self>, mut frame_rx: mpsc::UnboundedReceiver<(Envelope, String)>) {
        let this = Arc::clone(self);
        let handle = tokio::spawn(async move {
            while let Some((env, from_peer)) = frame_rx.recv().await {
                if !this.alive.load(Ordering::SeqCst) {
                    break;
                }
                this.handle_frame(env, &from_peer).await;
            }
        });
        self.tasks.lock().unwrap_or_else(|e| e.into_inner()).push(handle);
    }

    async fn handle_frame(self: &Arc<Self>, env: Envelope, from_peer: &str) {
        if !self.alive.load(Ordering::SeqCst) {
            return;
        }
        // 协议版本 / 房间不匹配直接丢弃（§24）。
        if env.version != PROTOCOL_VERSION || env.room_id != self.room_id {
            return;
        }
        match env.kind {
            MessageKind::Handshake => {} // 重复握手忽略
            MessageKind::PeerExchange => self.handle_peer_exchange(&env).await,
            MessageKind::Chat | MessageKind::Presence => self.handle_gossip_frame(env, from_peer).await,
        }
    }

    async fn handle_gossip_frame(self: &Arc<Self>, env: Envelope, from_peer: &str) {
        // 去重（§22）：已见 message_id 直接丢弃。
        let first_seen = self
            .seen
            .lock()
            .map(|mut seen| seen.check_and_mark(&env.message_id))
            .unwrap_or(false);
        if !first_seen {
            return;
        }
        // 解密失败 → 丢弃（§50：不暴露内部错误细节，不转发）。
        let plain = match cipher::decrypt(&self.key, &env.nonce, &env.payload) {
            Ok(p) => p,
            Err(_) => {
                log::debug!("LAN chat drop undecryptable message {}", env.message_id);
                return;
            }
        };
        let now = unix_now();
        match env.kind {
            MessageKind::Chat => {
                let Ok(payload) = protocol::decode_payload::<ChatPayload>(&plain) else { return };
                // 重放保护（§25）：时间戳窗口 ±10 分钟。
                if !protocol::timestamp_within_window(payload.timestamp, now) {
                    return;
                }
                if payload.msg_type == "text" {
                    self.sink.emit_message(&ChatMessage {
                        message_id: env.message_id.clone(),
                        sender_name: payload.sender_name,
                        content: payload.content,
                        timestamp: payload.timestamp,
                        mine: false,
                    });
                }
            }
            MessageKind::Presence => {
                let Ok(payload) = protocol::decode_payload::<PresencePayload>(&plain) else { return };
                if !protocol::timestamp_within_window(payload.timestamp, now) {
                    return;
                }
                let mut new_member = false;
                let _ = self.members.lock().map(|mut members| {
                    match payload.status {
                        PresenceStatus::Join | PresenceStatus::Heartbeat => {
                            let entry = members.entry(env.sender_id.clone()).or_insert_with(|| {
                                new_member = true;
                                MemberInfo {
                                    nickname: payload.sender_name.clone(),
                                    last_seen: Instant::now(),
                                }
                            });
                            entry.nickname = payload.sender_name.clone();
                            entry.last_seen = Instant::now();
                        }
                        PresenceStatus::Leave => {
                            // 不移除自己（防御性）。
                            if env.sender_id != self.peer_id {
                                members.remove(&env.sender_id);
                            }
                        }
                    }
                });
                self.emit_state();
                // 新成员加入时回广播一次自己的 Presence，加速成员列表收敛。
                if new_member && payload.status == PresenceStatus::Join {
                    if let Ok(mine) = self.presence_envelope(PresenceStatus::Join) {
                        self.broadcast_except(&mine, None);
                    }
                }
            }
            _ => {}
        }
        // Gossip 转发（§21/§23/§57）：hop_limit 递减，排除来源连接。
        if let Some(hop) = protocol::next_hop_limit(env.hop_limit) {
            let mut fwd = env;
            fwd.hop_limit = hop;
            self.broadcast_except(&fwd, Some(from_peer));
        }
    }

    async fn handle_peer_exchange(self: &Arc<Self>, env: &Envelope) {
        let Ok(payload) = protocol::decode_payload::<PeerExchangePayload>(&env.payload) else { return };
        let mut to_dial = Vec::new();
        for raw in payload.peers.iter().take(MAX_EXCHANGED_ADDRS) {
            let Ok(addr) = raw.parse::<SocketAddr>() else { continue };
            if let Ok(mut known) = self.known_addrs.lock() {
                known.insert(addr);
            }
            to_dial.push(addr);
        }
        // Partial Mesh（§56）：连接数未达上限时补拨新地址。
        for addr in to_dial {
            if self.should_dial(addr) {
                let this = Arc::clone(self);
                let handle = tokio::spawn(async move {
                    if let Err(e) = this.connect(addr).await {
                        log::debug!("LAN chat peer-exchange dial {} failed: {}", addr, e);
                    }
                });
                self.tasks.lock().unwrap_or_else(|e| e.into_inner()).push(handle);
            }
        }
    }

    /// 是否应该拨号：存活、连接数未达上限、未在拨号、未连接同地址。
    fn should_dial(&self, addr: SocketAddr) -> bool {
        if !self.alive.load(Ordering::SeqCst) {
            return false;
        }
        let peers_ok = self
            .peers
            .lock()
            .map(|peers| peers.len() < MAX_PEERS && !peers.values().any(|h| h.addr == addr || h.dial_addr() == addr))
            .unwrap_or(false);
        let connecting_ok = self.connecting.lock().map(|c| !c.contains(&addr)).unwrap_or(false);
        peers_ok && connecting_ok
    }

    // ------------------------------------------------------------------
    // 周期任务
    // ------------------------------------------------------------------

    /// Presence 心跳：每 30 秒广播一次（§：加入一次 + 周期 + 离开一次）。
    fn spawn_presence_loop(self: &Arc<Self>) {
        let this = Arc::clone(self);
        let handle = tokio::spawn(async move {
            let mut interval = tokio::time::interval(PRESENCE_INTERVAL);
            interval.tick().await; // 跳过立即触发的那一拍（加入时已广播）
            while this.alive.load(Ordering::SeqCst) {
                interval.tick().await;
                if !this.alive.load(Ordering::SeqCst) {
                    break;
                }
                if let Ok(env) = this.presence_envelope(PresenceStatus::Heartbeat) {
                    this.broadcast_except(&env, None);
                }
            }
        });
        self.tasks.lock().unwrap_or_else(|e| e.into_inner()).push(handle);
    }

    /// 维护任务：seen cache 清理、成员超时剔除、连接数补拨（§52/§56）。
    fn spawn_maintenance_loop(self: &Arc<Self>) {
        let this = Arc::clone(self);
        let handle = tokio::spawn(async move {
            let mut interval = tokio::time::interval(MAINTENANCE_INTERVAL);
            interval.tick().await;
            while this.alive.load(Ordering::SeqCst) {
                interval.tick().await;
                if !this.alive.load(Ordering::SeqCst) {
                    break;
                }
                if let Ok(mut seen) = this.seen.lock() {
                    seen.prune_expired();
                }
                // 剔除超时成员（不移除自己）。
                let mut changed = false;
                let _ = this.members.lock().map(|mut members| {
                    let self_id = this.peer_id.clone();
                    let before = members.len();
                    members.retain(|id, info| id == &self_id || info.last_seen.elapsed() < MEMBER_TIMEOUT);
                    changed = members.len() != before;
                });
                if changed {
                    this.emit_state();
                }
                // 连接数低于目标时，从已知地址补拨。
                let need = this
                    .peers
                    .lock()
                    .map(|p| p.len() < MIN_PEERS)
                    .unwrap_or(false);
                if need {
                    let candidates: Vec<SocketAddr> = this
                        .known_addrs
                        .lock()
                        .map(|k| k.iter().cloned().collect())
                        .unwrap_or_default();
                    for addr in candidates {
                        if this.should_dial(addr) {
                            let that = Arc::clone(&this);
                            let h = tokio::spawn(async move {
                                let _ = that.connect(addr).await;
                            });
                            this.tasks.lock().unwrap_or_else(|e| e.into_inner()).push(h);
                        }
                    }
                }
            }
        });
        self.tasks.lock().unwrap_or_else(|e| e.into_inner()).push(handle);
    }

    // ------------------------------------------------------------------
    // mDNS（§15-§17）
    // ------------------------------------------------------------------

    fn start_mdns(self: &Arc<Self>) {
        let advertise_name = if self.room_name.is_empty() {
            self.room_id.clone()
        } else {
            self.room_name.clone()
        };
        match discovery::advertise(&self.room_id, &advertise_name, &self.peer_id, self.port) {
            Ok(pair) => {
                if let Ok(mut g) = self.mdns.lock() {
                    *g = Some(pair);
                }
            }
            Err(e) => log::warn!("LAN chat mDNS advertise unavailable: {e}"),
        }
        // browse 同房间 peer 并自动连接；on_peer 在 mdns 线程上跑，
        // 通过捕获的 runtime Handle 派发回 tokio。
        let this = Arc::clone(self);
        let rt = tokio::runtime::Handle::current();
        if let Some(pair) = discovery::browse_room(
            self.peer_id.clone(),
            self.room_id.clone(),
            Arc::clone(&self.alive),
            move |ip, port| {
                let Ok(addr) = format!("{ip}:{port}").parse::<SocketAddr>() else { return };
                let this = Arc::clone(&this);
                rt.spawn(async move {
                    if this.should_dial(addr) {
                        if let Err(e) = this.connect(addr).await {
                            log::debug!("LAN chat mDNS dial {} failed: {}", addr, e);
                        }
                    }
                });
            },
        ) {
            if let Ok(mut g) = self.mdns_browse.lock() {
                *g = Some(pair);
            }
        } else {
            log::warn!("LAN chat mDNS browse unavailable");
        }
    }

    // ------------------------------------------------------------------
    // 生命周期（§26-§29、§54）
    // ------------------------------------------------------------------

    /// 离开 / 关闭房间：广播 leave（graceful 时）→ 停 mDNS → 关连接 →
    /// 清空消息缓存 / 成员 / peer 列表。Room Key 由 Zeroizing 在 drop 时清零。
    pub async fn leave(&self, graceful: bool) {
        if !self.alive.swap(false, Ordering::SeqCst) {
            return;
        }
        log::info!("LAN chat leaving room {}", self.room_id);
        if graceful {
            if let Ok(env) = self.presence_envelope(PresenceStatus::Leave) {
                self.broadcast_except(&env, None);
                // 尽力而为地等 UDP 报文发出去。
                tokio::time::sleep(Duration::from_millis(150)).await;
            }
        }
        self.endpoint.close(0u32.into(), b"leave");
        if let Ok(mut tasks) = self.tasks.lock() {
            for t in tasks.drain(..) {
                t.abort();
            }
        }
        if let Ok(mut g) = self.mdns.lock() {
            if let Some((daemon, fullname)) = g.take() {
                let _ = daemon.unregister(&fullname);
                let _ = daemon.shutdown();
            }
        }
        if let Ok(mut g) = self.mdns_browse.lock() {
            if let Some((daemon, thread)) = g.take() {
                let _ = daemon.shutdown();
                let _ = thread.join();
            }
        }
        // 清空内存状态（§28）。
        if let Ok(mut peers) = self.peers.lock() {
            peers.clear();
        }
        if let Ok(mut members) = self.members.lock() {
            members.clear();
        }
        if let Ok(mut seen) = self.seen.lock() {
            seen.clear();
        }
        if let Ok(mut known) = self.known_addrs.lock() {
            known.clear();
        }
        if let Ok(mut connecting) = self.connecting.lock() {
            connecting.clear();
        }
    }
}

impl Drop for ChatManager {
    /// 兜底清理（进程退出等未走 leave 的路径）：同步部分尽力而为。
    fn drop(&mut self) {
        self.alive.store(false, Ordering::SeqCst);
        self.endpoint.close(0u32.into(), b"drop");
        if let Ok(mut tasks) = self.tasks.lock() {
            for t in tasks.drain(..) {
                t.abort();
            }
        }
        if let Ok(mut g) = self.mdns.lock() {
            if let Some((daemon, fullname)) = g.take() {
                let _ = daemon.unregister(&fullname);
                let _ = daemon.shutdown();
            }
        }
        if let Ok(mut g) = self.mdns_browse.lock() {
            if let Some((daemon, _)) = g.take() {
                let _ = daemon.shutdown();
            }
        }
        // key 为 Zeroizing<[u8; 32]>，随 drop 自动清零（§28/§29）。
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discovery::mdns::NearbyRoom;

    /// 内存事件 sink：消息进 channel，房间状态计数（§61 集成测试用）。
    struct TestSink {
        messages: mpsc::UnboundedSender<ChatMessage>,
        states: Mutex<Vec<RoomState>>,
    }

    impl TestSink {
        fn new() -> (Arc<Self>, mpsc::UnboundedReceiver<ChatMessage>) {
            let (tx, rx) = mpsc::unbounded_channel();
            (
                Arc::new(Self {
                    messages: tx,
                    states: Mutex::new(Vec::new()),
                }),
                rx,
            )
        }

        fn state_emits(&self) -> usize {
            self.states.lock().unwrap().len()
        }
    }

    impl ChatEventSink for TestSink {
        fn emit_room_state(&self, state: &RoomState) {
            self.states.lock().unwrap().push(state.clone());
        }
        fn emit_message(&self, message: &ChatMessage) {
            let _ = self.messages.send(message.clone());
        }
        fn emit_rooms(&self, _rooms: &[NearbyRoom]) {}
        fn emit_error(&self, message: &str) {
            eprintln!("test sink error event: {message}");
        }
    }

    fn test_secret() -> Zeroizing<String> {
        Zeroizing::new("integration-test-secret-0123456789abcdef".to_string())
    }

    async fn wait_until(cond: impl Fn() -> bool, timeout_secs: u64) -> bool {
        let deadline = Instant::now() + Duration::from_secs(timeout_secs);
        while Instant::now() < deadline {
            if cond() {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        cond()
    }

    /// 本机 loopback 双节点：create + bootstrap join + 双向消息 + 去重 + 成员列表。
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn lan_chat_loopback_two_nodes_exchange_messages() {
        let (sink_a, mut rx_a) = TestSink::new();
        let a = ChatManager::create(sink_a.clone(), "集成测试房".into(), &test_secret(), "alice".into(), false)
            .await
            .expect("create room");
        let bootstrap: SocketAddr = format!("127.0.0.1:{}", a.port()).parse().unwrap();

        let (sink_b, mut rx_b) = TestSink::new();
        let b = ChatManager::join(
            sink_b.clone(),
            a.room_id().to_string(),
            &test_secret(),
            "bob".into(),
            Some(bootstrap),
            false,
        )
        .await
        .expect("join room");

        assert!(
            wait_until(|| a.connected_count() == 1 && b.connected_count() == 1, 10).await,
            "peers should connect over loopback"
        );

        a.send_message("hello from alice").await.unwrap();
        let msg = tokio::time::timeout(Duration::from_secs(10), rx_b.recv())
            .await
            .expect("b should receive within 10s")
            .expect("channel open");
        assert_eq!(msg.content, "hello from alice");
        assert_eq!(msg.sender_name, "alice");
        assert!(!msg.mine);

        // 去重（§22/§61 Replay）：同一 message_id 不能重复投递。
        let dup = tokio::time::timeout(Duration::from_millis(500), rx_b.recv()).await;
        assert!(dup.is_err(), "duplicate message delivery detected");

        b.send_message("hi from bob").await.unwrap();
        // 先排掉 rx_a 上 A 自己消息的 mine=true 回显。
        let echo_a = tokio::time::timeout(Duration::from_secs(2), rx_a.recv())
            .await
            .expect("a echo")
            .expect("channel open");
        assert!(echo_a.mine);
        assert_eq!(echo_a.content, "hello from alice");
        let msg = tokio::time::timeout(Duration::from_secs(10), rx_a.recv())
            .await
            .expect("a should receive within 10s")
            .expect("channel open");
        assert_eq!(msg.content, "hi from bob");
        assert_eq!(msg.sender_name, "bob");

        // 成员列表由可解密的 Presence 维护（§12.2）：双方都应看到 2 个成员。
        assert!(
            wait_until(|| a.member_count() == 2 && b.member_count() == 2, 10).await,
            "both sides should see 2 members"
        );
        assert!(sink_a.state_emits() > 0, "room state events should be emitted");

        b.leave(true).await;
        a.leave(true).await;
        assert_eq!(a.connected_count(), 0);
        assert_eq!(b.connected_count(), 0);
    }

    /// Gossip 中继（§21-§23、§62）：A—B—C 链式组网，A 发的消息经 B 转发到 C，
    /// 且只投递一次。
    #[tokio::test(flavor = "multi_thread", worker_threads = 6)]
    async fn lan_chat_gossip_relay_via_middle_node() {
        let (sink_a, _rx_a) = TestSink::new();
        let a = ChatManager::create(sink_a, "中继测试".into(), &test_secret(), "alice".into(), false)
            .await
            .expect("create");

        let (sink_b, _rx_b) = TestSink::new();
        let addr_a: SocketAddr = format!("127.0.0.1:{}", a.port()).parse().unwrap();
        let b = ChatManager::join(sink_b, a.room_id().to_string(), &test_secret(), "bob".into(), Some(addr_a), false)
            .await
            .expect("b joins");

        let (sink_c, mut rx_c) = TestSink::new();
        let addr_b: SocketAddr = format!("127.0.0.1:{}", b.port()).parse().unwrap();
        let c = ChatManager::join(sink_c, a.room_id().to_string(), &test_secret(), "carol".into(), Some(addr_b), false)
            .await
            .expect("c joins");

        assert!(
            wait_until(|| b.connected_count() >= 1 && c.connected_count() >= 1, 10).await,
            "chain topology should establish"
        );

        a.send_message("relay me").await.unwrap();
        let msg = tokio::time::timeout(Duration::from_secs(10), rx_c.recv())
            .await
            .expect("c should receive relayed message within 10s")
            .expect("channel open");
        assert_eq!(msg.content, "relay me");
        assert_eq!(msg.sender_name, "alice");

        // 即使 A 与 C 经 Peer Exchange 直连，去重也保证只投递一次（§22）。
        let dup = tokio::time::timeout(Duration::from_millis(800), rx_c.recv()).await;
        assert!(dup.is_err(), "gossip duplicate delivery detected");

        // 成员收敛：每个节点最终应看到 3 个成员。
        assert!(
            wait_until(|| a.member_count() == 3 && b.member_count() == 3 && c.member_count() == 3, 10).await,
            "membership should converge to 3"
        );

        c.leave(true).await;
        b.leave(true).await;
        a.leave(true).await;
    }

    /// 错误 secret 的 peer：握手能过（room_id 相同），但消息全部解密失败被丢弃（§50）。
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn lan_chat_wrong_secret_messages_are_dropped() {
        let (sink_a, mut rx_a) = TestSink::new();
        let a = ChatManager::create(sink_a, "密钥测试".into(), &test_secret(), "alice".into(), false)
            .await
            .expect("create");

        let (sink_b, mut rx_b) = TestSink::new();
        let bootstrap: SocketAddr = format!("127.0.0.1:{}", a.port()).parse().unwrap();
        let wrong = Zeroizing::new("a-different-secret-entirely".to_string());
        let b = ChatManager::join(sink_b, a.room_id().to_string(), &wrong, "mallory".into(), Some(bootstrap), false)
            .await
            .expect("join (transport ok, crypto isolated)");

        assert!(wait_until(|| a.connected_count() == 1 && b.connected_count() == 1, 10).await);

        // 双向消息都应因解密失败被丢弃（§50），不出现在任何一侧。
        // 注意先排掉各自 sink 上 mine=true 的本地回显。
        a.send_message("should be unreadable for b").await.unwrap();
        b.send_message("should be unreadable for a").await.unwrap();
        let echo_a = tokio::time::timeout(Duration::from_secs(2), rx_a.recv()).await.unwrap().unwrap();
        let echo_b = tokio::time::timeout(Duration::from_secs(2), rx_b.recv()).await.unwrap().unwrap();
        assert!(echo_a.mine && echo_b.mine);
        assert!(tokio::time::timeout(Duration::from_secs(2), rx_b.recv()).await.is_err());
        assert!(tokio::time::timeout(Duration::from_secs(2), rx_a.recv()).await.is_err());
        // 成员列表也不应包含对方（Presence 同样无法解密）。
        assert_eq!(a.member_count(), 1);
        assert_eq!(b.member_count(), 1);

        b.leave(false).await;
        a.leave(false).await;
    }
}
