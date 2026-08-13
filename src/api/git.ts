import { invoke } from "@tauri-apps/api/core";
import type { FileDiff } from "@/types/git";

/** Diff rendering options (Roadmap §9 diff settings). */
export interface DiffOptions {
  ignoreWhitespace: boolean;
  ignoreWhitespaceEol: boolean;
  ignoreCase: boolean;
}

export function getDiff(
  repoPath: string,
  options?: DiffOptions,
): Promise<FileDiff[]> {
  return invoke<FileDiff[]>("get_diff", { repoPath, options });
}
