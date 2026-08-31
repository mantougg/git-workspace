//! 启动前端口预检（R-14 §79 `PortOccupied`）。
//!
//! 从 Runtime 配置的 VM options / Program Arguments 解析**显式声明**的端口，
//! 启动前 bind 探测；被占用即返回 `PortOccupied`（带占用方 PID / 进程名，
//! §80 可行动提示），避免应用启动后因端口冲突崩溃才报错。
//!
//! 边界：只预检显式端口（`--server.port=0` 随机端口跳过）；默认端口
//! （Spring Boot 8080 未显式声明）不做预检——完整端口管理归 R-16
//! Health Check / Port Manager。

use std::net::TcpListener;

use crate::error::{AppError, AppResult};
use crate::process::port::{detect_port_occupier, PortOccupier};
use crate::runtime::config::{RuntimeApplicationConfig, RuntimeKind};

/// 从配置解析显式声明的端口（去重、升序）。支持：
/// - VM options：`-Dserver.port=N`
/// - Program Arguments：`--server.port=N` / `--port=N`
/// `0`（随机端口）与非法值忽略。
pub fn explicit_ports(config: &RuntimeApplicationConfig) -> Vec<u16> {
    if config.kind == RuntimeKind::Node {
        return explicit_node_ports(config);
    }
    let mut ports: Vec<u16> = Vec::new();
    for arg in config
        .vm_options
        .iter()
        .chain(config.program_arguments.iter())
    {
        let value = arg
            .strip_prefix("-Dserver.port=")
            .or_else(|| arg.strip_prefix("--server.port="))
            .or_else(|| arg.strip_prefix("--port="));
        if let Some(value) = value {
            if let Ok(port) = value.parse::<u16>() {
                if port > 0 && !ports.contains(&port) {
                    ports.push(port);
                }
            }
        }
    }
    ports.sort_unstable();
    ports
}

fn explicit_node_ports(config: &RuntimeApplicationConfig) -> Vec<u16> {
    let mut ports = Vec::new();
    let args = &config.program_arguments;
    let mut index = 0;
    while index < args.len() {
        let arg = args[index].as_str();
        let inline = arg
            .strip_prefix("--port=")
            .or_else(|| arg.strip_prefix("-p="));
        let value = inline.or_else(|| {
            if arg == "--port" || arg == "-p" {
                args.get(index + 1).map(String::as_str)
            } else {
                None
            }
        });
        if let Some(value) = value {
            if let Ok(port) = value.parse::<u16>() {
                if port > 0 && !ports.contains(&port) {
                    ports.push(port);
                }
            }
            if inline.is_none() {
                index += 1;
            }
        }
        index += 1;
    }
    if let Some(value) = config
        .environment
        .get("PORT")
        .or_else(|| config.runtime_environment.get("PORT"))
    {
        if let Ok(port) = value.trim().parse::<u16>() {
            if port > 0 && !ports.contains(&port) {
                ports.push(port);
            }
        }
    }
    ports.sort_unstable();
    ports
}

/// 预检：任一显式端口被占用即返回 `PortOccupied`（尽力附带占用方信息）。
pub fn preflight(config: &RuntimeApplicationConfig) -> AppResult<()> {
    for port in explicit_ports(config) {
        if TcpListener::bind(("127.0.0.1", port)).is_err() {
            // bind 失败视为被占用（权限等罕见情况也按占用处理，宁可提示）。
            let occupier = detect_port_occupier(port)
                .unwrap_or(PortOccupier {
                    pid: None,
                    process_name: None,
                });
            return Err(AppError::PortOccupied {
                port,
                pid: occupier.pid,
                process_name: occupier.process_name,
            });
        }
        // 探测即释放：listener drop 后端口恢复空闲。
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config_with(args: (&str, Vec<&str>)) -> RuntimeApplicationConfig {
        let (kind, args) = args;
        let mut config = RuntimeApplicationConfig::default();
        match kind {
            "vm" => config.vm_options = args.into_iter().map(ToOwned::to_owned).collect(),
            "args" => config.program_arguments = args.into_iter().map(ToOwned::to_owned).collect(),
            _ => unreachable!(),
        }
        config
    }

    #[test]
    fn parses_explicit_ports_from_vm_options_and_args() {
        let config = config_with(("vm", vec!["-Xmx1g", "-Dserver.port=8080", "-Dother=1"]));
        assert_eq!(explicit_ports(&config), vec![8080]);

        let config = config_with(("args", vec!["--server.port=9090", "--port=7070"]));
        assert_eq!(explicit_ports(&config), vec![7070, 9090]);

        // 两种来源合并去重。
        let mut config = config_with(("vm", vec!["-Dserver.port=8080"]));
        config.program_arguments = vec!["--server.port=8080".into()];
        assert_eq!(explicit_ports(&config), vec![8080]);
    }

    #[test]
    fn ignores_random_and_invalid_ports() {
        let config = config_with(("args", vec!["--server.port=0", "--port=99999", "--port=abc"]));
        assert!(explicit_ports(&config).is_empty());
        let config = config_with(("args", vec!["--port"]));
        assert!(explicit_ports(&config).is_empty());
    }

    #[test]
    fn preflight_detects_occupied_port() {
        // 先占住一个随机端口，再预检同一端口应报 PortOccupied。
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind ephemeral");
        let port = listener.local_addr().unwrap().port();
        let mut config = RuntimeApplicationConfig::default();
        config.program_arguments = vec![format!("--server.port={port}")];

        let error = preflight(&config).unwrap_err();
        match error {
            AppError::PortOccupied {
                port: occupied,
                pid: _,
                process_name: _,
            } => assert_eq!(occupied, port),
            other => panic!("expected PortOccupied, got {other:?}"),
        }
    }

    #[test]
    fn preflight_passes_when_port_free() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind ephemeral");
        let port = listener.local_addr().unwrap().port();
        drop(listener); // 释放端口后再预检应通过。

        let mut config = RuntimeApplicationConfig::default();
        config.program_arguments = vec![format!("--server.port={port}")];
        preflight(&config).expect("free port must pass preflight");

        // 无显式端口（随机端口 / 未声明）直接通过。
        let mut config = RuntimeApplicationConfig::default();
        config.program_arguments = vec!["--server.port=0".into()];
        preflight(&config).expect("random port must skip preflight");
    }

    #[test]
    fn parses_node_cli_and_environment_ports() {
        let mut config = RuntimeApplicationConfig::default();
        config.kind = RuntimeKind::Node;
        config.program_arguments = vec![
            "--port".into(),
            "3000".into(),
            "-p=5173".into(),
            "--port=8080".into(),
        ];
        config.environment.insert("PORT".into(), "4000".into());
        assert_eq!(explicit_ports(&config), vec![3000, 4000, 5173, 8080]);
    }

    #[test]
    fn node_without_explicit_port_skips_preflight() {
        let mut config = RuntimeApplicationConfig::default();
        config.kind = RuntimeKind::Node;
        config.program_arguments = vec!["--host".into(), "localhost".into()];
        assert!(explicit_ports(&config).is_empty());
    }
}
