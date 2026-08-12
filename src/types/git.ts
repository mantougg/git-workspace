export interface FileDiff {
  oldPath: string;
  newPath: string;
  status: string;
  hunks: Hunk[];
}

export interface Hunk {
  oldStart: number;
  oldLines: number;
  newStart: number;
  newLines: number;
  lines: DiffLine[];
}

export interface DiffLine {
  lineType: string;
  content: string;
  oldLine: number | null;
  newLine: number | null;
}
