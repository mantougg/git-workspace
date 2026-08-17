/** Pre-commit safety finding (T-11, §5): forbidden / large file / secret. */
export interface CommitScanFinding {
  path: string;
  /** "forbidden" | "large_file" | "secret" */
  kind: string;
  detail: string;
}

/** Resolved commit identity (T-11 §54): repo > group > git default. */
export interface CommitIdentity {
  name: string;
  email: string;
  /** "repo" | "group" | "mixed" */
  source: string;
}
