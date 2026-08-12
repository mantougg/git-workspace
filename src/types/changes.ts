/** A single changed file within a repository. */
export interface FileChange {
  /** Path relative to the repository root, using `/` separators. */
  path: string;
  /** `untracked` | `modified` | `deleted` | `added` | `renamed` | `typechange` */
  status: string;
}

/** File-level change summary for one repository. */
export interface RepoChanges {
  repoPath: string;
  repoName: string;
  /** Path relative to the workspace root, used to build the directory tree. */
  relativePath: string;
  branch: string;
  isDetached: boolean;
  ahead: number;
  behind: number;
  /** Sorted changed files; empty means clean. */
  changes: FileChange[];
}
