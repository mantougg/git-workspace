import type { FileDiff, Hunk } from "@/types/git";

/**
 * IntelliJ 风格的行状态精化：git 原始 diff 只有 add / delete / context，
 * 一段 `-` 紧跟一段 `+` 实际是「修改」，孤立的 `+` 是「新增」，
 * 孤立的 `-` 是「删除」。行号栏色条与全文视图的状态叠加都用它。
 */
export type RefinedLineStatus = "added" | "modified" | "deleted" | "context";

/** 精化单个 hunk 内每一行的状态（下标与 hunk.lines 对齐）。 */
export function refineHunkLines(hunk: Hunk): RefinedLineStatus[] {
  const out: RefinedLineStatus[] = [];
  const lines = hunk.lines;
  for (let i = 0; i < lines.length; i++) {
    const type = lines[i].lineType;
    if (type === "add") {
      out.push(lines[i - 1]?.lineType === "delete" ? "modified" : "added");
    } else if (type === "delete") {
      out.push(lines[i + 1]?.lineType === "add" ? "modified" : "deleted");
    } else {
      out.push("context");
    }
  }
  return out;
}

export interface FileLineStatus {
  /** 新文件行号（1 起）→ added / modified。 */
  status: Map<number, "added" | "modified">;
  /** 新文件行号（1 起）→ 紧邻其前的已删除行数（行号栏红色标记用）。 */
  deletedBefore: Map<number, number>;
  /** 文件末尾被删除、没有后续新行可挂靠的行数。 */
  deletedAtEnd: number;
}

/** 把一个文件的 diff hunks 投影到全文行号上，供「查看整个文件」叠加状态。 */
export function buildFileLineStatus(file: FileDiff): FileLineStatus {
  const status = new Map<number, "added" | "modified">();
  const deletedBefore = new Map<number, number>();
  let pendingDeletes = 0;
  for (const hunk of file.hunks) {
    const refined = refineHunkLines(hunk);
    hunk.lines.forEach((line, i) => {
      if (refined[i] === "deleted") {
        pendingDeletes++;
        return;
      }
      if (line.newLine == null) return;
      if (refined[i] === "added" || refined[i] === "modified") {
        status.set(line.newLine, refined[i]);
      }
      if (pendingDeletes > 0) {
        deletedBefore.set(
          line.newLine,
          (deletedBefore.get(line.newLine) ?? 0) + pendingDeletes,
        );
        pendingDeletes = 0;
      }
    });
  }
  return { status, deletedBefore, deletedAtEnd: pendingDeletes };
}
