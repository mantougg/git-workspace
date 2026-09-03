//! 双网卡路由分流（Windows 优先）：网线走内网、WiFi/热点走外网。
//!
//! 读取走 PowerShell `Get-Net*` + `ConvertTo-Json`（结构化输出，不受系统
//! 语言影响；单元素数组会被 PowerShell 解包成裸对象，解析侧统一归一化）。
//! 命令生成是纯函数（可单测）；应用经 `Start-Process -Verb RunAs` 提权执行
//! 生成的脚本（触发 UAC，用户可审查命令全文后再执行）。

use crate::error::{AppError, AppResult};
use serde::{Deserialize, Serialize};

/// 一块网卡的 IPv4 概况（聚合自 Get-NetIPInterface / Get-NetIPAddress /
/// Get-NetRoute 三个表，按 ifIndex 关联）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetInterface {
    pub if_index: u32,
    pub name: String,
    pub ips: Vec<String>,
    /// 默认网关（0.0.0.0/0 路由的 NextHop；可能多个）。
    pub gateways: Vec<String>,
    pub metric: Option<u32>,
    pub connected: bool,
}

/// 分流方案：内网网段经 lan_if 的网关出网线，其余流量靠调低 wan_if 的
/// 接口 metric 走 WiFi/热点。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SplitPlan {
    /// 内网网卡 ifIndex（网线）。
    pub lan_if: u32,
    /// 内网网关 IP（该网卡 0.0.0.0/0 路由的 NextHop）。
    pub lan_gateway: String,
    /// 外网网卡 ifIndex（WiFi/热点）。
    pub wan_if: u32,
    /// 内网网段 CIDR 列表（如 10.0.0.0/8）。
    pub prefixes: Vec<String>,
}

// ------------------------------------------------------------------
// 读取：PowerShell + ConvertTo-Json
// ------------------------------------------------------------------

#[cfg(windows)]
const PS_QUERY: &str = r#"
$i = @(Get-NetIPInterface -AddressFamily IPv4 | Where-Object { $_.InterfaceAlias -notlike '*Loopback*' } | Select-Object ifIndex,InterfaceAlias,InterfaceMetric,ConnectionState);
$a = @(Get-NetIPAddress -AddressFamily IPv4 | Select-Object ifIndex,IPAddress,PrefixLength);
$r = @(Get-NetRoute -AddressFamily IPv4 -DestinationPrefix '0.0.0.0/0' -ErrorAction SilentlyContinue | Select-Object ifIndex,NextHop);
[pscustomobject]@{ interfaces=$i; addresses=$a; defaults=$r } | ConvertTo-Json -Depth 4 -Compress
"#;

