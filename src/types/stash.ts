/** A single stash entry, index 0 = most recent (T-10). */
export interface StashEntry {
  /** Position in the stash stack (the N in `stash@{N}`). */
  index: number;
  /** Stash commit oid. */
  oid: string;
  /** Full message, e.g. "On master: my message" / "WIP on ...". */
  message: string;
  /** Stash creation time. */
  time: string;
}
