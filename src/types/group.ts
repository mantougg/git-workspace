export interface RepoGroup {
  id: number;
  workspaceId: number;
  name: string;
  parentId: number | null;
  sortOrder: number;
}

export interface CreateGroupRequest {
  workspaceId: number;
  name: string;
  parentId: number | null;
}
