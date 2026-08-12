export interface Workspace {
  id: number;
  name: string;
  path: string;
  scanDepth: number;
  createdAt: string;
  updatedAt: string;
}

export interface CreateWorkspaceRequest {
  name: string;
  path: string;
  scanDepth?: number;
}

export interface UpdateWorkspaceRequest {
  name?: string;
  scanDepth?: number;
}
