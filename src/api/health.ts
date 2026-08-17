import { invoke } from "@tauri-apps/api/core";
import type { RepoHealthExtra, WorkspaceHealth } from "@/types/health";

export function getWorkspaceHealth(
  workspaceId: number,
): Promise<WorkspaceHealth> {
  return invoke<WorkspaceHealth>("get_workspace_health", { workspaceId });
}

export function getHealthExtras(
  repoPaths: string[],
): Promise<RepoHealthExtra[]> {
  return invoke<RepoHealthExtra[]>("get_health_extras", { repoPaths });
}
