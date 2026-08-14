<template>
  <VirtualList :items="rows" :item-height="ROW_HEIGHT" class="unified-diff">
    <template #row="{ item }">
      <div v-if="item.type === 'header'" class="hunk-header">
        {{ item.text }}
      </div>
      <div v-else :class="['diff-line', item.line.lineType]">
        <span class="line-num old">{{ item.line.oldLine ?? "" }}</span>
        <span class="line-num new">{{ item.line.newLine ?? "" }}</span>
        <span class="line-prefix">{{ prefix(item.line.lineType) }}</span>
        <span class="line-content">{{ item.line.content }}</span>
      </div>
    </template>
  </VirtualList>
</template>

<script setup lang="ts">
import { computed } from "vue";
import VirtualList from "@/components/common/VirtualList.vue";
import type { DiffLine, FileDiff } from "@/types/git";

const props = defineProps<{
  file: FileDiff;
}>();

/** Fixed row height (px) required by VirtualList. */
const ROW_HEIGHT = 21;

type Row = { type: "header"; text: string } | { type: "line"; line: DiffLine };

// Flatten hunks into a uniform row list so a single virtual window covers
// both hunk headers and diff lines (T-04 frontend rendering budget).
const rows = computed<Row[]>(() => {
  const out: Row[] = [];
  for (const hunk of props.file.hunks) {
    out.push({
      type: "header",
      text: `@@ -${hunk.oldStart},${hunk.oldLines} +${hunk.newStart},${hunk.newLines} @@`,
    });
    for (const line of hunk.lines) {
      out.push({ type: "line", line });
    }
  }
  return out;
});

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
  height: 21px;
  line-height: 21px;
  background: #f0f0f0;
  color: #909399;
  padding: 0 8px;
  font-size: 12px;
  white-space: pre;
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
