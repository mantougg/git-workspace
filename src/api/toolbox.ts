import { invoke } from "@tauri-apps/api/core";

/** 网卡 IPv4 概况（对齐 route_split.rs 的 camelCase 序列化）。 */
export interface NetInterface {
  ifIndex: number;
  name: string;
  ips: string[];
  gateways: string[];
  metric: number | null;
  connected: boolean;
}

/** 分流方案：内网网段走 lanIf 的网关，其余流量走低 metric 的 wanIf。 */
export interface SplitPlan {
  lanIf: number;
  lanGateway: string;
  wanIf: number;
  prefixes: string[];
}

export function toolboxListNetInterfaces() {
  return invoke<NetInterface[]>("toolbox_list_net_interfaces");
}

/** 命令预览（纯计算，不执行）；restore=true 生成恢复命令。 */
export function toolboxRoutePlanPreview(plan: SplitPlan, restore: boolean) {
  return invoke<string[]>("toolbox_route_plan_preview", { plan, restore });
}

/** 提权执行命令（触发 UAC）。调用前必须经用户确认（confirmed 在后端强制）。 */
export function toolboxRouteApply(commands: string[]) {
  return invoke<void>("toolbox_route_apply", { commands, confirmed: true });
}

/** 随机密钥（三种编码同源，对齐 crypto/secret.rs 的 camelCase 序列化）。 */
export interface GeneratedSecret {
  hex: string;
  base64: string;
  base64Url: string;
}

/** 生成随机密钥（bits 可选 128 / 192 / 256，缺省 256）。 */
export function toolboxGenerateSecret(bits?: number) {
  return invoke<GeneratedSecret>("toolbox_generate_secret", { bits });
}
