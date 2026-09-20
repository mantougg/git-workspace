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

/** Full working-directory file content (「查看整个文件」模式，后端 read_workdir_file)。
 *  每行的 git 状态由前端从 diff hunks 叠加，不由后端计算。 */
export interface WorkdirFile {
  totalLines: number;
  lines: string[];
}
