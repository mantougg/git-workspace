//! 工具箱后端命令：路由分流（双网卡：内网走网线、外网走 WiFi/热点）。

use tauri::command;

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
