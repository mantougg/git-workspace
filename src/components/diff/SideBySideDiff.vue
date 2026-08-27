<template>
  <VirtualList :items="rows" :item-height="ROW_HEIGHT" class="side-by-side-diff">
    <template #row="{ item }">
      <div v-if="item.type === 'header'" class="hunk-header">
        {{ item.text }}
      </div>
      <div v-else class="diff-row">
        <div
          :class="['diff-cell', item.old?.type]"
          :title="item.old?.content"
        >
          <template v-if="item.old">
            <span class="line-num">{{ item.old.num ?? "" }}</span>
            <span class="line-prefix">{{ item.old.type === "delete" ? "-" : " " }}</span>
            <span class="line-content">{{ item.old.content }}</span>
          </template>
        </div>
        <div
          :class="['diff-cell', item.new?.type]"
          :title="item.new?.content"
        >
          <template v-if="item.new">
            <span class="line-num">{{ item.new.num ?? "" }}</span>
            <span class="line-prefix">{{ item.new.type === "add" ? "+" : " " }}</span>
            <span class="line-content">{{ item.new.content }}</span>
          </template>
        </div>
      </div>
    </template>
  </VirtualList>
</template>

<script setup lang="ts">
import { computed } from "vue";
import VirtualList from "@/components/common/VirtualList.vue";
import type { FileDiff, Hunk } from "@/types/git";

const props = defineProps<{
  file: FileDiff;
}>();

/** Fixed row height (px) required by VirtualList. */
const ROW_HEIGHT = 21;

interface AlignedLine {
  num: number | null;
  content: string;
  type: string;
}

type Row =
  | { type: "header"; text: string }
  | { type: "pair"; old: AlignedLine | null; new: AlignedLine | null };

function oldLines(hunk: Hunk): AlignedLine[] {
  return hunk.lines
    .filter((l) => l.lineType !== "add")
    .map((l) => ({
      num: l.oldLine,
      content: l.content,
      type: l.lineType,
    }));
}

function newLines(hunk: Hunk): AlignedLine[] {
  return hunk.lines
    .filter((l) => l.lineType !== "delete")
    .map((l) => ({
      num: l.newLine,
      content: l.content,
      type: l.lineType,
    }));
}

// Flatten hunks into header + paired old/new rows so a single virtual window
// bounds the DOM node count (T-04 frontend rendering budget).
const rows = computed<Row[]>(() => {
  const out: Row[] = [];
  for (const hunk of props.file.hunks) {
    out.push({
      type: "header",
      text: `@@ -${hunk.oldStart},${hunk.oldLines} +${hunk.newStart},${hunk.newLines} @@`,
    });
    const left = oldLines(hunk);
    const right = newLines(hunk);
    for (let i = 0; i < Math.max(left.length, right.length); i++) {
      out.push({
        type: "pair",
        old: left[i] ?? null,
        new: right[i] ?? null,
      });
    }
  }
  return out;
});
</script>

<style scoped>
.side-by-side-diff {
  font-family: "Cascadia Code", "Fira Code", Consolas, monospace;
  font-size: 13px;
}

.hunk-header {
  height: 21px;
  line-height: 21px;
  background: #f0f0f0;
  color: var(--gw-text-dim);
  padding: 0 8px;
  font-size: 12px;
  white-space: pre;
}

.diff-row {
  display: flex;
  height: 21px;
}

/* Virtual rows are fixed-height, so long lines cannot wrap (that would break
   the height math); they clip with an ellipsis and stay readable via the
   title tooltip. */
.diff-cell {
  display: flex;
  align-items: center;
  flex: 1;
  min-width: 0;
  height: 21px;
  line-height: 21px;
  padding: 0 8px;
  white-space: pre;
  overflow: hidden;
}

.diff-cell:first-child {
  border-right: 1px solid #e0e0e0;
}

.diff-cell.delete {
  background: #ffebe9;
}

.diff-cell.add {
  background: #e6ffec;
}

.diff-cell.context {
  background: #fafafa;
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
  overflow: hidden;
  text-overflow: ellipsis;
}
</style>
