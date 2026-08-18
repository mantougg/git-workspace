/** Spring Boot application discovery payloads (R-06). */

export interface SpringBootCandidate {
  className: string
  simpleName: string
  module: string
  sourcePath: string
}

export interface SpringBootProject {
  projectPath: string
  module: string
  springBootPlugin: boolean
  springBootDependency: boolean
  isSpringBoot: boolean
  candidates: SpringBootCandidate[]
  defaultMainClass: string | null
  sourceFilesScanned: number
  sourceScanTruncated: boolean
}

export interface SpringBootWorkspaceResult {
  projects: SpringBootProject[]
  elapsedMs: number
}
