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
