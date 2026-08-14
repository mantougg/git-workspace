import { invoke } from "@tauri-apps/api/core";
import type { BranchOverview, CompareResult } from "@/types/branch";

/** List local / remote branches and tags (also persists a snapshot to SQLite). */
export function listBranches(repoPath: string): Promise<BranchOverview> {
  return invoke<BranchOverview>("list_branches", { repoPath });
}

export function createBranch(
  repoPath: string,
  name: string,
  target?: string,
): Promise<void> {
  return invoke<void>("create_branch", {
    repoPath,
    name,
    target: target ?? null,
  });
}

export function checkoutBranch(repoPath: string, name: string): Promise<void> {
  return invoke<void>("checkout_branch", { repoPath, name });
}

export function deleteBranch(
  repoPath: string,
  name: string,
  force?: boolean,
): Promise<void> {
  return invoke<void>("delete_branch", {
    repoPath,
    name,
    force: force ?? null,
  });
}

export function renameBranch(
  repoPath: string,
  oldName: string,
  newName: string,
): Promise<void> {
  return invoke<void>("rename_branch", { repoPath, oldName, newName });
}

/** Set or clear (omit `upstream`) the upstream of a local branch. */
export function setUpstream(
  repoPath: string,
  branchName: string,
  upstream?: string,
): Promise<void> {
  return invoke<void>("set_upstream", {
    repoPath,
    branchName,
    upstream: upstream ?? null,
  });
}

/** Create a local branch tracking the given remote branch (e.g. "origin/feature"). */
export function trackRemoteBranch(
  repoPath: string,
  remoteBranch: string,
): Promise<void> {
  return invoke<void>("track_remote_branch", { repoPath, remoteBranch });
}

/** Push a specific local branch; returns the git command output. */
export function pushBranch(repoPath: string, branch: string): Promise<string> {
  return invoke<string>("push_branch", { repoPath, branch });
}

/** Compare two revisions: commit差集 in both directions + tree diff. */
export function compareBranches(
  repoPath: string,
  base: string,
  other: string,
): Promise<CompareResult> {
  return invoke<CompareResult>("compare_branches", { repoPath, base, other });
}
