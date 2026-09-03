//! 工具箱后端命令：路由分流（双网卡：内网走网线、外网走 WiFi/热点）、随机密钥生成。

use tauri::command;

use crate::crypto::secret::{self, RandomSecret};
use crate::error::{AppError, AppResult};
use crate::network::route_split::{self, NetInterface, SplitPlan};

/// 列出本机 IPv4 网卡（名称 / IP / 默认网关 / metric / 连接态）。
#[command]
pub fn toolbox_list_net_interfaces() -> AppResult<Vec<NetInterface>> {
    route_split::list_interfaces()
}

/// 生成分流（apply）或恢复（restore）命令预览——纯计算，不执行。
#[command]
pub fn toolbox_route_plan_preview(plan: SplitPlan, restore: bool) -> AppResult<Vec<String>> {
    if restore {
        route_split::build_restore_commands(&plan)
    } else {
        route_split::build_apply_commands(&plan)
    }
}

/// 执行命令（**危险操作**：修改系统路由表/网卡 metric，触发 UAC 提权）。
/// 必须带 `confirmed=true`（同 runtime.kill_port_process 的二次确认约束）。
#[command]
pub fn toolbox_route_apply(commands: Vec<String>, confirmed: bool) -> AppResult<()> {
    if !confirmed {
        return Err(AppError::Permission(
            "执行将修改系统路由表与网卡 metric（需管理员权限）。\
             请确认预览命令无误后，带 confirmed=true 重试"
                .into(),
        ));
    }
    route_split::run_elevated(&commands)
}

/// 支持的密钥位数（对应 16/24/32 字节）。
const SECRET_BITS: [u32; 3] = [128, 192, 256];

/// 生成随机密钥（设计文档 §39），默认 256-bit，输出 hex / base64 / base64url。
#[command]
pub fn toolbox_generate_secret(bits: Option<u32>) -> AppResult<RandomSecret> {
    let bits = bits.unwrap_or(256);
    if !SECRET_BITS.contains(&bits) {
        return Err(AppError::Other(format!(
            "不支持的密钥位数 {bits}（可选 {}）",
            SECRET_BITS.map(|b| b.to_string()).join(" / ")
        )));
    }
    Ok(secret::random_secret((bits / 8) as usize))
}
