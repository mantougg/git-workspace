/** A single reflog line, newest first (T-14). */
export interface ReflogEntry {
  /** 0-based position from the tip (the N in `HEAD@{N}`). */
  index: number;
  /** Display selector, e.g. `HEAD@{0}` or `main@{2}`. */
  selector: string;
  oldOid: string;
  newOid: string;
  /** Reflog message, e.g. "commit: add x" / "reset: moving to abc". */
  summary: string;
  /** First line of the commit the ref moved to. */
  commitMessage: string;
  time: string;
}
