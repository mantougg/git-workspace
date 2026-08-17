<template>
  <VirtualList :items="rows" :item-height="ROW_HEIGHT" class="unified-diff">
    <template #row="{ item }">
      <div v-if="item.type === 'header'" class="hunk-header">
        <span class="hunk-text">{{ item.text }}</span>
        <template v-if="mode">
          <button
            class="hunk-btn"
            @click.stop="emitOp({ kind: 'hunk', hunkIndex: item.hunkIndex })"
          >
            {{ mode === "stage" ? "Stage Hunk" : "Unstage Hunk" }}
          </button>
          <button
            v-if="item.selectedCount > 0"
            class="hunk-btn primary"
            @click.stop="
              emitOp({
                kind: 'lines',
                hunkIndex: item.hunkIndex,
                lineIndices: selectedLinesOf(item.hunkIndex),
              })
            "
          >
            {{
              mode === "stage"
                ? `Stage ${item.selectedCount} 行`
                : `Unstage ${item.selectedCount} 行`
            }}
          </button>
        </template>
      </div>
      <div
        v-else
        :class="[
          'diff-line',
          item.line.lineType,
          { selectable: item.selectable, selected: item.selected },
        ]"
        @click="toggleLine(item)"
      >
        <span class="line-num old">{{ item.line.oldLine ?? "" }}</span>
        <span class="line-num new">{{ item.line.newLine ?? "" }}</span>
        <span class="line-prefix">{{ prefix(item.line.lineType) }}</span>
        <span class="line-content">{{ item.line.content }}</span>
      </div>
    </template>
  </VirtualList>
</template>

<script setup lang="ts">
import { computed, ref, watch } from "vue";
import VirtualList from "@/components/common/VirtualList.vue";
import type { DiffLine, FileDiff } from "@/types/git";

/** A staging operation requested from the diff view (T-12). */
export interface StageOp {
  kind: "hunk" | "lines";
  hunkIndex: number;
  lineIndices?: number[];
}

const props = withDefaults(
  defineProps<{
    file: FileDiff;
    /**
     * Interactive staging mode (T-12): "stage" for the unstaged view,
     * "unstage" for the staged view, null/undefined for read-only
     * (compare / commit diffs, or while Ignore options are active).
     */
    mode?: "stage" | "unstage" | null;
  }>(),
  { mode: null },
);

const emit = defineEmits<{
  op: [op: StageOp];
}>();

/** Fixed row height (px) required by VirtualList. */
const ROW_HEIGHT = 21;

type Row =
  | { type: "header"; text: string; hunkIndex: number; selectedCount: number }
  | {
      type: "line";
      line: DiffLine;
      hunkIndex: number;
      lineIndex: number;
      selectable: boolean;
      selected: boolean;
    };

/** Selected change lines, keyed `${hunkIndex}:${lineIndex}`. */
const selection = ref<Set<string>>(new Set());

// A reload (or file switch) invalidates hunk/line indices: drop the selection.
watch(
  () => props.file,
  () => selection.value.clear(),
);

// Flatten hunks into a uniform row list so a single virtual window covers
// both hunk headers and diff lines (T-04 frontend rendering budget).
const rows = computed<Row[]>(() => {
  const out: Row[] = [];
  const interactive = props.mode !== null;
  props.file.hunks.forEach((hunk, hunkIndex) => {
    const selectedCount = interactive ? countSelected(hunkIndex) : 0;
    out.push({
      type: "header",
      text: `@@ -${hunk.oldStart},${hunk.oldLines} +${hunk.newStart},${hunk.newLines} @@`,
      hunkIndex,
      selectedCount,
    });
    hunk.lines.forEach((line, lineIndex) => {
      const selectable =
        interactive &&
        (line.lineType === "add" || line.lineType === "delete");
      out.push({
        type: "line",
        line,
        hunkIndex,
        lineIndex,
        selectable,
        selected: selectable && selection.value.has(`${hunkIndex}:${lineIndex}`),
      });
    });
  });
  return out;
});

function countSelected(hunkIndex: number): number {
  let n = 0;
  for (const key of selection.value) {
    if (key.startsWith(`${hunkIndex}:`)) n++;
  }
  return n;
}

function selectedLinesOf(hunkIndex: number): number[] {
  const out: number[] = [];
  for (const key of selection.value) {
    const [h, l] = key.split(":");
    if (Number(h) === hunkIndex) out.push(Number(l));
  }
  return out.sort((a, b) => a - b);
}

function toggleLine(item: Extract<Row, { type: "line" }>) {
  if (!item.selectable) return;
  const key = `${item.hunkIndex}:${item.lineIndex}`;
  const next = new Set(selection.value);
  if (next.has(key)) {
    next.delete(key);
  } else {
    next.add(key);
  }
  selection.value = next;
}

function emitOp(op: StageOp) {
  emit("op", op);
}

function prefix(type: string): string {
  switch (type) {
    case "add":
      return "+";
    case "delete":
      return "-";
    default:
      return " ";
  }
}
</script>

<style scoped>
.unified-diff {
  font-family: "Cascadia Code", "Fira Code", Consolas, monospace;
  font-size: 13px;
}

.hunk-header {
  display: flex;
  align-items: center;
  gap: 8px;
  height: 21px;
  line-height: 21px;
  background: #f0f0f0;
  color: #909399;
  padding: 0 8px;
  font-size: 12px;
  white-space: pre;
}

.hunk-text {
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
}

.hunk-btn {
  flex-shrink: 0;
  height: 17px;
  line-height: 15px;
  padding: 0 6px;
  font-size: 11px;
  border: 1px solid #c0c4cc;
  border-radius: 3px;
  background: #fff;
  color: #606266;
  cursor: pointer;
}

.hunk-btn:hover {
  border-color: #409eff;
  color: #409eff;
}

.hunk-btn.primary {
  border-color: #409eff;
  color: #409eff;
}

.diff-line {
  display: flex;
  align-items: center;
  height: 21px;
  line-height: 21px;
  padding: 0 8px;
  white-space: pre;
}

.diff-line.add {
  background: #e6ffec;
}

.diff-line.delete {
  background: #ffebe9;
}

.diff-line.selectable {
  cursor: pointer;
}

.diff-line.selectable:hover {
  filter: brightness(0.96);
}

.diff-line.selected.add {
  background: #b7eb8f;
}

.diff-line.selected.delete {
  background: #ffa39e;
}

.line-num {
  display: inline-block;
  width: 40px;
  text-align: right;
  color: #adb1b8;
  user-select: none;
  flex-shrink: 0;
}

.line-prefix {
  display: inline-block;
  width: 16px;
  text-align: center;
  flex-shrink: 0;
}

.line-content {
  flex: 1;
}
</style>
