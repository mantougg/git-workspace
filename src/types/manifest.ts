/** Workspace Manifest（T-33）相关 IPC 类型，与 core/manifest.rs 对齐。 */

/** Manifest 中的单个仓库条目；path 为相对 workspace 根的路径（`/` 分隔）。 */
export interface ManifestRepo {
  path: string;
  name: string;
  /** 无 remote 的本地仓库为 null，导入时不可克隆。 */
  remoteUrl: string | null;
  defaultBranch: string | null;
  /** 分组名（来自 repo_groups），仅作元数据。 */
  group: string | null;
  tags: string[];
}

/** `gitworkspace.json` 文档结构。 */
export interface WorkspaceManifest {
  version: number;
  name: string;
  exportedAt: string;
  repositories: ManifestRepo[];
}

/** 导入预览中单个仓库的处理动作。 */
export type CloneAction = "clone" | "skipExisting" | "noUrl";

export interface ClonePlanItem {
  path: string;
  name: string;
  remoteUrl: string | null;
  defaultBranch: string | null;
  group: string | null;
  tags: string[];
  /** 绝对克隆目标路径（workspace 根 + 相对路径）。 */
  destPath: string;
  action: CloneAction;
}

/** 导入预览汇总：将克隆 N / 已存在跳过 M / 无 URL 不可克隆 K。 */
export interface ClonePlan {
  workspaceRoot: string;
  toClone: number;
  skipExisting: number;
  noUrl: number;
  items: ClonePlanItem[];
}
