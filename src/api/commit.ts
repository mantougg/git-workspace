import { invoke } from "@tauri-apps/api/core";
import type { CommitIdentity, CommitScanFinding } from "@/types/commit";

/**
 * Pre-commit safety scan (T-11): returns forbidden/large-file/secret
 * findings for the paths that would be committed, without committing.
 */
export function scanCommit(
  repoPath: string,
  files: string[],
  indexOnly: boolean,
): Promise<CommitScanFinding[]> {
  return invoke<CommitScanFinding[]>("scan_commit", {
    repoPath,
    files,
    indexOnly,
  });
}

/** Resolved commit identity for a repo (null = git default signature). */
export function getCommitIdentity(
  repoPath: string,
): Promise<CommitIdentity | null> {
  return invoke<CommitIdentity | null>("get_commit_identity", { repoPath });
}

/** Set or clear (both null) the per-repo commit identity override. */
export function setRepoIdentity(
  repoPath: string,
  name: string | null,
  email: string | null,
): Promise<void> {
  return invoke<void>("set_repo_identity", { repoPath, name, email });
}

/** Set or clear (both null) the per-group commit identity override. */
export function setGroupIdentity(
  groupId: number,
  name: string | null,
  email: string | null,
): Promise<void> {
  return invoke<void>("set_group_identity", { groupId, name, email });
}
