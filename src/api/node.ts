import { invoke } from "@tauri-apps/api/core"
import type {
  NodeExecutable,
  NodeExecutableRequest,
  NodeInstallRequest,
  NodeProjectNode,
  NodeScanCandidate,
} from "@/types/node"

/** Discover and index package.json files in a workspace. */
export function nodeListProjects(workspaceId: number): Promise<NodeProjectNode[]> {
  return invoke<NodeProjectNode[]>("node_list_projects", { workspaceId })
}

export function nodeListExecutables(): Promise<NodeExecutable[]> {
  return invoke<NodeExecutable[]>("node_list_executables")
}

export function nodeAddExecutable(request: NodeExecutableRequest): Promise<NodeExecutable> {
  return invoke<NodeExecutable>("node_add_executable", { request })
}

export function nodeValidateExecutable(id: number): Promise<NodeExecutable> {
  return invoke<NodeExecutable>("node_validate_executable", { id })
}

export function nodeRemoveExecutable(id: number): Promise<void> {
  return invoke("node_remove_executable", { id })
}

export function nodePruneExecutables(): Promise<number> {
  return invoke<number>("node_prune_executables")
}

/** N-10：扫描本机 node / 包管理器候选（只读发现，登记走 nodeAddExecutable）。 */
export function nodeScanExecutables(): Promise<NodeScanCandidate[]> {
  return invoke<NodeScanCandidate[]>("node_scan_executables")
}

export function nodeInstall(request: NodeInstallRequest): Promise<string> {
  return invoke<string>("node_install", { request })
}
