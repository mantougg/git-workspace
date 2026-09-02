import { invoke } from "@tauri-apps/api/core";
import type { CommitRequest } from "@/types/task";
import type { RepoStatus } from "@/types/repository";

export const WATCHER_EVENTS = {
  statusChanged: "watcher_status_changed",
} as const;

export function batchFetch(repoPaths: string[]): Promise<string[]> {
  return invoke<string[]>("batch_fetch", { repoPaths });
}

export function batchPull(repoPaths: string[]): Promise<string[]> {
  return invoke<string[]>("batch_pull", { repoPaths });
}

export function batchPush(repoPaths: string[]): Promise<string[]> {
  return invoke<string[]>("batch_push", { repoPaths });
}

export function batchCommit(commits: CommitRequest[]): Promise<string[]> {
  return invoke<string[]>("batch_commit", { commits });
}

export function syncFetch(repoPath: string): Promise<void> {
  return invoke<void>("sync_fetch", { repoPath });
}

export function syncPull(repoPath: string): Promise<RepoStatus> {
  return invoke<RepoStatus>("sync_pull", { repoPath });
}

export function syncPush(repoPath: string): Promise<void> {
  return invoke<void>("sync_push", { repoPath });
}

export function startWatcher(repoPaths: string[]): Promise<void> {
  return invoke<void>("start_watcher", { repoPaths });
}

export function watcherStatus(): Promise<boolean> {
  return invoke<boolean>("watcher_status");
}

export function stopWatcher(): Promise<void> {
  return invoke<void>("stop_watcher");
}
