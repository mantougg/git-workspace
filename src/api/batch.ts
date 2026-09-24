import { invoke } from "@tauri-apps/api/core";
import type { DivergedFollowupItem, DivergedStrategy, DryRunItem } from "@/types/batch";

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

/**
 * GF-15：批量分叉跟进——对 dry-run / 失败汇总里识别出的分叉仓库按策略执行
 * （merge 建合并提交 / rebase 变基本地提交 / --ff-only 重试）。后端逐仓
 * fetch + 复判分叉后落地，逐仓返回结果（部分完成语义：冲突/失败不阻断其余
 * 仓库）。执行输出镜像到 Git Console（取消入口同 GF-07 单仓网络操作）。
 */
export function batchFollowupDiverged(
  repoPaths: string[],
  strategy: DivergedStrategy,
): Promise<DivergedFollowupItem[]> {
  return invoke<DivergedFollowupItem[]>("batch_followup_diverged", {
    repoPaths,
    strategy,
    opId: null,
  });
}
