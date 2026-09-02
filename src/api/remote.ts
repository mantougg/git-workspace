import { invoke } from "@tauri-apps/api/core";

export interface RemoteInfo {
  platform: string;
  host: string;
  owner: string;
  repo: string;
  url: string;
}

export interface RemotePrResult {
  number: number;
  url: string;
}

export interface RemoteCiStatus {
  state: string;
  url: string;
}

/** 平台识别（读 origin）。无 origin / 无法识别 → 抛错。 */
export function detectRemote(repoPath: string): Promise<RemoteInfo> {
  return invoke<RemoteInfo>("detect_remote", { repoPath });
}

/**
 * 构造 Open URL（前端经 shell plugin 打开浏览器）。
 * target：repo / issues / pulls / ci / new-pr:source..target / pull:7 / issue:3
 */
export function remoteOpenUrl(repoPath: string, target: string): Promise<string> {
  return invoke<string>("remote_open_url", { repoPath, target });
}

export function createPullRequest(input: {
  repoPath: string;
  source: string;
  target: string;
  title: string;
  body: string;
  draft: boolean;
}): Promise<RemotePrResult> {
  return invoke<RemotePrResult>("create_pull_request", { ...input });
}

export function getCiStatus(repoPath: string, gitRef: string): Promise<RemoteCiStatus> {
  return invoke<RemoteCiStatus>("get_ci_status", { repoPath, gitRef });
}

/** 解析 token（keyring → 系统 git credential），不落盘明文。 */
export function resolveRemoteToken(platform: string, host: string): Promise<string | null> {
  return invoke<string | null>("resolve_remote_token", { platform, host });
}

export function saveRemoteToken(platform: string, host: string, token: string): Promise<void> {
  return invoke<void>("save_remote_token", { platform, host, token });
}

export function deleteRemoteToken(platform: string, host: string): Promise<void> {
  return invoke<void>("delete_remote_token", { platform, host });
}
