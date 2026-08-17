import { invoke } from "@tauri-apps/api/core";
import type { ClonePlan, WorkspaceManifest } from "@/types/manifest";

/** 导出当前工作区为 Manifest 并写入 `filePath`（经保存对话框选择）。 */
export function exportWorkspaceManifest(
  workspaceId: number,
  filePath: string,
): Promise<WorkspaceManifest> {
  return invoke<WorkspaceManifest>("export_workspace_manifest", {
    workspaceId,
    filePath,
  });
}

/** 读取并校验 Manifest 文件（经打开对话框选择）。 */
export function readManifestFile(filePath: string): Promise<WorkspaceManifest> {
  return invoke<WorkspaceManifest>("read_manifest_file", { filePath });
}

/** 计算导入预览 / 克隆计划（将克隆 / 已存在跳过 / 无 URL）。 */
export function planManifestClone(
  manifest: WorkspaceManifest,
  workspaceRoot: string,
): Promise<ClonePlan> {
  return invoke<ClonePlan>("plan_manifest_clone", {
    manifest,
    workspaceRoot,
  });
}
