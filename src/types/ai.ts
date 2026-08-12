export interface ReviewResult {
  summary: string;
  issues: ReviewIssue[];
}

export interface ReviewIssue {
  severity: string;
  category: string;
  file: string;
  description: string;
}

export interface SearchResult {
  repoPath: string;
  filePath: string;
  snippet: string;
  rank: number;
}
