//! Health Check（R-16，§41）：运行中应用的探针式健康检测。
//!
//! 职责：
//! - 检查配置 [`HealthCheckConfig`]（随 Runtime 配置持久化，`.gitworkspace/
//!   runtimes/<name>.json`）：Port / HTTP / TCP / Actuator 四种检查方式，
//!   `Auto` 先试 Actuator、连不上回退 TCP 端口探测；
//! - 健康状态机 `Starting / Healthy / Unhealthy / Stopped`（迁移经
//!   `runtime_health_changed` 事件广播；复用 R-12 的 `HealthStatus`，新增
//!   探针取值，up/down 生命周期推导保持不变）；
//! - [`HealthEngine`] 每个探针一个监控线程：低频轮询 + Unhealthy 指数退避，
//!   进程行落终态即自停（`stop_monitor` 由 Process Manager 在退出收尾时
//!   调用，双保险防轮询泄漏）。
//!
//! 边界（任务文档「架构/性能注意点」）：
//! - HTTP 检查超时短且有上限（连接 2s / 读 3s，可配置），检查在独立线程，
//!   不阻塞调度；
//! - 探针错误详情不携带秘密（纯连接/HTTP 状态信息），快照跨 IPC 前仍过
//!   一遍脱敏防御；
//! - 就绪门限（R-15 依赖服务等待 Healthy）经 [`HealthEngine::snapshot`]
//!   查询，不直接暴露线程句柄。

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::error::AppResult;
use crate::runtime::events::{
    HealthChangedPayload, HealthStatus, RuntimeEmission, RuntimeEventEmitter, EVENT_HEALTH_CHANGED,
};
use crate::runtime::launch::store;

// ---------------------------------------------------------------------------
// 检查配置（随 Runtime 配置持久化，serde 向后兼容：全部字段可缺省）
// ---------------------------------------------------------------------------

/// 检查方式（§41）。`auto` = Actuator 优先、连接失败回退 TCP。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum HealthCheckKind {
    #[default]
    Auto,
    Port,
    Http,
    Tcp,
    Actuator,
}

impl HealthCheckKind {
    pub fn as_str(self) -> &'static str {
        match self {
            HealthCheckKind::Auto => "auto",
            HealthCheckKind::Port => "port",
            HealthCheckKind::Http => "http",
            HealthCheckKind::Tcp => "tcp",
            HealthCheckKind::Actuator => "actuator",
        }
    }
}

/// 每应用的健康检查配置。`None` 的字段回落默认值（[`HealthCheckConfig`]
/// 常量），保证 schema 向后兼容（全局约束 §8）。
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthCheckConfig {
    #[serde(default)]
    pub kind: HealthCheckKind,
    /// 检查目标主机；缺省 `127.0.0.1`。
    #[serde(default)]
    pub host: Option<String>,
    /// 检查端口；缺省用启动日志探测到的端口。
    #[serde(default)]
    pub port: Option<u16>,
    /// HTTP / Actuator 路径；缺省 `/actuator/health`。
    #[serde(default)]
    pub path: Option<String>,
    /// 轮询间隔毫秒；缺省 5000，下限 500。
    #[serde(default)]
    pub interval_ms: Option<u64>,
    /// 单次检查超时毫秒；缺省 2000，上限 10000。
    #[serde(default)]
    pub timeout_ms: Option<u64>,
    /// 连续成功多少次翻转 Healthy；缺省 1。
    #[serde(default)]
    pub healthy_after: Option<u32>,
    /// 连续失败多少次翻转 Unhealthy；缺省 3。
    #[serde(default)]
    pub unhealthy_after: Option<u32>,
}

pub const DEFAULT_INTERVAL_MS: u64 = 5000;
pub const DEFAULT_TIMEOUT_MS: u64 = 2000;
const MIN_INTERVAL_MS: u64 = 500;
const MAX_TIMEOUT_MS: u64 = 10_000;
pub const DEFAULT_HEALTHY_AFTER: u32 = 1;
pub const DEFAULT_UNHEALTHY_AFTER: u32 = 3;
pub const DEFAULT_ACTUATOR_PATH: &str = "/actuator/health";

impl HealthCheckConfig {
    pub fn effective_host(&self) -> String {
        self.host.clone().unwrap_or_else(|| "127.0.0.1".into())
    }

