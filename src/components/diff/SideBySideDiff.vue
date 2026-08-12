<template>
  <div class="side-by-side-diff">
    <div v-for="(hunk, hi) in file.hunks" :key="hi" class="hunk">
      <div class="hunk-header">
        @@ -{{ hunk.oldStart }},{{ hunk.oldLines }} +{{ hunk.newStart }},{{
          hunk.newLines
        }} @@
      </div>
      <div class="hunk-body">
        <div class="diff-col old-col">
          <div
            v-for="(line, li) in oldLines(hunk)"
            :key="li"
            :class="['diff-line', line.type]"
          >
            <span class="line-num">{{ line.num ?? "" }}</span>
            <span class="line-prefix">-</span>
            <span class="line-content">{{ line.content }}</span>
          </div>
        </div>
        <div class="diff-col new-col">
          <div
            v-for="(line, li) in newLines(hunk)"
            :key="li"
            :class="['diff-line', line.type]"
          >
            <span class="line-num">{{ line.num ?? "" }}</span>
            <span class="line-prefix">+</span>
            <span class="line-content">{{ line.content }}</span>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import type { FileDiff, Hunk } from "@/types/git";

defineProps<{
  file: FileDiff;
}>();

interface AlignedLine {
  num: number | null;
  content: string;
  type: string;
}

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
</script>

<style scoped>
.side-by-side-diff {
  font-family: "Cascadia Code", "Fira Code", Consolas, monospace;
  font-size: 13px;
  line-height: 1.6;
}

.hunk-header {
  background: #f0f0f0;
  color: #909399;
  padding: 2px 8px;
  font-size: 12px;
}

.hunk-body {
  display: flex;
}

.diff-col {
  flex: 1;
  overflow-x: auto;
}

.old-col {
  border-right: 1px solid #e0e0e0;
}

.diff-line {
  display: flex;
  align-items: baseline;
  padding: 0 8px;
  white-space: pre-wrap;
  word-break: break-all;
  min-height: 1.6em;
}

.diff-line.delete {
  background: #ffebe9;
}

.diff-line.add {
  background: #e6ffec;
}

.diff-line.context {
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
}
</style>
