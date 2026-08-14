/** Outcome of a merge operation (T-15). */
export type MergeOutcome =
  | { status: "upToDate" }
  | { status: "fastForward"; to: string }
  | { status: "merged"; commitOid: string }
  | { status: "squashed" }
  | {
      status: "conflict";
      files: string[];
      /** HEAD before the merge (abort target hint). */
      baseOid: string | null;
    };
