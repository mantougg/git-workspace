import { invoke } from "@tauri-apps/api/core";

// ── Submodule ────────────────────────────────────────────────

export interface SubmoduleEntry {
  path: string;
  sha: string;
  status: "synced" | "modified" | "uninitialized" | "conflict" | "unknown";
  url: string | null;
  branch: string | null;
}

export function listSubmodules(repoPath: string): Promise<SubmoduleEntry[]> {
  return invoke<SubmoduleEntry[]>("list_submodules", { repoPath });
}

/** op：init / update / sync / add（需 url）/ remove。 */
export function submoduleOp(
  repoPath: string,
  op: string,
  path?: string,
  url?: string,
): Promise<string> {
  return invoke<string>("submodule_op", {
    repoPath,
    op,
    path: path ?? null,
    url: url ?? null,
  });
}

// ── Git LFS ──────────────────────────────────────────────────

export interface LfsFile {
  path: string;
  state: "synced" | "pointer" | "dirty";
}

export function lfsList(repoPath: string): Promise<LfsFile[]> {
  return invoke<LfsFile[]>("lfs_list", { repoPath });
}

/** op：fetch / pull / push；include 仅 fetch 有效（--include 模式）。 */
export function lfsOp(repoPath: string, op: string, include?: string): Promise<string> {
  return invoke<string>("lfs_op", { repoPath, op, include: include ?? null });
}

export interface LfsLock {
  id: string;
  path: string;
  owner: string | null;
}

export function lfsLocks(repoPath: string): Promise<LfsLock[]> {
  return invoke<LfsLock[]>("lfs_locks", { repoPath });
}

export function lfsLockOp(repoPath: string, op: "lock" | "unlock", path: string): Promise<string> {
  return invoke<string>("lfs_lock_op", { repoPath, op, path });
}

// ── Git Hooks ────────────────────────────────────────────────

export interface HookInfo {
  name: string;
  exists: boolean;
  enabled: boolean;
}

export function listHooks(repoPath: string): Promise<HookInfo[]> {
  return invoke<HookInfo[]>("list_hooks", { repoPath });
}

export function getHook(repoPath: string, name: string): Promise<string> {
  return invoke<string>("get_hook", { repoPath, name });
}

export function saveHook(repoPath: string, name: string, content: string): Promise<void> {
  return invoke<void>("save_hook", { repoPath, name, content });
}

export function setHookEnabled(repoPath: string, name: string, enabled: boolean): Promise<void> {
  return invoke<void>("set_hook_enabled", { repoPath, name, enabled });
}

export interface HookRunResult {
  exitCode: number | null;
  output: string;
}

export function runHook(repoPath: string, name: string): Promise<HookRunResult> {
  return invoke<HookRunResult>("run_hook", { repoPath, name });
}
