import { invoke } from "@tauri-apps/api/core";
import type { ReviewResult, SearchResult } from "@/types/ai";

export function aiReview(
  repoPath: string,
  apiKey: string,
  apiUrl?: string,
): Promise<ReviewResult> {
  return invoke<ReviewResult>("ai_review", {
    repoPath,
    apiKey,
    apiUrl,
  });
}

export function buildCodeIndex(repoPath: string): Promise<void> {
  return invoke<void>("build_code_index", { repoPath });
}

export function aiSearch(query: string): Promise<SearchResult[]> {
  return invoke<SearchResult[]>("ai_search", { query });
}

export function clearCodeIndex(repoPath: string): Promise<void> {
  return invoke<void>("clear_code_index", { repoPath });
}
