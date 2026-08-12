import { invoke } from "@tauri-apps/api/core";
import type { FileDiff } from "@/types/git";

export function getDiff(repoPath: string): Promise<FileDiff[]> {
  return invoke<FileDiff[]>("get_diff", { repoPath });
}
