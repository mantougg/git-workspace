import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import type { JdkInstallation } from "@/types/jdk";

/**
 * R-04 JDK Manager API。
 * 所有调用走 Tauri IPC，对应 src-tauri/src/commands/jdk.rs 的 #[tauri::command]。
 */

/** 触发本机 JDK 多来源发现并批量 upsert。返回发现并入库的数量。 */
export function discoverJdks(): Promise<number> {
  return invoke<number>("discover_jdks");
}

/** 列出注册表全部 JDK。 */
export function listJdks(): Promise<JdkInstallation[]> {
  return invoke<JdkInstallation[]>("list_jdks");
}

/** 按 id 取单条 JDK。 */
export function getJdk(id: number): Promise<JdkInstallation | null> {
  return invoke<JdkInstallation | null>("get_jdk", { id });
}

/**
 * 手动添加 JDK：弹出目录选择器，校验通过才入库。
 * 无效路径由后端返回 JdkNotFound 可行动错误（前端在此捕获并展示）。
 */
export async function addJdkManualByPicker(): Promise<JdkInstallation | null> {
  const selected = await open({ directory: true, multiple: false, title: "选择 JDK 根目录" });
  if (typeof selected !== "string" || !selected) {
    return null;
  }
  return invoke<JdkInstallation>("add_jdk_manual", { homePath: selected });
}

/** 手动添加 JDK（直接传路径，供输入框使用）。 */
export function addJdkManual(homePath: string): Promise<JdkInstallation> {
  return invoke<JdkInstallation>("add_jdk_manual", { homePath });
}

/** 强制复检单条 JDK。返回更新后的条目。 */
export function validateJdk(id: number): Promise<JdkInstallation> {
  return invoke<JdkInstallation>("validate_jdk", { id });
}

/** 惰性校验：把 home 已不存在的条目标记失效。返回被标记的条数。 */
export function pruneInvalidJdks(): Promise<number> {
  return invoke<number>("prune_invalid_jdks");
}

/** 按 id 删除单条 JDK。 */
export function removeJdk(id: number): Promise<void> {
  return invoke<void>("remove_jdk", { id });
}
