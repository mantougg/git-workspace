/** T-19 Workspace Health IPC types (source of truth: Rust core/health.rs). */

export interface HealthWeights {
  dirty: number;
  conflict: number;
  ahead: number;
  behind: number;
  detached: number;
  missingRemote: number;
  diverged: number;
  untracked: number;
  largeFiles: number;
  lfsError: number;
  submoduleError: number;
}

export interface RepoHealth {
  repoPath: string;
  repoName: string;
  branch: string;
  anomalies: string[];
  score: number;
}

export interface WorkspaceHealth {
  score: number;
  total: number;
  anomalous: number;
  repos: RepoHealth[];
  weights: HealthWeights;
}

export interface RepoHealthExtra {
  repoPath: string;
  largeFiles: number;
  largestFileBytes: number;
  lfsError: boolean;
  submoduleError: boolean;
}