    pub fn effective_path(&self) -> String {
        self.path.clone().unwrap_or_else(|| DEFAULT_ACTUATOR_PATH.into())
    }

    pub fn effective_interval(&self) -> Duration {
        Duration::from_millis(self.interval_ms.unwrap_or(DEFAULT_INTERVAL_MS).max(MIN_INTERVAL_MS))
    }

    pub fn effective_timeout(&self) -> Duration {
        Duration::from_millis(self.timeout_ms.unwrap_or(DEFAULT_TIMEOUT_MS).min(MAX_TIMEOUT_MS))
    }

    pub fn effective_healthy_after(&self) -> u32 {
        self.healthy_after.unwrap_or(DEFAULT_HEALTHY_AFTER).max(1)
    }

    pub fn effective_unhealthy_after(&self) -> u32 {
        self.unhealthy_after.unwrap_or(DEFAULT_UNHEALTHY_AFTER).max(1)
    }

    pub fn validate(&self) -> AppResult<()> {
        let interval = self.interval_ms.unwrap_or(DEFAULT_INTERVAL_MS);
        if interval < MIN_INTERVAL_MS {
            return Err(crate::error::AppError::RuntimeConfig(format!(
                "健康检查间隔 {interval}ms 过小（下限 {MIN_INTERVAL_MS}ms），避免刷爆应用"
            )));
        }
        let timeout = self.timeout_ms.unwrap_or(DEFAULT_TIMEOUT_MS);
        if timeout > MAX_TIMEOUT_MS {
            return Err(crate::error::AppError::RuntimeConfig(format!(
                "健康检查超时 {timeout}ms 超过上限 {MAX_TIMEOUT_MS}ms"
            )));
        }
        if let Some(path) = &self.path {
            if !path.starts_with('/') {
                return Err(crate::error::AppError::RuntimeConfig(format!(
                    "健康检查路径 '{path}' 必须以 / 开头"
                )));
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 探针（纯函数 + 短超时 IO，便于单测）
// ---------------------------------------------------------------------------

/// 一次探测的结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeOutcome {
    pub healthy: bool,
    pub detail: String,
}

/// TCP 端口探测：连接成功 = 端口已监听。
pub fn tcp_probe(host: &str, port: u16, timeout: Duration) -> ProbeOutcome {
    let target = format!("{host}:{port}");
    match TcpStream::connect_timeout(
        &target
            .to_socket_addrs_lossy()
            .next()
            .unwrap_or(std::net::SocketAddr::from(([127, 0, 0, 1], port))),
        timeout,
    ) {
        Ok(_) => ProbeOutcome {
            healthy: true,
            detail: format!("TCP {target} 可连接"),
        },
        Err(e) => ProbeOutcome {
            healthy: false,
            detail: format!("TCP {target} 连接失败：{e}"),
        },
    }
}

/// `ToSocketAddrs` 的无 panic 包装（主机名解析失败返回空迭代器）。
trait ToSocketAddrsLossy {
    fn to_socket_addrs_lossy(&self) -> std::vec::IntoIter<std::net::SocketAddr>;
}

impl ToSocketAddrsLossy for String {
    fn to_socket_addrs_lossy(&self) -> std::vec::IntoIter<std::net::SocketAddr> {
        use std::net::ToSocketAddrs;
        let mut addrs = self
            .as_str()
            .to_socket_addrs()
            .map(Iterator::collect::<Vec<_>>)
            .unwrap_or_default();
        // IPv6 localhost 等场景下保留端口兜底。
        if addrs.is_empty() {
            if let Some((host, port)) = self.rsplit_once(':') {
                if let Ok(port) = port.parse::<u16>() {
                    let ip = if host.is_empty() || host == "localhost" {
                        std::net::IpAddr::from([127, 0, 0, 1])
                    } else if let Ok(ip) = host.parse::<std::net::IpAddr>() {
                        ip
                    } else {
                        std::net::IpAddr::from([127, 0, 0, 1])
                    };
                    addrs.push(std::net::SocketAddr::new(ip, port));
                }
            }
        }
        addrs.into_iter()
    }
}

/// 极简 HTTP/1.1 GET（本地 Actuator 场景；HTTPS 不在此路径）。
/// 返回 `(状态码, 响应体)`；解析失败返回错误文本。
pub fn http_get(host: &str, port: u16, path: &str, timeout: Duration) -> Result<(u16, String), String> {
    let target = format!("{host}:{port}");
    let addr = target
        .to_socket_addrs_lossy()
        .next()
        .ok_or_else(|| format!("主机解析失败：{target}"))?;
    let mut stream = TcpStream::connect_timeout(&addr, timeout).map_err(|e| format!("连接 {target} 失败：{e}"))?;
    stream
        .set_read_timeout(Some(timeout))
        .map_err(|e| format!("设置读超时失败：{e}"))?;
    stream
        .set_write_timeout(Some(timeout))
        .map_err(|e| format!("设置写超时失败：{e}"))?;
    let request = format!(
        "GET {path} HTTP/1.1\r\nHost: {host}:{port}\r\nConnection: close\r\nUser-Agent: \
         GitWorkspace-Health/1.0\r\nAccept: application/json\r\n\r\n"
    );
    stream
        .write_all(request.as_bytes())
        .map_err(|e| format!("发送请求失败：{e}"))?;
    let mut bytes = Vec::new();
    let mut buf = [0u8; 4096];
    // 读到 EOF 或上限（256KB）为止；read_timeout 到点会 Err，用已读内容兜底。
    loop {
        match stream.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                bytes.extend_from_slice(&buf[..n]);
                if bytes.len() > 256 * 1024 {
                    break;
                }
            }
            Err(e) => {
                if bytes.is_empty() {
                    return Err(format!("读取响应失败：{e}"));
                }
                break;
            }
        }
    }
    parse_http_response(&String::from_utf8_lossy(&bytes))
        .ok_or_else(|| "HTTP 响应解析失败（非 HTTP/1.x 响应）".to_string())
}

/// 解析 HTTP 响应：`(状态码, Body)`。纯函数（单测覆盖）。
pub fn parse_http_response(raw: &str) -> Option<(u16, String)> {
    let (head, body) = raw.split_once("\r\n\r\n").unwrap_or((raw, ""));
    let status_line = head.lines().next()?;
    let mut parts = status_line.split_whitespace();
    let _version = parts.next()?;
    let code = parts.next()?.parse::<u16>().ok()?;
    Some((code, body.to_string()))
}

/// 从 Actuator `/actuator/health` 响应体提取 `"status"` 字段（`"UP"` /
/// `"DOWN"`）。容忍 JSON 字段顺序与空白。纯函数（单测覆盖）。
pub fn actuator_status(body: &str) -> Option<String> {
    let needle = "\"status\"";
    let start = body.find(needle)?;
    let rest = &body[start + needle.len()..];
    let colon = rest.find(':')?;
    let rest = rest[colon + 1..].trim_start();
    let quote = rest.find('"')?;
    let rest = &rest[quote + 1..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

/// 按配置执行一次检查。`port` 为 None 表示还没有可用端口（应用未监听）。
pub fn evaluate_check(config: &HealthCheckConfig, port: Option<u16>) -> ProbeOutcome {
    let host = config.effective_host();
    let timeout = config.effective_timeout();
    let Some(port) = port else {
        return ProbeOutcome {
            healthy: false,
            detail: "尚未探测到应用监听端口，等待启动日志".into(),
        };
    };
    match config.kind {
        HealthCheckKind::Port => tcp_probe(&host, port, timeout),
        HealthCheckKind::Tcp => tcp_probe(&host, port, timeout),
        HealthCheckKind::Http => match http_get(&host, port, &config.effective_path(), timeout) {
            Ok((code, _)) if (200..400).contains(&code) => ProbeOutcome {
                healthy: true,
                detail: format!("HTTP {code}"),
            },
            Ok((code, _)) => ProbeOutcome {
                healthy: false,
                detail: format!("HTTP {code}（期望 2xx/3xx）"),
            },
            Err(e) => ProbeOutcome {
                healthy: false,
                detail: e,
            },
        },
        HealthCheckKind::Actuator | HealthCheckKind::Auto => {
            let path = config.effective_path();
            match http_get(&host, port, &path, timeout) {
                Ok((code, body)) if (200..300).contains(&code) => match actuator_status(&body).as_deref() {
                    Some("UP") => ProbeOutcome {
                        healthy: true,
                        detail: format!("Actuator {path} UP"),
                    },
                    Some(other) => ProbeOutcome {
                        healthy: false,
                        detail: format!("Actuator {path} 状态 {other}"),
                    },
                    None => ProbeOutcome {
                        healthy: true,
                        detail: format!("Actuator {path} HTTP {code}（响应无 status 字段，按可达处理）"),
                    },
                },
                Ok((code, _)) if config.kind == HealthCheckKind::Auto => {
                    // Auto：Actuator 不可达（404 等）回退 TCP 端口探测。
                    let mut fallback = tcp_probe(&host, port, timeout);
                    fallback
                        .detail
                        .push_str(&format!("（Actuator {path} HTTP {code}，回退 TCP）"));
                    fallback
                }
                Ok((code, _)) => ProbeOutcome {
                    healthy: false,
                    detail: format!("Actuator {path} HTTP {code}（期望 2xx）"),
                },
                Err(e) if config.kind == HealthCheckKind::Auto => {
                    let mut fallback = tcp_probe(&host, port, timeout);
                    fallback.detail.push_str(&format!("（Actuator 不可达：{e}，回退 TCP）"));
                    fallback
                }
                Err(e) => ProbeOutcome {
                    healthy: false,
                    detail: e,
                },
            }
        }
    }
}

// ---------------------------------------------------------------------------
// HealthEngine：探针状态机与事件
// ---------------------------------------------------------------------------

/// 单进程健康快照（IPC 可序列化，§41 UI 展示）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthSnapshot {
    pub process_id: i64,
    pub workspace_id: i64,
    pub runtime_name: String,
    pub phase: HealthStatus,
    /// 最近一次探测时间（RFC3339）；尚未探测为 None。
    pub last_checked_at: Option<String>,
    /// 最近一次探测详情（连接/HTTP 状态信息，无秘密）。
    pub last_detail: Option<String>,
}

struct MonitorHandle {
    stop: Arc<AtomicBool>,
}

/// 健康检查引擎。每个受监控进程一个独立线程（低频轮询）。
pub struct HealthEngine {
    db: Arc<Mutex<Connection>>,
    emitter: Arc<dyn RuntimeEventEmitter>,
    monitors: Mutex<HashMap<i64, MonitorHandle>>,
    states: Mutex<HashMap<i64, HealthSnapshot>>,
}

const STOP_POLL_SLICE: Duration = Duration::from_millis(100);
/// Unhealthy 退避：间隔 × 4，上限 60s（「Unhealthy 不刷请求」）。
const UNHEALTHY_BACKOFF: u32 = 4;
const BACKOFF_CAP_MS: u64 = 60_000;

impl HealthEngine {
    pub fn new(db: Arc<Mutex<Connection>>, emitter: Arc<dyn RuntimeEventEmitter>) -> Arc<Self> {
        Arc::new(Self {
            db,
            emitter,
            monitors: Mutex::new(HashMap::new()),
            states: Mutex::new(HashMap::new()),
        })
    }

    fn emit(&self, payload: &HealthChangedPayload) {
        self.emitter.emit(RuntimeEmission::new(EVENT_HEALTH_CHANGED, payload));
    }

    /// 为进程启动探针线程。配置（`health_check`）缺失时不监控——R-12 的
    /// 生命周期推导（Running→up / 停止→down）保持原语义。
    /// 重复调用（同一 process_id 已在监控）为 no-op。
    ///
    /// 注意：句柄注册发生在**线程内**（配置确认存在后）——无配置的线程
    /// 立即退出且不留表项，[`HealthEngine::has_monitor`] 因此能区分
    /// 「未配置探针」与「探针尚未注册」。
    pub fn start_monitor(self: &Arc<Self>, process_id: i64, workspace_id: i64, runtime_name: &str) {
        {
            let monitors = self.monitors.lock().unwrap();
            if monitors.contains_key(&process_id) {
                return;
            }
        }
        let this = Arc::clone(self);
        let runtime_name = runtime_name.to_string();
        let stop = Arc::new(AtomicBool::new(false));
        let stop_flag = Arc::clone(&stop);
        std::thread::Builder::new()
            .name(format!("health-{process_id}"))
            .spawn(move || {
                // 配置确认存在后才注册句柄；随后在循环里仍可能被 stop_monitor
                // 摘牌（双保险：stop 标志 + 行终态自愈）。
                if !this.config_exists(workspace_id, &runtime_name) {
                    return;
                }
                let registered = {
                    let mut monitors = this.monitors.lock().unwrap();
                    if monitors.contains_key(&process_id) {
                        false
                    } else {
                        monitors.insert(
                            process_id,
                            MonitorHandle {
                                stop: Arc::clone(&stop_flag),
                            },
                        );
                        true
                    }
                };
                if registered {
                    this.monitor_loop(process_id, workspace_id, &runtime_name, &stop_flag);
                }
            })
            .inspect_err(|e| log::warn!("R-16: failed to spawn health monitor #{process_id}: {e}"))
            .ok();
    }

    /// 该 Runtime 配置是否存在 health_check（供线程内的注册前置检查）。
    fn config_exists(&self, workspace_id: i64, runtime_name: &str) -> bool {
        let conn = match self.db.lock() {
            Ok(conn) => conn,
            Err(_) => return false,
        };
        crate::runtime::config::load_config_unredacted(&conn, workspace_id, runtime_name)
            .ok()
            .and_then(|config| config.health_check)
            .is_some()
    }

    /// 停止探针线程。进程行已终态（或正在停止）时补发 `Stopped`（此前
    /// 处于探针状态机时），保证 UI 不停留在 Healthy。
    pub fn stop_monitor(&self, process_id: i64) {
        let handle = self.monitors.lock().unwrap().remove(&process_id);
        if let Some(handle) = handle {
            handle.stop.store(true, Ordering::Relaxed);
        }
        self.finalize_stopped(process_id);
    }

    /// 进程已终态：快照翻转为 Stopped 并广播（此前无探针状态时不发）。
    fn finalize_stopped(&self, process_id: i64) {
        let mut states = self.states.lock().unwrap();
        if let Some(snapshot) = states.get_mut(&process_id) {
            if snapshot.phase != HealthStatus::Stopped {
                snapshot.phase = HealthStatus::Stopped;
                snapshot.last_checked_at = Some(chrono::Utc::now().to_rfc3339());
                self.emit(&HealthChangedPayload {
                    workspace_id: snapshot.workspace_id,
                    process_id: snapshot.process_id,
                    runtime_name: snapshot.runtime_name.clone(),
                    health: HealthStatus::Stopped,
                    at: snapshot.last_checked_at.clone().unwrap_or_default(),
                });
            }
        }
    }

    /// 当前快照（无探针 / 未启动监控为 None）。
    pub fn snapshot(&self, process_id: i64) -> Option<HealthSnapshot> {
        self.states.lock().unwrap().get(&process_id).cloned()
    }

    /// 进程是否有探针在跑（R-15 就绪门限用：无探针立即就绪）。
    pub fn has_monitor(&self, process_id: i64) -> bool {
        self.monitors.lock().unwrap().contains_key(&process_id)
    }

    /// workspace 下全部探针快照（Dashboard 汇总）。
    pub fn snapshots(&self, workspace_id: i64) -> Vec<HealthSnapshot> {
        self.states
            .lock()
            .unwrap()
            .values()
            .filter(|s| s.workspace_id == workspace_id)
            .cloned()
            .collect()
    }

    fn monitor_loop(self: &Arc<Self>, process_id: i64, workspace_id: i64, runtime_name: &str, stop: &AtomicBool) {
        // 配置加载：无 health_check 配置 → 不监控，直接退出（R-12 语义保持）。
        let config = {
            let conn = match self.db.lock() {
                Ok(conn) => conn,
                Err(_) => return,
            };
            match crate::runtime::config::load_config_unredacted(&conn, workspace_id, runtime_name) {
                Ok(config) => config.health_check,
                Err(e) => {
                    log::debug!("R-16: health monitor #{process_id} exiting, config unavailable: {e}");
                    return;
                }
            }
        };
        let Some(config) = config else {
            return;
        };
        let interval = config.effective_interval();
        let healthy_after = config.effective_healthy_after();
        let unhealthy_after = config.effective_unhealthy_after();

        // 初始 Starting。
        let at = chrono::Utc::now().to_rfc3339();
        {
            let mut states = self.states.lock().unwrap();
            states.insert(
                process_id,
                HealthSnapshot {
                    process_id,
                    workspace_id,
                    runtime_name: runtime_name.to_string(),
                    phase: HealthStatus::Starting,
                    last_checked_at: Some(at.clone()),
                    last_detail: Some("健康检查已启动".into()),
                },
            );
        }
        self.emit(&HealthChangedPayload {
            workspace_id,
            process_id,
            runtime_name: runtime_name.to_string(),
            health: HealthStatus::Starting,
            at,
        });

        let mut consecutive_ok = 0u32;
        let mut consecutive_fail = 0u32;
        let mut backoff_ms;

        loop {
            // 停止信号按 100ms 切片等待（退出延迟有界）。
            if sleep_interruptible(stop, interval) {
                break;
            }

            // 进程行终态自愈（防 stop_monitor 链路遗漏导致轮询泄漏）。
            let row_status = {
                let conn = match self.db.lock() {
                    Ok(conn) => conn,
                    Err(_) => break,
                };
                store::get_process(&conn, process_id).ok().flatten().map(|r| r.status)
            };
            if row_status.is_some_and(|s| s.is_terminal()) {
                self.finalize_stopped(process_id);
                break;
            }

            // 目标端口：显式配置 > 启动日志探测端口（最近一个）。
            let port = config.port.or_else(|| {
                let conn = self.db.lock().ok()?;
                store::get_process(&conn, process_id)
                    .ok()
                    .flatten()?
                    .ports
                    .last()
                    .copied()
            });
            let outcome = evaluate_check(&config, port);
            let at = chrono::Utc::now().to_rfc3339();

            let new_phase = if outcome.healthy {
                consecutive_fail = 0;
                consecutive_ok = consecutive_ok.saturating_add(1);
                (consecutive_ok >= healthy_after).then_some(HealthStatus::Healthy)
            } else {
                consecutive_ok = 0;
                consecutive_fail = consecutive_fail.saturating_add(1);
                (consecutive_fail >= unhealthy_after).then_some(HealthStatus::Unhealthy)
            };

            // 退避：Unhealthy 状态下拉长轮询间隔。
            backoff_ms = if outcome.healthy {
                interval.as_millis() as u64
            } else {
                (interval.as_millis() as u64 * UNHEALTHY_BACKOFF as u64).min(BACKOFF_CAP_MS)
            };

            if let Some(phase) = new_phase {
                let mut states = self.states.lock().unwrap();
                if let Some(snapshot) = states.get_mut(&process_id) {
                    if snapshot.phase != phase {
                        snapshot.phase = phase;
                        snapshot.last_checked_at = Some(at.clone());
                        snapshot.last_detail = Some(outcome.detail.clone());
                        self.emit(&HealthChangedPayload {
                            workspace_id,
                            process_id,
                            runtime_name: runtime_name.to_string(),
                            health: phase,
                            at: at.clone(),
                        });
                    }
                }
            } else {
                // 无迁移也刷新详情（UI 下次拉取可见最新探测信息）。
                let mut states = self.states.lock().unwrap();
                if let Some(snapshot) = states.get_mut(&process_id) {
                    snapshot.last_checked_at = Some(at);
                    snapshot.last_detail = Some(outcome.detail);
                }
            }

            if sleep_interruptible(stop, Duration::from_millis(backoff_ms)) {
                break;
            }
        }
        self.monitors.lock().unwrap().remove(&process_id);
    }
}

/// 可中断 sleep：`true` = 期间收到停止信号。
fn sleep_interruptible(stop: &AtomicBool, total: Duration) -> bool {
    let deadline = std::time::Instant::now() + total;
    loop {
        if stop.load(Ordering::Relaxed) {
            return true;
        }
        let now = std::time::Instant::now();
        if now >= deadline {
            return false;
        }
        std::thread::sleep(STOP_POLL_SLICE.min(deadline - now));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- 纯函数：HTTP 解析 / Actuator 状态 ----

    #[test]
    fn parse_http_response_extracts_status_and_body() {
        let raw = "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n{\"status\":\"UP\"}";
        let (code, body) = parse_http_response(raw).unwrap();
        assert_eq!(code, 200);
        assert_eq!(body, "{\"status\":\"UP\"}");
        assert!(parse_http_response("garbage").is_none());
    }

    #[test]
    fn actuator_status_reads_up_and_down() {
        assert_eq!(
            actuator_status(r#"{"components":null,"status":"UP"}"#).as_deref(),
            Some("UP")
        );
        assert_eq!(
            actuator_status("{\n  \"status\" : \"DOWN\",\n  \"x\": 1\n}").as_deref(),
            Some("DOWN")
        );
        assert_eq!(actuator_status("{\"nope\":1}"), None);
    }

    #[test]
    fn tcp_probe_reports_closed_port_as_unhealthy() {
        // 挑一个大概率空闲的端口绑定失败场景：先占住，再探测同端口应成功。
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let ok = tcp_probe("127.0.0.1", port, Duration::from_millis(500));
        assert!(ok.healthy, "{}", ok.detail);
        drop(listener);
        // 关闭后探测大概率失败（TIME_WAIT 不影响 connect 拒绝）。
        let closed = tcp_probe("127.0.0.1", port, Duration::from_millis(500));
        assert!(!closed.healthy, "{}", closed.detail);
    }

    #[test]
    fn evaluate_check_without_port_waits_for_detection() {
        let config = HealthCheckConfig {
            kind: HealthCheckKind::Port,
            ..Default::default()
        };
        let outcome = evaluate_check(&config, None);
        assert!(!outcome.healthy);
        assert!(outcome.detail.contains("端口"));
    }

    #[test]
    fn evaluate_check_port_kind_uses_tcp() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let config = HealthCheckConfig {
            kind: HealthCheckKind::Port,
            port: Some(port),
            ..Default::default()
        };
        assert!(evaluate_check(&config, Some(port)).healthy);
    }

    #[test]
    fn evaluate_check_http_kind_maps_status_codes() {
        // 本地起一个极简 HTTP 服务验证 Http / Actuator 探针全链路。
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            for stream in listener.incoming() {
                let mut stream = stream.unwrap();
                let mut buf = [0u8; 1024];
                let _ = stream.read(&mut buf);
                let _ = stream.write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 15\r\n\r\n{\"status\":\"UP\"}",
                );
            }
        });
        let http_config = HealthCheckConfig {
            kind: HealthCheckKind::Http,
            port: Some(port),
            path: Some("/health".into()),
            ..Default::default()
        };
        let outcome = evaluate_check(&http_config, Some(port));
        assert!(outcome.healthy, "{}", outcome.detail);

        let actuator_config = HealthCheckConfig {
            kind: HealthCheckKind::Actuator,
            port: Some(port),
            path: Some("/actuator/health".into()),
            ..Default::default()
        };
        let outcome = evaluate_check(&actuator_config, Some(port));
        assert!(outcome.healthy, "{}", outcome.detail);
        assert!(outcome.detail.contains("UP"));

        let auto_config = HealthCheckConfig {
            kind: HealthCheckKind::Auto,
            port: Some(port),
            ..Default::default()
        };
        let outcome = evaluate_check(&auto_config, Some(port));
        assert!(outcome.healthy, "{}", outcome.detail);
        drop(server);
    }

    #[test]
    fn health_config_defaults_and_clamps() {
        let config = HealthCheckConfig::default();
        assert_eq!(config.effective_host(), "127.0.0.1");
        assert_eq!(config.effective_path(), DEFAULT_ACTUATOR_PATH);
        assert_eq!(config.effective_interval(), Duration::from_millis(DEFAULT_INTERVAL_MS));
        assert_eq!(config.effective_timeout(), Duration::from_millis(DEFAULT_TIMEOUT_MS));
        assert_eq!(config.effective_healthy_after(), 1);
        assert_eq!(config.effective_unhealthy_after(), 3);
        let small = HealthCheckConfig {
            interval_ms: Some(0),
            timeout_ms: Some(999_999),
            ..Default::default()
        };
        assert_eq!(small.effective_interval(), Duration::from_millis(MIN_INTERVAL_MS));
        assert_eq!(small.effective_timeout(), Duration::from_millis(MAX_TIMEOUT_MS));
    }

    #[test]
    fn health_config_serializes_camel_case_with_defaults() {
        let text = r#"{"kind":"auto"}"#;
        let config: HealthCheckConfig = serde_json::from_str(text).unwrap();
        assert_eq!(config.kind, HealthCheckKind::Auto);
        assert!(config.port.is_none());
        let roundtrip: HealthCheckConfig = serde_json::from_str(&serde_json::to_string(&config).unwrap()).unwrap();
        assert_eq!(config, roundtrip);
    }

    // ---- sleep_interruptible ----

    #[test]
    fn sleep_interruptible_stops_early() {
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(150));
            flag.store(true, Ordering::Relaxed);
        });
        let start = std::time::Instant::now();
        let stopped = sleep_interruptible(&stop, Duration::from_secs(5));
        assert!(stopped);
        assert!(start.elapsed() < Duration::from_secs(2));
        assert!(!sleep_interruptible(&AtomicBool::new(false), Duration::from_millis(50)));
    }
}
