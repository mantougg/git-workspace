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

/**
 * Push a specific local branch; returns the git command output.
 *
 * GF-07：`opId` 可选——传入后该次 push 可经 `cancelGitOp` 取消；缺省时后端
 * 自行生成并经 `git_op_started` 事件下发（Git Console 的取消入口用它）。
 */
export function pushBranch(
  repoPath: string,
  branch: string,
  opId?: string,
): Promise<string> {
  return invoke<string>("push_branch", { repoPath, branch, opId: opId ?? null });
}

/** Compare two revisions: commit差集 in both directions + tree diff. */
export function compareBranches(
  repoPath: string,
  base: string,
  other: string,
): Promise<CompareResult> {
  return invoke<CompareResult>("compare_branches", { repoPath, base, other });
}

/**
 * Create a tag (GF-04). `target` defaults to HEAD; passing a `message`
 * creates an annotated tag, omitting it a lightweight one.
 */
export function createTag(
  repoPath: string,
  name: string,
  message?: string,
  target?: string,
): Promise<void> {
  return invoke<void>("create_tag", {
    repoPath,
    name,
    message: message ?? null,
    target: target ?? null,
  });
}

/** Delete a local tag; a remote copy (if any) is left untouched. */
export function deleteTag(repoPath: string, name: string): Promise<void> {
  return invoke<void>("delete_tag", { repoPath, name });
}

/**
 * Push a tag to the default remote; returns the git command output.
 *
 * GF-04：`force` / `forceWithLease` 默认 false（Roadmap §47：force push 默认禁用、
 * 用户显式开启，`--force-with-lease` 为推荐方案）——git 本身拒绝覆盖远程已有标签。
 */
export function pushTag(
  repoPath: string,
  name: string,
  force?: boolean,
  forceWithLease?: boolean,
  opId?: string,
): Promise<string> {
  return invoke<string>("push_tag", {
    repoPath,
    name,
    force: force ?? null,
    forceWithLease: forceWithLease ?? null,
    opId: opId ?? null,
  });
}

/**
 * Whether a tag already exists on the remote (GF-04). Backed by
 * `git ls-remote --tags`; when the network is unavailable it falls back to
 * local remote-tracking refs, which can only under-report.
 */
export function tagPushedToRemote(
  repoPath: string,
  name: string,
  opId?: string,
): Promise<boolean> {
  return invoke<boolean>("tag_pushed_to_remote", {
    repoPath,
    name,
    opId: opId ?? null,
  });
}
