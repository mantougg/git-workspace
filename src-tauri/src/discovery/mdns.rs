//! mDNS 发现（设计文档 §15-§17）。
//!
//! 广播服务类型 `_devtoolbox-chat._udp.local`，TXT 携带 `v=1` / `room=<room_id>`
//! / `name=<房间名>` / `peer=<peer_id>`——**绝不广播 Shared Secret**（§15）。
//!
//! 两类用途：
//! - [`advertise`]：进房后广播本房间，让同子网 peer 自动发现；
//! - [`RoomBrowser`]：未进房时持续 browse，向前端推送附近房间列表；
//! - [`browse_room`]：进房后 browse 同房间的其他 peer 并回调地址用于自动连接。

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use mdns_sd::{ResolvedService, ServiceDaemon, ServiceEvent, ServiceInfo};
use serde::Serialize;

use crate::error::{AppError, AppResult};

/// mDNS 服务类型（§15）。
pub const SERVICE_TYPE: &str = "_devtoolbox-chat._udp.local.";

/// 附近房间（事件 `lan_chat_rooms` 元素）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NearbyRoom {
    pub room_id: String,
    pub room_name: String,
    /// 优先 IPv4 地址。
    pub addr: String,
    pub port: u16,
}

/// 进房后广播本房间。返回 (daemon, fullname)；调用方负责在离房时
/// `unregister` + `shutdown`。
///
/// mDNS 不可用的环境（容器 / 禁播网络）返回 Err，调用方降级为仅
/// Manual Bootstrap，不影响房间功能。
pub fn advertise(
    room_id: &str,
    room_name: &str,
    peer_id: &str,
    port: u16,
) -> AppResult<(ServiceDaemon, String)> {
    let daemon = ServiceDaemon::new().map_err(|e| AppError::LanChat(format!("启动网络发现失败: {e}")))?;
    // TXT 只含协议版本与房间元数据，secret 绝不进广播（§15）。
    let properties: HashMap<String, String> = [
        ("v".to_string(), "1".to_string()),
        ("room".to_string(), room_id.to_string()),
        ("name".to_string(), room_name.to_string()),
        ("peer".to_string(), peer_id.to_string()),
    ]
    .into_iter()
    .collect();
    let host = format!("{peer_id}.local.");
    let info = ServiceInfo::new(SERVICE_TYPE, peer_id, &host, "", port, properties)
        .map_err(|e| AppError::LanChat(format!("注册网络发现服务失败: {e}")))?
        .enable_addr_auto();
    let fullname = info.get_fullname().to_string();
    daemon
        .register(info)
        .map_err(|e| AppError::LanChat(format!("广播房间失败: {e}")))?;
    Ok((daemon, fullname))
}

/// 从 ResolvedService 提取房间信息；非本协议服务返回 None。
fn parse_resolved(info: &ResolvedService) -> Option<(String, String, String, String, u16)> {
    let props = info.get_properties();
    if props.get_property_val_str("v") != Some("1") {
        return None;
    }
    let room = props.get_property_val_str("room")?.to_string();
    let peer = props.get_property_val_str("peer").unwrap_or("").to_string();
    let name = props.get_property_val_str("name").unwrap_or("").to_string();
    let addr = info
        .get_addresses()
        .iter()
        .find(|ip| matches!(ip, mdns_sd::ScopedIp::V4(_)))
        .or_else(|| info.get_addresses().iter().next())?
        .to_ip_addr();
    Some((room, name, peer, addr.to_string(), info.get_port()))
}

