import { invoke } from "@tauri-apps/api/core";
import type {
  RepoStatus,
  RepositoryWithStatus,
} from "@/types/repository";

export function scanRepositories(
  workspaceId: number,
): Promise<RepositoryWithStatus[]> {
  return invoke<RepositoryWithStatus[]>("scan_repositories", {
    workspaceId,
  });
}

export function listRepositories(
  workspaceId: number,
): Promise<RepositoryWithStatus[]> {
  return invoke<RepositoryWithStatus[]>("list_repositories", {
    workspaceId,
  });
}

export function refreshRepositoryStatus(
  repoPath: string,
): Promise<RepoStatus> {
  return invoke<RepoStatus>("refresh_repository_status", {
    repoPath,
  });
}
