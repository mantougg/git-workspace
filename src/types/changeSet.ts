/** Workspace Change Set (T-22) IPC types — mirror of `core/change_set.rs`. */

export interface ChangeSet {
  id: number;
  workspaceId: number;
  name: string;
  description: string | null;
  createdAt: string;
  updatedAt: string;
}

export interface ChangeSetRepo {
  changeSetId: number;
  repoId: number;
  repoPath: string;
  repoName: string;
  relativePath: string;
  targetBranch: string | null;
}

export interface ChangeSetRepoInput {
  repoId: number;
  targetBranch?: string | null;
}

export interface CreateChangeSetRequest {
  workspaceId: number;
  name: string;
  description?: string | null;
  repos?: ChangeSetRepoInput[];
}

export interface UpdateChangeSetRequest {
  id: number;
  name?: string | null;
  description?: string | null;
}

export interface ChangeSetRepoSummary {
  repo: ChangeSetRepo;
  currentBranch: string | null;
  /** Unpushed commits on the current branch (T-02 status cache). */
  ahead: number;
  behind: number;
  files: number;
  added: number;
  deleted: number;
  error: string | null;
}

export interface ChangeSetSummary {
  changeSet: ChangeSet;
  repositories: number;
  files: number;
  added: number;
  deleted: number;
  /** Total unpushed commits across member repos. */
  commits: number;
  repos: ChangeSetRepoSummary[];
}
