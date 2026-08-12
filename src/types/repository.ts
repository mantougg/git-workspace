export interface Repository {
  id: number | null;
  workspaceId: number;
  path: string;
  name: string;
  relativePath: string;
  isFavorite: boolean;
  tags: string[];
  groupId: number | null;
}

export interface RepoStatus {
  branch: string;
  isDetached: boolean;
  ahead: number;
  behind: number;
  modified: number;
  added: number;
  deleted: number;
  untracked: number;
  staged: number;
  isClean: boolean;
}

export interface RepositoryWithStatus {
  repository: Repository;
  status: RepoStatus | null;
  lastError: string | null;
}
