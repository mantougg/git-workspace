<template>
  <div class="unified-diff">
    <div v-for="(hunk, hi) in file.hunks" :key="hi" class="hunk">
      <div class="hunk-header">
        @@ -{{ hunk.oldStart }},{{ hunk.oldLines }} +{{ hunk.newStart }},{{
          hunk.newLines
        }} @@
      </div>
      <div
        v-for="(line, li) in hunk.lines"
        :key="li"
        :class="['diff-line', line.lineType]"
      >
        <span class="line-num old">{{ line.oldLine ?? "" }}</span>
        <span class="line-num new">{{ line.newLine ?? "" }}</span>
        <span class="line-prefix">{{ prefix(line.lineType) }}</span>
        <span class="line-content">{{ line.content }}</span>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import type { FileDiff } from "@/types/git";

defineProps<{
  file: FileDiff;
}>();

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
  line-height: 1.6;
  /* Grow to fit the widest line so long lines scroll horizontally
     instead of wrapping. */
  width: max-content;
  min-width: 100%;
}

.hunk-header {
  background: #f0f0f0;
  color: #909399;
  padding: 2px 8px;
  font-size: 12px;
}

.diff-line {
  display: flex;
  align-items: baseline;
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
