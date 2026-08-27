import { invoke } from "@tauri-apps/api/core";
import type { CommitHeatmap } from "@/types/heatmap";

/** F-01b：当前用户在 workspace 全部仓库的提交按天计数（默认 365 天）。 */
export function getCommitHeatmap(
  workspaceId: number,
  days?: number,
): Promise<CommitHeatmap> {
  return invoke<CommitHeatmap>("get_commit_heatmap", { workspaceId, days });
}
