import { invoke } from "@tauri-apps/api/core"
import type { NodeProjectNode } from "@/types/node"

/** Discover and index package.json files in a workspace. */
export function nodeListProjects(workspaceId: number): Promise<NodeProjectNode[]> {
  return invoke<NodeProjectNode[]>("node_list_projects", { workspaceId })
}
