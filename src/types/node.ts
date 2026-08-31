/** Indexed Node.js package metadata returned by `node_list_projects`. */
export interface NodeProjectNode {
  projectId: number
  repositoryId: number | null
  path: string
  name: string
  version: string
  packageManager: string | null
  /** JSON object text preserving package.json script order. */
  scriptsJson: string
}

export type NodeExecutableKind = "node" | "packageManager"
export type NodePackageManager = "npm" | "pnpm" | "yarn" | "bun"

export interface NodeExecutable {
  id: number | null
  kind: NodeExecutableKind
  packageManager: NodePackageManager | null
  executablePath: string
  version: string | null
  rawOutput: string
  isValid: boolean
  lastChecked: string
  createdAt: string | null
  updatedAt: string | null
}

export interface NodeExecutableRequest {
  kind: NodeExecutableKind
  packageManager?: NodePackageManager | null
  executablePath: string
}

export interface NodeInstallRequest {
  projectDir: string
  packageManager: NodePackageManager
  confirmed?: boolean
}
