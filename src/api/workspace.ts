import { invoke } from "@tauri-apps/api/core";
import type {
  CreateWorkspaceRequest,
  UpdateWorkspaceRequest,
  Workspace,
} from "@/types/workspace";

export function addWorkspace(
  req: CreateWorkspaceRequest,
): Promise<Workspace> {
  return invoke<Workspace>("add_workspace", { req });
}

export function listWorkspaces(): Promise<Workspace[]> {
  return invoke<Workspace[]>("list_workspaces");
}

export function removeWorkspace(id: number): Promise<void> {
  return invoke<void>("remove_workspace", { id });
}

export function updateWorkspace(
  id: number,
  req: UpdateWorkspaceRequest,
): Promise<Workspace> {
  return invoke<Workspace>("update_workspace", { id, req });
}
