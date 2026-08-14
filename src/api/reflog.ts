import { invoke } from "@tauri-apps/api/core";
import type { ReflogEntry } from "@/types/reflog";

/** Read a reflog (default HEAD), newest first. */
export function getReflog(
  repoPath: string,
  reference?: string,
  max?: number,
): Promise<ReflogEntry[]> {
  return invoke<ReflogEntry[]>("get_reflog", {
    repoPath,
    reference: reference ?? null,
    max: max ?? null,
  });
}