#[cfg(windows)]
pub fn list_interfaces() -> AppResult<Vec<NetInterface>> {
    use std::os::windows::process::CommandExt;
    let mut cmd = std::process::Command::new("powershell");
    cmd.args(["-NoProfile", "-NonInteractive", "-Command", PS_QUERY]);
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW（同 process/port.rs）
    let out = cmd.output()?;
    if !out.status.success() {
        return Err(AppError::Other(format!(
            "查询网卡信息失败：{}",
            String::from_utf8_lossy(&out.stderr)
        )));
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    parse_interfaces(&stdout)
}

#[cfg(not(windows))]
pub fn list_interfaces() -> AppResult<Vec<NetInterface>> {
    Err(AppError::Other(
        "路由分流工具目前仅支持 Windows（macOS/Linux 可用 route/ip route 等价实现，待补）".into(),
    ))
}

/// 解析 PS 查询结果。三层表各自可能是裸对象（单条）或数组，统一归一化。
fn parse_interfaces(json: &str) -> AppResult<Vec<NetInterface>> {
    let root: serde_json::Value = serde_json::from_str(json.trim())?;
    let interfaces = as_array(&root["interfaces"]);
    let addresses = as_array(&root["addresses"]);
    let defaults = as_array(&root["defaults"]);

    let mut out: Vec<NetInterface> = Vec::new();
    for iface in &interfaces {
        let if_index = iface["ifIndex"].as_u64().unwrap_or(0) as u32;
        let name = iface["InterfaceAlias"].as_str().unwrap_or("").to_string();
        let connected = iface["ConnectionState"].as_str() == Some("Connected");
        let metric = iface["InterfaceMetric"].as_u64().map(|m| m as u32);
        let ips = addresses
            .iter()
            .filter(|a| a["ifIndex"].as_u64() == Some(if_index as u64))
            .filter_map(|a| {
                let ip = a["IPAddress"].as_str()?;
                let plen = a["PrefixLength"].as_u64()?;
                Some(format!("{ip}/{plen}"))
            })
            .collect();
        let gateways = defaults
            .iter()
            .filter(|r| r["ifIndex"].as_u64() == Some(if_index as u64))
            .filter_map(|r| r["NextHop"].as_str().map(str::to_string))
            .filter(|g| g != "0.0.0.0")
            .collect();
        out.push(NetInterface { if_index, name, ips, gateways, metric, connected });
    }
    Ok(out)
}

/// PowerShell ConvertTo-Json 对单元素数组输出裸对象 —— 归一化为数组。
fn as_array(v: &serde_json::Value) -> Vec<serde_json::Value> {
    match v {
        serde_json::Value::Array(arr) => arr.clone(),
        serde_json::Value::Null => Vec::new(),
        other => vec![other.clone()],
    }
}

// ------------------------------------------------------------------
// 命令生成（纯函数，可单测）
// ------------------------------------------------------------------

/// 校验并归一化 CIDR → (网络地址, 点分掩码)。host 位清零。
pub fn parse_cidr(cidr: &str) -> AppResult<(String, String)> {
    let (ip_str, prefix_str) = cidr
        .trim()
        .split_once('/')
        .ok_or_else(|| AppError::Other(format!("网段「{cidr}」缺少 / 前缀长度")))?;
    let prefix: u8 = prefix_str
        .parse()
        .ok()
        .filter(|p| *p <= 32)
        .ok_or_else(|| AppError::Other(format!("网段「{cidr}」前缀长度非法（0-32）")))?;
    let octets: Vec<u8> = ip_str
        .split('.')
        .map(|s| s.parse::<u8>())
        .collect::<Result<_, _>>()
        .map_err(|_| AppError::Other(format!("网段「{cidr}」IP 非法")))?;
    let [a, b, c, d]: [u8; 4] = octets
        .try_into()
        .map_err(|_| AppError::Other(format!("网段「{cidr}」须为点分四段 IPv4")))?;
    let ip = u32::from_be_bytes([a, b, c, d]);
    let mask: u32 = if prefix == 0 { 0 } else { u32::MAX << (32 - prefix) };
    let net = ip & mask;
    Ok((
        std::net::Ipv4Addr::from(net).to_string(),
        std::net::Ipv4Addr::from(mask).to_string(),
    ))
}

/// 分流命令：外网卡 metric 调低（优先默认路由），内网网段加持久静态路由。
pub fn build_apply_commands(plan: &SplitPlan) -> AppResult<Vec<String>> {
    if plan.lan_if == plan.wan_if {
        return Err(AppError::Other("内网与外网网卡不能是同一块".into()));
    }
    if plan.prefixes.is_empty() {
        return Err(AppError::Other("至少需要一个内网网段".into()));
    }
    let mut cmds = vec![
        format!("Set-NetIPInterface -InterfaceIndex {} -InterfaceMetric 10", plan.wan_if),
        format!("Set-NetIPInterface -InterfaceIndex {} -InterfaceMetric 60", plan.lan_if),
    ];
    for cidr in &plan.prefixes {
        let (net, mask) = parse_cidr(cidr)?;
        cmds.push(format!(
            "route -p add {net} mask {mask} {} METRIC 5 IF {}",
            plan.lan_gateway, plan.lan_if
        ));
    }
    Ok(cmds)
}

/// 恢复命令：删除静态路由 + 两张网卡恢复自动 metric。
pub fn build_restore_commands(plan: &SplitPlan) -> AppResult<Vec<String>> {
    let mut cmds = Vec::new();
    for cidr in &plan.prefixes {
        let (net, _) = parse_cidr(cidr)?;
        cmds.push(format!("route delete {net}"));
    }
    cmds.push(format!(
        "Set-NetIPInterface -InterfaceIndex {} -AutomaticMetric Enabled",
        plan.wan_if
    ));
    cmds.push(format!(
        "Set-NetIPInterface -InterfaceIndex {} -AutomaticMetric Enabled",
        plan.lan_if
    ));
    Ok(cmds)
}

// ------------------------------------------------------------------
// 提权执行：写临时 ps1 → Start-Process -Verb RunAs（触发 UAC）
// ------------------------------------------------------------------

#[cfg(windows)]
pub fn run_elevated(commands: &[String]) -> AppResult<()> {
    use std::os::windows::process::CommandExt;
    let path = std::env::temp_dir().join(format!(
        "gw_route_split_{}.ps1",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
    ));
    // 追加 Read-Host：提权窗口执行完停留，便于用户查看每行命令结果。
    let mut script = commands.join("\r\n");
    script.push_str("\r\nWrite-Host ''; Write-Host '执行完毕，按回车关闭窗口。'; Read-Host\r\n");
    std::fs::write(&path, script)?;
    let arg = format!(
        "Start-Process -FilePath powershell -ArgumentList '-NoProfile','-ExecutionPolicy','Bypass','-File','{}' -Verb RunAs -Wait",
        path.to_string_lossy()
    );
    let mut cmd = std::process::Command::new("powershell");
    cmd.args(["-NoProfile", "-NonInteractive", "-Command", &arg]);
    cmd.creation_flags(0x0800_0000);
    let status = cmd.status()?;
    let _ = std::fs::remove_file(&path);
    if !status.success() {
        return Err(AppError::Other(
            "提权执行失败（UAC 被取消或权限不足），分流未生效".into(),
        ));
    }
    Ok(())
}

#[cfg(not(windows))]
pub fn run_elevated(_commands: &[String]) -> AppResult<()> {
    Err(AppError::Other("路由分流工具目前仅支持 Windows".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan() -> SplitPlan {
        SplitPlan {
            lan_if: 12,
            lan_gateway: "10.20.0.1".into(),
            wan_if: 8,
            prefixes: vec!["10.0.0.0/8".into(), "172.16.0.0/12".into()],
        }
    }

    #[test]
    fn parse_cidr_normalizes_host_bits() {
        assert_eq!(parse_cidr("10.1.2.3/8").unwrap(), ("10.0.0.0".into(), "255.0.0.0".into()));
        assert_eq!(
            parse_cidr("172.16.0.0/12").unwrap(),
            ("172.16.0.0".into(), "255.240.0.0".into())
        );
        assert_eq!(parse_cidr("0.0.0.0/0").unwrap(), ("0.0.0.0".into(), "0.0.0.0".into()));
    }

    #[test]
    fn parse_cidr_rejects_bad_input() {
        assert!(parse_cidr("10.0.0.0").is_err());
        assert!(parse_cidr("10.0.0.0/33").is_err());
        assert!(parse_cidr("10.0.0/8").is_err());
        assert!(parse_cidr("10.0.0.256/8").is_err());
    }

    #[test]
    fn apply_commands_lower_wan_metric_and_add_persistent_routes() {
        let cmds = build_apply_commands(&plan()).unwrap();
        assert_eq!(cmds[0], "Set-NetIPInterface -InterfaceIndex 8 -InterfaceMetric 10");
        assert_eq!(cmds[1], "Set-NetIPInterface -InterfaceIndex 12 -InterfaceMetric 60");
        assert_eq!(cmds[2], "route -p add 10.0.0.0 mask 255.0.0.0 10.20.0.1 METRIC 5 IF 12");
        assert_eq!(cmds[3], "route -p add 172.16.0.0 mask 255.240.0.0 10.20.0.1 METRIC 5 IF 12");
    }

    #[test]
    fn apply_commands_reject_same_if_and_empty_prefixes() {
        let mut p = plan();
        p.wan_if = p.lan_if;
        assert!(build_apply_commands(&p).is_err());
        let mut p = plan();
        p.prefixes = vec![];
        assert!(build_apply_commands(&p).is_err());
    }

    #[test]
    fn restore_commands_delete_routes_and_reset_metrics() {
        let cmds = build_restore_commands(&plan()).unwrap();
        assert_eq!(cmds[0], "route delete 10.0.0.0");
        assert_eq!(cmds[1], "route delete 172.16.0.0");
        assert!(cmds[2].contains("-AutomaticMetric Enabled"));
        assert!(cmds[3].contains("-AutomaticMetric Enabled"));
    }

    #[test]
    fn parse_interfaces_normalizes_single_item_arrays() {
        // PowerShell 单元素数组解包成裸对象的回归。
        let json = r#"{
          "interfaces": {"ifIndex": 12, "InterfaceAlias": "以太网", "InterfaceMetric": 25, "ConnectionState": "Connected"},
          "addresses": [{"ifIndex": 12, "IPAddress": "10.20.0.5", "PrefixLength": 24}],
          "defaults": {"ifIndex": 12, "NextHop": "10.20.0.1"}
        }"#;
        let ifaces = parse_interfaces(json).unwrap();
        assert_eq!(ifaces.len(), 1);
        assert_eq!(ifaces[0].name, "以太网");
        assert_eq!(ifaces[0].ips, vec!["10.20.0.5/24"]);
        assert_eq!(ifaces[0].gateways, vec!["10.20.0.1"]);
        assert!(ifaces[0].connected);
    }
}
