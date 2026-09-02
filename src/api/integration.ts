import { invoke } from "@tauri-apps/api/core";

/** 终端类型（与后端 TerminalKind kebab-case 对齐） */
export type TerminalKind =
  | "system"
  | "powershell"
  | "cmd"
  | "git-bash"
  | "windows-terminal";

/** IDE 类型（与后端 IdeKind kebab-case 对齐） */
export type IdeKind = "vscode" | "idea" | "cursor" | "zed";

export interface IntegrationTargets {
  terminals: string[];
  ides: string[];
}

/** 在指定终端打开目录（kind 缺省为平台默认终端）。 */
export function openInTerminal(
  path: string,
  kind: TerminalKind = "system",
): Promise<void> {
  return invoke<void>("open_in_terminal", { path, kind });
}

/** 在指定 IDE 打开仓库目录 / 文件 / worktree 目录。 */
export function openInIde(path: string, ide: IdeKind): Promise<void> {
  return invoke<void>("open_in_ide", { path, ide });
}

/** 当前平台可用的终端 / IDE 列表（用于渲染菜单，避免必失败项）。 */
export function listIntegrationTargets(): Promise<IntegrationTargets> {
  return invoke<IntegrationTargets>("list_integration_targets");
}
