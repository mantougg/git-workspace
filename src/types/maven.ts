type DependencyScope = "compile" | "provided" | "runtime" | "test" | "system" | "import"
type DependencySource = "workspaceSource" | "localRepository" | "remoteRepository"
type ResolutionReason =
  | "workspaceExactMatch"
  | "localArtifactExists"
  | "remoteArtifactMissingLocally"
  | "versionNotExactForSource"
  | "workspaceVersionMismatch"
  | "ambiguousWorkspaceCoordinate"
  | "missingVersion"

interface MavenCoordinates {
  groupId: string
  artifactId: string
  version: string
}

interface MavenParent extends MavenCoordinates {
  relativePath: string | null
}

interface MavenDependency {
  groupId: string
  artifactId: string
  version: string | null
  scope: DependencyScope
  optional: boolean
  depType: string
  classifier: string | null
  exclusions: MavenCoordinates[]
}

interface MavenProfile {
  id: string
  properties: Record<string, string>
  dependencies: MavenDependency[]
}

interface MavenPlugin {
  groupId: string
  artifactId: string
  version: string | null
}

export interface MavenProject {
  path: string
  groupId: string
  artifactId: string
  version: string
  packaging: string
  parent: MavenParent | null
  modules: Array<{ path: string }>
  dependencies: MavenDependency[]
  dependencyManagement: MavenDependency[]
  profiles: MavenProfile[]
  properties: Record<string, string>
  plugins: MavenPlugin[]
  fileHash: string
}

interface MavenProjectNode {
  projectId: number
  repositoryId: number | null
  path: string
  coordinates: MavenCoordinates
  packaging: string
  pomHash: string
}

interface MavenModuleLink {
  parentProjectId: number
  moduleProjectId: number | null
  declaredPath: string
}

interface SourceMapping {
  coordinates: MavenCoordinates
  repositoryId: number | null
  projectId: number
  projectPath: string
}

interface DependencyEdge {
  dependencyId: number
  fromProjectId: number
  dependency: MavenDependency
  source: DependencySource
  sourceProjectId: number | null
  resolvedPath: string | null
  reason: ResolutionReason
}

export interface DependencyGraph {
  workspaceId: number
  fingerprint: string
  projects: MavenProjectNode[]
  dependencies: DependencyEdge[]
  modules: MavenModuleLink[]
  sourceMappings: SourceMapping[]
}

type RuntimeScopeMode = "auto" | "manual" | "hybrid"

export type RuntimeScope =
  | { mode: "auto" }
  | { mode: "manual"; projectIds: number[] }
  | { mode: "hybrid"; includeProjectIds: number[]; excludeProjectIds: number[] };

export interface RuntimeClosure {
  workspaceId: number
  rootProjectId: number
  graphFingerprint: string
  mode: RuntimeScopeMode
  projects: MavenProjectNode[]
}

type RuntimeReactorKind = "existing" | "synthetic"

export interface RuntimeReactorPlan {
  kind: RuntimeReactorKind
  pomPath: string
  modulePaths: string[]
  arguments: string[]
}

// ── R-05 Maven 检测与执行策略 ────────────────────────────────────
// Source of truth: Rust maven/exec_model.rs (MavenSource / MavenVersionInfo /
// MavenExecutable / ResolvedMaven / MavenExecutionRequest). serde uses camelCase.

/** Maven 可执行体来源（§18 优先级链）。 */
export type MavenSource = "projectWrapper" | "configured" | "system";

/** `mvn -v` 解析出的版本信息。 */
export interface MavenVersionInfo {
  majorVersion?: number
  fullVersion?: string
  raw: string
}

/** 一个已检测的 Maven 可执行体。 */
export interface MavenExecutable {
  id?: number
  executablePath: string
  source: MavenSource
  /** wrapper 所属项目路径（非 wrapper 为 null）。 */
  projectPath?: string | null
  majorVersion?: number
  fullVersion?: string
  isValid: boolean
  lastChecked: string
  rawVersion?: string
  createdAt?: string
  updatedAt?: string
}

/** 为项目解析出的最终生效 Maven（§18 优先级链结果）。 */
export interface ResolvedMaven {
  executable: MavenExecutable
  localRepository: string
  usesWrapper: boolean
}

/** Maven 执行请求（供 Build Engine R-09 构造命令用）。 */
export interface MavenExecutionRequest {
  workingDir: string
  executable: string
  goals: string[]
  extraArgs?: string[]
  viaCmdC: boolean
  localRepository?: string | null
}
