import { invoke } from "@tauri-apps/api/core";
import type {
  ChangeSet,
  ChangeSetRepo,
  ChangeSetRepoInput,
  ChangeSetSummary,
  CreateChangeSetRequest,
  UpdateChangeSetRequest,
} from "@/types/changeSet";

export function listChangeSets(workspaceId: number): Promise<ChangeSet[]> {
  return invoke<ChangeSet[]>("list_change_sets", { workspaceId });
}

export function createChangeSet(
  req: CreateChangeSetRequest,
): Promise<ChangeSet> {
  return invoke<ChangeSet>("create_change_set", { req });
}

export function updateChangeSet(
  req: UpdateChangeSetRequest,
): Promise<ChangeSet> {
  return invoke<ChangeSet>("update_change_set", { req });
}

export function deleteChangeSet(id: number): Promise<void> {
  return invoke<void>("delete_change_set", { id });
}

export function getChangeSetSummary(id: number): Promise<ChangeSetSummary> {
  return invoke<ChangeSetSummary>("get_change_set_summary", { id });
}

/** Add repos (or update target branches); returns the full membership. */
export function addChangeSetRepositories(
  changeSetId: number,
  repos: ChangeSetRepoInput[],
): Promise<ChangeSetRepo[]> {
  return invoke<ChangeSetRepo[]>("add_change_set_repositories", {
    changeSetId,
    repos,
  });
}

/** Remove one repo; returns the remaining membership. */
export function removeChangeSetRepository(
  changeSetId: number,
  repoId: number,
): Promise<ChangeSetRepo[]> {
  return invoke<ChangeSetRepo[]>("remove_change_set_repository", {
    changeSetId,
    repoId,
  });
}
