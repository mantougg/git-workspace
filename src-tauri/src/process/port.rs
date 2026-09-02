//! 端口占用检测（R-14 §79 `PortOccupied`）：定位占用端口的进程 PID 与名称。
//!
//! 系统命令尽力而为：找不到命令 / 解析失败时返回 `pid: None`（仍算占用），
//! 由上层以 `PortOccupied` 报出「端口不可用」；占用方信息用于 §80 可行动
//! 提示（占用进程名 + PID + 建议动作）。
//!
//! 解析函数（`parse_netstat_*` / `parse_lsof_*`）保持纯函数，便于单测；
//! 系统调用仅在运行时（GUI 进程）发生。

use std::io::ErrorKind;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpListener};
use std::path::PathBuf;
use std::process::Command;

/// 占用方信息。跨 IPC 的结构化字段（§80：占用进程 PID / 进程名 / 可执行路径）。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PortOccupier {
    pub pid: Option<u32>,
    pub process_name: Option<String>,
    /// 占用进程可执行文件绝对路径（sysinfo 探测；权限受限/失败为 None）。
    #[serde(default)]
    pub executable_path: Option<String>,
}

/// 探测端口占用。返回 `None` 表示端口空闲（或探测失败但无法确认占用——
/// 上层需自行 bind 兜底）；`Some` 表示被占用。
pub fn detect_port_occupier(port: u16) -> Option<PortOccupier> {
    #[cfg(windows)]
    {
        detect_windows(port)
    }
    #[cfg(not(windows))]
    {
        detect_unix(port)
    }
}

/// Conservative TCP listener probe shared by Port Manager and Runtime
/// preflight. The OS listener table catches wildcard/specific-address
/// combinations that a single loopback bind can miss; socket binds remain the
/// fallback when netstat/lsof is unavailable or cannot identify the process.
pub fn is_port_in_use(port: u16) -> bool {
    if detect_port_occupier(port).is_some() {
        return true;
    }

    let addresses = [
        SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), port),
        SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port),
        SocketAddr::new(IpAddr::V6(Ipv6Addr::UNSPECIFIED), port),
        SocketAddr::new(IpAddr::V6(Ipv6Addr::LOCALHOST), port),
    ];

    addresses
        .iter()
        .any(|address| match TcpListener::bind(address) {
            Ok(listener) => {
                drop(listener);
                false
            }
            Err(error)
                if address.is_ipv6()
                    && matches!(
                        error.kind(),
                        ErrorKind::AddrNotAvailable | ErrorKind::Unsupported
                    ) =>
            {
                // IPv6 may be disabled on the host; it is not evidence that the
                // TCP port is occupied.
                false
            }
            Err(_) => true,
        })
}

/// 用 sysinfo 查询 pid 的可执行文件绝对路径。归一化 + 文件存在校验
/// （sysinfo 部分平台路径带尾分隔/损坏则丢弃）。失败返回 None。
fn executable_path_for_pid(pid: u32) -> Option<String> {
    use sysinfo::{Pid, ProcessRefreshKind, RefreshKind, System, UpdateKind};
    let mut system = System::new_with_specifics(
        RefreshKind::new()
            .with_processes(ProcessRefreshKind::new().with_exe(UpdateKind::OnlyIfNotSet)),
    );
    system.refresh_processes();
    let raw = system
        .process(Pid::from_u32(pid))
        .and_then(|process| process.exe())
        .map(|path| path.to_string_lossy().into_owned())?;
    let trimmed = raw.trim().trim_end_matches(['/', '\\']).to_string();
    if trimmed.is_empty() {
        return None;
    }
    let path = PathBuf::from(&trimmed);
    if path.is_file() {
        Some(trimmed)
    } else {
        None
    }
}

#[cfg(windows)]
fn detect_windows(port: u16) -> Option<PortOccupier> {
    let output = Command::new("netstat").arg("-ano").output().ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    let pid = parse_netstat_occupier(&text, port)?;
    let process_name = process_name_windows(pid);
    let executable_path = executable_path_for_pid(pid);
    Some(PortOccupier {
        pid: Some(pid),
        process_name,
        executable_path,
    })
}

#[cfg(not(windows))]
fn detect_unix(port: u16) -> Option<PortOccupier> {
    let output = Command::new("lsof")
        .args(["-nP", &format!("-iTCP:{port}"), "-sTCP:LISTEN"])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    let pid = parse_lsof_pid(&text)?;
    let process_name = process_name_unix(pid);
    let executable_path = executable_path_for_pid(pid);
    Some(PortOccupier {
        pid: Some(pid),
        process_name,
        executable_path,
    })
}

/// 从 `netstat -ano` 输出解析监听 `port` 的进程 PID（LISTENING 行尾 token）。
/// 纯函数（单测覆盖）。
pub fn parse_netstat_occupier(output: &str, port: u16) -> Option<u32> {
    for line in output.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        let Some(local_address) = fields.get(1) else {
            continue;
        };
        if parse_endpoint_port(local_address) != Some(port)
            || !fields
                .iter()
                .any(|field| field.eq_ignore_ascii_case("LISTENING"))
        {
            continue;
        }
        let Some(pid) = fields.last().and_then(|token| token.parse::<u32>().ok()) else {
            continue;
        };
        if pid > 0 {
            return Some(pid);
        }
    }
    None
}

