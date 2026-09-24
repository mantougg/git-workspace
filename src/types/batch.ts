/** One repo's dry-run outcome in a batch Pull/Push pre-flight (T-20). */
export interface DryRunItem {
  repoPath: string;
  repoName: string;
  /**
   * "up_to_date" | "fast_forward" | "diverged" | "conflict" |
   * "no_upstream" | "error"
   */
  category: string;
  ahead: number;
  behind: number;
  detail: string;
}

/**
 * GF-15：分叉跟进策略。
 * - "merge"：创建合并提交（分叉已消失时退化为快进）
 * - "rebase"：把本地提交变基到上游 tip
 * - "ff_only"：重试 `git pull --ff-only`（远程可能已变动/强推）
 */
export type DivergedStrategy = "merge" | "rebase" | "ff_only";

/** GF-15：批量分叉跟进中单个仓库的结果（部分完成语义）。 */
export interface DivergedFollowupItem {
  repoPath: string;
  repoName: string;
  /**
   * "merged" | "rebased" | "up_to_date" | "conflict" | "failed" |
   * "skipped" | "cancelled"
   */
  outcome: string;
  /** outcome === "conflict" 时："merge" | "rebase"（驱动冲突队列走哪套 continue/abort）。 */
  conflictOp: string | null;
  /** 冲突文件列表（conflict outcome）。 */
  files: string[];
  /** 操作前的 HEAD（冲突队列的 abort 目标提示）。 */
  baseOid: string | null;
  /** rebase 成功时重放到上游的本地提交数。 */
  rewritten: number;
  /** 执行时复判的影响范围：本地 ahead / 远程 behind 提交数。 */
  ahead: number;
  behind: number;
  detail: string;
}