/// 未进房时的附近房间浏览器：持续 browse，变化时回调全量列表。
pub struct RoomBrowser {
    daemon: ServiceDaemon,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl RoomBrowser {
    /// `on_change` 在房间列表变化时以全量快照回调（`Vec<NearbyRoom>`）。
    /// `self_peer` 用于过滤本节点自己的广播。
    pub fn start(
        self_peer: Arc<Mutex<Option<String>>>,
        on_change: impl Fn(Vec<NearbyRoom>) + Send + Sync + 'static,
    ) -> AppResult<Self> {
        let daemon = ServiceDaemon::new().map_err(|e| AppError::LanChat(format!("启动网络发现失败: {e}")))?;
        let receiver = daemon
            .browse(SERVICE_TYPE)
            .map_err(|e| AppError::LanChat(format!("浏览附近房间失败: {e}")))?;
        let stop = Arc::new(AtomicBool::new(false));
        let stop_flag = Arc::clone(&stop);
        let thread = std::thread::Builder::new()
            .name("lan-chat-discovery".into())
            .spawn(move || {
                let mut rooms: HashMap<String, NearbyRoom> = HashMap::new();
                while !stop_flag.load(Ordering::Relaxed) {
                    let event = match receiver.recv_timeout(Duration::from_millis(500)) {
                        Ok(e) => e,
                        Err(_) => continue, // 超时 / 断开：靠 stop 标志退出
                    };
                    let changed = match event {
                        ServiceEvent::ServiceResolved(info) => {
                            match parse_resolved(&info) {
                                Some((room, name, peer, addr, port)) => {
                                    let is_self = self_peer
                                        .lock()
                                        .map(|g| g.as_deref() == Some(peer.as_str()) && !peer.is_empty())
                                        .unwrap_or(false);
                                    if is_self {
                                        false
                                    } else {
                                        let new_room = NearbyRoom {
                                            room_id: room,
                                            room_name: name,
                                            addr,
                                            port,
                                        };
                                        rooms
                                            .insert(info.fullname.clone(), new_room.clone())
                                            .map(|old| old != new_room)
                                            .unwrap_or(true)
                                    }
                                }
                                None => false,
                            }
                        }
                        ServiceEvent::ServiceRemoved(_, fullname) => rooms.remove(&fullname).is_some(),
                        _ => false,
                    };
                    if changed {
                        let mut list: Vec<NearbyRoom> = rooms.values().cloned().collect();
                        list.sort_by(|a, b| a.room_id.cmp(&b.room_id));
                        on_change(list);
                    }
                }
            })
            .map_err(|e| AppError::LanChat(format!("启动网络发现线程失败: {e}")))?;
        Ok(Self {
            daemon,
            stop,
            thread: Some(thread),
        })
    }
}

impl Drop for RoomBrowser {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = self.daemon.stop_browse(SERVICE_TYPE);
        let _ = self.daemon.shutdown();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// 进房后的房间 peer 浏览器：发现同房间的其他 peer 时回调 (ip, port)。
/// 用于同子网自动组网（§15/§17）。
pub fn browse_room(
    self_peer_id: String,
    room_id: String,
    stop: Arc<AtomicBool>,
    on_peer: impl Fn(String, u16) + Send + Sync + 'static,
) -> Option<(ServiceDaemon, std::thread::JoinHandle<()>)> {
    let daemon = ServiceDaemon::new().ok()?;
    let receiver = daemon.browse(SERVICE_TYPE).ok()?;
    let thread = std::thread::Builder::new()
        .name("lan-chat-room-browse".into())
        .spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                let event = match receiver.recv_timeout(Duration::from_millis(500)) {
                    Ok(e) => e,
                    Err(_) => continue,
                };
                if let ServiceEvent::ServiceResolved(info) = event {
                    if let Some((room, _, peer, addr, port)) = parse_resolved(&info) {
                        if room == room_id && !peer.is_empty() && peer != self_peer_id {
                            on_peer(addr, port);
                        }
                    }
                }
            }
        })
        .ok()?;
    Some((daemon, thread))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    /// mDNS 广播 + 浏览回环。CI / 容器无组播时 skip 并打印原因（项目测试惯例）。
    #[test]
    fn mdns_advertise_and_browse_roundtrip_or_skip() {
        let (daemon, fullname) = match advertise("room-test-1", "测试房", "peer-test01", 45678) {
            Ok(pair) => pair,
            Err(e) => {
                eprintln!("skip mdns test: advertise unavailable in this environment: {e}");
                return;
            }
        };
        let browser_daemon = match ServiceDaemon::new() {
            Ok(d) => d,
            Err(e) => {
                eprintln!("skip mdns test: browse daemon unavailable: {e}");
                let _ = daemon.shutdown();
                return;
            }
        };
        let receiver = match browser_daemon.browse(SERVICE_TYPE) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("skip mdns test: browse unavailable: {e}");
                let _ = daemon.shutdown();
                let _ = browser_daemon.shutdown();
                return;
            }
        };

        let deadline = Instant::now() + Duration::from_secs(5);
        let mut found = false;
        while Instant::now() < deadline {
            match receiver.recv_timeout(Duration::from_millis(500)) {
                Ok(ServiceEvent::ServiceResolved(info)) if info.fullname == fullname => {
                    assert_eq!(info.get_port(), 45678);
                    assert_eq!(info.get_property_val_str("room"), Some("room-test-1"));
                    assert_eq!(info.get_property_val_str("name"), Some("测试房"));
                    assert_eq!(info.get_property_val_str("v"), Some("1"));
                    // TXT 绝不携带 secret（§15）。
                    assert!(info.get_property_val_str("secret").is_none());
                    found = true;
                    break;
                }
                _ => {}
            }
        }
        let _ = daemon.unregister(&fullname);
        let _ = daemon.shutdown();
        let _ = browser_daemon.shutdown();
        if !found {
            eprintln!("skip mdns test: loopback multicast resolution unavailable (CI without multicast)");
        }
    }
}
