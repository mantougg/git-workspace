/** T-28 符号索引 IPC 类型（与 Rust serde camelCase 对齐） */

export interface SymbolIndexStats {
  filesScanned: number;
  filesReindexed: number;
  filesSkipped: number;
  symbols: number;
  refs: number;
}

export interface SymbolHit {
  repoPath: string;
  filePath: string;
  name: string;
  kind: string;
  line: number;
  endLine: number | null;
  container: string | null;
  signature: string | null;
}

export interface SymbolRefHit {
  repoPath: string;
  filePath: string;
  name: string;
  line: number;
  isCall: boolean;
}

export interface SymbolCallHit {
  name: string;
  repoPath: string;
  filePath: string;
  line: number;
  kind: string;
  callCount: number;
}
