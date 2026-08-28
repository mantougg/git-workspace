//! Port Manager（R-16，§81）：端口占用检测与处理手段。
//!
//! - 检测：bind 兜底判定（bind 成功 = 空闲，避免 lsof/netstat 缺失时的
//!   误报），占用时经 `process::port::detect_port_occupier` 识别占用方
//!   （PID / 进程名，R-14 已有解析底座，纯函数单测覆盖）；
//! - Kill：跨进程终止任意 PID 属危险操作（全局约束 §3），IPC 层必须
//!   `confirmed=true` 二次确认；实现走 sysinfo（TERM 优雅优先，Windows
//!   无 TERM 语义直接 kill）。
//!
//! 「Change Runtime Port（改写应用配置）」落在 IPC 层：改的是 GitWorkspace
//! 自己的 Runtime 配置（`program_arguments` 注入 `--server.port=`），
//! 不触碰用户项目文件（全局约束 §2 用户项目只读）。

use std::net::TcpListener;
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// 端口检查结果（跨 IPC）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PortCheckResult {
    pub port: u16,
    /// true = 被占用（bind 失败）。
    pub in_use: bool,
    /// 占用方信息；探测失败（命令缺失等）时为 None，仅 `in_use` 可信。
    pub occupier: Option<crate::process::port::PortOccupier>,
}

/// 检查端口占用。以 bind 实测为准：bind 成功 → 空闲；失败 → 占用并尽力
/// 识别占用方（找不到 netstat/lsof 时 occupier 为 None）。
pub fn check_port(port: u16) -> PortCheckResult {
    let bind = TcpListener::bind(("127.0.0.1", port));
    match bind {
        Ok(listener) => {
            drop(listener);
            PortCheckResult {
                port,
                in_use: false,
                occupier: None,
            }
        }
        Err(_) => PortCheckResult {
            port,
            in_use: true,
            occupier: crate::process::port::detect_port_occupier(port),
        },
    }
}

/// 跨进程 Kill 结果（跨 IPC）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PortKillOutcome {
    pub pid: u32,
    /// 进程名（kill 前查询；查询失败为 None）。
    pub process_name: Option<String>,
    /// true = 已终止（含 TERM 升级 KILL）。
    pub killed: bool,
}

/// 终止任意进程（危险操作，调用方必须已二次确认）。TERM 优雅优先，
/// 3s 未退出升级 KILL；进程不存在时返回 `killed: false`（幂等）。
pub fn kill_external_process(pid: u32) -> PortKillOutcome {
    let mut system = sysinfo::System::new_with_specifics(
        sysinfo::RefreshKind::new().with_processes(sysinfo::ProcessRefreshKind::new()),
    );
    system.refresh_processes();
    let Some(process) = system.process(sysinfo::Pid::from_u32(pid)) else {
        log::info!("R-16: kill_external_process pid={pid} not found (already gone)");
        return PortKillOutcome {
            pid,
            process_name: None,
            killed: false,
        };
    };
    let process_name = process.name().to_string();
    let process_name = if process_name.is_empty() {
        None
    } else {
        Some(process_name)
    };
    // TERM 优雅优先；Windows 无 TERM 语义（kill_with 返回 None）直接 kill。
    let graceful = process.kill_with(sysinfo::Signal::Term).unwrap_or(false);
    if graceful {
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        while std::time::Instant::now() < deadline {
            system.refresh_processes();
            if system.process(sysinfo::Pid::from_u32(pid)).is_none() {
                log::info!("R-16: pid={pid} terminated gracefully (TERM)");
                return PortKillOutcome {
                    pid,
                    process_name,
                    killed: true,
                };
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    // KILL 升级（或 TERM 不可用）。
    system.refresh_processes();
    let killed = system
        .process(sysinfo::Pid::from_u32(pid))
        .map(|p| p.kill())
        .is_some();
    log::info!("R-16: pid={pid} kill outcome: killed={killed}");
    PortKillOutcome {
        pid,
        process_name,
        killed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_port_reports_free_and_occupied() {
        // 系统分配一个空闲端口 → 空闲。
        let probe = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let free_port = probe.local_addr().unwrap().port();
        drop(probe);
        // 先占住一个端口再检查：必然 in_use。
        let listener = TcpListener::bind(("127.0.0.1", free_port)).unwrap();
        let result = check_port(free_port);
        assert!(result.in_use);
        drop(listener);
        // 释放后大概率空闲（本机自占，无并发竞争方）。
        let result = check_port(free_port);
        assert!(!result.in_use, "port {free_port} should be free after close");
    }

    #[test]
    fn kill_external_process_missing_pid_is_idempotent() {
        // 不存在的 PID（u32 上界附近）→ killed=false，不 panic。
        let outcome = kill_external_process(u32::MAX - 1);
        assert!(!outcome.killed);
        assert!(outcome.process_name.is_none());
    }
}