fn parse_endpoint_port(endpoint: &str) -> Option<u16> {
    endpoint
        .trim_matches(['[', ']'])
        .rsplit(':')
        .next()
        .and_then(|value| value.parse::<u16>().ok())
}

/// 从 `tasklist /FI "PID eq N" /FO CSV /NH` 输出解析进程名（首列去引号）。
/// 无有效行（空 / "INFO: No tasks are running"）返回 `None`。纯函数（单测覆盖）。
pub fn parse_tasklist_name(output: &str) -> Option<String> {
    let line = output.lines().next()?.trim();
    if line.is_empty() || line.starts_with("INFO:") {
        return None;
    }
    let name = line.split(',').next()?.trim_matches('"').trim();
    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}

/// 从 `lsof -nP -iTCP:<port> -sTCP:LISTEN` 输出解析 PID（第二列）。
/// 纯函数（单测覆盖）。
pub fn parse_lsof_pid(output: &str) -> Option<u32> {
    for line in output.lines() {
        let mut parts = line.split_whitespace();
        let Some(_command) = parts.next() else {
            continue;
        };
        let Some(pid) = parts.next() else {
            continue;
        };
        if let Ok(pid) = pid.parse::<u32>() {
            return Some(pid);
        }
    }
    None
}

#[cfg(windows)]
fn process_name_windows(pid: u32) -> Option<String> {
    let filter = format!("PID eq {pid}");
    let output = Command::new("tasklist")
        .args(["/FI", &filter, "/FO", "CSV", "/NH"])
        .output()
        .ok()?;
    parse_tasklist_name(&String::from_utf8_lossy(&output.stdout))
}

#[cfg(not(windows))]
fn process_name_unix(pid: u32) -> Option<String> {
    std::fs::read_to_string(format!("/proc/{pid}/comm"))
        .ok()
        .map(|s| s.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const NETSTAT_SAMPLE: &str = "\
  Proto  Local Address          Foreign Address        State           PID
  TCP    0.0.0.0:8080           0.0.0.0:0              LISTENING       4242
  TCP    0.0.0.0:8081           0.0.0.0:0              LISTENING       5000
  TCP    127.0.0.1:5432         0.0.0.0:0              LISTENING       900
  TCP    0.0.0.0:445           0.0.0.0:0              LISTENING       4
  TCP    10.0.0.5:8080          10.0.0.9:55000         ESTABLISHED     4242
";

    #[test]
    fn netstat_parser_finds_listening_occupier() {
        assert_eq!(parse_netstat_occupier(NETSTAT_SAMPLE, 8080), Some(4242));
        assert_eq!(parse_netstat_occupier(NETSTAT_SAMPLE, 5432), Some(900));
    }

    #[test]
    fn netstat_parser_ignores_established_and_missing() {
        // 8080 的 ESTABLISHED 行不应命中（只认 LISTENING）。
        assert_eq!(parse_netstat_occupier(NETSTAT_SAMPLE, 9999), None);
    }

    #[test]
    fn netstat_parser_matches_exact_local_port() {
        let sample = "\
  TCP    0.0.0.0:18080         0.0.0.0:0              LISTENING       1234
  TCP    [::]:8080             [::]:0                 LISTENING       5678
";
        assert_eq!(parse_netstat_occupier(sample, 8080), Some(5678));
        assert_eq!(parse_netstat_occupier(sample, 808), None);
        assert_eq!(parse_netstat_occupier(sample, 1808), None);
    }

    #[test]
    fn tasklist_parser_extracts_name() {
        let csv = "\"java.exe\",\"4242\",\"Console\",\"1\",\" 12,345 K\"\r\n";
        assert_eq!(parse_tasklist_name(csv).as_deref(), Some("java.exe"));
        assert_eq!(parse_tasklist_name(""), None);
        assert_eq!(parse_tasklist_name("INFO: No tasks are running"), None);
    }

    #[test]
    fn lsof_parser_extracts_pid() {
        let sample = "\
COMMAND PID USER   FD   TYPE DEVICE SIZE/OFF NODE NAME
java    4242 user   123u IPv4 123456      0t0 TCP *:8080 (LISTEN)
";
        assert_eq!(parse_lsof_pid(sample), Some(4242));
        assert_eq!(parse_lsof_pid(""), None);
    }

    #[test]
    fn port_probe_detects_ipv4_wildcard_listener() {
        let listener = TcpListener::bind((Ipv4Addr::UNSPECIFIED, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        assert!(is_port_in_use(port));
        drop(listener);
        assert!(!is_port_in_use(port));
    }

    #[test]
    fn port_probe_detects_ipv6_listener_when_available() {
        let listener = match TcpListener::bind((Ipv6Addr::LOCALHOST, 0)) {
            Ok(listener) => listener,
            Err(error) => {
                eprintln!("IPv6 unavailable; skipping IPv6 port probe test: {error}");
                return;
            }
        };
        let port = listener.local_addr().unwrap().port();
        assert!(is_port_in_use(port));
        drop(listener);
    }
}
