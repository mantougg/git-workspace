import { invoke } from "@tauri-apps/api/core";
import type { CreateGroupRequest, RepoGroup } from "@/types/group";

export function listGroups(workspaceId: number): Promise<RepoGroup[]> {
  return invoke<RepoGroup[]>("list_groups", { workspaceId });
}

export function createGroup(req: CreateGroupRequest): Promise<RepoGroup> {
  return invoke<RepoGroup>("create_group", { req });
}

export function deleteGroup(id: number): Promise<void> {
  return invoke<void>("delete_group", { id });
}

export function assignGroup(
  repoPath: string,
  groupId: number | null,
): Promise<void> {
  return invoke<void>("assign_group", {
    repoPath,
    groupId,
  });
}
