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
