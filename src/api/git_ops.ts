import { invoke } from "@tauri-apps/api/core";
import type { CommitRequest } from "@/types/task";
import type { RepoStatus } from "@/types/repository";
import type { SmartPullResult } from "@/types/git_ops";

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

export function syncFetch(repoPath: string, opId?: string): Promise<void> {
  return invoke<void>("sync_fetch", { repoPath, opId: opId ?? null });
}

export function syncPull(repoPath: string, opId?: string): Promise<RepoStatus> {
  return invoke<RepoStatus>("sync_pull", { repoPath, opId: opId ?? null });
}

export function smartPull(repoPath: string, opId?: string): Promise<SmartPullResult> {
  return invoke<SmartPullResult>("smart_pull", { repoPath, opId: opId ?? null });
}

export function syncPush(repoPath: string, opId?: string): Promise<void> {
  return invoke<void>("sync_push", { repoPath, opId: opId ?? null });
}

/**
 * GF-07：取消一个进行中的单仓网络操作。
 *
 * `opId` 来自调用方传入或 `git_op_started` 事件（见 GIT_OP_EVENTS）。
 * op 已结束时后端返回 NotFound——取消入口通常已撤下，调用方可忽略。
 */
export function cancelGitOp(opId: string): Promise<void> {
  return invoke<void>("cancel_git_op", { opId });
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
