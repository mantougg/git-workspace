import { invoke } from "@tauri-apps/api/core";
import type { DryRunItem } from "@/types/batch";

/**
 * Selector query over workspace repositories (T-20 §52):
 * `@group:x` / `@tag:y` / `@status:dirty|clean|conflict|ahead|behind|favorite`
 * and plain-text name tokens, ANDed. Returns matching repo paths.
 */
export function selectRepos(
  workspaceId: number,
  query: string,
): Promise<string[]> {
  return invoke<string[]>("select_repos", { workspaceId, query });
}

/** Bulk branch operation (T-20): checkout / create / delete per repo. */
export function batchBranchOp(
  repoPaths: string[],
  op: "checkout" | "create" | "delete",
  name: string,
  force: boolean,
): Promise<string[]> {
  return invoke<string[]>("batch_branch_op", { repoPaths, op, name, force });
}

/**
 * Dry-run Pull/Push impact report (T-20): local-only computation, no repo
 * mutation. `op` is "pull" | "push".
 */
export function batchDryRun(
  repoPaths: string[],
  op: "pull" | "push",
): Promise<DryRunItem[]> {
  return invoke<DryRunItem[]>("batch_dry_run", { repoPaths, op });
}
